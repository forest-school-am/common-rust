//! Subscriber setup and the crate's public surface. Assembly only — the
//! pieces live in config.rs, format.rs, designator.rs and span.rs.
//!
//! Every emission names a designator as its tracing target, so logs can be
//! filtered by concern rather than by module path:
//!
//! ```
//! let name = "alice";
//! common_logging::info!(common_logging::AUTH, user = %name, "signed in");
//! common_logging::warn!(common_logging::HTTP, status = 400, "unknown audience");
//! ```
//!
//! Structured fields come before the message. A concern outside the shared
//! vocabulary is declared with `custom!`, never invented inline:
//!
//! ```
//! common_logging::info!(common_logging::custom!("scheduler"), run = 7, "tick");
//! ```

mod config;
mod designator;
mod format;
mod span;

pub use tracing;

pub use config::{Deployment, Format, LogConfig};
pub use designator::{AUTH, BUSINESS, HTTP, STORAGE, UPSTREAM};
pub use span::{gen_reqid, request_span, set_actor};

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

pub fn init() {
    init_with(LogConfig::from_env());
}

pub fn init_with(cfg: LogConfig) {
    let filter = EnvFilter::try_new(&cfg.filter).unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::registry().with(filter);
    match cfg.format {
        Format::Json => {
            registry
                .with(
                    fmt::layer()
                        .json()
                        .flatten_event(true)
                        .with_current_span(true)
                        .with_span_list(false),
                )
                .try_init()
                .ok();
        }
        Format::Human => {
            registry
                .with(format::CaptureLayer)
                .with(fmt::layer().event_format(format::HumanFormat))
                .try_init()
                .ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::MakeWriter;

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

    fn human(buf: Buf) -> impl tracing::Subscriber {
        tracing_subscriber::registry()
            .with(EnvFilter::new("trace"))
            .with(format::CaptureLayer)
            .with(
                fmt::layer()
                    .event_format(format::HumanFormat)
                    .with_writer(buf),
            )
    }
    fn json(buf: Buf) -> impl tracing::Subscriber {
        tracing_subscriber::registry()
            .with(EnvFilter::new("trace"))
            .with(
                fmt::layer()
                    .json()
                    .flatten_event(true)
                    .with_current_span(true)
                    .with_span_list(false)
                    .with_writer(buf),
            )
    }

    #[test]
    fn human_layout_exact_shape() {
        let buf = Buf::new();
        tracing::subscriber::with_default(human(buf.clone()), || {
            let span = request_span("rq0001");
            let _g = span.enter();
            set_actor(&span, "bob");
            crate::info!(AUTH, "hello world");
        });
        let out = buf.string();
        let line = out.trim_end();
        let p: Vec<&str> = line.splitn(7, ' ').collect();
        assert_eq!(p.len(), 7, "line: {line:?}");
        assert!(
            p[0].contains('T') && p[0].contains(':'),
            "timestamp: {}",
            p[0]
        );
        assert_eq!(p[1], "INFO");
        assert_eq!(p[2], "auth");
        assert!(p[3].contains(".rs:"), "file:row: {}", p[3]);
        assert_eq!(p[4], "rq0001");
        assert_eq!(p[5], "[bob]");
        assert!(p[6].contains("hello world"), "message: {}", p[6]);
    }

    #[test]
    fn human_actor_dash_before_auth() {
        let buf = Buf::new();
        tracing::subscriber::with_default(human(buf.clone()), || {
            let span = request_span("rq0002");
            let _g = span.enter();
            crate::warn!(UPSTREAM, "no user yet");
        });
        let out = buf.string();
        let line = out.trim_end();
        let p: Vec<&str> = line.splitn(7, ' ').collect();
        assert_eq!(p[1], "WARN");
        assert_eq!(p[2], "upstream");
        assert_eq!(p[4], "rq0002");
        assert_eq!(p[5], "[-]", "actor must be - before auth: {line}");
    }

    #[test]
    fn json_shape_has_level_target_message_and_span() {
        let buf = Buf::new();
        tracing::subscriber::with_default(json(buf.clone()), || {
            let span = request_span("rq0003");
            let _g = span.enter();
            set_actor(&span, "carol");
            crate::info!(BUSINESS, count = 3, "did a thing");
        });
        let out = buf.string();
        let line = out.lines().next().expect("one json line");
        let v: serde_json::Value = serde_json::from_str(line).expect("valid json line");
        assert_eq!(v["level"], "INFO");
        assert_eq!(v["target"], "business");
        assert_eq!(v["message"], "did a thing"); // flatten_event
        assert_eq!(v["count"], 3);
        assert_eq!(v["span"]["reqid"], "rq0003");
        assert_eq!(v["span"]["actor"], "carol");
    }

    #[test]
    fn human_span_fields_dedupe_by_name() {
        let buf = Buf::new();
        tracing::subscriber::with_default(human(buf.clone()), || {
            let outer = tracing::info_span!("cron_run", task = "hello");
            let _o = outer.enter();
            let inner = tracing::info_span!("execute", task = "hello");
            let _i = inner.enter();
            crate::info!(
                custom!("scheduler"),
                task = "hello",
                run_id = 1,
                "run started"
            );
        });
        let line1 = buf.string();
        let line1 = line1.trim_end();
        assert_eq!(
            line1.matches("task=").count(),
            1,
            "task must print once: {line1}"
        );
        assert!(line1.contains("run_id=1"), "event fields intact: {line1}");

        let buf = Buf::new();
        tracing::subscriber::with_default(human(buf.clone()), || {
            let outer = tracing::info_span!("cron_run", task = "hello");
            let _o = outer.enter();
            let inner = tracing::info_span!("execute", task = "hello");
            let _i = inner.enter();
            crate::info!(custom!("scheduler"), run_id = 2, "run finished");
        });
        let line2 = buf.string();
        let line2 = line2.trim_end();
        assert_eq!(
            line2.matches("task=").count(),
            1,
            "span field appended once: {line2}"
        );
    }

    #[test]
    fn custom_designator_prefixes_c_and_sets_target() {
        assert_eq!(custom!("scheduler"), "c-scheduler");
        let buf = Buf::new();
        tracing::subscriber::with_default(json(buf.clone()), || {
            crate::info!(custom!("scheduler"), "run started");
        });
        let v: serde_json::Value =
            serde_json::from_str(buf.string().lines().next().unwrap()).unwrap();
        assert_eq!(v["target"], "c-scheduler");
    }
}
