//! Build script: strips the served shim from TypeScript into the binary.
//! Anything needed at RUN time belongs in src/, not here.

use std::path::PathBuf;

fn main() {
    for path in ["src", "Cargo.toml", "build.rs"] {
        println!("cargo:rerun-if-changed={path}");
    }

    strip_shim();
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
