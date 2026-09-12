//! That the refusal line survives both filter axes (§8.3a), driven as a real
//! process because an in-process subscriber is not the thing an operator
//! configures. Anything assertable without spawning belongs in src/.

use std::path::Path;
use std::process::Command;

const PROBE: &str = env!("CARGO_BIN_EXE_refusal_probe");
const ORDINARY: &str = "ordinary startup event";
const REFUSAL: &str = "refusing to start: invalid configuration";

fn probe_is_newer_than_the_code_it_exercises() {
    let probe = Path::new(PROBE)
        .metadata()
        .and_then(|m| m.modified())
        .expect("the probe binary must exist");
    for source in ["src/refuse.rs", "src/bin/refusal_probe.rs"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(source);
        let src = path
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            probe >= src,
            "{source} is newer than the probe binary — the capture would be \
             testing a previous build"
        );
    }
}

fn run(env: &[(&str, &str)]) -> (Vec<serde_json::Value>, Option<i32>) {
    probe_is_newer_than_the_code_it_exercises();
    let mut cmd = Command::new(PROBE);
    for key in ["RUST_LOG", "LOG_DESIGNATORS", "LOG_FORMAT", "DEPLOYMENT_TYPE"] {
        cmd.env_remove(key);
    }
    for (key, value) in env {
        cmd.env(key, value);
    }
    let out = cmd.output().expect("the probe must run");
    let stdout = String::from_utf8(out.stdout).expect("stdout must be utf8");
    let lines = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not one JSON line: {l:?}: {e}")))
        .collect();
    (lines, out.status.code())
}

fn message(line: &serde_json::Value) -> String {
    line["message"].as_str().unwrap_or_default().to_owned()
}

#[test]
fn unfiltered_the_probe_emits_both_lines_in_order() {
    let (lines, code) = run(&[]);
    let messages: Vec<String> = lines.iter().map(message).collect();
    assert_eq!(
        messages,
        vec![ORDINARY.to_owned(), REFUSAL.to_owned()],
        "the emptiness guard for every case below: with nothing filtered the \
         probe must emit the ordinary event AND the refusal, in that order"
    );
    assert_eq!(code, Some(1));
}

#[test]
fn the_refusal_survives_every_adversarial_filter_and_the_ordinary_line_does_not() {
    for env in [
        vec![("RUST_LOG", "some_other=debug")],
        vec![("LOG_DESIGNATORS", "business=info")],
        vec![("LOG_DESIGNATORS", "c-scheduler=debug")],
        vec![
            ("RUST_LOG", "some_other=debug"),
            ("LOG_DESIGNATORS", "business=info"),
        ],
        vec![
            ("RUST_LOG", "some_other=debug"),
            ("LOG_DESIGNATORS", "c-scheduler=debug"),
        ],
    ] {
        let (lines, code) = run(&env);
        assert_eq!(
            lines.len(),
            1,
            "{env:?} must leave exactly the refusal: {:?}",
            lines.iter().map(message).collect::<Vec<_>>()
        );
        let line = &lines[0];
        assert_eq!(message(line), REFUSAL, "{env:?}");
        assert_eq!(line["level"], "ERROR", "{env:?}");
        assert_eq!(line["designator"], "startup", "{env:?}");
        assert_eq!(line["variable"], "PROBE_BIND", "{env:?}");
        assert_eq!(line["value"], "not-an-address", "{env:?}");
        assert_eq!(
            line["accepted"], "a socket address such as \"0.0.0.0:8080\"",
            "{env:?}"
        );
        assert_eq!(line["detail"], "-", "{env:?}");
        assert_eq!(
            line["target"], "refusal_probe::boot",
            "{env:?}: the refusal must carry the CALLER's module, not \
             common_logging's"
        );
        assert!(
            line["timestamp"].is_string(),
            "{env:?}: the line keeps the shape of every other line"
        );
        assert_eq!(code, Some(1), "{env:?}");
    }
}

#[test]
fn an_ordinary_startup_event_is_still_filterable() {
    let (lines, _) = run(&[("LOG_DESIGNATORS", "business=info")]);
    let messages: Vec<String> = lines.iter().map(message).collect();
    assert!(
        !messages.contains(&ORDINARY.to_owned()),
        "the bypass is scoped to the refusal line; an ordinary startup event \
         must stay filterable: {messages:?}"
    );
}

#[test]
fn the_refusal_is_the_last_line_before_the_exit() {
    for env in [
        vec![],
        vec![("RUST_LOG", "some_other=debug")],
        vec![("LOG_DESIGNATORS", "c-scheduler=debug")],
    ] {
        let (lines, code) = run(&env);
        assert_eq!(
            message(lines.last().expect("at least the refusal")),
            REFUSAL,
            "{env:?}: a harness reading the tail must find the refusal there"
        );
        assert_eq!(code, Some(1), "{env:?}");
    }
}
