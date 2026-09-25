# common-oidc

One in-app OIDC implementation for every stand backend (DECISIONS.md R1/R2).
Protocol from the [`openidconnect`] crate; this crate adds the stand's policy:
BFF sessions, per-request userinfo (fail closed), silent `prompt=none`
re-login, a downward-closure group gate, and a served browser shim for the
401→re-auth contract. No app login/logout buttons — logout lives only at
authentik.

- **Version:** `0.3.0`
- **Toolchain:** Rust 1.98.0 (workspace standard).
- Member of the `common-rust` workspace (`crates/oidc`); `common-logging` is a
  workspace sibling, not a git dep, as of 0.3.0.
- **Depends on** the stand crate `common-logging` (§8 logging) and nothing else
  of ours: `common-templating` is a consumer-side concern (it renders the
  `PageConfig` this crate defines; this crate never renders anything).

## Depend on it

`common-oidc` is a member of the `common-rust` workspace, so the dependency
points at the repository and cargo selects the member by package name:

```toml
[dependencies]
common-oidc = { git = "https://github.com/forest-school-am/common-rust.git", tag = "v0.3.0" }
```

Stand builds do not go to the network. The single shared cargo patch at
`Les/.cargo/config.toml` redirects this dependency to the local working copy, so
every repo under `Les/` builds against whatever `common-rust` currently is.
Cargo walks up from the build directory and MERGES that file: it applies to
every sibling, there is nothing to symlink, and no repo may keep a
`.cargo/config.toml` of its own.

A missing or wrong path in that file does not fail — cargo silently falls back
to the published crate and rewrites your lockfile to say so. Run
`sh stand/check-cargo-patch.sh` if a build behaves oddly. Note also that `cargo … --locked`
is unusable fleet-wide under this patch (unused-record ordering is
non-deterministic); that is a lock-check failure, not a build failure.

**Migration in progress:** the patch still keys on `common-rust-oidc.git`, a URL
that predates the merge of these crates into one workspace and was never a real
remote. Consumer manifests and the patch move to the repository URL above
together; until they do, a stand manifest keeps the per-crate spelling.

## 1. Backends with browser users (BFF)

```rust
use common_oidc::{OidcConfig, OidcState, MemoryStore, Principal};
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
    "my_app_session",                                            // THIS app's cookie
);
// server→authentik calls go here if the browser hostname isn't reachable
// from the backend (e.g. inside docker): config.backchannel = Some(...);
// stand self-signed CA on the backchannel: config.danger_accept_invalid_certs = true;

let oidc = OidcState::discover(config, MemoryStore::default()).await?;
let app = axum::Router::new()
    .route("/", axum::routing::get(index))
    .merge(common_oidc::router(oidc.clone())) // callback + /oidc/login + /common-oidc.js
    .with_state(AppState { oidc });
```

`common_oidc::router` mounts three routes: the OIDC **callback** (the path of
your `redirect_url`), the **login-start** route (`/oidc/login`, silent by
default), and the served **`/common-oidc.js`** shim.

### Where login state lives

A login in flight — the CSRF `state`, the PKCE verifier, and where to land
afterwards — is held **server-side** in a `FlowStore`. The `oidc_flow` cookie
carries an opaque id and nothing else, so none of those values is ever
something the browser can read or choose.

`OidcState::discover` builds an in-memory flow store for you; there is nothing
to configure. Abandoned logins expire after 10 minutes. Override it only if
logins must survive a restart or be shared across replicas — the callback lands
on whichever instance the browser reaches, and an in-memory flow is invisible
to the others:

```rust
let oidc = OidcState::discover(config, MemoryStore::default())
    .await?
    .with_flow_store(my_shared_flow_store);
```

That is the same constraint your `SessionStore` already has, so a deployment
that has solved it for sessions solves it here the same way.

### The Principal extractor

Add `Principal` to any handler; it runs userinfo **per request** (no cache,
fail closed) and, if the access token has expired, drives a silent re-auth
(browser nav → redirect; XHR/fetch → 401 + `X-Common-OIDC-Reauth`). If refresh
is enabled (Track B) it first tries a server-side refresh, once.

```rust
use common_oidc::Principal;

async fn index(user: Principal) -> String {
    format!("hello {}", user.username) // logged-in-as indicator
}

// downward-closure gate (parents inherit children): 403 on failure
async fn admin(user: Principal) -> Result<String, common_oidc::MissingGroup> {
    user.require_group("cron-admins")?;
    Ok("secret".into())
}
```

`Principal { username, effective_groups }` — `effective_groups` is the
downward closure of group **names** (R123).

`require_group` (above) gates one handler and returns 403 on failure. Most
apps instead gate once with the typed extractors — `Authenticated`,
`GatedBy<P>` over the predicate algebra (`HasGroup`, `And`, `Or`, `Not` in
`src/predicate.rs`) — so a compiling handler is a checked handler. Both read
the same downward-closure `effective_groups`.

`OidcSection` is the `[oidc]` section for a consumer's `#[derive(Config)]`
root: nest it, then `section.to_config(deployment)` yields the `OidcConfig`.

### Frontend: the 401 shim

Load the served shim; it wraps `fetch` so an expired session triggers a silent
top-level `prompt=none` bounce and the page lands back where it was. Zero auth
logic in the app.

```html
<script type="module">
  import { installReauthGuard } from "https://my-app.dev.local/common-oidc.js";
  installReauthGuard(); // auto-runs on import too; call is idempotent
  // now just fetch("/api/…") — 401s with the re-auth signal self-heal
</script>
```

Guards built in: single-flight (the first 401 drives the top-level bounce; any
concurrent 401s see the raw response while the page is already navigating) and
a loop breaker (a bounce won't re-fire within 10s; the server also escalates
to interactive exactly once when the SSO session is truly dead).

### Serving the shim (R64/R65 — nothing to copy)

The shim is **static and lives in the binary**. `build.rs` strips
`src/common-oidc.ts` with esbuild into `OUT_DIR`; the crate exposes it as
`common_oidc::SHIM_JS` and `common_oidc::router` serves it at
`GET /common-oidc.js` with `Cache-Control: public, max-age=31536000,
immutable`. There is no template to copy, no `assets_dir`, and no runtime
integrity pin: compile-time inclusion IS the pin, because a shim inside the
binary cannot drift from the crate that serves it. **Restart is deploy.**

Mount the router and put one tag in your page:

```rust
let app = Router::new()
    .route("/", get(index))
    .merge(common_oidc::router(oidc));
```
```html
<script type="module" src="/common-oidc.js"></script>
```

**The login path is not baked into the shim.** It reads it from the page's
config block, lazily, at the 401 — so a page that never 401s never touches the
block:

```html
<script type="application/json" id="config">{"assetsOrigin":"…","loginPath":"/oidc/login"}</script>
```

which `common_templating::render` writes for you from this crate's
`PageConfig`. `PageConfig::new(&oidc_config, user, launcher_url, assets_origin)`
copies `loginPath` and `logoutPath` off the `OidcConfig` the router was mounted
with, so a page cannot end up unable to bounce or pointing a logout control at
a route that does not exist; `PageConfig::without_oidc` is for a build that
mounts no router at all.

**esbuild is a build dependency of this crate**, so it must be on `PATH`
wherever you build — add `pkgs.esbuild` to your dev shell's `packages` and to
`nativeBuildInputs` if you package the binary with nix.

### Testing a SPA against the shim (`SHIM_TEST_STUB`)

A test runner cannot import the served shim. Two reasons, both structural:
the module **installs its guard at load** — it wraps `window.fetch` for every
later caller, reads a `#config` element a test page does not have, and on a
flagged 401 calls `location.assign`, which jsdom refuses outright — and the
runner resolves neither the bundler's `external` nor the tsconfig `paths`
mapping that make the served specifier `/common-oidc.js` point anywhere.

So the crate ships the stub too, the same way it ships the types: one source,
written out by the consumer, no vendored copy to drift.

```rust
// build.rs, beside the SHIM_DTS you already write
std::fs::write(out_dir.join("common-oidc-stub.ts"), common_oidc::SHIM_TEST_STUB)?;
```

```ts
// vitest.config.ts — the alias key is the exact specifier the generated
// client imports.
resolve: {
  alias: { "/common-oidc.js": resolve(__dirname, "src/test/common-oidc-stub.ts") },
}
```

The stub carries the shim's transport **byte for byte** — a test in this crate
fails if the two drift — so a test still exercises the real query building, the
`FormData` passthrough, the 204 case and the `CallFailure` shape. Only the
guard differs: it is a no-op, and it is not run at load, so `fetch` stays
whatever the test installed.

If you find yourself asserting on re-auth behaviour, do it against the served
shim in a browser test, not here: the stub deliberately has none.

### Emitting the 401 from your own error chokepoint (§3.1)

Canon §3.1 gives a service ONE `AppError` owning every status mapping, so an
adopter renders its own 401 rather than letting the `Principal` extractor
reject. Call the crate instead of rebuilding the contract:

```rust
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            // delegate: header, login path and cookie clearing all come from
            // the crate and follow it through future contract changes
            AppError::Unauthenticated(oidc) => oidc.unauthorized_response(),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found").into_response(),
            // …
        }
    }
}
```

**Do not hand-build the 401 from the header name.** The contract is more than
the header: the response also **clears the session cookie**, and a service that
copies only the header leaves a dead cookie in the browser — the cookie
stalling the session ruling forbids. Anything added to the contract later
lands in this method and adopters inherit it without a code change.

`REAUTH_HEADER` is exported too, but for **assertions** — an e2e check of the
401 contract should reference the const rather than retyping the string, since
it has already been renamed once (`X-Stand-OIDC-Reauth` →
`X-Common-OIDC-Reauth`) and a copied literal fails silently, with re-auth
quietly ceasing to work and no compile error.

**When not to call it:** only where common-oidc is actually wired. Advertising
re-auth while no login route is mounted — a dev-stub auth mode, say — points
the shim at a 404 and loops. The type system already enforces this: an
`OidcState` only exists where the crate is wired, so a dev stub that has none
cannot call the method by construction.

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
use common_oidc::{BearerValidator, ValidationError};

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
let config = OidcConfig::new(issuer, client_id, redirect_url, cookie_name)
    .request_refresh_tokens(); // adds offline_access; server-side refresh only
```

Until the canary is green, **do not call this** — a logged-out identity could
otherwise be resurrected via a surviving refresh token (that is exactly what
the live canary guards, and why it currently fails on stock authentik).

## Tests

`cargo test` runs unit + mock-authentik integration tests offline. The live
canary (`tests/live_canary.rs`) is gated behind `COMMON_OIDC_LIVE=1` and needs the
teststand up (its `common-oidc-canary` provider); it is the acceptance test for
Track B — green means refresh tokens die with their session.

See `DESIGN_NOTES.md` for parked (not-yet-built) ideas.
