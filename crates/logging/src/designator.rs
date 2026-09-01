//! The designator vocabulary and the macros that emit through it. A new
//! designator goes here; a new way of RENDERING one goes in format.rs.

pub const AUTH: &str = "auth";
pub const BUSINESS: &str = "business";
pub const UPSTREAM: &str = "upstream";
pub const STORAGE: &str = "storage";
pub const HTTP: &str = "http";

/// A custom designator MUST be listed and explained in the owning repo's
/// README.md, and — when proposed by an agent — confirmed by the operator
/// before it lands.
#[macro_export]
macro_rules! custom {
    ($name:literal) => {
        ::core::concat!("c-", $name)
    };
}

#[macro_export]
macro_rules! error {
    ($designator:expr, $($arg:tt)+) => {
        $crate::tracing::error!(target: $designator, $($arg)+)
    };
}

#[macro_export]
macro_rules! warn {
    ($designator:expr, $($arg:tt)+) => {
        $crate::tracing::warn!(target: $designator, $($arg)+)
    };
}

#[macro_export]
macro_rules! info {
    ($designator:expr, $($arg:tt)+) => {
        $crate::tracing::info!(target: $designator, $($arg)+)
    };
}

#[macro_export]
macro_rules! debug {
    ($designator:expr, $($arg:tt)+) => {
        $crate::tracing::debug!(target: $designator, $($arg)+)
    };
}

#[macro_export]
macro_rules! trace {
    ($designator:expr, $($arg:tt)+) => {
        $crate::tracing::trace!(target: $designator, $($arg)+)
    };
}
