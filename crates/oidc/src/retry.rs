//! How long a request may spend waiting on the IdP before its session is
//! given up on. WHAT is worth retrying is decided by `Upstream` in error.rs;
//! what to do once the budget is spent belongs to the caller in web.rs.

use std::future::Future;
use std::time::Duration;

use backon::{ExponentialBuilder, Retryable};
use tokio::time::Instant;

use crate::error::Upstream;

/// `backon` schedules the gaps between attempts and does NOT bound the
/// operation itself, which is why this timeout lives inside the retried
/// closure rather than around the schedule.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(1);

/// backon stops when the NEXT sleep would exceed the total, so the delays
/// UNDER-run this rather than being clamped to it. `max_delay` is deliberately
/// absent: under this total the base never gets past 4s, so it could not take
/// effect.
const DELAY_BUDGET: Duration = Duration::from_secs(10);

fn base_schedule() -> ExponentialBuilder {
    ExponentialBuilder::default()
        .with_min_delay(Duration::from_secs(1))
        .with_factor(2.0)
        .with_total_delay(Some(DELAY_BUDGET))
        .with_max_times(5)
}

/// backon's jitter only ever ADDS — `delay + delay * rand[0,1)` — so the
/// schedule gets LONGER, never shorter, and reaching the total sooner can drop
/// a retry: under jitter the attempt COUNT is not a property this has.
fn schedule() -> ExponentialBuilder {
    base_schedule().with_jitter()
}

pub(crate) const UPSTREAM_BUDGET: Duration = Duration::from_secs(14);

pub(crate) struct Deadline(Instant);

impl Deadline {
    pub(crate) fn starting_now() -> Self {
        Self(Instant::now() + UPSTREAM_BUDGET)
    }

    fn expired(&self) -> bool {
        Instant::now() >= self.0
    }

    fn remaining(&self) -> Duration {
        self.0.saturating_duration_since(Instant::now())
    }
}

async fn within_on<T, F, Fut>(
    deadline: &Deadline,
    builder: ExponentialBuilder,
    mut call: F,
) -> Result<T, Upstream>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Upstream>>,
{
    let attempt = move || {
        let running = call();
        async move {
            tokio::time::timeout(ATTEMPT_TIMEOUT, running)
                .await
                .unwrap_or_else(|_| {
                    Err(Upstream::Unreachable(format!(
                        "no response within {ATTEMPT_TIMEOUT:?}"
                    )))
                })
        }
    };

    let scheduled = attempt
        .retry(builder)
        .when(|e| matches!(e, Upstream::Unreachable(_)) && !deadline.expired());

    // `when` only withholds authorisation for a NEW attempt; one already
    // authorised runs to completion, so the schedule is timed out as a whole.
    tokio::time::timeout(deadline.remaining(), scheduled)
        .await
        .unwrap_or_else(|_| {
            Err(Upstream::Unreachable(
                "the request's upstream budget was exhausted".to_owned(),
            ))
        })
}

pub(crate) async fn within<T, F, Fut>(deadline: &Deadline, call: F) -> Result<T, Upstream>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Upstream>>,
{
    within_on(deadline, schedule(), call).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    async fn gaps_for(builder: ExponentialBuilder) -> Vec<Duration> {
        let start = Instant::now();
        let marks: Mutex<Vec<Duration>> = Mutex::new(Vec::new());
        let _: Result<(), Upstream> = within_on(&Deadline::starting_now(), builder, || {
            marks.lock().unwrap().push(start.elapsed());
            async { Err(Upstream::Unreachable("refused".into())) }
        })
        .await;
        let marks = marks.into_inner().unwrap();
        marks.windows(2).map(|w| w[1] - w[0]).collect()
    }

    #[tokio::test(start_paused = true)]
    async fn without_jitter_the_budget_yields_a_fixed_schedule() {
        let gaps = gaps_for(base_schedule()).await;
        assert_eq!(
            gaps,
            vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4)
            ],
            "1+2+4=7s; the next delay would be 8s and 7+8 exceeds the 10s \
             total, so backon stops rather than clamping"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn jitter_widens_every_delay_but_never_the_budget() {
        const BASES: [u64; 3] = [1, 2, 4];

        for seed in 0..200u64 {
            let gaps = gaps_for(base_schedule().with_jitter().with_jitter_seed(seed)).await;

            assert!(
                (2..=3).contains(&gaps.len()),
                "seed {seed}: {} delays — jitter may drop a retry by reaching \
                 the budget sooner, but never add one",
                gaps.len()
            );

            let total: Duration = gaps.iter().sum();
            assert!(
                total <= DELAY_BUDGET,
                "seed {seed}: delays summed to {total:?}, over the {DELAY_BUDGET:?} budget"
            );

            for (i, gap) in gaps.iter().enumerate() {
                let base = Duration::from_secs(BASES[i]);
                assert!(
                    *gap >= base && *gap <= base * 2,
                    "seed {seed} delay {i}: {gap:?} outside [{base:?}, {:?}] — \
                     backon's jitter only ADDS, so a delay below its base means \
                     the implementation started subtracting",
                    base * 2
                );
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_permanent_hang_stays_inside_the_budget() {
        let start = Instant::now();
        let attempts = Mutex::new(0usize);

        let out: Result<(), Upstream> = within(&Deadline::starting_now(), || {
            *attempts.lock().unwrap() += 1;
            async {
                std::future::pending::<()>().await;
                unreachable!()
            }
        })
        .await;

        assert!(matches!(out, Err(Upstream::Unreachable(_))));
        let attempts = *attempts.lock().unwrap();
        assert!(
            (3..=4).contains(&attempts),
            "3 or 4 attempts, never more: {attempts}"
        );
        assert!(
            start.elapsed() <= UPSTREAM_BUDGET,
            "a hung upstream must stay inside the budget: {:?}",
            start.elapsed()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_rejection_is_not_retried() {
        let attempts = Mutex::new(0usize);

        let out: Result<(), Upstream> = within(&Deadline::starting_now(), || {
            *attempts.lock().unwrap() += 1;
            async { Err(Upstream::Rejected("invalid_grant".into())) }
        })
        .await;

        assert_eq!(out, Err(Upstream::Rejected("invalid_grant".into())));
        assert_eq!(
            *attempts.lock().unwrap(),
            1,
            "an answer must cost one attempt, not six"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_hang_that_recovers_returns_the_value_and_stops_retrying() {
        let attempts = Mutex::new(0usize);

        let out = within(&Deadline::starting_now(), || {
            let mut n = attempts.lock().unwrap();
            *n += 1;
            let hang = *n < 3;
            async move {
                if hang {
                    std::future::pending::<()>().await;
                }
                Ok::<_, Upstream>("identity")
            }
        })
        .await;

        assert_eq!(out.unwrap(), "identity");
        assert_eq!(*attempts.lock().unwrap(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn three_hung_calls_sharing_a_deadline_stay_inside_the_budget() {
        for seed in 0..100u64 {
            let start = Instant::now();
            let deadline = Deadline::starting_now();
            let mut attempts_per_call = Vec::new();

            for _ in 0..3 {
                let n = Mutex::new(0usize);
                let _: Result<(), Upstream> = within_on(
                    &deadline,
                    base_schedule().with_jitter().with_jitter_seed(seed),
                    || {
                        *n.lock().unwrap() += 1;
                        async {
                            std::future::pending::<()>().await;
                            unreachable!()
                        }
                    },
                )
                .await;
                attempts_per_call.push(n.into_inner().unwrap());
            }

            assert!(
                start.elapsed() <= UPSTREAM_BUDGET,
                "seed {seed}: a whole resolution took {:?}, over the {UPSTREAM_BUDGET:?} \
                 budget — three calls must not each cost a full schedule",
                start.elapsed()
            );
            assert_eq!(
                attempts_per_call[2], 1,
                "seed {seed}: by the third call the budget is spent, so it gets \
                 one attempt and no retry — {attempts_per_call:?}"
            );
        }
    }
}
