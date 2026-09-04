//! How long a request may spend waiting on the IdP before its session is
//! given up on. WHAT is worth retrying is decided by `Upstream` in error.rs;
//! what to do once the budget is spent belongs to the caller in web.rs.

use std::future::Future;
use std::time::Duration;

use tokio::time::Instant;

use crate::error::Upstream;

/// R27: the hard total a single request may spend on IdP calls, across every
/// attempt and every call it makes. §6.1's invariant is that an upstream
/// outage can never hang the service; this is what enforces it.
pub(crate) const UPSTREAM_BUDGET: Duration = Duration::from_secs(30);

/// R27: the first attempt's timeout. Each subsequent attempt doubles it, and
/// the budget clamps the last one — with a 30s budget that yields 1+2+4+8+15.
/// The attempt COUNT is deliberately not written down anywhere: it falls out
/// of the budget, so changing the budget is one edit.
const FIRST_TIMEOUT: Duration = Duration::from_secs(1);

/// One request's share of upstream time, shared across every call it makes.
/// Held rather than recomputed so that a userinfo call, a refresh and a second
/// userinfo cannot each start a fresh 30s and add up to ninety.
pub(crate) struct Deadline(Instant);

impl Deadline {
    pub(crate) fn starting_now() -> Self {
        Self(Instant::now() + UPSTREAM_BUDGET)
    }

    fn remaining(&self) -> Duration {
        self.0.saturating_duration_since(Instant::now())
    }
}

/// Run `call` under an escalating per-attempt timeout until it succeeds, is
/// rejected, or the shared budget runs out.
///
/// THE BUDGET IS THE AUTHORITY, NOT THE DOUBLING. The final attempt is clamped
/// to whatever remains rather than overshooting it or being skipped, so the
/// worst case is exactly the budget.
pub(crate) async fn within<T, F, Fut>(deadline: &Deadline, mut call: F) -> Result<T, Upstream>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Upstream>>,
{
    let mut timeout = FIRST_TIMEOUT;
    let mut last =
        Upstream::Unreachable(format!("no attempt completed within {UPSTREAM_BUDGET:?}"));

    loop {
        let remaining = deadline.remaining();
        if remaining.is_zero() {
            return Err(last);
        }
        let this_attempt = timeout.min(remaining);

        match tokio::time::timeout(this_attempt, call()).await {
            Ok(Ok(value)) => return Ok(value),

            // An answer. Not retried at any speed — see `Upstream`.
            Ok(Err(rejected @ Upstream::Rejected(_))) => return Err(rejected),

            // FAST-FAILURE PATH — NOT YET RULED. The call came back before its
            // timeout (connection refused, DNS failure), so retrying costs no
            // wall-clock: five attempts would burn in milliseconds, hammering
            // an IdP that is actively refusing and logging the user out just
            // as fast as before R27. Fixing that needs either a delay between
            // attempts or a budget spent as wall-clock regardless of how each
            // attempt failed, and which one is the user's call, not this
            // function's. Until it is ruled, a fast failure is returned as-is
            // — the pre-R27 behaviour, so nothing regresses while the question
            // is open.
            Ok(Err(fast @ Upstream::Unreachable(_))) => return Err(fast),

            // A HANG: the attempt consumed its whole timeout. This is the path
            // R27 specifies and the only one that currently retries.
            Err(_elapsed) => {
                last = Upstream::Unreachable(format!("no response within {this_attempt:?}"));
            }
        }

        timeout = timeout.saturating_mul(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The budget bounds the total, and the clamped final attempt neither
    /// overshoots nor is skipped. Timings are asserted as a window because a
    /// loaded machine adds scheduling slop, but the window is far tighter than
    /// the failure it guards against (an unclamped fifth attempt would reach
    /// 31s, and no bound at all would hang forever).
    #[tokio::test(start_paused = true)]
    async fn a_permanent_hang_costs_exactly_the_budget() {
        let attempts = AtomicUsize::new(0);
        let started = Instant::now();

        let out: Result<(), Upstream> = within(&Deadline::starting_now(), || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async {
                std::future::pending::<()>().await;
                unreachable!()
            }
        })
        .await;

        assert!(matches!(out, Err(Upstream::Unreachable(_))));
        assert_eq!(
            started.elapsed(),
            UPSTREAM_BUDGET,
            "a hung upstream must cost the budget, no more and no less"
        );
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            5,
            "1+2+4+8 then a final attempt clamped to the remaining 15s"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_hang_that_recovers_returns_the_value_and_stops_retrying() {
        let attempts = AtomicUsize::new(0);

        let out = within(&Deadline::starting_now(), || {
            let n = attempts.fetch_add(1, Ordering::SeqCst);
            async move {
                if n < 2 {
                    std::future::pending::<()>().await;
                }
                Ok::<_, Upstream>("identity")
            }
        })
        .await;

        assert_eq!(out.unwrap(), "identity");
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            3,
            "must stop the moment it succeeds"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_rejection_is_not_retried() {
        let attempts = AtomicUsize::new(0);

        let out: Result<(), Upstream> = within(&Deadline::starting_now(), || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Err(Upstream::Rejected("invalid_grant".into())) }
        })
        .await;

        assert_eq!(out, Err(Upstream::Rejected("invalid_grant".into())));
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            1,
            "an answer must cost one attempt, not five"
        );
    }

    /// Pins the CURRENT behaviour of the unruled fast path so that ruling it
    /// changes a test deliberately rather than silently.
    #[tokio::test(start_paused = true)]
    async fn a_fast_failure_is_not_yet_retried() {
        let attempts = AtomicUsize::new(0);

        let out: Result<(), Upstream> = within(&Deadline::starting_now(), || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Err(Upstream::Unreachable("connection refused".into())) }
        })
        .await;

        assert!(matches!(out, Err(Upstream::Unreachable(_))));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
}
