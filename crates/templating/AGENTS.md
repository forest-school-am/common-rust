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
  `render(shell, origin, config)` convenience and the config-block escaping.
- `src/assets_origin.rs` — `AssetsOrigin` (§12.6): the `ASSETS_ORIGIN`
  option, its refusal shape, and its substitution into the shell's CSP.
- `tests/consumer.rs` — an asset-serving service in miniature: shell compiled
  at boot, rendered per request, static file, CSP layer.

## Invariants
- THE ENGINE IS `upon` 0.11 WITH ONLY ITS `serde` FEATURE: no filters, no
  functions, no escaping, no custom syntax (R114 item 4). A shell's grammar
  is `{{name}}` and nothing else; the crate must not grow a use for more.
- EXACTLY TWO RUNTIME VALUES, `assets_origin` (raw) and `config` (escaped
  JSON). `render::Values` is the whole context; a third field is a
  shell-contract change first and a crate change second.
- THE CONFIG IS ANY `Serialize` AND THIS CRATE DOES NOT KNOW ITS FIELDS
  (R114 item 5). The block's shape is a contract between its writer
  (`common_oidc::PageConfig`) and its readers in common-ui, pinned there — no
  page-config type, no ts-rs, no `bindings/` here. A config that cannot
  serialise is `RenderError::Render`, not a panic.
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
  shape, one `{{assets_origin}}`. The variable's NAME is
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
  CLASS, dev included (USER RULING). `render` takes `&AssetsOrigin`, not an
  `Option`, so the type makes a shell-rendering service resolve the dev
  `Ok(None)` into a refusal of its own rather than emitting a page that names
  an origin nobody chose. Prod-required stays the PARSE rule — a service that
  serves no shell may still run without one.
- THE CSP IS THE SHELL'S, NOT THIS CRATE'S. `csp()` substitutes the origin
  into the policy the consumer vendored and refuses one with no
  `{{assets_origin}}`: the shell's CSP must allow this origin, and that is the
  whole of what this crate says about it. What the directives should be is
  common-ui's contract; the tests here pin the substitution and the refusal.

## Run / test
`nix develop --impure -c cargo test -p common-templating` at the WORKSPACE
root (frozen 1.98.0; the flake sets
`CARGO_TARGET_DIR=/home/dev/.cache/common-rust-target` for all three members).
Library only; `cargo build` is the build path (R11(a)).

## Stand context
Implements DECISIONS.md R6 / CODESTYLE.md §9.7–§9.8; engine swap under R114
item 4, page config moved out to common-oidc under R114 item 5. Consumers: cron, les-forms, les-registry (shell + origin) and
authentik-role-UI (static files + origin). Builds serialized under R4's disk
regime.
