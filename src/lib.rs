//! # common-oidc — the stand's one way to do in-app OIDC (session ruling v5)
//!
//! Every stand app authenticates the same way: in-app OIDC against the shared
//! authentik, browser-facing state limited to ONE HttpOnly session cookie
//! (BFF — tokens never reach the browser), and per-request enforcement.
//!
//! The **protocol** (discovery, PKCE S256 authorization code, token exchange,
//! refresh, userinfo) comes from the `openidconnect` crate — nothing
//! hand-rolled. This crate hand-writes only the **policy**:
//!
//! - **No login/logout buttons.** An unauthenticated browser request is sent
//!   through `prompt=none` silent login automatically; interactive login is
//!   the automatic fallback, exactly once (loop-breaker). There is no logout
//!   route — logout lives solely at authentik (`/if/user/`, see
//!   [`user_portal_url`]); apps observe SSO death via the next request.
//! - **userinfo per request, no cache, fail closed** (ruling v4, unchanged).
//!   The [`Principal`] extractor hits userinfo on every request.
//! - **Instant logout.** authentik revokes access AND refresh tokens when the
//!   SSO session ends, so the very next userinfo fails and the local session
//!   is destroyed. (Version-dependent behavior — the live canary test in
//!   `tests/live_canary.rs` pins it; run it against any new authentik.)
//! - **Frictionless refresh, server-side only.** When userinfo rejects the
//!   access token the backend redeems the refresh token and retries userinfo
//!   ONCE — no browser round-trip. Refresh tokens live in the server-side
//!   session store only.
//!
//! ## Wiring (axum)
//!
//! ```ignore
//! let config = OidcConfig::new(issuer, "my-app", redirect_url);
//! let oidc = OidcState::discover(config, MemoryStore::default()).await?;
//! let app = Router::new()
//!     .route("/", get(index))
//!     .route("/admin", get(admin))
//!     .with_state(AppState { oidc: oidc.clone(), .. })
//!     .merge(common_oidc::router(oidc));  // apply app state before merging
//!
//! // logged-in-as indicator (the extractor also runs userinfo per request)
//! async fn index(p: Principal) -> String { format!("hello {}", p.username) }
//!
//! // a gated handler: `?` on require_group returns 403 (GateDenied)
//! async fn admin(p: Principal) -> Result<String, GateDenied> {
//!     p.require_group(&cron_admins_uuid)?;      // downward-closure gate
//!     Ok("secret".into())
//! }
//! // Or gate in middleware with `p.in_group(&uuid)` — the pattern most apps
//! // use, checking once at the router layer instead of per handler.
//! ```
//!
//! [`router`] also mounts a login-start route (`/oidc/login`, silent by
//! default) and serves the framework-free browser shim at `/common-oidc.js`,
//! which implements the client side of the 401 contract (wrap `fetch`; on a
//! 401 carrying `X-Common-OIDC-Reauth`, bounce top-level through silent
//! re-auth — single-flight + loop-guarded). Frontends load it from the same
//! backend that speaks the contract, so they can never version-skew.
//!
//! Pure API services validating `Authorization: Bearer` callers (the mint
//! pattern) skip the BFF machinery and use [`BearerValidator`] (userinfo per
//! request, fail closed, no discovery).

mod bearer;
mod client;
mod config;
mod error;
mod principal;
mod store;
mod web;

// The served shim's integrity pin, derived from `templates/common-oidc.js.jinja`
// at build time (build.rs, §9.8). Never the template content — just its hash.
include!(concat!(env!("OUT_DIR"), "/shim_hash.rs"));

pub use bearer::{BearerValidator, ValidationError};
pub use client::{OidcClient, TokenBundle};
pub use config::OidcConfig;
pub use error::Error;
pub use principal::{GateDenied, Principal};
// BoxFuture is named in the SessionStore trait signature, so external impls
// need it; AuthRedirect is the Principal extractor's rejection type.
pub use store::{BoxFuture, MemoryStore, Session, SessionStore};
pub use web::{router, user_portal_url, AuthRedirect, OidcState, REAUTH_HEADER};
