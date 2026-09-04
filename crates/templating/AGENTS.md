# AGENTS.md — common-templating

## Purpose
The stand's shared template + static-asset rendering (CODESTYLE.md §9.7):
minijinja setup + an mtime/params-keyed `AssetCache` over a validated asset
directory. Used only by services that serve assets — kept SEPARATE from
`common-logging` so binaries that serve nothing don't pull minijinja.

## Layout
- `src/lib.rs` — `Builder` (boot validation + pins), `AssetCache`
  (`render` / `render_ctx` / `static_file`), `RenderError`, `sha256`, and the
  test suite.
- `src/invalidation.rs` — the `Invalidation` strategies behind
  `TEMPLATE_INVALIDATION`, their `OPTIONS` help text, and the `Watch` that arms
  dnotify/inotify. Anything about NOTICING a change on disk goes here; anything
  about turning a template into bytes stays in lib.rs.

## Invariants
- Two cache modes, both invalidate on the next request after their key
  changes: `render` on (mtime, params), `static_file` on (mtime). Editing a
  served file on disk MUST take effect without a restart.
- A third entry point, `render_ctx`, is NOT cached: it takes a `Serialize`
  context for data-driven pages (§9.4 exempts them) and is the only one that
  resolves `{% extends %}` / `{% include %}`, through a path loader. It decides
  staleness via `TEMPLATE_INVALIDATION` (default `per-request`, unrecognised
  value refuses to start) — that option governs `render_ctx` ALONE; `render`
  and `static_file` key on mtime and never consult it.
- Boot validation (§9.6): bad dir / missing / unparseable required template
  refuses to boot — never a render-time surprise.
- Integrity pins (§9.7b/§9.8): a pinned file's sha256 is verified at boot AND
  on every reload; mismatch refuses to serve. Pins are for server-enforced
  logic and library templates that must not drift.
- Autoescape (§9.3): HTML on, JS/text off — by extension.
- Asset names are UNTRUSTED (§9.5b): every name-taking method (render,
  render_ctx, static_file) routes through `safe_path` first — rejects absolute
  paths and any `..`/root/prefix component, then canonicalizes and requires
  containment under the resolved root (catches an in-root symlink pointing
  out). The rejection lives HERE so no adopter serving assets by request path
  can forget it. Negative-space tests cover `..`, absolute, and symlink escape.
- Scope is SERVED content only (§9.7a); embedded non-served data is fine
  elsewhere.

## Run / test
`nix develop --impure -c cargo test` at the WORKSPACE root (frozen 1.98.0; the
flake sets `CARGO_TARGET_DIR=/home/dev/.cache/common-rust-target` for all three
members). Library only; `cargo build` is the build path (R11(a)).

## Stand context
Implements DECISIONS.md R6 / CODESTYLE.md §9.7–§9.8. First consumers:
common-oidc (its served shim → a pinned library template, §9.8) and mint
(searchbase.js). Builds serialized under R4's disk regime.
