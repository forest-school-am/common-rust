# common-ui-build

The common-ui pin for a consumer's `build.rs` (R114, "vendored" item). One
line in the app's `Cargo.toml` is the whole pin; this crate turns it into the
verified shell, marker contract and types the build needs, and caches them so
every later build is offline.

- **Version:** `0.3.0` (workspace).
- Member of the `common-rust` workspace (`crates/ui-build`).

## Pin

```toml
[package.metadata.common-ui]
prefix = "common-ui@b9f049d05d44"

[build-dependencies]
common-ui-build = { path = "../common-rust/crates/ui-build" }   # relative to the app
```

A path dependency, unlike the other three stand crates' git+patch shape:
cargo loads a patched git source's original repo to resolve it, and this crate
has no remote (nothing is pushed, R21; the three older URLs exist on GitHub at
pre-merge tags). It becomes a git dependency on push day like the others.
A bump is editing the `prefix` line.

## What a build.rs does

```rust
let pin = common_ui_build::Pin::load().unwrap_or_else(|e| panic!("{e}"));
let html = common_ui_build::stamp(&pin.files.shell, &values);   // (marker, value) pairs
pin.csp();          // the prefix's policy, {{assets_origin}} still in it
pin.sri("base.css"); pin.sri_table(); pin.themes();  // digests from the manifest, never typed
pin.write_dts(&out_dir);       // common-ui.d.ts for the typecheck (tsconfig `paths`)
pin.write_manifest(&out_dir);  // common-ui.manifest.json, for the app's own tests
```

`Pin::load` reads `[package.metadata.common-ui] prefix` from
`$CARGO_MANIFEST_DIR/Cargo.toml`, emits `cargo:rerun-if-changed` for it and
`rerun-if-env-changed` for `ASSETS_ORIGIN` and `LES_CA`, then:

1. serves the pin from `$XDG_CACHE_HOME/common-ui/<prefix>/` (default
   `~/.cache/common-ui/<prefix>/`) when all four files are there, re-verifying
   every digest on read — a tampered cache is a build error naming the
   directory, never a refetch;
2. otherwise fetches `<origin>/<prefix>/manifest.json` — and if that is 404,
   `<origin>/manifest.json` **only when its `prefix` is the pinned one**, else
   fails naming the per-prefix manifest common has to publish — then
   `shell.html`, `shell-markers.json`, `common-ui.d.ts`, verifies each against
   the manifest's sha384, and stores all four in the cache.

Origin: `ASSETS_ORIGIN` (default `https://assets.dev.redaether`). CA: `LES_CA`
(default `/mnt/host/workspace/Les/stand/certs/ca.crt`) as the **only** trust
root. The client is `ureq` 2 over rustls/ring — the provider reqwest's
`rustls-tls` already selects in every consumer.

Trust is the vendor directories' trust: their lock digests were copied from
this manifest over this TLS to this origin. A prefix is content-addressed and
immutable, so its name pins its bytes.

## No marker-set check (R114.2)

There is no build-time check that the stamped page is complete. The app's
boot renders every stamped shell through `common_templating::Shell`, and
upon refuses any unfilled `{{marker}}` by name; a build-time comparison of
the stamped set against `shell-markers.json`'s `build`/`runtime` arrays
duplicated that with a second source of truth. The file is still fetched and
verified — the CSP lives in it — but those two arrays are not parsed.

## Tests

`cargo test -p common-ui-build`: fixtures are the real `common-ui@b9f049d05d44`
files and root manifest; a map-backed `Source` stands in for the origin, so no
test touches the network.
