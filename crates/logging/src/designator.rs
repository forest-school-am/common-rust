//! The designator vocabulary, the field it travels in, and the macros that
//! emit through it. A new designator goes here; a new way of RENDERING one
//! goes in format.rs; deciding which ones reach the output goes in filter.rs.

/// The emission macros spell this name as a LITERAL: tracing resolves field
/// names at expansion time, so a const cannot be substituted there.
pub const FIELD: &str = "designator";

pub const AUTH: &str = "auth";
pub const BUSINESS: &str = "business";
pub const UPSTREAM: &str = "upstream";
pub const STORAGE: &str = "storage";
pub const HTTP: &str = "http";

pub const STAND: [&str; 5] = [AUTH, BUSINESS, UPSTREAM, STORAGE, HTTP];

pub const CUSTOM_PREFIX: &str = "c-";

/// §8.3, and nothing here enforces either half: a custom designator MUST be
/// listed and explained in the owning repo's README.md, and one proposed by an
/// agent MUST be confirmed by the operator before it lands.
#[macro_export]
macro_rules! custom {
    ($name:literal) => {
        ::core::concat!("c-", $name)
    };
}

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
