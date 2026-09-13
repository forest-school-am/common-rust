# AGENTS.md — common-oidc

## Purpose
The stand's ONE in-app OIDC implementation (CODESTYLE.md §5.1). Protocol from
the `openidconnect` crate, hand-written policy on top: BFF sessions,
per-request userinfo (fail closed), silent `prompt=none` relogin, a
downward-closure group gate, and a served browser shim for the 401→re-auth
contract. No login/logout UI — logout lives only at authentik.

## Layout
- `src/lib.rs` — curated `pub use` surface over the private modules below.
- `src/config.rs` — `OidcConfig` (new() + defaults; refresh default-OFF).
- `src/client.rs` — `OidcClient`: discovery, PKCE, exchange, refresh, userinfo.
- `src/bearer.rs` — `BearerValidator` for bearer-API services (no discovery).
- `src/principal.rs` — `Principal` + the single identity-contract enforcer.
- `src/store.rs` — server-side state the browser holds only an id for:
  `SessionStore`/`MemoryStore` for established sessions, `FlowStore`/
  `MemoryFlowStore` for logins in flight.
- `src/web.rs` — router, `Principal` extractor, and the `resolve_session` seam.
- `src/common-oidc.ts` — the served 401→silent-relogin shim. TypeScript in
  erasable syntax; a new browser behaviour goes here, never into a service.
- `build.rs` — strips the shim into `OUT_DIR` with esbuild and records the
  crate's clean/dirty source state.
- `tests/mock_flow.rs` — offline mock-authentik integration tests.
- `tests/live_canary.rs` — §7.4 canary: the instant-logout acceptance test.

## Invariants
- Refresh is DEFAULT OFF (no `offline_access`); opt in only via
  `OidcConfig::request_refresh_tokens()`, and only once the live canary is
  green (DECISIONS.md R2). The canary is the go/no-go — keep it.
- `Principal::from_userinfo` is the ONLY place `sub`/`effective_groups` are
  parsed (UUIDs, fail-closed). Both the BFF and bearer paths route through it.
- `resolve_session` is the mechanism; the extractor is thin policy on top.
- **The browser never holds flow data.** The CSRF `state`, the PKCE verifier
  and the post-login `next` live in the `FlowStore`; the `oidc_flow` cookie
  carries an opaque 256-bit id and nothing else. A `state` the client supplies
  both sides of is not a CSRF control, and a PKCE verifier the client holds
  defeats PKCE — so an id naming no live flow simply restarts login. Do not
  move any of it back into the cookie; if it ever must live client-side it
  needs a signed or encrypted jar, never a plain one.
- Identity is resolved in exactly one place; gates read the downward-closure
  `effective_groups` by UUID, never names.
- Explicit rustls, default-features off (openssl-free); feature-trimmed deps.

## Run / test
`nix develop --impure -c cargo test` at the WORKSPACE root (frozen 1.98.0; the
flake sets `CARGO_TARGET_DIR=/home/dev/.cache/common-rust-target` for all three
members). Library only; `cargo build` is the build path (R11(a)), there is no
`nix build`. The live canary is env-gated: `COMMON_OIDC_LIVE=1` with the teststand
up (its `common-oidc-canary` provider).

## Canon compliance
- Logs through `common-logging` (§8.5): designator macros (AUTH events for
  code-exchange/redirect/refresh/session-destroy). Instrumented (§8.2):
  `resolve_session` and client.rs's `exchange_code` / `refresh` /
  `principal_from_access_token`. NOT instrumented, and known: `discover`
  (client.rs) and `BearerValidator::validate` (bearer.rs).
- THE SHIM IS STATIC AND LIVES IN THE BINARY (R64/R65). `build.rs` strips
  `src/common-oidc.ts` with esbuild into `OUT_DIR`; `SHIM_JS` is an
  `include_str!` of the result; the router serves it at `/common-oidc.js` with
  `immutable` caching. There is no `assets_dir`, no on-disk copy for an adopter
  to make, and no runtime integrity pin: compile-time inclusion IS the pin,
  because a shim inside the binary cannot drift from the crate that serves it
  (§9.8 amended by R65). Restart is deploy.
- THE LOGIN PATH IS NOT IN THE SHIM. It reads
  `JSON.parse(document.getElementById("config").textContent).login_path`
  lazily, at the 401 rather than at import, so a page that never 401s never
  touches the block. `Config.login_path` is therefore mandatory and populated
  from `OidcConfig`, so a service cannot forget it. `serves_shim_and_login_route`
  asserts the path is ABSENT from the served bytes — the old test asserted it
  was baked in, which is the same test inverted.
- ESBUILD IS A BUILD DEPENDENCY OF THIS CRATE, so every consumer needs it on
  PATH. cron and les-forms already carry it; searchbase and role-ui had to add
  it. That is the cost of the shim being TypeScript, and it is worth knowing
  before adding a second TS source here.
- §4.4: discover() enforces the deployment class — refuses
  `danger_accept_invalid_certs` under `Deployment::Prod`, and refuses a shim
  built from a dirty crate tree (§9.8b). The prod-required / dev-only / neutral
  classification TABLE that §4.4 requires beside the config struct does not
  exist yet; `cookie_name` and `request_refresh_tokens` carry prose docs only.

## Build note
`build.rs` runs `esbuild` over `src/common-oidc.ts` into `OUT_DIR`. The strip
is a type strip, never a compile (`erasableSyntaxOnly`): the output is the
input minus annotations. `cargo:rerun-if-changed=src` is what refreshes it.
