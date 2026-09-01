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

const FLOW_COOKIE: &str = "oidc_flow";
pub const REAUTH_HEADER: &str = "X-Common-OIDC-Reauth";
const SHIM_TEMPLATE: &str = "common-oidc.js.jinja";

/// Only same-origin absolute paths are valid post-login redirect targets —
/// never a scheme, host, or protocol-relative `//evil` (open-redirect guard).
fn safe_next(raw: Option<&String>) -> String {
    match raw {
        Some(p) if p.starts_with('/') && !p.starts_with("//") => p.clone(),
        _ => "/".into(),
    }
}

#[derive(Clone)]
pub struct OidcState {
    pub client: Arc<OidcClient>,
    pub store: Arc<dyn SessionStore>,
    pub assets: Arc<AssetCache>,
}

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
    pub async fn discover(
        config: OidcConfig,
        store: impl SessionStore,
    ) -> Result<Self, crate::OidcError> {
        if matches!(config.deployment, Deployment::Prod) && config.danger_accept_invalid_certs {
            return Err(crate::OidcError::Config(
                "danger_accept_invalid_certs is dev-only and refused under DEPLOYMENT_TYPE=prod"
                    .into(),
            ));
        }
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


    pub fn unauthorized_response(&self) -> Response {
        self.unauthorized_with_jar(CookieJar::new())
    }

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

    #[tracing::instrument(skip_all)]
    pub async fn resolve_session(&self, jar: &CookieJar) -> Option<(Principal, Session)> {
        let sid = jar.get(self.config().cookie_name.as_str())?.value().to_owned();
        let session = self.store.get(&sid).await?;

        if let Ok(p) = self.client.principal_from_access_token(&session.access_token).await {
            return Some((p, session));
        }

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

        common_logging::info!(common_logging::AUTH, "session tokens dead — destroying local session");
        self.store.remove(&sid).await;
        None
    }
}

pub fn user_portal_url(config: &OidcConfig) -> String {
    let o = &config.issuer;
    let port = o.port().map(|p| format!(":{p}")).unwrap_or_default();
    format!("{}://{}{}/if/user/", o.scheme(), o.host_str().unwrap_or_default(), port)
}

#[derive(Serialize, Deserialize)]
struct Flow {
    s: String,
    v: String,
    n: String,
    i: bool,
}

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

/// [`CookieJar::remove`] is not enough on its own — it only emits anything when
/// the ORIGINAL cookie was in the request, so it is silently a no-op on a jar
/// built from nothing. That is fine inside the extractor (the request carried
/// the cookie) but wrong for [`OidcState::unauthorized_response`], which an
/// adopter calls without a jar. Caught by the parity test, not by review.
fn expiring_removal(name: &str) -> Cookie<'static> {
    Cookie::parse(format!("{name}=; Path=/; Max-Age=0"))
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| removal_cookie(name))
}

fn removal_cookie(name: &str) -> Cookie<'static> {
    let mut c = Cookie::new(name.to_owned(), "");
    c.set_path("/");
    c
}

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

pub fn router(state: OidcState) -> Router {
    let callback_path = state.config().redirect_url.path().to_owned();
    let login_path = state.config().login_path.clone();
    Router::new()
        .route(&callback_path, get(callback))
        .route(&login_path, get(login))
        .route("/common-oidc.js", get(client_js))
        .with_state(state)
}

async fn login(
    State(oidc): State<OidcState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let next = safe_next(params.get("next"));
    let silent = params.get("prompt").map(String::as_str) != Some("login");
    start_login(&oidc, jar, next, silent, !silent)
}

async fn client_js(State(oidc): State<OidcState>) -> Response {
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
        return start_login(&oidc, jar, "/".into(), true, false);
    };

    if let Some(error) = params.get("error") {
        let jar = jar.remove(removal_cookie(FLOW_COOKIE));
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
        common_logging::debug!(common_logging::AUTH, path = %next, "no session — silent re-auth redirect");
        AuthRedirect(start_login(oidc, jar, next, true, false))
    } else {
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
        assert!(dirty_source_refusal(Deployment::Prod, "Dirty(3)").is_some());
        assert!(dirty_source_refusal(Deployment::Dev, "Dirty(3)").is_none());
    }

    #[test]
    fn clean_and_unknown_both_boot_but_mean_different_things() {
        assert!(dirty_source_refusal(Deployment::Prod, "Clean").is_none());
        assert!(dirty_source_refusal(Deployment::Prod, "Unknown").is_none());
    }
}
