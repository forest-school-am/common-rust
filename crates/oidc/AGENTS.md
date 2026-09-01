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
- `src/store.rs` — `SessionStore` trait + `MemoryStore`.
- `src/web.rs` — router, `Principal` extractor, and the `resolve_session` seam.
- `src/assets/common-oidc.js` — the served 401→silent-relogin shim.
- `tests/mock_flow.rs` — offline mock-authentik integration tests.
- `tests/live_canary.rs` — §7.4 canary: the instant-logout acceptance test.

## Invariants
- Refresh is DEFAULT OFF (no `offline_access`); opt in only via
  `OidcConfig::request_refresh_tokens()`, and only once the live canary is
  green (DECISIONS.md R2). The canary is the go/no-go — keep it.
- `Principal::from_userinfo` is the ONLY place `sub`/`effective_groups` are
  parsed (UUIDs, fail-closed). Both the BFF and bearer paths route through it.
- `resolve_session` is the mechanism; the extractor is thin policy on top.
- Identity is resolved in exactly one place; gates read the downward-closure
  `effective_groups` by UUID, never names.
- Explicit rustls, default-features off (openssl-free); feature-trimmed deps.

## Run / test
`nix develop --impure -c cargo test` (frozen 1.98.0;
`CARGO_TARGET_DIR=/home/dev/.cache/common-oidc-target`). Library only — nothing
to `nix build`. The live canary is env-gated: `COMMON_OIDC_LIVE=1` with the teststand
up (its `common-oidc-canary` provider).

## Canon compliance (restructure wave — DONE in 0.2.0)
- Logs through `common-logging` (§8.5): designator macros (AUTH events for
  code-exchange/redirect/refresh/session-destroy), resolve_session +
  client.rs async fns instrumented (§8.2).
- Served shim is a `common-templating` on-disk template (§9.2a/§9.7/§9.8):
  `templates/common-oidc.js.jinja`, loaded from `assets_dir`, version-pinned by
  `COMMON_OIDC_JS_SHA256` (derived in build.rs — `rerun-if-changed` is
  load-bearing) and asserted at discover() boot. NO `include_str!`/`.replace`.
  Adopters MUST copy the template into their `assets/` (README recipe).
- §4.4: discover() enforces the deployment class — refuses
  `danger_accept_invalid_certs` under `Deployment::Prod`. Options classified in
  the OidcConfig doc.

## Build note
`build.rs` reads `templates/common-oidc.js.jinja` and emits its sha256 as
`COMMON_OIDC_JS_SHA256` (build-dep sha2). The `cargo:rerun-if-changed` on the
template is LOAD-BEARING — without it a template edit wouldn't refresh the pin.
