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

/// R27a: six attempts with delays of 1, 2, 4, 8 and 15 seconds between them.
/// The delays sum to exactly 30s, which is where the ruling's total lives.
///
/// The final 15 is `max_delay` CLAMPING the doubling's 16, which is what makes
/// the sequence expressible as configuration rather than as a hardcoded
/// exception — and it is the part a careless config edit would silently break,
/// so the schedule is asserted in full below.
fn schedule() -> ExponentialBuilder {
    ExponentialBuilder::default()
        .with_min_delay(Duration::from_secs(1))
        .with_factor(2.0)
        .with_max_delay(Duration::from_secs(15))
        .with_max_times(5)
}

/// One full schedule's worst case: six 1s attempts plus the 30s of delay
/// between them. This caps a whole RESOLUTION rather than a single call, so a
/// userinfo, a refresh and a second userinfo cannot each run a fresh schedule
/// and spend it three times over.
pub(crate) const UPSTREAM_BUDGET: Duration = Duration::from_secs(36);

/// One request's share of upstream time, shared across every call it makes.
pub(crate) struct Deadline(Instant);

impl Deadline {
    pub(crate) fn starting_now() -> Self {
        Self(Instant::now() + UPSTREAM_BUDGET)
    }

    fn expired(&self) -> bool {
        Instant::now() >= self.0
    }
}

/// Run `call` on the R27a schedule until it succeeds, is rejected, or the
/// request's shared budget runs out.
pub(crate) async fn within<T, F, Fut>(deadline: &Deadline, mut call: F) -> Result<T, Upstream>
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

    attempt
        .retry(schedule())
        // A rejection is the IdP answering, so it ends the schedule at once —
        // retrying an answer is wrong as well as slow. The deadline stops the
        // second and third calls of one resolution from starting schedules
        // the request has no time left for.
        .when(|e| matches!(e, Upstream::Unreachable(_)) && !deadline.expired())
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Virtual time, so the schedule is asserted exactly rather than within a
    /// tolerance. The gaps are spelled out here rather than derived from
    /// `schedule()`: reading them back off the builder would assert nothing.
    #[tokio::test(start_paused = true)]
    async fn the_schedule_is_six_attempts_with_gaps_of_1_2_4_8_and_15_seconds() {
        let start = Instant::now();
        let marks: Mutex<Vec<Duration>> = Mutex::new(Vec::new());

        let out: Result<(), Upstream> = within(&Deadline::starting_now(), || {
            marks.lock().unwrap().push(start.elapsed());
            async { Err(Upstream::Unreachable("refused".into())) }
        })
        .await;

        assert!(matches!(out, Err(Upstream::Unreachable(_))));
        let marks = marks.into_inner().unwrap();
        assert_eq!(marks.len(), 6, "six attempts: the first plus five retries");

        let gaps: Vec<u64> = marks.windows(2).map(|w| (w[1] - w[0]).as_secs()).collect();
        assert_eq!(
            gaps,
            vec![1, 2, 4, 8, 15],
            "the doubling must be clamped to 15 by max_delay, not run on to 16"
        );
        assert_eq!(
            start.elapsed(),
            Duration::from_secs(30),
            "the delays are where the ruling's 30s total lives"
        );
    }

    /// The same schedule, but every attempt consumes its full timeout. This is
    /// the §6.1 case — an IdP that accepts connections and never answers.
    #[tokio::test(start_paused = true)]
    async fn a_permanent_hang_costs_the_whole_schedule_and_no_more() {
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
        assert_eq!(*attempts.lock().unwrap(), 6);
        assert_eq!(
            start.elapsed(),
            UPSTREAM_BUDGET,
            "six 1s attempts plus 30s of delay is the worst case, and the \
             budget must equal it or the last attempt is cut off"
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

    /// The budget is per RESOLUTION. A second call sharing a spent deadline
    /// gets one attempt, not a fresh schedule — without this, the three calls
    /// `resolve_session` can make would cost the budget three times over.
    #[tokio::test(start_paused = true)]
    async fn a_shared_deadline_stops_a_second_call_starting_a_fresh_schedule() {
        let deadline = Deadline::starting_now();

        let first: Result<(), Upstream> = within(&deadline, || async {
            std::future::pending::<()>().await;
            unreachable!()
        })
        .await;
        assert!(matches!(first, Err(Upstream::Unreachable(_))));

        let after_first = Instant::now();
        let attempts = Mutex::new(0usize);
        let second: Result<(), Upstream> = within(&deadline, || {
            *attempts.lock().unwrap() += 1;
            async { Err(Upstream::Unreachable("still down".into())) }
        })
        .await;

        assert!(matches!(second, Err(Upstream::Unreachable(_))));
        assert_eq!(
            *attempts.lock().unwrap(),
            1,
            "the budget was already spent, so the second call gets one try"
        );
        assert_eq!(
            after_first.elapsed(),
            Duration::ZERO,
            "and it must not sleep a delay it has no budget for"
        );
    }
}
