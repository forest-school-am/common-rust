//! Refusing to start: the one line a failed boot prints, and the exit. Only
//! the refusal path belongs here — bringing the subscriber up for a config
//! that IS valid is lib.rs, and what makes a value invalid is config.rs.

use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;

/// ```no_run
/// # let raw = "nope".to_string();
/// let refusal = common_logging::Refusal::new(
///     "REGISTRY_BIND",
///     raw,
///     "a socket address such as \"0.0.0.0:8080\"",
/// );
/// common_logging::refuse!(refusal);
/// ```
#[macro_export]
macro_rules! refuse {
    ($refusal:expr) => {{
        $crate::__refusal_line!($refusal);
        $crate::__exit_refused()
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __refusal_line {
    ($refusal:expr) => {{
        let refusal: $crate::Refusal = $refusal;
        $crate::__unfiltered(|| {
            $crate::error::startup!(
                variable = refusal.variable.as_str(),
                value = %refusal.value,
                accepted = %refusal.accepted,
                detail = refusal.detail.as_deref().unwrap_or("-"),
                "refusing to start: invalid configuration"
            );
        });
    }};
}

#[doc(hidden)]
pub fn __unfiltered(emit: impl FnOnce()) {
    let subscriber = tracing_subscriber::registry().with(
        fmt::layer()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(false),
    );
    tracing::subscriber::with_default(subscriber, emit);
}

#[doc(hidden)]
pub fn __exit_refused() -> ! {
    let _ = std::io::Write::flush(&mut std::io::stdout());
    std::process::exit(1);
}
