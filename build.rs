//! Derives the served shim's integrity pin (CODESTYLE.md §9.8) at build time:
//! the sha256 of `templates/common-oidc.js.jinja` is emitted as a crate const,
//! so it can NEVER drift from the template it pins (a hand-maintained const
//! could). Adopters load the template from disk at runtime and the crate
//! refuses to boot if their on-disk copy's hash doesn't match this const.
//!
//! The `rerun-if-changed` line is LOAD-BEARING: without it cargo would not
//! re-run this script when only the template changes, the const would keep the
//! OLD hash, and every adopter with the new template would fail the boot pin
//! (or worse, an adopter with a stale template would pass). A pin that goes
//! stale is worse than no pin.

use std::path::PathBuf;

use sha2::{Digest, Sha256};

fn main() {
    let template = "templates/common-oidc.js.jinja";
    println!("cargo:rerun-if-changed={template}");

    let bytes = std::fs::read(template)
        .unwrap_or_else(|e| panic!("cannot read {template}: {e}"));
    let hash: [u8; 32] = Sha256::digest(&bytes).into();

    let bytes_list = hash.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", ");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("shim_hash.rs");
    std::fs::write(
        &out,
        format!("pub(crate) const COMMON_OIDC_JS_SHA256: [u8; 32] = [{bytes_list}];\n"),
    )
    .unwrap();
}
