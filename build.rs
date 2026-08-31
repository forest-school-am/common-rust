//! Derives the served shim's integrity pin (CODESTYLE.md §9.8) at build time:
//! the sha256 of `templates/common-oidc.js.jinja` is emitted as a crate const,
//! so it can NEVER drift from the template it pins (a hand-maintained const
//! could). Adopters load the template from disk at runtime and the crate
//! refuses to boot if their on-disk copy's hash doesn't match this const.
//!
//! It also publishes the template DIRECTORY to dependents' build scripts via
//! the `links` mechanism: `cargo:assets=<dir>` on a crate declaring
//! `links = "common-oidc"` becomes `DEP_COMMON_OIDC_ASSETS` in every direct
//! dependent's build script. That is what lets adopters copy the shim without a
//! `../common-oidc` sibling path, so the copy recipe works when this crate is a
//! GIT dependency checked out under `~/.cargo/git/checkouts/` (§9.8).
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

    // Published to dependents as DEP_COMMON_OIDC_ASSETS (see module docs).
    // cwd for a build script is the package root, so this resolves inside a
    // cargo git checkout exactly as it does in a sibling working tree.
    let assets = std::fs::canonicalize("templates")
        .unwrap_or_else(|e| panic!("cannot canonicalize templates dir: {e}"));
    println!("cargo:assets={}", assets.display());

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
