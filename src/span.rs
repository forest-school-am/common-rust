//! The request root span (CODESTYLE.md §8.2): the one span the HTTP
//! middleware opens per request, carrying `reqid` (unique per request) and
//! `actor` (`-` until auth resolves, then the username). Every event emitted
//! while it is entered inherits these in both output formats. Kept to three
//! tiny helpers so §8.2 ("open the request root span") is one line to obey.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::field::Empty;
use tracing::Span;

use crate::designator::HTTP;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A short, process-unique request id (epoch-nanos low bits + a counter).
/// Not a UUID — a reqid only needs to be distinguishable within a log stream.
pub fn gen_reqid() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:08x}{:04x}", nanos as u32, (n & 0xffff) as u32)
}

/// Open the request root span (target `http`, INFO). `actor` starts empty
/// (renders as `-`); call [`set_actor`] once auth resolves. Enter it for the
/// request's duration (`let _g = span.enter();`) or `.instrument(span)` an
/// async handler so every event carries `reqid`/`actor`.
pub fn request_span(reqid: &str) -> Span {
    tracing::info_span!(target: HTTP, "request", reqid = reqid, actor = Empty)
}

/// Fill in the resolved username on the request span (the identity-resolution
/// point, §5.2). Before this, events show `[-]`.
pub fn set_actor(span: &Span, actor: &str) {
    span.record("actor", actor);
}
