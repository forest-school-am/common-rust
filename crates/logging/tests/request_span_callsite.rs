//! The call-site macros, asserted from OUTSIDE common-logging because a test
//! inside it cannot fail. Anything observable from within belongs in src/.

use std::io;
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{fmt, EnvFilter};

mod middleware {
    pub fn open(reqid: &str) -> tracing::Span {
        common_logging::request_span!(reqid)
    }

    pub fn open_owned(reqid: String) -> tracing::Span {
        common_logging::request_span!(&reqid)
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

fn json_subscriber(filter: &str, buf: Buf) -> impl tracing::Subscriber {
    tracing_subscriber::registry()
        .with(EnvFilter::new(filter))
        .with(
            fmt::layer()
                .json()
                .flatten_event(true)
                .with_current_span(true)
                .with_span_list(false)
                .with_writer(buf),
        )
}

fn emit_under(filter: &str) -> String {
    let buf = Buf::new();
    tracing::subscriber::with_default(json_subscriber(filter, buf.clone()), || {
        let span = middleware::open("rq-callsite");
        let _entered = span.enter();
        common_logging::set_actor(&span, "alice");
        common_logging::info!(common_logging::HTTP, "request served");
    });
    buf.string()
}

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

#[test]
fn an_owned_reqid_records_as_a_plain_string() {
    let buf = Buf::new();
    let owned: String = common_logging::gen_reqid();
    tracing::subscriber::with_default(
        json_subscriber("request_span_callsite=debug", buf.clone()),
        || {
            let span = middleware::open_owned(owned.clone());
            let _entered = span.enter();
            common_logging::info!(common_logging::HTTP, "request served");
        },
    );
    let out = buf.string();
    let line = out.lines().next().expect("the event must emit");
    let v: serde_json::Value = serde_json::from_str(line).expect("valid json");
    assert_eq!(
        v["span"]["reqid"], owned,
        "an owned reqid must reach the span as a plain string, not a Debug \
         rendering of one: {line}"
    );
}

#[test]
fn a_refusal_line_carries_the_callers_module_and_the_startup_designator() {
    let buf = Buf::new();
    tracing::subscriber::with_default(
        json_subscriber("request_span_callsite=debug", buf.clone()),
        || {
            common_logging::__refusal_line!(common_logging::Refusal::new(
                "REGISTRY_BIND",
                "not-an-address",
                "a socket address such as \"0.0.0.0:8080\"",
            ));
        },
    );
    let out = buf.string();
    let line = out.lines().next().expect("the refusal must emit");
    let v: serde_json::Value = serde_json::from_str(line).expect("valid json");

    assert_eq!(
        v["target"], "request_span_callsite",
        "a refusal must carry the CALLER's module, or the documented \
         RUST_LOG=my_service=debug filters out the only line a failed boot \
         ever prints: {line}"
    );
    assert_eq!(v["level"], "ERROR");
    assert_eq!(v["designator"], "startup");
    assert_eq!(v["variable"], "REGISTRY_BIND");
    assert_eq!(v["value"], "not-an-address");
    assert!(
        v["accepted"]
            .as_str()
            .is_some_and(|a| a.contains("0.0.0.0:8080")),
        "the accepted form is the operator's only instruction: {line}"
    );
    assert_eq!(v["detail"], "-", "an absent detail renders as - like actor");

    let elsewhere = Buf::new();
    tracing::subscriber::with_default(
        json_subscriber("common_logging=debug", elsewhere.clone()),
        || {
            common_logging::__refusal_line!(common_logging::Refusal::new(
                "REGISTRY_BIND",
                "not-an-address",
                "a socket address",
            ));
        },
    );
    assert!(
        elsewhere.string().is_empty(),
        "a filter naming common-logging must NOT be what surfaces a \
         consumer's refusal; if it does, `refuse!` became a function: {}",
        elsewhere.string()
    );
}
