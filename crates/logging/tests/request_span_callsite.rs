//! That `request_span!` takes the CALLER's module path, asserted from outside
//! common-logging because that is the only place it can be asserted honestly.
//!
//! A unit test inside the crate would see common-logging's module path whether
//! the span came from a macro or a function — which is precisely the bug — so
//! it could pass while the defect was present. This file is a separate crate,
//! so a `RUST_LOG` naming it is exactly the service-scoped filter an operator
//! would set.

use std::io;
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{fmt, EnvFilter};

/// Stands in for a consumer's HTTP middleware: a module that is neither the
/// test root nor common-logging.
mod middleware {
    pub fn open(reqid: &str) -> tracing::Span {
        common_logging::request_span!(reqid)
    }
}

#[derive(Clone)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl Buf {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Vec::new())))
    }
    fn string(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

struct Guard(Arc<Mutex<Vec<u8>>>);

impl io::Write for Guard {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Buf {
    type Writer = Guard;
    fn make_writer(&'a self) -> Guard {
        Guard(self.0.clone())
    }
}

fn emit_under(filter: &str) -> String {
    let buf = Buf::new();
    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::new(filter))
        .with(
            fmt::layer()
                .json()
                .flatten_event(true)
                .with_current_span(true)
                .with_span_list(false)
                .with_writer(buf.clone()),
        );
    tracing::subscriber::with_default(subscriber, || {
        let span = middleware::open("rq-callsite");
        let _entered = span.enter();
        common_logging::set_actor(&span, "alice");
        common_logging::info!(common_logging::HTTP, "request served");
    });
    buf.string()
}

/// THE REGRESSION. This is the filter R28 exists to make work, and it must
/// carry `reqid` with it. Against a `request_span` FUNCTION the span's target
/// is `common_logging::span`, this filter does not enable it, and the event
/// below emits with no span object at all — silently, which is the whole
/// problem.
#[test]
fn a_service_scoped_rust_log_keeps_reqid_and_actor() {
    let out = emit_under("request_span_callsite=debug");
    let line = out.lines().next().expect("the event must emit");
    let v: serde_json::Value = serde_json::from_str(line).expect("valid json");

    assert_eq!(v["message"], "request served");
    assert_eq!(
        v["span"]["reqid"], "rq-callsite",
        "the request span must survive a RUST_LOG scoped to the CALLER's \
         module, or every line loses its correlation handle: {line}"
    );
    assert_eq!(v["span"]["actor"], "alice");
}

/// The span really does carry the caller's module path, not this crate's —
/// stated separately so a future change that reintroduced a fixed target
/// would fail here with a readable reason rather than only as a missing
/// `reqid` two tests over.
#[test]
fn the_span_target_is_the_callers_module_not_common_loggings() {
    let out = emit_under("request_span_callsite::middleware=debug,request_span_callsite=debug");
    assert!(
        out.contains("rq-callsite"),
        "a filter naming the caller's own submodule must enable the span: {out}"
    );

    let elsewhere = emit_under("common_logging=debug");
    assert!(
        !elsewhere.contains("rq-callsite"),
        "a filter naming common-logging must NOT be what enables a consumer's \
         request span; if it does, the target moved back: {elsewhere}"
    );
}
