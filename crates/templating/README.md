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
| `render_ctx(name, &ctx)` | not cached | data-driven pages: a `Serialize` context, `{% extends %}`/`{% include %}` |

`render_ctx` is the §9.4 exemption — per-request domain data (lists, tables,
histories) renders fresh every time, because keying a cache on serialized
per-request data is pure waste. It is also the only entry point that resolves
`{% extends %}` and `{% include %}`, through a path loader over the asset dir.

- **Boot validation (§9.6):** the asset dir must exist and every required
  template must be present and parse — `Builder::build` refuses otherwise. The
  asset dir is a classified config option in the host service.
- **Autoescape (§9.3):** on for `.html`/`.htm`, off for JS/text.
- **Integrity pins (§9.7b / §9.8):** `pin(name, sha256)` verifies a file's
  content hash at boot AND on every reload, refusing to serve on mismatch —
  for logic the server also enforces (dual-use assets) and for library
  templates that must not drift from the crate version. Produce the constant
  with `common_templating::sha256(bytes)`.

## Template invalidation (`TEMPLATE_INVALIDATION`)

`render_ctx` keeps parsed templates in a minijinja environment, so it needs to
be told when one changed on disk. `Builder::invalidation` selects how; unset is
`per-request`, and an unrecognised value refuses to start. **This option governs
`render_ctx` only** — `render` and `static_file` key on mtime and never consult
it.

| value | how |
|---|---|
| `per-request` (default) | clear the environment on every render; the only strategy that cannot silently degrade, at the cost of a reparse per render |
| `dnotify` | per-directory kernel events via raw `fcntl(F_NOTIFY)`; the kernel option that WORKS on this stand's 9p share |
| `inotify` | per-inode kernel events; **does not work on 9p** — the watch succeeds and then stays silent forever |

`common_templating::INVALIDATION_OPTIONS` is the help text for this option as
data (§10.0f) — print it from your `--help` rather than restating the table.

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
