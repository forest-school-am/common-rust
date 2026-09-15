//! Emitting through a designator: the five level primitives
//! (`info!(designator | …)`), the per-level modules with one macro per
//! designator (`info::auth!(…)`, `info::custom!(tag | …)`), and the
//! first-argument form kept as a deprecated alias for one release.
//!
//! The thirty static macros come out of one generating macro; each is
//! `#[macro_export]`ed under a hidden root name and re-exported into its
//! level module by single-segment `pub use` — the textual-scope form, which
//! is the one rustc allows for a macro-expanded `macro_export`.

/// The one place the field is written. `designator = %d` is a Display
/// capture: zero-cost for the stand variants, one small format for `Custom`.
#[doc(hidden)]
#[macro_export]
macro_rules! __designated {
    ($level:ident, $designator:expr, $($arg:tt)+) => {{
        let designator: $crate::Designator = ::core::convert::Into::into($designator);
        $crate::tracing::$level!(designator = %designator, $($arg)+)
    }};
}

/// The first-argument form, `info!(AUTH, …)`: deprecated, gone next release.
#[deprecated(
    since = "0.4.0",
    note = "write log::<level>::<designator>!(…) (log::info::auth!(…)), or the \
            primitive <level>!(designator | …); the comma form goes next release"
)]
#[doc(hidden)]
#[macro_export]
macro_rules! __first_argument_form {
    ($level:ident, $designator:expr, $($arg:tt)+) => {
        $crate::__designated!($level, $designator, $($arg)+)
    };
}

/// `custom!("name")` as a designator VALUE: deprecated, gone next release —
/// the custom designator now travels inside the call,
/// `log::info::custom!("name" | …)`.
#[deprecated(
    since = "0.4.0",
    note = "write log::<level>::custom!(\"name\" | …); custom!(\"name\") as a \
            value goes next release"
)]
#[macro_export]
macro_rules! custom {
    ($name:literal) => {
        $crate::Designator::from($name)
    };
}

// The level primitives. Two live arms — a literal or a path before the `|`
// (macro_rules permits `|` after those two fragments and not after an
// expression, so a call expression needs a `let` first) — and the deprecated
// comma arm. Hand-written, not generated: the static macros below reach them
// as `$crate::info!`, and a macro-expanded `macro_export` cannot be reached
// by absolute path from inside this crate.

/// `error!(designator | fields…, "message")`. Prefer `error::auth!(…)`.
#[macro_export]
macro_rules! error {
    ($d:literal | $($arg:tt)+) => { $crate::__designated!(error, $d, $($arg)+) };
    ($d:path | $($arg:tt)+) => { $crate::__designated!(error, $d, $($arg)+) };
    ($d:expr, $($arg:tt)+) => { $crate::__first_argument_form!(error, $d, $($arg)+) };
}

/// `warn!(designator | fields…, "message")`. Prefer `warn::auth!(…)`.
#[macro_export]
macro_rules! warn {
    ($d:literal | $($arg:tt)+) => { $crate::__designated!(warn, $d, $($arg)+) };
    ($d:path | $($arg:tt)+) => { $crate::__designated!(warn, $d, $($arg)+) };
    ($d:expr, $($arg:tt)+) => { $crate::__first_argument_form!(warn, $d, $($arg)+) };
}

/// `info!(designator | fields…, "message")`. Prefer `info::auth!(…)`.
#[macro_export]
macro_rules! info {
    ($d:literal | $($arg:tt)+) => { $crate::__designated!(info, $d, $($arg)+) };
    ($d:path | $($arg:tt)+) => { $crate::__designated!(info, $d, $($arg)+) };
    ($d:expr, $($arg:tt)+) => { $crate::__first_argument_form!(info, $d, $($arg)+) };
}

/// `debug!(designator | fields…, "message")`. Prefer `debug::auth!(…)`.
#[macro_export]
macro_rules! debug {
    ($d:literal | $($arg:tt)+) => { $crate::__designated!(debug, $d, $($arg)+) };
    ($d:path | $($arg:tt)+) => { $crate::__designated!(debug, $d, $($arg)+) };
    ($d:expr, $($arg:tt)+) => { $crate::__first_argument_form!(debug, $d, $($arg)+) };
}

/// `trace!(designator | fields…, "message")`. Prefer `trace::auth!(…)`.
#[macro_export]
macro_rules! trace {
    ($d:literal | $($arg:tt)+) => { $crate::__designated!(trace, $d, $($arg)+) };
    ($d:path | $($arg:tt)+) => { $crate::__designated!(trace, $d, $($arg)+) };
    ($d:expr, $($arg:tt)+) => { $crate::__first_argument_form!(trace, $d, $($arg)+) };
}

/// Fills one level module: six static macros (no designator argument, each
/// expanding to the level primitive with its stand designator) plus
/// `custom!(tag | …)`, which takes a path or a string literal before the `|`.
/// `$dol` is the `$` token, passed in so the generated macros can spell their
/// own metavariables.
macro_rules! level_module {
    (
        $dol:tt $level:ident, custom = $custom:ident,
        $( $name:ident = $designator:ident => $exported:ident ),+ $(,)?
    ) => {
        $(
            #[doc(hidden)]
            #[macro_export]
            macro_rules! $exported {
                ($dol($dol arg:tt)+) => {
                    $crate::$level!($crate::$designator | $dol($dol arg)+)
                };
            }
            pub use $exported as $name;
        )+

        #[doc(hidden)]
        #[macro_export]
        macro_rules! $custom {
            ($dol tag:literal | $dol($dol arg:tt)+) => {
                $crate::$level!($dol tag | $dol($dol arg)+)
            };
            ($dol tag:path | $dol($dol arg:tt)+) => {
                $crate::$level!($dol tag | $dol($dol arg)+)
            };
        }
        pub use $custom as custom;
    };
}

/// `error::auth!(…)`, `error::business!(…)`, …, `error::custom!(tag | …)`.
pub mod error {
    level_module! {
        $ error, custom = __log_error_custom,
        auth = AUTH => __log_error_auth,
        business = BUSINESS => __log_error_business,
        upstream = UPSTREAM => __log_error_upstream,
        storage = STORAGE => __log_error_storage,
        http = HTTP => __log_error_http,
        startup = STARTUP => __log_error_startup,
    }
}

/// `warn::auth!(…)`, `warn::business!(…)`, …, `warn::custom!(tag | …)`.
pub mod warn {
    level_module! {
        $ warn, custom = __log_warn_custom,
        auth = AUTH => __log_warn_auth,
        business = BUSINESS => __log_warn_business,
        upstream = UPSTREAM => __log_warn_upstream,
        storage = STORAGE => __log_warn_storage,
        http = HTTP => __log_warn_http,
        startup = STARTUP => __log_warn_startup,
    }
}

/// `info::auth!(…)`, `info::business!(…)`, …, `info::custom!(tag | …)`.
pub mod info {
    level_module! {
        $ info, custom = __log_info_custom,
        auth = AUTH => __log_info_auth,
        business = BUSINESS => __log_info_business,
        upstream = UPSTREAM => __log_info_upstream,
        storage = STORAGE => __log_info_storage,
        http = HTTP => __log_info_http,
        startup = STARTUP => __log_info_startup,
    }
}

/// `debug::auth!(…)`, `debug::business!(…)`, …, `debug::custom!(tag | …)`.
pub mod debug {
    level_module! {
        $ debug, custom = __log_debug_custom,
        auth = AUTH => __log_debug_auth,
        business = BUSINESS => __log_debug_business,
        upstream = UPSTREAM => __log_debug_upstream,
        storage = STORAGE => __log_debug_storage,
        http = HTTP => __log_debug_http,
        startup = STARTUP => __log_debug_startup,
    }
}

/// `trace::auth!(…)`, `trace::business!(…)`, …, `trace::custom!(tag | …)`.
pub mod trace {
    level_module! {
        $ trace, custom = __log_trace_custom,
        auth = AUTH => __log_trace_auth,
        business = BUSINESS => __log_trace_business,
        upstream = UPSTREAM => __log_trace_upstream,
        storage = STORAGE => __log_trace_storage,
        http = HTTP => __log_trace_http,
        startup = STARTUP => __log_trace_startup,
    }
}
