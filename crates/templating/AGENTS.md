# AGENTS.md — common-templating

## Purpose
The stand's shared template + static-asset rendering (CODESTYLE.md §9.7):
minijinja setup + an mtime/params-keyed `AssetCache` over a validated asset
directory. Used only by services that serve assets — kept SEPARATE from
`common-logging` so binaries that serve nothing don't pull minijinja.

## Layout
- `src/lib.rs` — the whole crate: `Builder` (boot validation + pins),
  `AssetCache` (`render` / `static_file`), `RenderError`, `sha256`, and the
  test suite.

## Invariants
- Two cache modes, both invalidate on the next request after their key
  changes: `render` on (mtime, params), `static_file` on (mtime). Editing a
  served file on disk MUST take effect without a restart.
- EXACTLY TWO entry points and the crate MUST NOT grow a third (§9.4, R30).
  Single-file templates are NOT enforced; hot-reload reloads modified templates
  only. A service needing per-request domain data owns its own engine.
- Boot validation (§9.6): bad dir / missing / unparseable required template
  refuses to boot — never a render-time surprise.
- Integrity pins (§9.7b/§9.8): a pinned file's sha256 is verified at boot AND
  on every reload; mismatch refuses to serve. Pins are for server-enforced
  logic and library templates that must not drift.
- NO escaping (§9.3): string parameters are injected verbatim into every file
  type. `parameters_are_injected_verbatim_whatever_the_file_type` pins it.
- Asset names are UNTRUSTED (§9.5b): every name-taking method (render,
  static_file) routes through `safe_path` first — rejects absolute
  paths and any `..`/root/prefix component, then canonicalizes and requires
  containment under the resolved root (catches an in-root symlink pointing
  out). The rejection lives HERE so no adopter serving assets by request path
  can forget it. Negative-space tests cover `..`, absolute, and symlink escape.
- Scope is SERVED content only (§9.7a); embedded non-served data is fine
  elsewhere.

- THE ASSET ORIGIN IS THIS CRATE'S (§12.6), not each service's: one option
  `ASSETS_ORIGIN` (unprefixed, like the common-logging options), one refusal
  shape, one CSP value, one `{{assets_origin}}`. The variable's NAME is
  exported as `ASSETS_ORIGIN_VARIABLE`, so a consumer adding a flag override
  names it without a second copy to drift; there is no separate "parameter"
  constant, because a lowercase one read as the variable and sent an operator
  to set `assets_origin`. `AssetsOrigin::parse`
  RETURNS a `common_logging::Refusal` rather than refusing itself — the SERVICE
  calls `refuse!`, so the line carries the service's module path (§1.3b).
  Accepts `https://<host>` and nothing else: no path, port, trailing slash,
  credentials, query or fragment, and the `detail` field says which rule the
  value broke. Prod-required: unset is `Ok(None)` under dev and a refusal under
  prod. `csp_layer()` hangs off a PRESENT origin, so a service without one
  cannot half-wire itself.
- A SERVICE THAT RENDERS THE SHELL REQUIRES `ASSETS_ORIGIN` IN EVERY DEPLOYMENT
  CLASS, dev included (USER RULING). `Config::new` takes `&AssetsOrigin`, not
  an `Option`, so the type makes a shell-rendering service resolve the dev
  `Ok(None)` into a refusal of its own rather than emitting a config block that
  names an origin nobody chose. Prod-required stays the PARSE rule — a service
  that serves no shell may still run without one.
- The CSP is §12.2's normative string, asserted literally in both the unit test
  and the served-response test. No `unsafe-*`, and
  `object-src`/`base-uri`/`form-action`/`frame-ancestors` are spelled out
  because they do NOT fall back to `default-src`.
- `frame-ancestors` is `'self'`, NOT `'none'`. `'none'` refuses same-origin
  framing as well as cross-origin, which blanks les-forms' editor preview
  (it frames its own `/render?preview=1`) and breaks cron's 360px harness,
  which measures inside a same-origin iframe because headless Firefox will not
  size a window below ~500px. `'self'` still refuses every cross-origin framer,
  which is the clickjacking threat the directive exists for.
- EACH DIRECTIVE NEEDS A CONSUMER THAT EXERCISES IT, and one that nothing
  exercises is unverified rather than safe. Two defects arrived this way: this
  crate has no page that frames another, so `'none'` passed every test here;
  and common-ui's CSP check served its CSS same-origin, so cross-origin
  `style-src` is still unexercised until les-forms' pages load the theme for
  real. Who exercises what today: `script-src`/`style-src` cross-origin →
  les-forms (pending), `frame-ancestors` same-origin → les-forms' preview and
  cron's harness, `form-action` → les-forms and cron, `connect-src` → the
  picker's remote source. `data:` was dropped from `img-src` under this rule —
  nothing in les-forms, cron or common-ui uses it — and comes back when a page
  ships an inline image.

## Run / test
`nix develop --impure -c cargo test` at the WORKSPACE root (frozen 1.98.0; the
flake sets `CARGO_TARGET_DIR=/home/dev/.cache/common-rust-target` for all three
members). Library only; `cargo build` is the build path (R11(a)).

## Stand context
Implements DECISIONS.md R6 / CODESTYLE.md §9.7–§9.8. First consumers:
common-oidc (its served shim → a pinned library template, §9.8) and mint
(searchbase.js). Builds serialized under R4's disk regime.
