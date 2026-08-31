# common-oidc

One in-app OIDC implementation for every stand backend (DECISIONS.md R1/R2).
Protocol from the [`openidconnect`] crate; this crate adds the stand's policy:
BFF sessions, per-request userinfo (fail closed), silent `prompt=none`
re-login, a downward-closure group gate, and a served browser shim for the
401→re-auth contract. No app login/logout buttons — logout lives only at
authentik.

- **Version:** `0.2.0`
- **Toolchain:** Rust 1.98.0 (workspace standard).
- **Depends on** the stand crates `common-logging` (§8 logging) and `common-templating`
  (§9 asset rendering).

> ## ⚠️ Upgrading to 0.2.0 is a BREAKING change with a REQUIRED build step
> 0.2.0 no longer embeds its JS shim — it renders an on-disk template that it
> **hash-verifies at boot**. If you bump to 0.2.0 without doing the assets step
> below, your service **crash-loops on startup** with an integrity-pin error —
> this is a hard **boot failure, not a warning**. You MUST, in the SAME change:
> 1. add the `just assets` copy step and run it (copies the crate's template
>    into your `assets/` — see [the recipe](#serving-the-shim-the-template-copy-recipe-98--required));
> 2. `.gitignore` the copied `assets/common-oidc.js.jinja` so a stale hand-copy
>    can never be committed — the copy is a build artifact, always re-derived;
> 3. set `OidcConfig.assets_dir` and a `deployment` class (`OidcConfig` gained
>    both fields).
> The loud crash is deliberate: it makes a stale shim impossible rather than
> silently serving one that disagrees with the backend's 401 contract.

## Depend on it

Until the repo is pushed, use a **plain path dep** with a NOTE naming the
canonical future remote:

```toml
[dependencies]
# NOTE: canonical remote is https://github.com/rebenkoy/common-oidc — switch to
# a git dep once it is pushed. Path dep until then.
common-oidc = { path = "../common-oidc" }
```

Do **not** use the stub-remote + `[patch]` form before the push: cargo 1.98
contacts the patched-away nonexistent git source whenever the resolver
actually runs (any dependency change), fails on credentials, and poisons the
cached git db (`--offline` then breaks too) — reproduced during R5 dep
removals. The `[patch]` redirect is only viable *after* the remote exists (and
is then unnecessary — just depend on the real git source).

Once pushed, switch to:

```toml
common-oidc = { git = "https://github.com/rebenkoy/common-oidc", tag = "v0.1.1" }
```

**nix-build caveat:** a path dep is outside the consumer's flake source tree,
so `nix build` (buildRustPackage) can't see it — dev-shell and docker builds
are unaffected (that's why deployments work). `nix build` starts working once
the real remote exists and you use the git dep (buildRustPackage fetches it;
add its hash to `cargoLock.outputHashes` — `nix build` prints the expected
hash on first failure).

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

### The Principal extractor

Add `Principal` to any handler; it runs userinfo **per request** (no cache,
fail closed) and, if the access token has expired, drives a silent re-auth
(browser nav → redirect; XHR/fetch → 401 + `X-Common-OIDC-Reauth`). If refresh
is enabled (Track B) it first tries a server-side refresh, once.

```rust
use common_oidc::Principal;
use uuid::Uuid;

async fn index(user: Principal) -> String {
    format!("hello {}", user.username) // logged-in-as indicator
}

// downward-closure gate (parents inherit children): 403 on failure
async fn admin(user: Principal) -> Result<String, common_oidc::GateDenied> {
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
  import { installReauthGuard } from "https://my-app.dev.local/common-oidc.js";
  installReauthGuard(); // auto-runs on import too; call is idempotent
  // now just fetch("/api/…") — 401s with the re-auth signal self-heal
</script>
```

Guards built in: single-flight (the first 401 drives the top-level bounce; any
concurrent 401s see the raw response while the page is already navigating) and
a loop breaker (a bounce won't re-fire within 10s; the server also escalates
to interactive exactly once when the SSO session is truly dead).

### Serving the shim: the template copy recipe (§9.8 — REQUIRED)

The shim is no longer embedded in the crate — it's an on-disk template
(`templates/common-oidc.js.jinja`) rendered through `common-templating`. Your app
serves it from its `assets_dir`, and the crate **refuses to boot** unless the
on-disk copy's hash matches the version this crate was built against (a pin
derived in `build.rs`, so it can never go stale — that `rerun-if-changed` line
is load-bearing).

So each adopter MUST mechanically copy the template into a conventional
`assets/` dir — **never a hand-copy** (a stale hand-copy is exactly the skew
this prevents). Add this to your `justfile`/`Makefile`:

```make
# copy common-oidc's served template into our assets dir (run before build)
assets:
	mkdir -p assets && cp ../common-oidc/templates/common-oidc.js.jinja assets/
```

**`.gitignore` the copied file** (`assets/common-oidc.js.jinja`) — it is a build
artifact re-derived from the crate every time, never edited in place. Committing
it invites exactly the stale hand-copy the boot pin exists to catch. The recipe
copies fresh; git never tracks it.

Then point the config at it:

```rust
let mut config = OidcConfig::new(issuer, client_id, redirect_url);
config.assets_dir = "assets".into();
```

For Docker: run `just assets` before `docker build` so the template lands in
your build context, then `COPY assets/ /app/assets/` in your Dockerfile.

**When you bump the common-oidc dependency, re-run `just assets` in the same
breath.** If you forget, the boot pin fails LOUD and EARLY — the app refuses to
start with an integrity-pin error — rather than silently serving a stale shim
that disagrees with the backend's 401 contract. That loud failure is the
feature, not a bug: it's what makes drift structurally impossible.

> **Forward flag (post-push migration, not yet needed):** the `../common-oidc`
> path only resolves while this crate is a sibling path dep. Once it's pushed
> and adopters switch to a git dep, the crate source lives under
> `~/.cargo/git/checkouts/` and the relative copy path breaks for everyone. The
> cargo-native fix is `links` + `DEP_COMMON_OIDC_ASSETS`: this crate's build
> script emits the template's absolute path, and dependents' build scripts read
> `DEP_COMMON_OIDC_ASSETS` to copy from it — working for path AND git deps. Not
> implemented now (adds build-script complexity, the push hasn't happened); it
> is the known next migration so no one is surprised a second time.

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
let config = OidcConfig::new(issuer, client_id, redirect_url)
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
