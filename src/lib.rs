//! # common-logging — shared logging for the Les stand (CODESTYLE.md §8)
//!
//! The single owner of logging mechanics, so nothing drifts per repo. Every
//! Les binary and library depends on it — including `common-oidc`, which logs
//! through here rather than raw `tracing` targets.
//!
//! ## A binary sets up logging in one call
//!
//! ```ignore
//! fn main() {
//!     common_logging::init(); // reads LOG_FORMAT + DEPLOYMENT_TYPE + RUST_LOG
//!     // …
//! }
//! ```
//!
//! ## Emit with a designator (§8.3) as the first argument
//!
//! ```ignore
//! use common_logging::{info, warn, AUTH, UPSTREAM};
//! // tracing idiom: structured fields FIRST, then the message.
//! info!(AUTH, user = %username, "signed in");
//! warn!(UPSTREAM, attempt = n, "retrying");
//! // project-specific designator (README-documented + operator-confirmed):
//! info!(common_logging::custom!("scheduler"), run_id = %id, "run started");
//! ```
//!
//! ## Open the request root span (§8.2) in the HTTP middleware
//!
//! ```ignore
//! let reqid = common_logging::gen_reqid();
//! let span = common_logging::request_span(&reqid);
//! // … once identity resolves:
//! common_logging::set_actor(&span, &username);
//! // run the handler inside the span (enter or .instrument)
//! ```
//!
//! ## Formats (§8.4)
//!
//! `LOG_FORMAT=human` → `timestamp level designator file:row reqid [actor]
//! message`; anything else (default) → JSON, one object per line, via
//! tracing-subscriber's JSON layer. `DEPLOYMENT_TYPE=prod|dev` (default dev)
//! sets the default verbosity (info vs debug) when `RUST_LOG` is unset.

mod config;
mod designator;
mod format;
mod span;

// Re-exported so the emission macros (which expand in the caller's crate) can
// reach tracing without the caller depending on it directly.
pub use tracing;

pub use config::{Deployment, Format, LogConfig};
// designator consts; the macros (custom!, error!/warn!/info!/debug!/trace!)
// are #[macro_export]ed to the crate root.
pub use designator::{AUTH, BUSINESS, HTTP, STORAGE, UPSTREAM};
pub use span::{gen_reqid, request_span, set_actor};

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

/// Initialize the global subscriber from the environment (§8.4/§8.5). Call
/// once at the top of `main`. Idempotent: a second call is a no-op rather
/// than a panic (a subscriber is already installed).
///
/// INFALLIBLE: it cannot fail or panic, so it is safe as the first statement
/// of `main` in a service with a no-panics-on-startup invariant.
///
/// It resolves `DEPLOYMENT_TYPE` LENIENTLY — anything that is not `"prod"`
/// becomes `Dev`, because logging must always come up. **That is not a §4.3
/// gate and calling this does not give you one.** A service that must refuse a
/// set-but-invalid value — and any service whose behaviour differs between
/// classes should — calls [`Deployment::from_env`] itself and handles the
/// `Err`. The two resolutions are deliberately different policies for
/// deliberately different jobs (§4.4y).
pub fn init() {
    init_with(LogConfig::from_env());
}

/// Initialize with an explicit config (tests, or a service that resolves the
/// config itself).
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
            .with(fmt::layer().event_format(format::HumanFormat).with_writer(buf))
    }
    fn json(buf: Buf) -> impl tracing::Subscriber {
        tracing_subscriber::registry().with(EnvFilter::new("trace")).with(
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
        // timestamp level designator file:row reqid [actor] message...
        let p: Vec<&str> = line.splitn(7, ' ').collect();
        assert_eq!(p.len(), 7, "line: {line:?}");
        assert!(p[0].contains('T') && p[0].contains(':'), "timestamp: {}", p[0]);
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
            // no set_actor -> actor is "-"
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
            // the real-world stutter: outer span, instrumented inner span and
            // the event all carrying `task` printed it three times pre-fix
            let outer = tracing::info_span!("cron_run", task = "hello");
            let _o = outer.enter();
            let inner = tracing::info_span!("execute", task = "hello");
            let _i = inner.enter();
            crate::info!(custom!("scheduler"), task = "hello", run_id = 1, "run started");
        });
        let line1 = buf.string();
        let line1 = line1.trim_end();
        assert_eq!(line1.matches("task=").count(), 1, "task must print once: {line1}");
        assert!(line1.contains("run_id=1"), "event fields intact: {line1}");

        // and a span-only field still appends (once) when the event lacks it
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
        assert_eq!(line2.matches("task=").count(), 1, "span field appended once: {line2}");
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
