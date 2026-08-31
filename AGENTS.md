# AGENTS.md — stand-oidc

## Purpose
The stand's ONE in-app OIDC implementation (CODESTYLE.md §5.1). Protocol from
the `openidconnect` crate, hand-written policy on top: BFF sessions,
per-request userinfo (fail closed), silent `prompt=none` relogin, a
downward-closure group gate, and a served browser shim for the 401→re-auth
contract. No login/logout UI — logout lives only at authentik.

## Layout
- `src/lib.rs` — curated `pub use` surface over the private modules below.
- `src/config.rs` — `OidcConfig` (new() + defaults; refresh default-OFF).
- `src/client.rs` — `StandClient`: discovery, PKCE, exchange, refresh, userinfo.
- `src/bearer.rs` — `BearerValidator` for bearer-API services (no discovery).
- `src/principal.rs` — `Principal` + the single identity-contract enforcer.
- `src/store.rs` — `SessionStore` trait + `MemoryStore`.
- `src/web.rs` — router, `Principal` extractor, and the `resolve_session` seam.
- `src/assets/stand-oidc.js` — the served 401→silent-relogin shim.
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
`CARGO_TARGET_DIR=/home/dev/.cache/stand-oidc-target`). Library only — nothing
to `nix build`. The live canary is env-gated: `STAND_LIVE=1` with the teststand
up (its `stand-oidc-canary` provider).

## Stand context / open migrations (restructure wave)
- Logs through `stand-log` (§8.5) — MIGRATION PENDING (currently one raw
  `tracing::warn`); instrument resolve_session/refresh/userinfo per §8.2 and
  emit AUTH-designator events for 401/redirect/gate decisions.
- The served shim will move to a `stand-render` template (§9.2a/§9.7/§9.8):
  no `include_str!`/`.replace`; template loaded from a validated `assets_dir`
  with a boot version-assertion against the crate's expected template.
- §4.4: discover() will take the deployment class and refuse
  `danger_accept_invalid_certs` under prod.
