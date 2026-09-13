//! Build script: strips the served shim from TypeScript into the binary.
//! Anything needed at RUN time belongs in src/, not here.

use std::path::PathBuf;

const ARTIFACT_SOURCES: [&str; 3] = ["src", "Cargo.toml", "build.rs"];

fn main() {
    for path in ARTIFACT_SOURCES {
        println!("cargo:rerun-if-changed={path}");
    }

    strip_shim();
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

/// The shim ships INSIDE the binary (R64/R65), so it cannot drift from the
/// crate that serves it and needs no runtime integrity pin. `erasableSyntaxOnly`
/// TypeScript means this is a type strip, never a compile: the output is the
/// input minus annotations.
fn strip_shim() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("common-oidc.js");
    let status = std::process::Command::new("esbuild")
        .arg("src/common-oidc.ts")
        .arg("--loader:.ts=ts")
        .arg("--format=esm")
        .arg("--target=firefox128,chrome120,safari17")
        .arg(format!("--outfile={}", out.display()))
        .status()
        .unwrap_or_else(|e| {
            panic!("esbuild is required to build common-oidc (R64) and did not run: {e}")
        });
    if !status.success() {
        panic!("esbuild refused src/common-oidc.ts");
    }
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
