use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::field::Empty;
use tracing::Span;

use crate::designator::HTTP;

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
    span.record("actor", actor);
}
