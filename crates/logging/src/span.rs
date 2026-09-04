//! Request-scoped spans: the fields every service attaches to a request
//! and how they are set. Not for spans a single module opens for itself.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use strum::{AsRefStr, EnumString, VariantNames};
use tracing::Span;

/// The request-field vocabulary. Each name is written once, in `serialize`;
/// `request_span` declares them and format.rs reads them back through the same
/// declaration, so the two can no longer drift (CODESTYLE 4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, EnumString, VariantNames)]
pub(crate) enum FixedField {
    #[strum(serialize = "reqid")]
    ReqId,
    #[strum(serialize = "actor")]
    Actor,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn gen_reqid() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:08x}{:04x}", nanos as u32, (n & 0xffff) as u32)
}

/// Opens the request root span (§8.2), carrying `reqid` and `actor`.
///
/// A MACRO, NOT A FUNCTION, AND THAT IS LOAD-BEARING (R28). Since designators
/// left `target`, a span's target is its module path — and for a function that
/// is THIS crate's path, not the caller's. `RUST_LOG=my_service=debug` would
/// then fail to enable the span, `enter()` would do nothing, and every event
/// inside would lose `reqid` and `actor` while still emitting: silent, which is
/// the failure R28 exists to abolish. Expanding at the call site gives the span
/// the caller's module path, so a service-scoped `RUST_LOG` covers it.
///
/// `tests/request_span_callsite.rs` asserts this from OUTSIDE this crate; a
/// test in here would carry common-logging's module path either way and so
/// could not fail for the right reason.
#[macro_export]
macro_rules! request_span {
    ($reqid:expr) => {
        $crate::tracing::info_span!(
            "request",
            reqid = $crate::__reqid($reqid),
            actor = $crate::tracing::field::Empty
        )
    };
}

/// Deref-coercion point. A macro has no function boundary to coerce at, so
/// `request_span!(&reqid)` with a `String` would otherwise have to spell the
/// conversion at every call site.
#[doc(hidden)]
pub fn __reqid(reqid: &str) -> &str {
    reqid
}

pub fn set_actor(span: &Span, actor: &str) {
    span.record(FixedField::Actor.as_ref(), actor);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spelled out rather than read off the declaration: a test that reuses it
    /// asserts nothing (CODESTYLE 4.5).
    #[test]
    fn the_vocabulary_spells_the_names_request_span_declares() {
        assert_eq!(FixedField::ReqId.as_ref(), "reqid");
        assert_eq!(FixedField::Actor.as_ref(), "actor");
    }

    #[test]
    fn request_span_declares_exactly_the_vocabulary() {
        tracing::subscriber::with_default(tracing_subscriber::registry(), || {
            let span = crate::request_span!("rq0000");
            let meta = span.metadata().expect("a span is enabled under a registry");
            let names: Vec<&str> = meta.fields().iter().map(|f| f.name()).collect();
            assert_eq!(names, FixedField::VARIANTS);
        });
    }
}
