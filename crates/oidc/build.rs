//! Build script: derives the served shim's integrity pin and publishes the
//! template directory to dependents. Anything needed at RUN time belongs in
//! src/, not here.

use std::path::PathBuf;

use sha2::{Digest, Sha256};

/// The paths whose content ends up in this crate's compiled artifact and its
/// served template. They are BOTH the rerun triggers below and the scope of
/// the dirty-tree check, and that coupling is the point: cargo re-runs this
/// script exactly when one of them changes, so the recorded state cannot
/// describe a tree the artifact was not built from. Widening the check without
/// widening the triggers reintroduces the staleness (see the note in
/// `source_state()`).
///
/// `build.rs` is in the list because it decides the integrity pin and this
/// very state — a half-edited build script is as unreproducible as a
/// half-edited template. Cargo re-runs a build script when its own source
/// changes regardless of `rerun-if-changed`, so naming it here costs nothing
/// and keeps the check's scope honest.
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
    // The path list is emitted alongside the state so the runtime refusal can
    // NAME the paths that were checked instead of restating them. Restating
    // them is how the message ends up describing a different set from the one
    // examined — which it briefly did, listing three of these four.
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

/// Renders a `SourceState` variant as Rust source. The emitted text is
/// type-checked against the enum in src/source_state.rs when the crate
/// compiles, so a variant renamed there fails the build rather than quietly
/// disabling the prod refusal.
///
/// SCOPE, and it is deliberately narrow: only `ARTIFACT_SOURCES` are examined,
/// not the whole repository. A dirty README or a churning Cargo.lock does not
/// change the bytes this crate ships, and — more importantly — a check wider
/// than the rerun triggers would go stale, because git state is not a file
/// cargo can watch. Scoped this way the two cannot disagree: dirt in these
/// paths IS an mtime change, so it always re-runs this script.
///
/// The one residue, which is fail-closed and therefore acceptable: committing
/// these files does not change their mtimes, so a previously-recorded `Dirty`
/// survives until something here is next edited. That over-refuses a prod boot
/// rather than under-refusing it, and a release build from a fresh checkout
/// records `Clean` correctly.
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
    format!("SourceState::Dirty({n})")
}
