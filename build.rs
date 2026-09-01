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
    // §9.8b needs FRESH dirtiness, and emitting any rerun-if-changed narrows
    // cargo's default "rerun on any package change" to just what is listed.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");

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
    // §9.8b: is the crate source we are being built FROM a dirty working tree?
    //
    // Under R11's shared cargo patch, consumers resolve this crate to a local
    // working copy, so an uncommitted or half-edited template flows through the
    // dependent's build into the served /common-oidc.js and out to BROWSERS.
    // The §9.8 integrity pin cannot catch that (§9.8a): the hash above and the
    // assets dir published to dependents derive from the SAME directory, so the
    // two sides cannot disagree. The pin guards post-build tampering of the
    // deployed asset; it says nothing about which crate source built it.
    //
    // Self-locating, so it costs nothing where it does not apply: as a git or
    // registry dependency this inspects a cargo checkout, which is always
    // clean. It only reports dirt under the patch, which is exactly where the
    // risk lives.
    let dirt = std::process::Command::new("git")
        .args(["-C", &std::env::var("CARGO_MANIFEST_DIR").unwrap(), "status", "--porcelain"])
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
                     DEPLOYMENT_TYPE=prod (§9.8b)."
                );
                format!("Dirty({n})")
            }
        }
        // Fail OPEN, but never silently: "could not determine" must not read as
        // "clean", or the guard becomes a false assurance. No git binary, or not
        // a work tree (a vendored/unpacked source), lands here.
        _ => "Unknown".to_owned(),
    };
    std::fs::write(
        PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("source_state.rs"),
        format!("pub(crate) const CRATE_SOURCE_STATE: &str = {state:?};\n"),
    )
    .unwrap();
}
