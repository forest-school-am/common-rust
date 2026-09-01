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

use common_logging::Deployment;
use common_templating::AssetCache;

use crate::client::OidcClient;
use crate::config::OidcConfig;
use crate::principal::Principal;
use crate::store::{Session, SessionStore};

/// Short-lived cookie carrying the in-flight login's PKCE verifier and state.
/// Private with no config field, so every adopter ships whatever is here.
const FLOW_COOKIE: &str = "oidc_flow";
/// Signal header on a 401 telling the served shim to drive silent re-auth.
///
/// Public so adopters can ASSERT it (e2e checks of the 401 contract) without
/// retyping the string. To BUILD a compliant 401, call
/// [`OidcState::unauthorized_response`] rather than assembling one from this —
/// the contract is more than the header (§11.1).
pub const REAUTH_HEADER: &str = "X-Common-OIDC-Reauth";
/// The served shim template — the adopter's build copies this from the crate's
/// `templates/` into their `assets_dir` (§9.8; see README recipe).
const SHIM_TEMPLATE: &str = "common-oidc.js.jinja";

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
    pub client: Arc<OidcClient>,
    pub store: Arc<dyn SessionStore>,
    pub assets: Arc<AssetCache>,
}

/// §9.8b decision, as a pure function so it is testable without a build-time
/// const or a live IdP (§1.1: logic that needs tests stays free of IO).
///
/// `state` is what build.rs derived about the crate SOURCE this was compiled
/// from: `Clean`, `Dirty(n)`, or `Unknown`.
pub(crate) fn dirty_source_refusal(deployment: Deployment, state: &str) -> Option<String> {
    if matches!(deployment, Deployment::Prod) && state.starts_with("Dirty") {
        return Some(format!(
            "common-oidc was built from a dirty working tree ({state}); the served shim \
             is unreproducible and this is refused under DEPLOYMENT_TYPE=prod (§9.8b). \
             Commit the crate, or build from a clean checkout."
        ));
    }
    None
}

impl OidcState {
    /// Run discovery and assemble the state. Validates completely at boot
    /// (§4.3): the §4.4 dev-only refusal, then the asset cache (dir exists,
    /// the shim template parses AND its hash matches the crate version —
    /// §9.6/§9.8), then OIDC discovery (IdP down ⇒ won't start).
    pub async fn discover(
        config: OidcConfig,
        store: impl SessionStore,
    ) -> Result<Self, crate::OidcError> {
        // §4.4: a dev-only toggle must never be enabled under prod.
        if matches!(config.deployment, Deployment::Prod) && config.danger_accept_invalid_certs {
            return Err(crate::OidcError::Config(
                "danger_accept_invalid_certs is dev-only and refused under DEPLOYMENT_TYPE=prod"
                    .into(),
            ));
        }
        // §9.8b: refuse to boot in PROD if this crate was built from a dirty
        // working tree. Under R11's shared patch, consumers resolve to a local
        // working copy, so an uncommitted template flows through the adopter's
        // build into the served /common-oidc.js and out to browsers — and the
        // §9.8 pin cannot catch it, because the pin and the published assets
        // dir derive from the same directory (§9.8a).
        //
        // Dev warns at build time (cargo:warning) and boots; prod refuses.
        // "Unknown" is deliberately NOT treated as clean — but it is also not
        // fatal, or a vendored source with no git available could never boot.
        if let Some(msg) = dirty_source_refusal(config.deployment, crate::CRATE_SOURCE_STATE) {
            return Err(crate::OidcError::Config(msg));
        }
        if crate::CRATE_SOURCE_STATE == "Unknown" {
            common_logging::warn!(
                common_logging::AUTH,
                "could not determine whether common-oidc was built from a clean tree \
                 (no git, or not a work tree) — this is NOT an assurance that it was"
            );
        }

        // §9.6/§9.8: build + version-pin the served shim from the adopter's
        // assets dir. A missing/stale/tampered template refuses to boot here.
        let assets = common_templating::Builder::new(&config.assets_dir)
            .pin(SHIM_TEMPLATE, crate::COMMON_OIDC_JS_SHA256)
            .build()
            .map_err(|e| crate::OidcError::Assets(e.to_string()))?;

        Ok(Self {
            client: Arc::new(OidcClient::discover(config).await?),
            store: Arc::new(store),
            assets: Arc::new(assets),
        })
    }


    /// The crate's contract-compliant 401 for an API/XHR caller with no live
    /// session — **call this from your §3.1 error chokepoint** instead of
    /// building a 401 yourself.
    ///
    /// It exists to resolve a real tension between two canon rules. §3.1 says
    /// a service has ONE `AppError` owning every status mapping, so the
    /// adopter renders its own 401; §11.1 says shared policy lives in the
    /// crate and is never copied per-repo. Without this method an adopter has
    /// to hardcode the wire contract to satisfy the first rule and thereby
    /// break the second.
    ///
    /// The contract is more than the header name, which is why exporting
    /// [`REAUTH_HEADER`] alone is not enough. This response also **removes the
    /// session cookie** — an adopter that copied just the header would leave a
    /// dead cookie in the browser, which is precisely the cookie-stalling the
    /// session ruling forbids. Anything added to the contract later (extra
    /// headers, a body shape) lands here and adopters inherit it with no code
    /// change.
    ///
    /// The login path comes from `config.login_path`, so a service that moved
    /// its login route stays consistent automatically.
    ///
    /// **When NOT to call it:** only when common-oidc is actually wired.
    /// Advertising re-auth while no login route is mounted (a dev-stub auth
    /// mode, say) points the shim at a 404 and produces a redirect loop. The
    /// type system already guards this — an `OidcState` only exists where the
    /// crate is wired — so a dev stub with no `OidcState` cannot call it by
    /// construction.
    pub fn unauthorized_response(&self) -> Response {
        self.unauthorized_with_jar(CookieJar::new())
    }

    /// One construction site for the 401 (the extractor passes the request's
    /// own jar so its other cookies survive; the public entry point starts
    /// from an empty jar, which emits only the removal).
    fn unauthorized_with_jar(&self, jar: CookieJar) -> Response {
        (
            StatusCode::UNAUTHORIZED,
            [(REAUTH_HEADER, self.config().login_path.as_str())],
            jar.add(expiring_removal(self.config().cookie_name.as_str())),
            "authentication required",
        )
            .into_response()
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
                    common_logging::debug!(common_logging::AUTH, "access token refreshed server-side");
                    return Some((p, refreshed));
                }
            }
        }

        // both tokens dead -> the SSO session is gone: destroy local state.
        common_logging::info!(common_logging::AUTH, "session tokens dead — destroying local session");
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

/// An UNCONDITIONAL clear: a `Set-Cookie` that expires the cookie immediately.
///
/// [`CookieJar::remove`] is not enough on its own — it only emits anything when
/// the ORIGINAL cookie was in the request, so it is silently a no-op on a jar
/// built from nothing. That is fine inside the extractor (the request carried
/// the cookie) but wrong for [`OidcState::unauthorized_response`], which an
/// adopter calls without a jar. Caught by the parity test, not by review.
fn expiring_removal(name: &str) -> Cookie<'static> {
    // `Max-Age=0` is set by parsing rather than `set_max_age`, because the
    // `time` crate is not a direct dependency of this crate and pulling one in
    // for a single zero-duration constant is not worth it. The name comes from
    // config; if it is not a legal cookie name, fall back to the plain form.
    Cookie::parse(format!("{name}=; Path=/; Max-Age=0"))
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| removal_cookie(name))
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
/// login-start route (`config.login_path`), and the served `/common-oidc.js`
/// shim. Merge into your app: `.merge(common_oidc::router(oidc))`.
pub fn router(state: OidcState) -> Router {
    let callback_path = state.config().redirect_url.path().to_owned();
    let login_path = state.config().login_path.clone();
    Router::new()
        .route(&callback_path, get(callback))
        .route(&login_path, get(login))
        .route("/common-oidc.js", get(client_js))
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
    // Rendered through common-templating (§9): the template is a validated,
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
            common_logging::error!(common_logging::UPSTREAM, error = %e, "serving common-oidc.js failed");
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
            common_logging::warn!(common_logging::AUTH, error = %e, "code exchange failed");
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
    if wants_html(parts) {
        let jar = jar.remove(removal_cookie(oidc.config().cookie_name.as_str()));
        let next = parts
            .uri
            .path_and_query()
            .map(|pq| pq.as_str().to_owned())
            .unwrap_or_else(|| "/".into());
        // silent first — invisible while the SSO session is alive; the
        // callback escalates to interactive on login_required
        common_logging::debug!(common_logging::AUTH, path = %next, "no session — silent re-auth redirect");
        AuthRedirect(start_login(oidc, jar, next, true, false))
    } else {
        // XHR/fetch: the crate's one 401 construction (see
        // OidcState::unauthorized_response).
        AuthRedirect(oidc.unauthorized_with_jar(jar))
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

#[cfg(test)]
mod source_state_tests {
    use super::*;

    #[test]
    fn prod_refuses_a_dirty_crate_source_and_dev_does_not() {
        // the case §9.8b exists for: a dirty tree can reach browsers via the
        // served shim, and the §9.8 pin cannot see it (§9.8a)
        assert!(dirty_source_refusal(Deployment::Prod, "Dirty(3)").is_some());
        // dev keeps working — a hard refusal during crate development would be
        // intolerable; the build-time cargo:warning carries dev
        assert!(dirty_source_refusal(Deployment::Dev, "Dirty(3)").is_none());
    }

    #[test]
    fn clean_and_unknown_both_boot_but_mean_different_things() {
        assert!(dirty_source_refusal(Deployment::Prod, "Clean").is_none());
        // "could not determine" must not be fatal (a vendored source with no
        // git would never boot) — but it is warned about at runtime rather
        // than silently treated as clean, which would be a false assurance.
        assert!(dirty_source_refusal(Deployment::Prod, "Unknown").is_none());
    }
}
