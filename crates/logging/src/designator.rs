//! The designator vocabulary, the field it travels in, and the macros that
//! emit through it. A new designator goes here; a new way of RENDERING one
//! goes in format.rs; deciding which ones reach the output goes in filter.rs.

/// The event field every designated emission carries (R28). Read back through
/// this const by filter.rs and format.rs.
///
/// The emission macros must spell it as a LITERAL — tracing resolves field
/// names at expansion time, so a const cannot be substituted there. It
/// therefore appears exactly once in the crate, in `__designated!` below, and
/// `the_emitted_field_is_the_declared_one` drives a real emission to assert
/// the literal and this const still agree. That test is the only thing holding
/// them together; do not delete it.
pub const FIELD: &str = "designator";

pub const AUTH: &str = "auth";
pub const BUSINESS: &str = "business";
pub const UPSTREAM: &str = "upstream";
pub const STORAGE: &str = "storage";
pub const HTTP: &str = "http";

/// The stand vocabulary, for validating operator input. A project designator
/// is anything carrying the `c-` prefix (§8.3) and cannot be enumerated here.
pub const STAND: [&str; 5] = [AUTH, BUSINESS, UPSTREAM, STORAGE, HTTP];

/// The prefix `custom!` applies, so a log reader can tell stand vocabulary
/// from project vocabulary at a glance (§8.3).
pub const CUSTOM_PREFIX: &str = "c-";

/// A custom designator MUST be listed and explained in the owning repo's
/// README.md, and — when proposed by an agent — confirmed by the operator
/// before it lands.
#[macro_export]
macro_rules! custom {
    ($name:literal) => {
        ::core::concat!("c-", $name)
    };
}

/// Every emission routes through here, so the field name is written once
/// rather than five times (CODESTYLE 4.5).
#[doc(hidden)]
#[macro_export]
macro_rules! __designated {
    ($level:ident, $designator:expr, $($arg:tt)+) => {
        $crate::tracing::$level!(designator = $designator, $($arg)+)
    };
}

#[macro_export]
macro_rules! error {
    ($designator:expr, $($arg:tt)+) => {
        $crate::__designated!(error, $designator, $($arg)+)
    };
}

#[macro_export]
macro_rules! warn {
    ($designator:expr, $($arg:tt)+) => {
        $crate::__designated!(warn, $designator, $($arg)+)
    };
}

#[macro_export]
macro_rules! info {
    ($designator:expr, $($arg:tt)+) => {
        $crate::__designated!(info, $designator, $($arg)+)
    };
}

#[macro_export]
macro_rules! debug {
    ($designator:expr, $($arg:tt)+) => {
        $crate::__designated!(debug, $designator, $($arg)+)
    };
}

#[macro_export]
macro_rules! trace {
    ($designator:expr, $($arg:tt)+) => {
        $crate::__designated!(trace, $designator, $($arg)+)
    };
}
