# common-templating

The Les stand's shared template + static-asset rendering (CODESTYLE.md §9):
minijinja plus a cache over a validated asset directory. Depended on only by
services that actually serve assets — deliberately separate from `common-logging`
(§9.7), which stays dependency-light for every binary.

- **Version:** `0.3.0` · **Toolchain:** Rust 1.98.0.
- Member of the `common-rust` workspace (`crates/templating`), alongside
  `common-logging` and `common-oidc`.

## Depend on it

Consumer manifests declare a git dependency:

```toml
[dependencies]
common-templating = { git = "https://github.com/forest-school-am/common-rust-templating.git", tag = "v0.1.2" }
```

**That URL and tag are documentation, not a pin, and no such remote exists** —
nothing in this fleet is pushed (R21), and `common-rust` has no remote at all.
The tag predates the workspace merge; do not reason about behaviour from it.

What actually resolves the dependency is the single shared cargo patch at
`/mnt/host/workspace/Les/.cargo/config.toml` (R22a/R22b), which redirects the
URL above to this workspace. Cargo walks up from the build directory and MERGES
that file, so it already applies to every repo under `Les/`: there is nothing to
symlink, and no repo may keep a `.cargo/config.toml` of its own. You therefore
always build whatever `common-rust` currently is.

A missing or wrong path in that file does not fail — cargo silently falls back
to the published crate and rewrites your lockfile to say so. Run
`sh stand/check-cargo-patch.sh` if a build behaves oddly.

## Use

```rust
use common_templating::{Builder, sha256};

// boot: validate the dir + required templates, pin server-enforced logic
let cache = Builder::new(&assets_dir)
    .require_template("common-oidc.js.jinja")           // §9.6 present + parses
    .pin("common-oidc.js.jinja", EXPECTED_SHA256)       // §9.8 no drift
    .build()?;                                          // refuses to boot otherwise

// §9.5 template mode — keyed by (mtime, params)
let js = cache.render("common-oidc.js.jinja", &[("login_path", "\"/oidc/login\"")])?;

// §9.5a static mode — keyed by (mtime) alone
let logo = cache.static_file("logo.svg")?;
```

Both modes invalidate on the next request after their key changes, so editing
a served file on disk takes effect without a restart. Wrap the `AssetCache` in
an `Arc` in your `AppState`.

## Modes and rules

| method | key | for |
|---|---|---|
| `render(name, params)` | (mtime, params) | minijinja templates with config baked in |
| `static_file(name)` | (mtime) | completely static served files |

EXACTLY TWO SHAPES, and the crate must not grow a third (§9.4). Templates cannot
reference each other — there is no `extends` or `include` — so one render reads
one file and its own mtime is a complete statement about staleness. A service
needing per-request domain data owns its own engine or does not server-render;
pre-rendering rows to HTML strings in Rust to fit this API is a §9.1 violation
rather than a workaround, and being tempted by it is the signal that the
service needs its own engine.

- **Boot validation (§9.6):** the asset dir must exist and every required
  template must be present and parse — `Builder::build` refuses otherwise. The
  asset dir is a classified config option in the host service.
- **Autoescape (§9.3):** on for `.html`/`.htm`, off for JS/text.
- **Integrity pins (§9.7b / §9.8):** `pin(name, sha256)` verifies a file's
  content hash at boot AND on every reload, refusing to serve on mismatch —
  for logic the server also enforces (dual-use assets) and for library
  templates that must not drift from the crate version. Produce the constant
  with `common_templating::sha256(bytes)`.

## Untrusted names (§9.5b)

Asset names are treated as untrusted input — an adopter that serves a bundle by
URL path passes a request-influenced name straight in. All three name-taking
methods reject absolute paths, `..` segments, and symlinks escaping the root
(canonicalize + containment) before any filesystem access, returning
`RenderError::UnsafeName`. The guard lives in the crate so no adopter has to
remember it.

## Scope (§9.7a)

Served content only. Embedded data never sent to a client (seed data,
fixtures, golden anchors) is out of scope and may stay embedded.

## Test

`cargo test` — substitution + both cache modes + edit-without-restart + boot
refusals (bad dir, missing/unparseable template) + pin match/mismatch/drift +
HTML-escape-vs-JS-raw. No network.
