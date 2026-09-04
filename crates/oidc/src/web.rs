//! The axum layer: routes, extractors, cookies, redirects, and the policy
//! decisions that shape a response. Anything that talks to the IdP belongs
//! in client.rs; anything a bearer-only API needs belongs in bearer.rs.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use axum::extract::{FromRef, FromRequestParts, Query, State};
use axum::http::{header, request::Parts, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use url::Url;
use uuid::Uuid;

use common_logging::Deployment;
use common_templating::AssetCache;

use crate::client::OidcClient;
use crate::config::OidcConfig;
use crate::principal::Principal;
use crate::store::{FlowState, FlowStore, MemoryFlowStore, Session, SessionStore};

const FLOW_COOKIE: &str = "oidc_flow";
pub const REAUTH_HEADER: &str = "X-Common-OIDC-Reauth";
const SHIM_TEMPLATE: &str = "common-oidc.js.jinja";

/// Reduce a post-login redirect target to a same-origin relative reference,
/// or to `/`. This is the open-redirect guard.
///
/// RESOLVE, do not pattern-match. A prefix test answers "does this look
/// relative", which is not the question — the question is "where does a
/// browser END UP". Those differ: `/\evil.example` and `/<TAB>/evil.example`
/// both look relative and both land on `evil.example`, because the WHATWG URL
/// parser folds `\` to `/` and strips tab/CR/LF before resolving. So the value
/// is resolved against a fixed base and the resulting ORIGIN is compared;
/// anything that moved origin is discarded.
///
/// The re-serialised path is then prefix-checked as well, because resolution
/// NORMALISES: `/..//evil.example` resolves same-origin but its path is
/// `//evil.example`, which would leave the origin all over again once emitted
/// into a `Location` header. Each half covers the other's blind spot.
///
/// Applied where `next` ENTERS, at `/oidc/login`, and again where it is USED.
/// The stored value cannot currently be tampered with — the flow lives in the
/// flow store and the browser holds only an opaque id — so the second
/// application is redundant today and deliberately kept: it is the layer that
/// still holds if a later change ever puts flow data back in the client's
/// hands.
fn safe_next(raw: Option<&str>) -> String {
    fn resolve(raw: &str) -> Option<String> {
        // Absolute-path form only, which is what the shim ever sends
        // (`location.pathname + search + hash`). Resolution alone would also
        // accept `a/b` and rewrite it to `/a/b`; that is same-origin and
        // harmless, but it widens the contract for no caller that exists.
        if !raw.starts_with('/') {
            return None;
        }
        // Opaque, unreachable base: `next` is only ever emitted as a relative
        // reference, so the host here is never used for anything but the
        // origin comparison.
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
    pub assets: Arc<AssetCache>,
}

pub(crate) fn dirty_source_refusal(deployment: Deployment, state: &str) -> Option<String> {
    if matches!(deployment, Deployment::Prod) && state.starts_with("Dirty") {
        return Some(format!(
            "common-oidc was built from a dirty working tree ({state}); the served shim \
             is unreproducible and this is refused under DEPLOYMENT_TYPE=prod. \
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
            flows: Arc::new(MemoryFlowStore::default()),
            assets: Arc::new(assets),
        })
    }

    /// Replace the default in-memory flow store. Needed only where logins must
    /// survive a restart or be shared across replicas — the callback lands on
    /// whichever instance the browser reaches, and an in-memory flow is
    /// invisible to the others. The same constraint already applies to
    /// [`SessionStore`], so a deployment that has solved it for sessions
    /// solves it here the same way.
    pub fn with_flow_store(mut self, flows: impl FlowStore) -> Self {
        self.flows = Arc::new(flows);
        self
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
        let sid = jar
            .get(self.config().cookie_name.as_str())?
            .value()
            .to_owned();
        let session = self.store.get(&sid).await?;

        if let Ok(p) = self
            .client
            .principal_from_access_token(&session.access_token)
            .await
        {
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
                if let Ok(p) = self
                    .client
                    .principal_from_access_token(&refreshed.access_token)
                    .await
                {
                    common_logging::debug!(
                        common_logging::AUTH,
                        "access token refreshed server-side"
                    );
                    return Some((p, refreshed));
                }
            }
        }

        common_logging::info!(
            common_logging::AUTH,
            "session tokens dead — destroying local session"
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

/// The browser receives an opaque id and nothing else; the CSRF state, the
/// PKCE verifier and the redirect target stay in the flow store. Generated
/// like a session id — two v4 UUIDs, 256 bits — because it is the only thing
/// binding a callback to the login that started it.
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
    let next = safe_next(params.get("next").map(String::as_str));
    let silent = params.get("prompt").map(String::as_str) != Some("login");
    start_login(&oidc, jar, next, silent, !silent).await
}

async fn client_js(State(oidc): State<OidcState>) -> Response {
    let login_path = oidc.config().login_path.clone();
    match oidc
        .assets
        .render(SHIM_TEMPLATE, &[("login_path", login_path.as_str())])
    {
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
    let flow_id = jar.get(FLOW_COOKIE).map(|c| c.value().to_owned());
    let flow = match &flow_id {
        Some(id) => oidc.flows.get(id).await,
        None => None,
    };
    // An id naming no live flow is indistinguishable from no id at all: the
    // browser cannot mint one the store will recognise, so an unknown value is
    // an expired or already-finished login, not a signal.
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

async fn unauthenticated(oidc: &OidcState, parts: &Parts, jar: CookieJar) -> AuthRedirect {
    if wants_html(parts) {
        let jar = jar.remove(removal_cookie(oidc.config().cookie_name.as_str()));
        let next = safe_next(parts.uri.path_and_query().map(|pq| pq.as_str()));
        common_logging::debug!(common_logging::AUTH, path = %next, "no session — silent re-auth redirect");
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

    /// Every value here LOOKS relative and a prefix check passes it, but a
    /// WHATWG parser — which is what the browser reading our `Location`
    /// header is — resolves each one onto another origin. Verified against
    /// `url::Url::join` before being written down, not assumed.
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

#[cfg(test)]
mod flow_cookie_tests {
    use super::*;

    /// Replaces `flow_wire`, which pinned the single-letter JSON keys of a
    /// cookie that no longer carries a payload. What matters now is the
    /// opposite property: that the cookie carries NOTHING but an opaque id.
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
