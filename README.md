# stand-oidc

One in-app OIDC implementation for every stand backend (DECISIONS.md R1/R2).
Protocol from the [`openidconnect`] crate; this crate adds the stand's policy:
BFF sessions, per-request userinfo (fail closed), silent `prompt=none`
re-login, a downward-closure group gate, and a served browser shim for the
401→re-auth contract. No app login/logout buttons — logout lives only at
authentik.

- **Version:** `0.1.0`
- **Toolchain:** Rust 1.98.0 (workspace standard).

## Depend on it

Declare the **canonical remote** and patch it to the local checkout — so
manifests are already in their final, pushed form. Dependency block in your
crate:

```toml
[dependencies]
stand-oidc = { git = "https://github.com/rebenkoy/stand-oidc", branch = "main" }
```

Patch block at the **workspace ROOT** manifest (cargo never contacts a
fully-patched git source, so the not-yet-real URL is fine):

```toml
[patch."https://github.com/rebenkoy/stand-oidc"]
stand-oidc = { path = "../stand-oidc" }
```

`../stand-oidc` matches the docker build-context layout adopters already
stage. **When the user pushes the repo, un-stubbing is just deleting the patch
block — nothing else changes.**

**nix-build caveat:** while patched to a local `path`, the source is still
outside the consumer's flake tree, so `nix build` (buildRustPackage) can't see
it — same limitation as a bare path dep. Dev-shell and docker builds are
unaffected (that's why deployments work), and `nix build` starts working once
the real remote exists and the patch block is removed (buildRustPackage then
fetches the git dep; add its hash to `cargoLock.outputHashes` — `nix build`
prints the expected hash on first failure).

Footnote — a direct local git dep also works if you don't want a patch block
(`stand-oidc = { git = "file:///mnt/host/workspace/stand-oidc", branch = "main" }`),
and the same `git+file://` URL works as a flake input.

## 1. Backends with browser users (BFF)

```rust
use stand_oidc::{OidcConfig, OidcState, MemoryStore, Principal};
use url::Url;

#[derive(Clone)]
struct AppState { oidc: OidcState /* , … */ }
impl axum::extract::FromRef<AppState> for OidcState {
    fn from_ref(s: &AppState) -> OidcState { s.oidc.clone() }
}

let config = OidcConfig::new(
    Url::parse("https://auth.dev.local/application/o/my-app/")?, // browser issuer
    "my-app",                                                    // public client id (PKCE)
    Url::parse("https://my-app.dev.local/oidc/callback")?,       // registered redirect
);
// server→authentik calls go here if the browser hostname isn't reachable
// from the backend (e.g. inside docker): config.backchannel = Some(...);
// stand self-signed CA on the backchannel: config.danger_accept_invalid_certs = true;

let oidc = OidcState::discover(config, MemoryStore::default()).await?;
let app = axum::Router::new()
    .route("/", axum::routing::get(index))
    .merge(stand_oidc::router(oidc.clone())) // callback + /oidc/login + /stand-oidc.js
    .with_state(AppState { oidc });
```

`stand_oidc::router` mounts three routes: the OIDC **callback** (the path of
your `redirect_url`), the **login-start** route (`/oidc/login`, silent by
default), and the served **`/stand-oidc.js`** shim.

### The Principal extractor

Add `Principal` to any handler; it runs userinfo **per request** (no cache,
fail closed) and, if the access token has expired, drives a silent re-auth
(browser nav → redirect; XHR/fetch → 401 + `X-Stand-OIDC-Reauth`). If refresh
is enabled (Track B) it first tries a server-side refresh, once.

```rust
use stand_oidc::Principal;
use uuid::Uuid;

async fn index(user: Principal) -> String {
    format!("hello {}", user.username) // logged-in-as indicator
}

// downward-closure gate (parents inherit children): 403 on failure
async fn admin(user: Principal) -> Result<String, stand_oidc::GateDenied> {
    let cron_admins: Uuid = "d427f013-3bef-45e7-96aa-32545b58f845".parse().unwrap();
    user.require_group(&cron_admins)?;
    Ok("secret".into())
}
```

`Principal { uuid, username, email, effective_groups }` — `effective_groups`
is the downward closure of group **UUIDs** (never names); gate on UUIDs, which
the stand publishes in `deploy/teststand/state.json`.

`require_group` (above) gates one handler and returns 403 on failure. Most
apps instead gate once in **middleware** with `principal.in_group(&uuid)` —
extract the `Principal`, check `in_group`, and reject the whole route group in
one place rather than per handler. Both read the same downward-closure
`effective_groups`.

### Frontend: the 401 shim

Load the served shim; it wraps `fetch` so an expired session triggers a silent
top-level `prompt=none` bounce and the page lands back where it was. Zero auth
logic in the app.

```html
<script type="module">
  import { installReauthGuard } from "https://my-app.dev.local/stand-oidc.js";
  installReauthGuard(); // auto-runs on import too; call is idempotent
  // now just fetch("/api/…") — 401s with the re-auth signal self-heal
</script>
```

Guards built in: single-flight (the first 401 drives the top-level bounce; any
concurrent 401s see the raw response while the page is already navigating) and
a loop breaker (a bounce won't re-fire within 10s; the server also escalates
to interactive exactly once when the SSO session is truly dead).

## 2. Bearer-API services (the mint pattern)

Services that validate `Authorization: Bearer` callers (no browser session)
use `BearerValidator` — userinfo per request, fail closed, **no OIDC
discovery** (just the userinfo URL), sharing `Principal`'s identity contract:

> **mint is the pending adopter here.** searchbase's mint still has its own
> hand-written twin of this validator: mint is built in a Docker image whose
> context is the searchbase repo, and this crate lives in a separate repo
> outside that context, so mint can't take the dependency until the crate has
> a real (fetchable) remote or is vendored. Port it then.

```rust
use stand_oidc::{BearerValidator, ValidationError};

let validator = BearerValidator::new(reqwest::Client::new(), userinfo_url);
let principal = match validator.validate(bearer).await {
    Ok(p) => p,
    Err(ValidationError::Rejected) => return /* 401 */,
    Err(ValidationError::Upstream(m)) => return /* 502, fail closed */,
};
```

## 3. Refresh tokens — the config flag (default OFF)

Per **DECISIONS.md R2 Track A**, refresh is disabled stand-wide: short access
tokens + silent re-login, no `offline_access` requested anywhere. The default
`OidcConfig` reflects this — it does **not** request `offline_access`, so no
refresh token is issued and the BFF simply re-auths silently on expiry.

Refresh is opt-in for **Track B**, after the authentik fork revokes refresh
tokens at session end and `tests/live_canary.rs` goes green:

```rust
let config = OidcConfig::new(issuer, client_id, redirect_url)
    .request_refresh_tokens(); // adds offline_access; server-side refresh only
```

Until the canary is green, **do not call this** — a logged-out identity could
otherwise be resurrected via a surviving refresh token (that is exactly what
the live canary guards, and why it currently fails on stock authentik).

## Tests

`cargo test` runs unit + mock-authentik integration tests offline. The live
canary (`tests/live_canary.rs`) is gated behind `STAND_LIVE=1` and needs the
teststand up (its `stand-oidc-canary` provider); it is the acceptance test for
Track B — green means refresh tokens die with their session.

See `DESIGN_NOTES.md` for parked (not-yet-built) ideas.
