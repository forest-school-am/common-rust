//! The axum layer: routes, extractors, cookies, redirects, and the policy
//! decisions that shape a response. Anything that talks to the IdP belongs
//! in client.rs; anything a bearer-only API needs belongs in bearer.rs.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use axum::extract::{FromRef, FromRequestParts, Query, State};
use axum::http::HeaderMap;
use axum::http::{header, request::Parts, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use url::Url;
use uuid::Uuid;

use common_logging as log;
use common_logging::Deployment;

use crate::client::OidcClient;
use crate::config::OidcConfig;
use crate::error::Upstream;
use crate::principal::Principal;
use crate::retry;
use crate::store::{FlowState, FlowStore, MemoryFlowStore, Session, SessionStore};

const FLOW_COOKIE: &str = "oidc_flow";
pub const REAUTH_HEADER: &str = "X-Common-OIDC-Reauth";
fn safe_next(raw: Option<&str>) -> String {
    fn resolve(raw: &str) -> Option<String> {
        if !raw.starts_with('/') {
            return None;
        }
        let base = Url::parse("https://next.invalid/").ok()?;
        let resolved = base.join(raw).ok()?;
        if resolved.origin() != base.origin() {
            return None;
        }
        let mut out = resolved.path().to_owned();
        if let Some(query) = resolved.query() {
            out.push('?');
            out.push_str(query);
        }
        if let Some(fragment) = resolved.fragment() {
            out.push('#');
            out.push_str(fragment);
        }
        (out.starts_with('/') && !out.starts_with("//")).then_some(out)
    }
    raw.and_then(resolve).unwrap_or_else(|| "/".to_owned())
}

#[derive(Clone)]
pub struct OidcState {
    pub client: Arc<OidcClient>,
    pub store: Arc<dyn SessionStore>,
    pub flows: Arc<dyn FlowStore>,
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

        Ok(Self {
            client: Arc::new(OidcClient::discover(config).await?),
            store: Arc::new(store),
            flows: Arc::new(MemoryFlowStore::default()),
        })
    }

    /// The default store is per-process, so a callback landing on a different
    /// replica than the login sees no flow. Replacing it is how a multi-replica
    /// or restart-surviving deployment fixes that; nothing here detects it.
    pub fn with_flow_store(mut self, flows: impl FlowStore) -> Self {
        self.flows = Arc::new(flows);
        self
    }

    pub fn unauthorized_response(&self) -> Response {
        self.unauthorized_with_jar(CookieJar::new())
    }

    /// The response for a browser with no valid session: a silent re-auth
    /// redirect back to the path it was reaching. The `auth` module's
    /// extractors call this for an anonymous HTML request, since only the
    /// [`OidcState`] holds the flow store the redirect needs.
    pub async fn login_redirect(&self, parts: &Parts) -> Response {
        unauthenticated(self, parts, CookieJar::from_headers(&parts.headers))
            .await
            .into_response()
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

    /// The config the router was mounted with; `PageConfig::new` reads the
    /// login and logout paths off it per request.
    pub fn config(&self) -> &OidcConfig {
        self.client.config()
    }

    #[tracing::instrument(skip_all)]
    pub async fn resolve_session(&self, jar: &CookieJar) -> Option<(Principal, Session)> {
        let sid = jar
            .get(self.config().cookie_name.as_str())?
            .value()
            .to_owned();
        let session = self.store.get(&sid).await?;

        let deadline = retry::Deadline::starting_now();

        let rejected = match retry::within(&deadline, || {
            self.client
                .principal_from_access_token(&session.access_token)
        })
        .await
        {
            Ok(p) => return Some((p, session)),
            Err(Upstream::Unreachable(why)) => {
                log::warn::auth!(
                    reason = %why,
                    "identity unresolvable within the upstream budget — ending session"
                );
                self.store.remove(&sid).await;
                return None;
            }
            Err(Upstream::Rejected(why)) => why,
        };

        if let Some(rt) = &session.refresh_token {
            if let Ok(tokens) = retry::within(&deadline, || self.client.refresh(rt)).await {
                let refreshed = Session {
                    access_token: tokens.access_token,
                    refresh_token: tokens.refresh_token,
                    created: session.created,
                };
                self.store.put(sid.clone(), refreshed.clone()).await;
                if let Ok(p) = retry::within(&deadline, || {
                    self.client
                        .principal_from_access_token(&refreshed.access_token)
                })
                .await
                {
                    log::debug::auth!("access token refreshed server-side");
                    return Some((p, refreshed));
                }
            }
        }

        log::info::auth!(
            reason = %rejected,
            "session tokens rejected by the IdP — destroying local session"
        );
        self.store.remove(&sid).await;
        None
    }
}

pub fn user_portal_url(config: &OidcConfig) -> String {
    let o = &config.issuer;
    let port = o.port().map(|p| format!(":{p}")).unwrap_or_default();
    format!(
        "{}://{}{}/if/user/",
        o.scheme(),
        o.host_str().unwrap_or_default(),
        port
    )
}

fn base_cookie<'a>(name: &'a str, value: String, config: &OidcConfig) -> Cookie<'a> {
    let mut c = Cookie::new(name, value);
    c.set_path("/");
    c.set_http_only(true);
    c.set_same_site(SameSite::Lax);
    c.set_secure(config.cookie_secure);
    c
}

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

fn new_flow_id() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

async fn start_login(
    oidc: &OidcState,
    jar: CookieJar,
    next: String,
    silent: bool,
    interactive_tried: bool,
) -> Response {
    let auth = oidc.client.authorize_url(silent);
    let id = new_flow_id();
    oidc.flows
        .put(
            id.clone(),
            FlowState {
                state: auth.csrf_state,
                verifier: auth.pkce_verifier,
                next,
                interactive_tried,
                created: SystemTime::now(),
            },
        )
        .await;
    let jar = jar.add(base_cookie(FLOW_COOKIE, id, oidc.config()));
    (jar, Redirect::temporary(auth.url.as_str())).into_response()
}

pub fn router(state: OidcState) -> Router {
    let callback_path = state.config().redirect_url.path().to_owned();
    let login_path = state.config().login_path.clone();
    let logout_path = state.config().logout_path.clone();
    Router::new()
        .route(&callback_path, get(callback))
        .route(&login_path, get(login))
        .route(&logout_path, post(logout))
        .route("/common-oidc.js", get(client_js))
        .with_state(state)
}

/// A POST, never a GET: a GET would let any page anywhere log a user out with
/// an `<img src>`. There is no CSRF token in the form (R65.4 leaves that
/// open), so the request has to prove it came from this site some other way.
///
/// UNENFORCED PRECONDITION: this relies on the browser's `Sec-Fetch-Site` or
/// `Origin`. A proxy that strips both makes every logout a 403 — visibly, not
/// silently, which is the right way round for a security check.
async fn logout(State(oidc): State<OidcState>, jar: CookieJar, headers: HeaderMap) -> Response {
    if !from_this_site(&headers, oidc.config()) {
        log::warn::auth!("refused a logout that did not prove it came from this site");
        return StatusCode::FORBIDDEN.into_response();
    }

    // Server-side FIRST. Clearing only the cookie would leave a live session
    // for anyone who kept the value.
    if let Some(sid) = jar
        .get(oidc.config().cookie_name.as_str())
        .map(|c| c.value().to_owned())
    {
        oidc.store.remove(&sid).await;
    }

    let jar = jar.remove(expiring_removal(oidc.config().cookie_name.as_str()));

    /* R79, the user's words: "Logout should just send to authentik logout." So
    the browser goes to `end_session_endpoint` PLAIN — no id_token_hint, no
    post_logout_redirect_uri, nothing registered on the provider. Authentik's
    own page is the end of the trip, and without a hint that page asks the
    user to confirm, which is the intended shape rather than a shortcoming.

    The app session is already destroyed above, so a user who abandons the
    trip is still logged out HERE. 303 either way: the browser must turn the
    POST into a GET, and a 307 would re-POST to the destination. */
    match oidc.client.end_session_url() {
        Some(url) => (jar, Redirect::to(url.as_str())).into_response(),
        None => {
            log::warn::auth!(
                field = "end_session_endpoint",
                "the IdP's discovery document has no end_session_endpoint, so \
                 logout ended the app session only — the IdP session survives \
                 and the next page load will silently re-authenticate"
            );
            (jar, Redirect::to("/")).into_response()
        }
    }
}

/// `Sec-Fetch-Site` is the browser's own account of the request and page JS
/// cannot set it; `Origin` is the fallback for a client that sends no
/// fetch-metadata. Neither present is a REFUSAL, not a pass — otherwise a
/// header-less POST from anywhere would end a session.
fn from_this_site(headers: &HeaderMap, config: &OidcConfig) -> bool {
    if let Some(site) = headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
    {
        return site == "same-origin";
    }
    headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| origin == app_origin(config))
}

fn app_origin(config: &OidcConfig) -> String {
    let url = &config.redirect_url;
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    format!(
        "{}://{}{}",
        url.scheme(),
        url.host_str().unwrap_or_default(),
        port
    )
}

async fn login(
    State(oidc): State<OidcState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let next = safe_next(params.get("next").map(String::as_str));
    let silent = params.get("prompt").map(String::as_str) != Some("login");
    start_login(&oidc, jar, next, silent, !silent).await
}

async fn client_js() -> Response {
    // The bytes-and-Content-Type is common-routing's shared static-file
    // helper (review followup #9); the immutable cache is this route's own —
    // the shim is served under one stable URL and does not change per request.
    let mut resp =
        common_routing::serve_static(crate::SHIM_JS.as_bytes(), "text/javascript; charset=utf-8");
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    resp
}

async fn callback(
    State(oidc): State<OidcState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let flow_id = jar.get(FLOW_COOKIE).map(|c| c.value().to_owned());
    let flow = match &flow_id {
        Some(id) => oidc.flows.get(id).await,
        None => None,
    };
    let Some(flow) = flow else {
        return start_login(&oidc, jar, "/".into(), true, false).await;
    };
    let flow_id = flow_id.unwrap_or_default();

    if let Some(error) = params.get("error") {
        oidc.flows.remove(&flow_id).await;
        let jar = jar.remove(removal_cookie(FLOW_COOKIE));
        let needs_interaction = matches!(
            error.as_str(),
            "login_required" | "interaction_required" | "consent_required"
        );
        if needs_interaction && !flow.interactive_tried {
            return start_login(&oidc, jar, safe_next(Some(&flow.next)), false, true).await;
        }
        return (
            StatusCode::UNAUTHORIZED,
            jar,
            format!("authentication failed: {error}"),
        )
            .into_response();
    }

    let (Some(code), Some(state)) = (params.get("code"), params.get("state")) else {
        return (StatusCode::BAD_REQUEST, "missing code/state").into_response();
    };
    if *state != flow.state {
        return (StatusCode::BAD_REQUEST, "state mismatch").into_response();
    }

    match oidc.client.exchange_code(code.clone(), flow.verifier).await {
        Ok(tokens) => {
            oidc.flows.remove(&flow_id).await;
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
            let jar = jar.remove(removal_cookie(FLOW_COOKIE)).add(
                base_cookie(oidc.config().cookie_name.as_str(), sid, oidc.config()).into_owned(),
            );
            (jar, Redirect::temporary(&safe_next(Some(&flow.next)))).into_response()
        }
        Err(e) => {
            log::warn::auth!(error = %e, "code exchange failed");
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

async fn unauthenticated(oidc: &OidcState, parts: &Parts, jar: CookieJar) -> AuthRedirect {
    if wants_html(parts) {
        let jar = jar.remove(removal_cookie(oidc.config().cookie_name.as_str()));
        let next = safe_next(parts.uri.path_and_query().map(|pq| pq.as_str()));
        log::debug::auth!(path = %next, "no session — silent re-auth redirect");
        AuthRedirect(start_login(oidc, jar, next, true, false).await)
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
            None => Err(unauthenticated(&oidc, parts, jar).await),
        }
    }
}

#[cfg(test)]
mod safe_next_tests {
    use super::safe_next;

    #[test]
    fn values_that_look_relative_but_change_origin_are_refused() {
        for hostile in [
            "//evil.example/x",
            "/\\evil.example/x",  // backslash folds to `/`
            "/\t/evil.example/x", // tab is stripped, leaving `//`
            "/\r/evil.example/x", // CR likewise
            "/\n/evil.example/x", // LF likewise
            "/..//evil.example",  // normalises to a `//` path
            "https://evil.example",
            "http://evil.example",
            "\\/evil.example/x",
        ] {
            assert_eq!(
                safe_next(Some(hostile)),
                "/",
                "next={hostile:?} must not survive the guard"
            );
        }
    }

    #[test]
    fn ordinary_same_origin_targets_survive_intact() {
        assert_eq!(safe_next(Some("/me")), "/me");
        assert_eq!(safe_next(Some("/a/b?x=1&y=2")), "/a/b?x=1&y=2");
        assert_eq!(safe_next(Some("/a#frag")), "/a#frag");
        assert_eq!(safe_next(None), "/");
        assert_eq!(safe_next(Some("")), "/");
        assert_eq!(safe_next(Some("relative/no/slash")), "/");
    }
}

#[cfg(test)]
mod flow_cookie_tests {
    use super::*;

    #[test]
    fn the_cookie_value_is_opaque_and_reveals_no_flow_data() {
        let id = new_flow_id();
        assert_eq!(id.len(), 64, "two v4 UUIDs, simple form: {id}");
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "id must be opaque hex, nothing decodable: {id}"
        );
        assert_ne!(new_flow_id(), new_flow_id(), "ids must not repeat");
    }
}
