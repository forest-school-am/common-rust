//! How long a request may spend waiting on the IdP before its session is
//! given up on. WHAT is worth retrying is decided by `Upstream` in error.rs;
//! what to do once the budget is spent belongs to the caller in web.rs.

use std::future::Future;
use std::time::Duration;

use backon::{ExponentialBuilder, Retryable};
use tokio::time::Instant;

use crate::error::Upstream;

/// R27a: every attempt is bounded at one second. `backon` schedules the gaps
/// between attempts and does NOT bound the operation itself, so this timeout
/// stays inside the retried closure — without it a hung IdP would hold an
/// attempt open forever and no amount of scheduling would help.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(1);

/// R27b: the delay budget is `with_total_delay`, so the schedule is entirely
/// configuration — no sequence is hand-picked to sum to it. backon stops when
/// the NEXT sleep would exceed the total, so the delays UNDER-run 10s rather
/// than being clamped to it.
///
/// `max_delay` is deliberately absent: under a 10s total the base never gets
/// past 4s, so a max would be config that cannot take effect, which reads as
/// load-bearing and is not. `max_times` stays as a second, independent cap so
/// no silent default is in play — the total binds long before it does.
const DELAY_BUDGET: Duration = Duration::from_secs(10);

fn base_schedule() -> ExponentialBuilder {
    ExponentialBuilder::default()
        .with_min_delay(Duration::from_secs(1))
        .with_factor(2.0)
        .with_total_delay(Some(DELAY_BUDGET))
        .with_max_times(5)
}

/// R27b adds jitter, because fixed delays are synchronised BY an outage: every
/// session failing at T retries at exactly T+1, T+3, T+7, so the herd is a
/// property of the design rather than bad luck, and the largest wave lands
/// when a restarted IdP is least able to take it.
///
/// backon's jitter only ever ADDS — `delay + delay * rand[0,1)` — so a
/// jittered delay lands in `[base, 2*base)` and the schedule gets LONGER,
/// never shorter. Combined with the total that makes the ATTEMPT COUNT
/// non-deterministic: inflated delays reach the budget sooner and a retry is
/// dropped. That trade is accepted (R27b) — with jitter you can have a hard
/// budget or a guaranteed attempt count, not both, and the budget wins.
fn schedule() -> ExponentialBuilder {
    base_schedule().with_jitter()
}

/// Worst case for one whole RESOLUTION: four 1s attempts plus up to 10s of
/// delay. Caps the resolution rather than a single call, so a userinfo, a
/// refresh and a second userinfo cannot each run a fresh schedule.
pub(crate) const UPSTREAM_BUDGET: Duration = Duration::from_secs(14);

/// One request's share of upstream time, shared across every call it makes.
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

/// Split out so tests can pin the jitter seed. Production always uses
/// `schedule()`, whose seed is random per process — which is the point.
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
        // A rejection is the IdP answering, so it ends the schedule at once —
        // retrying an answer is wrong as well as slow. The deadline stops the
        // second and third calls of one resolution from starting schedules
        // the request has no time left for.
        .when(|e| matches!(e, Upstream::Unreachable(_)) && !deadline.expired());

    // AND the whole schedule is bounded by what is left of the budget. The
    // `when` check alone only stops NEW attempts being authorised; one already
    // authorised runs to completion, so without this the budget is a
    // suggestion that a delay plus an attempt can overrun — measured at 17.06s
    // against a 14s budget across 100 seeds, and structurally worse than that,
    // since a delay of up to 8s can be entered just before expiry.
    tokio::time::timeout(deadline.remaining(), scheduled)
        .await
        .unwrap_or_else(|_| {
            Err(Upstream::Unreachable(
                "the request's upstream budget was exhausted".to_owned(),
            ))
        })
}

/// Run `call` on the R27b schedule until it succeeds, is rejected, or the
/// request's shared budget runs out.
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

    /// Records when each attempt fired, under virtual time so the schedule is
    /// observed exactly rather than within a tolerance.
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

    /// THE CONTROL. Jitter is the only thing that makes the schedule vary, so
    /// the same budget without it must be exactly determinate. If this ever
    /// starts varying, the bounds test below is measuring two things at once
    /// and its looseness is hiding one of them.
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

    /// R27b WEAKENED THIS FROM EQUALITY TO BOUNDS, deliberately: with jitter
    /// the delays and the attempt count are both non-deterministic, so exact
    /// equality is no longer a property the system has. What remains asserted
    /// is what actually matters, and it is driven across many seeds — a single
    /// seed would pass while the distribution was broken, which is the shape
    /// of test that looks green and proves nothing.
    #[tokio::test(start_paused = true)]
    async fn jitter_widens_every_delay_but_never_the_budget() {
        // Bases are `min_delay * factor^i` and are NOT compounded by jitter,
        // so each position has a fixed base regardless of seed. Spelled out
        // rather than derived from the builder, which would assert nothing.
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

    /// Every attempt consumes its full timeout: the §6.1 case, an IdP that
    /// accepts connections and never answers. Bounded rather than exact, for
    /// the same reason as above.
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

    /// THE GUARANTEE A REQUEST ACTUALLY EXPERIENCES, which is not the same as
    /// the delay budget. `resolve_session` can make three upstream calls; the
    /// deadline stops a later one from starting a schedule the request has no
    /// time for, so they do not each cost a full schedule.
    ///
    /// It is a CEILING rather than the budget itself: the deadline authorises
    /// a new attempt only while budget remains, but an attempt already
    /// authorised runs to completion, so a resolution can overrun by at most
    /// one delay plus one attempt. Measured across seeds, the worst observed
    /// total was 16.6s — asserted here rather than reasoned about, because the
    /// overrun is exactly the kind of thing that is easy to argue away.
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
