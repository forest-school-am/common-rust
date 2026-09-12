//! Subscriber setup and the crate's public surface. Assembly only — the
//! pieces live in config.rs, filter.rs, format.rs, designator.rs and span.rs.
//!
//! ```
//! let name = "alice";
//! common_logging::info!(common_logging::AUTH, user = %name, "signed in");
//! common_logging::warn!(common_logging::HTTP, status = 400, "unknown audience");
//! ```
//!
//! ```
//! common_logging::info!(common_logging::custom!("scheduler"), run = 7, "tick");
//! ```

mod config;
mod designator;
mod filter;
mod format;
mod span;

pub use tracing;

pub use config::{Deployment, Format, LogConfig};
pub use designator::{AUTH, BUSINESS, HTTP, STORAGE, UPSTREAM};
pub use filter::Designators;
#[doc(hidden)]
pub use span::__reqid;
pub use span::{gen_reqid, set_actor};

use tracing_subscriber::layer::{Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

pub fn init() {
    let (cfg, complaint) = LogConfig::from_env();
    init_with(cfg);
    if let Some(why) = complaint {
        crate::error!(
            AUTH,
            reason = %why,
            "LOG_DESIGNATORS was not understood — every designator is passing; \
             the filter you set is NOT in effect"
        );
    }
}

pub fn init_with(cfg: LogConfig) {
    let env = EnvFilter::try_new(&cfg.filter).unwrap_or_else(|_| EnvFilter::new("info"));
    let designators = cfg.designators;
    match cfg.format {
        Format::Json => {
            tracing_subscriber::registry()
                .with(
                    fmt::layer()
                        .json()
                        .flatten_event(true)
                        .with_current_span(true)
                        .with_span_list(false)
                        .with_filter(env)
                        .with_filter(designators),
                )
                .try_init()
                .ok();
        }
        Format::Human => {
            tracing_subscriber::registry()
                // Unfiltered: it snapshots span fields for the formatter, so a
                // filter here would strip reqid from lines that are passing.
                .with(format::CaptureLayer)
                .with(
                    fmt::layer()
                        .event_format(format::HumanFormat)
                        .with_filter(env)
                        .with_filter(designators),
                )
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
            let span = crate::request_span!("rq0001");
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
            let span = crate::request_span!("rq0002");
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
    fn json_carries_the_designator_as_a_field_and_the_module_path_as_target() {
        let buf = Buf::new();
        tracing::subscriber::with_default(json(buf.clone()), || {
            let span = crate::request_span!("rq0003");
            let _g = span.enter();
            set_actor(&span, "carol");
            crate::info!(BUSINESS, count = 3, "did a thing");
        });
        let out = buf.string();
        let line = out.lines().next().expect("one json line");
        let v: serde_json::Value = serde_json::from_str(line).expect("valid json line");
        assert_eq!(v["level"], "INFO");
        assert_eq!(v["designator"], "business");
        assert_eq!(
            v["target"], "common_logging::tests",
            "the target is the module path again, which is what makes \
             RUST_LOG work normally"
        );
        assert_eq!(v["message"], "did a thing"); // flatten_event
        assert_eq!(v["count"], 3);
        assert_eq!(v["span"]["reqid"], "rq0003");
        assert_eq!(v["span"]["actor"], "carol");
    }

    #[test]
    fn the_emitted_field_is_the_declared_one() {
        assert_eq!(designator::FIELD, "designator");

        let buf = Buf::new();
        tracing::subscriber::with_default(json(buf.clone()), || {
            crate::warn!(STORAGE, "written");
        });
        let v: serde_json::Value =
            serde_json::from_str(buf.string().lines().next().unwrap()).unwrap();
        assert_eq!(
            v["designator"], "storage",
            "the macro emits a field the readers do not know about"
        );
    }

    #[test]
    fn the_designator_filter_and_rust_log_select_independently() {
        let emit = |env: &str, designators: &str| {
            let buf = Buf::new();
            let subscriber = tracing_subscriber::registry().with(
                fmt::layer()
                    .json()
                    .flatten_event(true)
                    .with_writer(buf.clone())
                    .with_filter(EnvFilter::new(env))
                    .with_filter(Designators::parse(Some(designators)).unwrap()),
            );
            tracing::subscriber::with_default(subscriber, || {
                crate::info!(AUTH, "an auth line");
                crate::info!(BUSINESS, "a business line");
            });
            buf.string()
        };

        let both = emit("trace", "auth=info,business=info");
        assert!(both.contains("an auth line") && both.contains("a business line"));

        let only_auth = emit("trace", "auth=info");
        assert!(only_auth.contains("an auth line"));
        assert!(
            !only_auth.contains("a business line"),
            "the designator axis must be able to restrict on its own"
        );

        let wrong_module = emit("nothing_matches=info", "auth=info");
        assert!(
            wrong_module.is_empty(),
            "RUST_LOG must still be able to exclude on its own: {wrong_module}"
        );
    }

    #[test]
    fn the_designator_prints_once_as_a_column_and_not_also_as_a_field() {
        let buf = Buf::new();
        tracing::subscriber::with_default(human(buf.clone()), || {
            crate::info!(AUTH, user = "bob", "hello");
        });
        let out = buf.string();
        let line = out.trim_end();
        assert_eq!(
            line.matches("auth").count(),
            1,
            "the designator belongs in the column and nowhere else: {line}"
        );
        assert!(
            !line.contains("designator="),
            "the designator must not also print as an event field: {line}"
        );
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
    fn custom_designator_prefixes_c_and_travels_in_the_field() {
        assert_eq!(custom!("scheduler"), "c-scheduler");
        let buf = Buf::new();
        tracing::subscriber::with_default(json(buf.clone()), || {
            crate::info!(custom!("scheduler"), "run started");
        });
        let v: serde_json::Value =
            serde_json::from_str(buf.string().lines().next().unwrap()).unwrap();
        assert_eq!(v["designator"], "c-scheduler");
    }

    #[test]
    fn a_custom_designator_can_be_filtered_on() {
        let buf = Buf::new();
        let subscriber = tracing_subscriber::registry().with(
            fmt::layer()
                .json()
                .flatten_event(true)
                .with_writer(buf.clone())
                .with_filter(Designators::parse(Some("c-scheduler=info")).unwrap()),
        );
        tracing::subscriber::with_default(subscriber, || {
            crate::info!(custom!("scheduler"), "kept");
            crate::info!(AUTH, "dropped");
        });
        let out = buf.string();
        assert!(out.contains("kept"), "{out}");
        assert!(!out.contains("dropped"), "{out}");
    }

    #[test]
    fn a_module_scoped_rust_log_now_selects_this_crates_events() {
        let buf = Buf::new();
        let subscriber = tracing_subscriber::registry().with(
            fmt::layer()
                .json()
                .flatten_event(true)
                .with_writer(buf.clone())
                .with_filter(EnvFilter::new("common_logging=info")),
        );
        tracing::subscriber::with_default(subscriber, || {
            crate::info!(AUTH, "visible by module path");
        });
        assert!(
            buf.string().contains("visible by module path"),
            "a module-scoped RUST_LOG must select events again"
        );
    }
}
