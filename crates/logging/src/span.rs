//! Request-scoped spans: the fields every service attaches to a request
//! and how they are set. Not for spans a single module opens for itself.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use strum::{AsRefStr, EnumString, VariantNames};
use tracing::field::Empty;
use tracing::Span;

use crate::designator::HTTP;

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

pub fn request_span(reqid: &str) -> Span {
    tracing::info_span!(target: HTTP, "request", reqid = reqid, actor = Empty)
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
            let span = request_span("rq0000");
            let meta = span.metadata().expect("a span is enabled under a registry");
            let names: Vec<&str> = meta.fields().iter().map(|f| f.name()).collect();
            assert_eq!(names, FixedField::VARIANTS);
        });
    }
}
