# AGENTS.md — common-templating

## Purpose
The stand's shared shell stamping + static-asset serving (CODESTYLE.md §9.7):
`upon` fills a built shell's two runtime markers, and an mtime-keyed
`AssetCache` serves files from a validated directory. Used only by services
that serve assets — kept SEPARATE from `common-logging` so binaries that serve
nothing don't pull a template engine.

## Layout
- `src/lib.rs` — `Builder` (boot validation + pins), `AssetCache`
  (`static_file`), `RenderError`, `sha256`, and the cache/path tests.
- `src/render.rs` — `Shell` (compile once, render per request), the total
  `render(shell, config)` convenience, the config-block escaping, and the
  marker constants `ORIGIN_MARKER` / `CONFIG_MARKER`.
- `src/config.rs` — `Config` and `User`, the data block a page reads first
  (R65); ts-rs exports them to `bindings/`.
- `src/assets_origin.rs` — `AssetsOrigin` (§12.6): the `ASSETS_ORIGIN`
  option, its refusal shape, and the CSP layer.
- `tests/consumer.rs` — an asset-serving service in miniature: shell compiled
  at boot, rendered per request, static file, CSP layer.

## Invariants
- THE ENGINE IS `upon` 0.11 WITH ONLY ITS `serde` FEATURE: no filters, no
  functions, no escaping, no custom syntax (R114 item 4). A shell's grammar
  is `{{name}}` and nothing else; the crate must not grow a use for more.
- EXACTLY TWO RUNTIME VALUES, `assets_origin` (raw) and `config` (escaped
  JSON). `render::Values` is the whole context; a third field is a
  shell-contract change first and a crate change second.
- AN UNSTAMPED MARKER IS A RENDER ERROR, NEVER SHIPPED. `{{title}}` left by a
  build.rs is `RenderError::Render` quoting the line
  (`an_unstamped_build_time_marker_is_a_render_error_naming_it`). The total
  `render()` panics on it by design: it is a build defect, not a request
  condition. `Shell::compile` at boot is the shape that refuses early.
- A LONE `}}` FAILS TO COMPILE (`a_lone_close_brace_fails_to_compile`). upon's
  rule, and the reason a stamped shell should be compiled at boot: a
  build-time value containing `}}` breaks the shell there, not per request.
- THE CONFIG BLOCK ESCAPES `<`, `>`, `&` AS `\u00XX`. That is what keeps a
  display name from closing the `<script type="application/json">` it sits
  in, and the page still parses it back unchanged. Kept as-is across the
  engine swap (R114 item 3); the four escaping tests pin it.
- ONE CACHE MODE: `static_file` keyed on (mtime), invalidating on the next
  request after the key changes. Editing a served file on disk MUST take
  effect without a restart. There is no parameterised file rendering any
  more; a service needing per-request domain data owns its own engine.
- Boot validation (§9.6): bad dir / missing or mismatching pinned file refuses
  to boot — never a serve-time surprise.
- Integrity pins (§9.7b/§9.8): a pinned file's sha256 is verified at boot AND
  on every reload; mismatch refuses to serve. Pins are for server-enforced
  logic and library files that must not drift.
- Asset names are UNTRUSTED (§9.5b): `static_file` routes through `safe_path`
  first — rejects absolute paths and any `..`/root/prefix component, then
  canonicalizes and requires containment under the resolved root (catches an
  in-root symlink pointing out). The rejection lives HERE so no adopter
  serving assets by request path can forget it. Negative-space tests cover
  `..`, absolute, and symlink escape.
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
`nix develop --impure -c cargo test -p common-templating` at the WORKSPACE
root (frozen 1.98.0; the flake sets
`CARGO_TARGET_DIR=/home/dev/.cache/common-rust-target` for all three members).
Library only; `cargo build` is the build path (R11(a)).

## Stand context
Implements DECISIONS.md R6 / CODESTYLE.md §9.7–§9.8; engine swap under R114
item 4. Consumers: cron, les-forms, les-registry (shell + origin) and
authentik-role-UI (static files + origin). Builds serialized under R4's disk
regime.
