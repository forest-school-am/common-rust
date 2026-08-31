//! Designators (CODESTYLE.md §8.3): every event carries one, set as the
//! tracing *target*, so a log reader can classify an event at a glance. The
//! common vocabulary lives here as `&'static str` consts (a target must be a
//! compile-time `&'static str`, which these are); project-specific
//! designators go through [`custom!`], which prefixes `c-` so stand
//! vocabulary and project vocabulary never blur.

/// Authentication / authorization / identity resolution.
pub const AUTH: &str = "auth";
/// Domain / business logic.
pub const BUSINESS: &str = "business";
/// Calls out to another service (IdP, DB proxy, remote API).
pub const UPSTREAM: &str = "upstream";
/// Persistence / caches / files.
pub const STORAGE: &str = "storage";
/// HTTP request lifecycle (the request span uses this target).
pub const HTTP: &str = "http";

/// Build a project-specific designator as a compile-time `&'static str`,
/// prefixed with `c-` (§8.3): `custom!("scheduler") == "c-scheduler"`.
///
/// A custom designator MUST be listed and explained in the owning repo's
/// README.md, and — when proposed by an agent — confirmed by the operator
/// before it lands.
#[macro_export]
macro_rules! custom {
    ($name:literal) => {
        ::core::concat!("c-", $name)
    };
}

/// Emit at a level with a designator as the first argument (§8.5,
/// `log!`-style). Everything after the designator is forwarded verbatim to
/// the matching `tracing` macro, so message + fields work as usual:
///
/// ```ignore
/// use stand_log::{info, AUTH};
/// // tracing idiom: structured fields first, message last.
/// info!(AUTH, user = %name, "signed in");
/// info!(stand_log::custom!("scheduler"), n = count, "tick");
/// ```
#[macro_export]
macro_rules! error {
    ($designator:expr, $($arg:tt)+) => {
        $crate::tracing::error!(target: $designator, $($arg)+)
    };
}

/// See [`error!`].
#[macro_export]
macro_rules! warn {
    ($designator:expr, $($arg:tt)+) => {
        $crate::tracing::warn!(target: $designator, $($arg)+)
    };
}

/// See [`error!`].
#[macro_export]
macro_rules! info {
    ($designator:expr, $($arg:tt)+) => {
        $crate::tracing::info!(target: $designator, $($arg)+)
    };
}

/// See [`error!`].
#[macro_export]
macro_rules! debug {
    ($designator:expr, $($arg:tt)+) => {
        $crate::tracing::debug!(target: $designator, $($arg)+)
    };
}

/// See [`error!`].
#[macro_export]
macro_rules! trace {
    ($designator:expr, $($arg:tt)+) => {
        $crate::tracing::trace!(target: $designator, $($arg)+)
    };
}
