//! Build script: derives the served shim's integrity pin and publishes the
//! template directory to dependents. Anything needed at RUN time belongs in
//! src/, not here.

use std::path::PathBuf;

use sha2::{Digest, Sha256};

fn main() {
    let template = "templates/common-oidc.js.jinja";
    println!("cargo:rerun-if-changed={template}");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");

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
    let dirt = std::process::Command::new("git")
        .args([
            "-C",
            &std::env::var("CARGO_MANIFEST_DIR").unwrap(),
            "status",
            "--porcelain",
        ])
        .output();
    let state = match dirt {
        Ok(o) if o.status.success() => {
            let files = String::from_utf8_lossy(&o.stdout);
            let n = files.lines().count();
            if n == 0 {
                "Clean".to_owned()
            } else {
                println!(
                    "cargo:warning=common-oidc is being built from a DIRTY working tree \
                     ({n} uncommitted file(s)). The served /common-oidc.js and anything else \
                     this crate ships are therefore unreproducible. Refused under \
                     DEPLOYMENT_TYPE=prod."
                );
                format!("Dirty({n})")
            }
        }
        _ => "Unknown".to_owned(),
    };
    std::fs::write(
        PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("source_state.rs"),
        format!("pub(crate) const CRATE_SOURCE_STATE: &str = {state:?};\n"),
    )
    .unwrap();
}
