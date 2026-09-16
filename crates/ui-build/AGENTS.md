# AGENTS.md — common-ui-build

## Purpose
The common-ui pin for consumer build scripts (R114, "vendored" item):
`[package.metadata.common-ui] prefix = "common-ui@<hex>"` in an app's
`Cargo.toml` is the whole pin. The crate fetches the prefix's manifest and
its three build-time files, verifies every byte string against the
manifest's sha384, caches them per prefix for offline builds, and gives a
`build.rs` the stamp / CSP / SRI-table / d.ts pieces the four consumers used
to re-implement over a vendor directory and a lock.

## Layout
- `src/lib.rs` — `read_prefix`, `Manifest`, `Markers` (the csp only),
  `Files`, `Pin` (`load`, `from_parts`, `sri`, `sri_table`, `themes`, `csp`,
  `write_dts`, `write_manifest`), `stamp`, `sha384`, `Error`.
- `src/fetch.rs` — the `Source` trait, the cache (`cache_dir`, `load`), the
  manifest rule (`manifest_for`), `LazyHttps` (ureq + rustls with the stand
  CA as the only root).
- `tests/pin.rs` — everything above over `tests/fixtures/` (the real
  `common-ui@b9f049d05d44` files and root manifest) and a map-backed `Source`.

## Invariants
- ONE LINE IS THE PIN. No lock file, no vendor directory, no revendor script;
  a bump is an edit to `prefix`. The prefix must be `common-ui@<hex>` — it
  names a cache directory and a URL segment.
- EVERY BYTE STRING IS VERIFIED against the manifest, on fetch AND on every
  read from the cache. A mismatch is `Error::Digest`, never a refetch and
  never a warning: the cache is what an offline build trusts.
- THE MANIFEST RULE: `<origin>/<prefix>/manifest.json`; on 404 the root
  manifest stands in ONLY when its `prefix` equals the pinned one, else
  `Error::NoManifest` naming the per-prefix manifest common must publish.
  Delete the fallback once common publishes per-prefix manifests and has
  backfilled b9f049d05d44 (followup in comments-on-currnt-state.md).
- THE CA IS THE ONLY ROOT. `LES_CA` (default the stand CA); no system store,
  no webpki roots. The origin is ours.
- NO NETWORK IN TESTS. `Source` is the seam; tests implement it over a map.
- `stamp` IS PLAIN `String::replace` IN ORDER, nothing escaped — today's
  consumer behaviour, kept byte-identical. Build-time templating on upon is
  a separate followup and lands here when it does.
- NO MARKER-SET CHECK (user, R114.2). The app's boot renders every stamped
  shell through `common_templating::Shell`; upon refuses an unfilled
  `{{marker}}` by name. A build-time comparison against `shell-markers.json`'s
  `build`/`runtime` arrays duplicated that with a second source of truth, so
  those arrays are not parsed — the file is fetched and verified for its csp.
- `sri_table` is every `<prefix>/` entry minus the three embedded files —
  exactly what the retired locks held under `assets` — sorted by name.
  `themes` includes `default`; a consumer that excludes it filters.
- The crate emits `cargo:rerun-if-changed=<Cargo.toml>` and
  `rerun-if-env-changed` for `ASSETS_ORIGIN` / `LES_CA` itself; a consumer
  build.rs adds only its own inputs.
