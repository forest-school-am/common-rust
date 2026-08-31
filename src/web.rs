//! The axum policy layer over the protocol client: the router (OIDC callback +
//! login-start + the served shim), the `Principal` extractor, and the
//! `resolve_session` seam it is built on (mechanism vs policy — §5.2), plus the
//! silent-relogin redirect / 401-with-reauth-header decisions. The only module
//! that speaks HTTP; client.rs speaks only OIDC.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use axum::extract::{FromRef, FromRequestParts, Query, State};
use axum::http::{header, request::Parts, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use stand_log::Deployment;
use stand_render::AssetCache;

use crate::client::StandClient;
use crate::config::OidcConfig;
use crate::principal::Principal;
use crate::store::{Session, SessionStore};

const FLOW_COOKIE: &str = "so_flow";
/// Signal header on a 401 telling the served shim to drive silent re-auth.
const REAUTH_HEADER: &str = "X-Stand-OIDC-Reauth";
/// The served shim template — the adopter's build copies this from the crate's
/// `templates/` into their `assets_dir` (§9.8; see README recipe).
const SHIM_TEMPLATE: &str = "stand-oidc.js.jinja";

/// Only same-origin absolute paths are valid post-login redirect targets —
/// never a scheme, host, or protocol-relative `//evil` (open-redirect guard).
fn safe_next(raw: Option<&String>) -> String {
    match raw {
        Some(p) if p.starts_with('/') && !p.starts_with("//") => p.clone(),
        _ => "/".into(),
    }
}

/// Shared OIDC state: embed in your app state (`FromRef`) and pass to
/// [`router`]. Cloning is cheap.
#[derive(Clone)]
pub struct OidcState {
    pub client: Arc<StandClient>,
    pub store: Arc<dyn SessionStore>,
    pub assets: Arc<AssetCache>,
}

impl OidcState {
    /// Run discovery and assemble the state. Validates completely at boot
    /// (§4.3): the §4.4 dev-only refusal, then the asset cache (dir exists,
    /// the shim template parses AND its hash matches the crate version —
    /// §9.6/§9.8), then OIDC discovery (IdP down ⇒ won't start).
    pub async fn discover(
        config: OidcConfig,
        store: impl SessionStore,
    ) -> Result<Self, crate::Error> {
        // §4.4: a dev-only toggle must never be enabled under prod.
        if matches!(config.deployment, Deployment::Prod) && config.danger_accept_invalid_certs {
            return Err(crate::Error::Config(
                "danger_accept_invalid_certs is dev-only and refused under DEPLOYMENT_TYPE=prod"
                    .into(),
            ));
        }
        // §9.6/§9.8: build + version-pin the served shim from the adopter's
        // assets dir. A missing/stale/tampered template refuses to boot here.
        let assets = stand_render::Builder::new(&config.assets_dir)
            .pin(SHIM_TEMPLATE, crate::STAND_OIDC_JS_SHA256)
            .build()
            .map_err(|e| crate::Error::Assets(e.to_string()))?;

        Ok(Self {
            client: Arc::new(StandClient::discover(config).await?),
            store: Arc::new(store),
            assets: Arc::new(assets),
        })
    }

    fn config(&self) -> &OidcConfig {
        self.client.config()
    }

    /// Resolve the caller's session cookie to a live [`Principal`] WITHOUT
    /// the extractor's redirect/401 policy — the reusable seam the
    /// [`Principal`] extractor is built on. Runs userinfo per request (v4,
    /// fail closed); if refresh is enabled and the access token is stale, it
    /// refreshes server-side and retries userinfo once. Returns the (possibly
    /// refreshed) [`Session`] alongside the principal so a caller can reach
    /// the live access token (e.g. a backend proxying mint). Returns `None`
    /// when there is no live session — no cookie, unknown session, or dead
    /// tokens (in which case the dead session is removed from the store).
    ///
    /// Use this directly when you need `Option<Principal>` rather than the
    /// extractor's auto-redirect (dev-stub modes, custom rejection handling,
    /// or session-by-cookie access to the token).
    #[tracing::instrument(skip_all)]
    pub async fn resolve_session(&self, jar: &CookieJar) -> Option<(Principal, Session)> {
        let sid = jar.get(self.config().cookie_name.as_str())?.value().to_owned();
        let session = self.store.get(&sid).await?;

        // v4: userinfo per request, no cache, fail closed.
        if let Ok(p) = self.client.principal_from_access_token(&session.access_token).await {
            return Some((p, session));
        }

        // v5: server-side refresh, then retry userinfo exactly once. A dead
        // SSO session kills the refresh token too (canary-tested), so logout
        // stays instant.
        if let Some(rt) = &session.refresh_token {
            if let Ok(tokens) = self.client.refresh(rt).await {
                let refreshed = Session {
                    access_token: tokens.access_token,
                    refresh_token: tokens.refresh_token,
                    created: session.created,
                };
                self.store.put(sid.clone(), refreshed.clone()).await;
                if let Ok(p) =
                    self.client.principal_from_access_token(&refreshed.access_token).await
                {
                    stand_log::debug!(stand_log::AUTH, "access token refreshed server-side");
                    return Some((p, refreshed));
                }
            }
        }

        // both tokens dead -> the SSO session is gone: destroy local state.
        stand_log::info!(stand_log::AUTH, "session tokens dead — destroying local session");
        self.store.remove(&sid).await;
        None
    }
}

/// authentik's user portal — the ONLY place sessions end (house rule: apps
/// ship no logout). Link your logged-in-as indicator here.
pub fn user_portal_url(config: &OidcConfig) -> String {
    let o = &config.issuer;
    let port = o.port().map(|p| format!(":{p}")).unwrap_or_default();
    format!("{}://{}{}/if/user/", o.scheme(), o.host_str().unwrap_or_default(), port)
}

/// In-flight login state, kept in a short-lived HttpOnly cookie between the
/// authorize redirect and the callback.
#[derive(Serialize, Deserialize)]
struct Flow {
    /// CSRF state
    s: String,
    /// PKCE verifier
    v: String,
    /// where to land after login (path?query)
    n: String,
    /// interactive attempt already made (loop breaker)
    i: bool,
}

// Cookie-safe transport for the JSON flow state (quotes/commas are not
// cookie-value safe).
fn hex_encode(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(s: &str) -> Option<String> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()
}

fn base_cookie<'a>(name: &'a str, value: String, config: &OidcConfig) -> Cookie<'a> {
    let mut c = Cookie::new(name, value);
    c.set_path("/");
    c.set_http_only(true);
    c.set_same_site(SameSite::Lax);
    c.set_secure(config.cookie_secure);
    c
}

fn removal_cookie(name: &str) -> Cookie<'static> {
    let mut c = Cookie::new(name.to_owned(), "");
    c.set_path("/");
    c
}

/// 302 into the authorize endpoint carrying a fresh flow cookie.
fn start_login(oidc: &OidcState, jar: CookieJar, next: String, silent: bool, interactive_tried: bool) -> Response {
    let (url, state, verifier) = oidc.client.authorize_url(silent);
    let flow = Flow { s: state, v: verifier, n: next, i: interactive_tried };
    let jar = jar.add(base_cookie(
        FLOW_COOKIE,
        hex_encode(&serde_json::to_string(&flow).expect("flow serializes")),
        oidc.config(),
    ));
    (jar, Redirect::temporary(url.as_str())).into_response()
}

/// Mount the OIDC routes: the callback (path from `config.redirect_url`), the
/// login-start route (`config.login_path`), and the served `/stand-oidc.js`
/// shim. Merge into your app: `.merge(stand_oidc::router(oidc))`.
pub fn router(state: OidcState) -> Router {
    let callback_path = state.config().redirect_url.path().to_owned();
    let login_path = state.config().login_path.clone();
    Router::new()
        .route(&callback_path, get(callback))
        .route(&login_path, get(login))
        .route("/stand-oidc.js", get(client_js))
        .with_state(state)
}

/// Login-start: begins a silent `prompt=none` auth (the callback escalates to
/// interactive once if the SSO session is dead) and returns to `?next=`.
async fn login(
    State(oidc): State<OidcState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let next = safe_next(params.get("next"));
    // default silent; `?prompt=login` forces interactive
    let silent = params.get("prompt").map(String::as_str) != Some("login");
    start_login(&oidc, jar, next, silent, !silent)
}

/// Serve the browser shim with the login path baked in (no-cache, like the
/// searchbase.js precedent — frontends always load the current contract).
async fn client_js(State(oidc): State<OidcState>) -> Response {
    // Rendered through stand-render (§9): the template is a validated,
    // version-pinned on-disk file; the cache re-renders only when the file or
    // params change. Browser gets no-store so it always sees the current one.
    let login_path = oidc.config().login_path.clone();
    match oidc.assets.render(SHIM_TEMPLATE, &[("login_path", login_path.as_str())]) {
        Ok(js) => (
            [
                (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            js.to_string(),
        )
            .into_response(),
        Err(e) => {
            stand_log::error!(stand_log::UPSTREAM, error = %e, "serving stand-oidc.js failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "shim unavailable").into_response()
        }
    }
}

async fn callback(
    State(oidc): State<OidcState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let Some(flow) = jar
        .get(FLOW_COOKIE)
        .and_then(|c| hex_decode(c.value()))
        .and_then(|json| serde_json::from_str::<Flow>(&json).ok())
    else {
        // stale or forged callback: just start over from the root
        return start_login(&oidc, jar, "/".into(), true, false);
    };

    if let Some(error) = params.get("error") {
        let jar = jar.remove(removal_cookie(FLOW_COOKIE));
        // silent attempt found no live SSO session -> escalate to an
        // interactive login automatically, exactly once
        let needs_interaction =
            matches!(error.as_str(), "login_required" | "interaction_required" | "consent_required");
        if needs_interaction && !flow.i {
            return start_login(&oidc, jar, flow.n, false, true);
        }
        return (StatusCode::UNAUTHORIZED, jar, format!("authentication failed: {error}"))
            .into_response();
    }

    let (Some(code), Some(state)) = (params.get("code"), params.get("state")) else {
        return (StatusCode::BAD_REQUEST, "missing code/state").into_response();
    };
    if *state != flow.s {
        return (StatusCode::BAD_REQUEST, "state mismatch").into_response();
    }

    match oidc.client.exchange_code(code.clone(), flow.v).await {
        Ok(tokens) => {
            let sid = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
            oidc.store
                .put(
                    sid.clone(),
                    Session {
                        access_token: tokens.access_token,
                        refresh_token: tokens.refresh_token,
                        created: SystemTime::now(),
                    },
                )
                .await;
            let jar = jar
                .remove(removal_cookie(FLOW_COOKIE))
                .add(base_cookie(oidc.config().cookie_name.as_str(), sid, oidc.config()).into_owned());
            (jar, Redirect::temporary(&flow.n)).into_response()
        }
        Err(e) => {
            stand_log::warn!(stand_log::AUTH, error = %e, "code exchange failed");
            (
                StatusCode::UNAUTHORIZED,
                jar.remove(removal_cookie(FLOW_COOKIE)),
                format!("authentication failed: {e}"),
            )
                .into_response()
        }
    }
}

/// Rejection of the [`Principal`] extractor: either a redirect into the
/// (silent) login flow for browser navigations, or a plain 401 for API/XHR
/// callers.
pub struct AuthRedirect(Response);

impl IntoResponse for AuthRedirect {
    fn into_response(self) -> Response {
        self.0
    }
}

fn wants_html(parts: &Parts) -> bool {
    parts.method == axum::http::Method::GET
        && parts
            .headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|a| a.contains("text/html"))
}

fn unauthenticated(oidc: &OidcState, parts: &Parts, jar: CookieJar) -> AuthRedirect {
    let jar = jar.remove(removal_cookie(oidc.config().cookie_name.as_str()));
    if wants_html(parts) {
        let next = parts
            .uri
            .path_and_query()
            .map(|pq| pq.as_str().to_owned())
            .unwrap_or_else(|| "/".into());
        // silent first — invisible while the SSO session is alive; the
        // callback escalates to interactive on login_required
        stand_log::debug!(stand_log::AUTH, path = %next, "no session — silent re-auth redirect");
        AuthRedirect(start_login(oidc, jar, next, true, false))
    } else {
        // XHR/fetch: 401 + the signal header so the served shim drives a
        // silent top-level re-auth and retries.
        AuthRedirect(
            (
                StatusCode::UNAUTHORIZED,
                [(REAUTH_HEADER, oidc.config().login_path.as_str())],
                jar,
                "authentication required",
            )
                .into_response(),
        )
    }
}

impl<S> FromRequestParts<S> for Principal
where
    OidcState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AuthRedirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let oidc = OidcState::from_ref(state);
        let jar = CookieJar::from_headers(&parts.headers);
        match oidc.resolve_session(&jar).await {
            Some((principal, _session)) => Ok(principal),
            None => Err(unauthenticated(&oidc, parts, jar)),
        }
    }
}
