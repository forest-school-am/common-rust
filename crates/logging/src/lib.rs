//! Subscriber setup and the crate's public surface. Assembly only — the
//! pieces live in config.rs, filter.rs, format.rs, designator.rs, macros.rs
//! and span.rs. `Refusal`, `Deployment` and `Format` are common-config's,
//! re-exported here under their old paths.
//!
//! ```
//! use common_logging as log;
//! let name = "alice";
//! log::info::auth!(user = %name, "signed in");
//! log::warn::http!(status = 400, "unknown audience");
//! ```
//!
//! ```
//! use common_logging as log;
//! log::info::custom!("scheduler" | run = 7, "tick");
//! ```

mod config;
mod designator;
mod filter;
mod format;
mod macros;
mod refuse;
mod span;

pub use tracing;

pub use common_config::{Deployment, Format, Refusal};
pub use config::{LogConfig, RUST_LOG_VARIABLE};
pub use designator::{Designator, AUTH, BUSINESS, HTTP, STAND, STARTUP, STORAGE, UPSTREAM};
pub use filter::Designators;
pub use macros::{debug, error, info, trace, warn};
#[doc(hidden)]
pub use refuse::{__exit_refused, __unfiltered};
#[doc(hidden)]
pub use span::__reqid;
pub use span::{gen_reqid, set_actor};

use tracing_subscriber::layer::{Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

/// Refusals raised here carry this crate's target; the binary's own post-load
/// checks go through `refuse!` at their site and carry the binary's.
pub fn boot<T: common_config::Root>() -> T {
    let config = match common_config::load::<T>() {
        Ok(config) => config,
        Err(refusal) => crate::refuse!(refusal),
    };
    let rust_log = std::env::var(RUST_LOG_VARIABLE).ok();
    match LogConfig::from_common(config.common(), rust_log.as_deref()) {
        Ok(cfg) => init_with(cfg),
        Err(refusal) => crate::refuse!(refusal),
    }
    config
}

pub fn init() {
    match LogConfig::from_env() {
        Ok(cfg) => init_with(cfg),
        Err(refusal) => crate::refuse!(refusal),
    }
}

pub fn init_with(cfg: LogConfig) {
    match cfg.env_filter() {
        Ok(env) => install(cfg.format, env, cfg.designators),
        Err(refusal) => crate::refuse!(refusal),
    }
}

pub(crate) fn install(format: Format, env: EnvFilter, designators: Designators) {
    match format {
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
            crate::info!(AUTH | "hello world");
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
            crate::warn!(UPSTREAM | "no user yet");
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
            crate::info!(BUSINESS | count = 3, "did a thing");
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
            crate::warn!(STORAGE | "written");
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
                crate::info!(AUTH | "an auth line");
                crate::info!(BUSINESS | "a business line");
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
            crate::info!(AUTH | user = "bob", "hello");
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
            crate::info!("scheduler" | task = "hello", run_id = 1, "run started");
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
            crate::info!("scheduler" | run_id = 2, "run finished");
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
        let buf = Buf::new();
        tracing::subscriber::with_default(json(buf.clone()), || {
            crate::info!("scheduler" | "run started");
        });
        let v: serde_json::Value =
            serde_json::from_str(buf.string().lines().next().unwrap()).unwrap();
        assert_eq!(v[designator::FIELD], "c-scheduler");
    }

    #[test]
    fn a_designator_held_in_a_variable_goes_through_the_primitive() {
        let buf = Buf::new();
        tracing::subscriber::with_default(json(buf.clone()), || {
            let chosen = Designator::from("poller");
            crate::debug!(chosen | "picked");
        });
        let v: serde_json::Value =
            serde_json::from_str(buf.string().lines().next().unwrap()).unwrap();
        assert_eq!(v["level"], "DEBUG");
        assert_eq!(v[designator::FIELD], "c-poller");
    }

    #[test]
    #[allow(deprecated)]
    fn the_deprecated_first_argument_form_still_emits() {
        assert_eq!(custom!("scheduler").to_string(), "c-scheduler");
        let buf = Buf::new();
        tracing::subscriber::with_default(json(buf.clone()), || {
            crate::info!(AUTH, user = "bob", "old form");
            crate::warn!(custom!("scheduler"), "old custom form");
        });
        let out = buf.string();
        let mut lines = out.lines();
        let v: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(v["level"], "INFO");
        assert_eq!(v[designator::FIELD], "auth");
        assert_eq!(v["user"], "bob");
        assert_eq!(v["message"], "old form");
        let v: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(v["level"], "WARN");
        assert_eq!(v[designator::FIELD], "c-scheduler");
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
            crate::info::custom!("scheduler" | "kept");
            crate::info!(AUTH | "dropped");
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
            crate::info!(AUTH | "visible by module path");
        });
        assert!(
            buf.string().contains("visible by module path"),
            "a module-scoped RUST_LOG must select events again"
        );
    }
}
