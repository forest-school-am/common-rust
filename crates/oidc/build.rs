//! Build script: derives the served shim's integrity pin and publishes the
//! template directory to dependents. Anything needed at RUN time belongs in
//! src/, not here.

use std::path::PathBuf;

use sha2::{Digest, Sha256};

const ARTIFACT_SOURCES: [&str; 4] = ["src", "templates", "Cargo.toml", "build.rs"];

fn main() {
    let template = "templates/common-oidc.js.jinja";
    for path in ARTIFACT_SOURCES {
        println!("cargo:rerun-if-changed={path}");
    }

    let assets = std::fs::canonicalize("templates")
        .unwrap_or_else(|e| panic!("cannot canonicalize templates dir: {e}"));
    println!("cargo:assets={}", assets.display());

    let bytes = std::fs::read(template).unwrap_or_else(|e| panic!("cannot read {template}: {e}"));
    let hash: [u8; 32] = Sha256::digest(&bytes).into();

    let bytes_list = hash
        .iter()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("shim_hash.rs");
    std::fs::write(
        &out,
        format!("pub(crate) const COMMON_OIDC_JS_SHA256: [u8; 32] = [{bytes_list}];\n"),
    )
    .unwrap();
    let source_list = ARTIFACT_SOURCES.map(|p| format!("{p:?}")).join(", ");
    std::fs::write(
        PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("source_state.rs"),
        format!(
            "pub(crate) const CRATE_SOURCE_STATE: SourceState = {};\n\
             pub(crate) const ARTIFACT_SOURCES: &[&str] = &[{source_list}];\n",
            source_state()
        ),
    )
    .unwrap();
}

/// THE SCOPE CANNOT EXCEED THE RERUN TRIGGERS: git state is not a file cargo
/// can watch, so a check over paths cargo is not watching goes stale with no
/// signal. Dirt under `ARTIFACT_SOURCES` is always an mtime change, so this
/// script always re-runs.
///
/// Residue, and it fails closed: committing these files does not change their
/// mtimes, so a recorded `Dirty` survives until something here is next edited.
/// A release build from a fresh checkout records `Clean`.
fn source_state() -> String {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut args = vec![
        "-C",
        &manifest_dir,
        "status",
        "--porcelain",
        "--untracked-files=all",
        "--",
    ];
    args.extend(ARTIFACT_SOURCES);

    let Ok(out) = std::process::Command::new("git").args(&args).output() else {
        return "SourceState::Unknown".to_owned();
    };
    if !out.status.success() {
        return "SourceState::Unknown".to_owned();
    }

    let n = String::from_utf8_lossy(&out.stdout).lines().count();
    if n == 0 {
        return "SourceState::Clean".to_owned();
    }
    println!(
        "cargo:warning=common-oidc is being built from a DIRTY working tree \
         ({n} uncommitted file(s) under {}). The served /common-oidc.js and \
         anything else this crate ships are therefore unreproducible. Refused \
         under DEPLOYMENT_TYPE=prod.",
        ARTIFACT_SOURCES.join(", ")
    );
    format!("SourceState::Dirty {{ uncommitted: {n} }}")
}
