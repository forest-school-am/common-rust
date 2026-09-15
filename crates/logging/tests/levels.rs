//! The per-level modules as a consumer sees them: `use common_logging as
//! log;` then `log::<level>::<designator>!(…)` and `log::<level>::custom!(tag
//! | …)`. From OUTSIDE the crate on purpose — the thirty static macros are
//! macro-expanded `macro_export`s, and the path a consumer writes is the one
//! thing worth asserting about them.

use std::io;
use std::sync::{Arc, Mutex};

use common_logging as log;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Clone)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl Buf {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Vec::new())))
    }
    fn lines(&self) -> Vec<serde_json::Value> {
        String::from_utf8(self.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).expect("a json line"))
            .collect()
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

/// Runs `emit` under a trace-level JSON subscriber and returns the lines.
fn capture(emit: impl FnOnce()) -> Vec<serde_json::Value> {
    let buf = Buf::new();
    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::new("trace"))
        .with(
            fmt::layer()
                .json()
                .flatten_event(true)
                .with_writer(buf.clone()),
        );
    tracing::subscriber::with_default(subscriber, emit);
    buf.lines()
}

/// A repo's own tag vocabulary, the way a consumer would declare it: a strum
/// enum plus the three-line `String` conversion that puts it on the custom
/// path.
#[derive(Debug, Clone, Copy, strum::Display)]
enum Tag {
    #[strum(serialize = "scheduler")]
    Scheduler,
    #[strum(serialize = "poller")]
    Poller,
}

impl From<Tag> for String {
    fn from(tag: Tag) -> String {
        tag.to_string()
    }
}

#[test]
fn error_module_emits_each_designator() {
    let lines = capture(|| {
        log::error::auth!(user = "bob", "e-auth");
        log::error::business!("e-business");
        log::error::upstream!("e-upstream");
        log::error::storage!("e-storage");
        log::error::http!("e-http");
        log::error::startup!("e-startup");
    });
    let got: Vec<(String, String)> = lines
        .iter()
        .map(|v| {
            (
                v["designator"].as_str().unwrap().into(),
                v["message"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("auth", "e-auth"),
            ("business", "e-business"),
            ("upstream", "e-upstream"),
            ("storage", "e-storage"),
            ("http", "e-http"),
            ("startup", "e-startup"),
        ]
        .map(|(d, m)| (d.to_string(), m.to_string()))
    );
    assert!(lines.iter().all(|v| v["level"] == "ERROR"));
    assert_eq!(lines[0]["user"], "bob", "fields travel untouched");
}

#[test]
fn warn_module_emits_each_designator() {
    let lines = capture(|| {
        log::warn::auth!("w");
        log::warn::business!("w");
        log::warn::upstream!("w");
        log::warn::storage!("w");
        log::warn::http!("w");
        log::warn::startup!(attempt = 2, "w");
    });
    let got: Vec<&str> = lines
        .iter()
        .map(|v| v["designator"].as_str().unwrap())
        .collect();
    assert_eq!(
        got,
        ["auth", "business", "upstream", "storage", "http", "startup"]
    );
    assert!(lines.iter().all(|v| v["level"] == "WARN"));
    assert_eq!(lines[5]["attempt"], 2);
}

#[test]
fn info_module_emits_each_designator() {
    let lines = capture(|| {
        log::info::auth!("i");
        log::info::business!("i");
        log::info::upstream!("i");
        log::info::storage!("i");
        log::info::http!(status = 200, "i");
        log::info::startup!("i");
    });
    let got: Vec<&str> = lines
        .iter()
        .map(|v| v["designator"].as_str().unwrap())
        .collect();
    assert_eq!(
        got,
        ["auth", "business", "upstream", "storage", "http", "startup"]
    );
    assert!(lines.iter().all(|v| v["level"] == "INFO"));
    assert_eq!(lines[4]["status"], 200);
}

#[test]
fn debug_module_emits_each_designator() {
    let lines = capture(|| {
        log::debug::auth!("d");
        log::debug::business!("d");
        log::debug::upstream!("d");
        log::debug::storage!("d");
        log::debug::http!("d");
        log::debug::startup!("d");
    });
    let got: Vec<&str> = lines
        .iter()
        .map(|v| v["designator"].as_str().unwrap())
        .collect();
    assert_eq!(
        got,
        ["auth", "business", "upstream", "storage", "http", "startup"]
    );
    assert!(lines.iter().all(|v| v["level"] == "DEBUG"));
}

#[test]
fn trace_module_emits_each_designator() {
    let lines = capture(|| {
        log::trace::auth!("t");
        log::trace::business!("t");
        log::trace::upstream!("t");
        log::trace::storage!("t");
        log::trace::http!("t");
        log::trace::startup!("t");
    });
    let got: Vec<&str> = lines
        .iter()
        .map(|v| v["designator"].as_str().unwrap())
        .collect();
    assert_eq!(
        got,
        ["auth", "business", "upstream", "storage", "http", "startup"]
    );
    assert!(lines.iter().all(|v| v["level"] == "TRACE"));
}

#[test]
fn custom_takes_a_path_before_the_bar() {
    let lines = capture(|| {
        log::info::custom!(Tag::Scheduler | run_id = 7, "run started");
        log::debug::custom!(Tag::Poller | "tick");
        log::warn::custom!(log::AUTH | "a stand designator is a path too");
    });
    assert_eq!(lines[0]["designator"], "c-scheduler");
    assert_eq!(lines[0]["run_id"], 7);
    assert_eq!(lines[0]["message"], "run started");
    assert_eq!(lines[0]["level"], "INFO");
    assert_eq!(lines[1]["designator"], "c-poller");
    assert_eq!(lines[1]["level"], "DEBUG");
    assert_eq!(lines[2]["designator"], "auth");
}

#[test]
fn custom_takes_a_literal_before_the_bar() {
    let lines = capture(|| {
        log::error::custom!("logs" | bytes = 12, "unreadable");
        log::trace::custom!("logs" | "fine");
    });
    assert_eq!(lines[0]["designator"], "c-logs");
    assert_eq!(lines[0]["bytes"], 12);
    assert_eq!(lines[0]["level"], "ERROR");
    assert_eq!(lines[1]["designator"], "c-logs");
    assert_eq!(lines[1]["level"], "TRACE");
}

#[test]
fn the_primitive_stays_public_and_takes_a_designator_before_the_bar() {
    let lines = capture(|| {
        log::info!(log::UPSTREAM | "primitive, stand path");
        log::info!("scheduler" | "primitive, literal");
        let chosen = log::Designator::from("poller");
        log::info!(chosen | "primitive, variable");
    });
    let got: Vec<&str> = lines
        .iter()
        .map(|v| v["designator"].as_str().unwrap())
        .collect();
    assert_eq!(got, ["upstream", "c-scheduler", "c-poller"]);
}

#[test]
fn the_target_is_the_callers_module_not_the_level_module() {
    let lines = capture(|| {
        log::info::auth!("from here");
    });
    assert_eq!(
        lines[0]["target"], "levels",
        "a static macro must expand at the call site, or RUST_LOG scoping breaks"
    );
}

/// One release of grace for the comma form; it must still emit, and from a
/// consumer it must warn (the `#[allow]` here is that warning, silenced).
#[test]
#[allow(deprecated)]
fn the_deprecated_aliases_still_emit_from_a_consumer() {
    let lines = capture(|| {
        log::info!(log::AUTH, user = "bob", "old form");
        log::error!(log::custom!("scheduler"), "old custom form");
    });
    assert_eq!(lines[0]["designator"], "auth");
    assert_eq!(lines[0]["user"], "bob");
    assert_eq!(lines[1]["designator"], "c-scheduler");
    assert_eq!(lines[1]["level"], "ERROR");
}
