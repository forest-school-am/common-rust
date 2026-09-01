//! BFF policy tests against a mock authentik: per-request userinfo,
//! server-side refresh (exactly once), silent→interactive escalation, and
//! the full PKCE code-exchange loop.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use tower::util::ServiceExt;
use url::Url;

use common_oidc::{
    BearerValidator, MemoryStore, OidcConfig, OidcState, Principal, Session, SessionStore,
    ValidationError,
};

const ALICE_SUB: &str = "8a2f1c77-9e04-4b7e-9b1a-2f3d4c5e6a7b";
const GROUP_A: &str = "d427f013-3bef-45e7-96aa-32545b58f845";

#[derive(Default)]
struct Mock {
    valid_access: Mutex<HashSet<String>>,
    valid_refresh: Mutex<HashSet<String>>,
    userinfo_calls: AtomicUsize,
    refresh_calls: AtomicUsize,
    exchange_calls: AtomicUsize,
}

async fn mock_token(State(m): State<Arc<Mock>>, body: String) -> impl IntoResponse {
    let form: std::collections::HashMap<_, _> = url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect();
    match form.get("grant_type").map(String::as_str) {
        Some("refresh_token") => {
            m.refresh_calls.fetch_add(1, Ordering::SeqCst);
            let ok = form
                .get("refresh_token")
                .is_some_and(|rt| m.valid_refresh.lock().unwrap().contains(rt));
            if ok {
                m.valid_access.lock().unwrap().insert("at-refreshed".into());
                Json(json!({
                    "access_token": "at-refreshed", "token_type": "Bearer",
                    "expires_in": 600, "refresh_token": "rt-rotated",
                }))
                .into_response()
            } else {
                (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_grant"}))).into_response()
            }
        }
        Some("authorization_code") => {
            m.exchange_calls.fetch_add(1, Ordering::SeqCst);
            let good = form.get("code").map(String::as_str) == Some("goodcode")
                && form.get("code_verifier").is_some_and(|v| !v.is_empty());
            if good {
                m.valid_access.lock().unwrap().insert("at-fresh".into());
                m.valid_refresh.lock().unwrap().insert("rt-fresh".into());
                Json(json!({
                    "access_token": "at-fresh", "token_type": "Bearer",
                    "expires_in": 600, "refresh_token": "rt-fresh",
                }))
                .into_response()
            } else {
                (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_grant"}))).into_response()
            }
        }
        _ => (StatusCode::BAD_REQUEST, Json(json!({"error": "unsupported_grant_type"}))).into_response(),
    }
}

async fn mock_userinfo(
    State(m): State<Arc<Mock>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    m.userinfo_calls.fetch_add(1, Ordering::SeqCst);
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    if m.valid_access.lock().unwrap().contains(bearer) {
        Json(json!({
            "sub": ALICE_SUB,
            "preferred_username": "alice",
            "email": "alice@teststand.local",
            "effective_groups": [GROUP_A],
        }))
        .into_response()
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

/// Serve a mock authentik; returns (its base url, handle to its state).
async fn spawn_mock() -> (String, Arc<Mock>) {
    let mock = Arc::new(Mock::default());
    let base_path = "/application/o/test";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let disco_base = base.clone();
    let disco = move || {
        let b = disco_base.clone();
        async move {
            Json(json!({
                "issuer": format!("{b}/application/o/test"),
                "authorization_endpoint": format!("{b}/application/o/authorize/"),
                "token_endpoint": format!("{b}/application/o/token/"),
                "userinfo_endpoint": format!("{b}/application/o/userinfo/"),
                "jwks_uri": format!("{b}/application/o/test/jwks/"),
            }))
        }
    };
    let app = Router::new()
        .route(&format!("{base_path}/.well-known/openid-configuration"), get(disco))
        .route("/application/o/token/", post(mock_token))
        .route("/application/o/userinfo/", get(mock_userinfo))
        .with_state(mock.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, mock)
}

async fn oidc_state(base: &str, store: MemoryStore) -> OidcState {
    // these tests exercise the server-side refresh path, so opt in
    let mut config = OidcConfig::new(
        Url::parse(&format!("{base}/application/o/test/")).unwrap(),
        "test-client",
        Url::parse("http://app.example/oidc/callback").unwrap(),
    )
    .request_refresh_tokens();
    config.cookie_secure = false;
    // serve the shim from the crate's own templates dir (its hash matches the
    // build.rs pin); a bad dir/stale template would refuse discovery.
    config.assets_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/templates").into();
    OidcState::discover(config, store).await.expect("discovery against mock")
}

async fn me(p: Principal) -> String {
    format!("{}|{}|{:?}", p.username, p.uuid, p.effective_groups)
}

fn app(oidc: OidcState) -> Router {
    Router::new()
        .route("/me", get(me))
        .with_state(oidc.clone())
        .merge(common_oidc::router(oidc))
}

async fn seed_session(access: &str, refresh: Option<&str>) -> (MemoryStore, &'static str) {
    let store = MemoryStore::default();
    let session = Session {
        access_token: access.into(),
        refresh_token: refresh.map(String::from),
        created: SystemTime::now(),
    };
    store.put("sid1".into(), session).await;
    (store, "sid1")
}

fn get_req(path: &str, cookies: &str, html: bool) -> Request<Body> {
    let mut b = Request::builder().uri(path).header(header::COOKIE, cookies);
    if html {
        b = b.header(header::ACCEPT, "text/html,application/xhtml+xml");
    } else {
        b = b.header(header::ACCEPT, "application/json");
    }
    b.body(Body::empty()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn bearer_validator_shares_the_identity_contract() {
    let (base, mock) = spawn_mock().await;
    mock.valid_access.lock().unwrap().insert("at-api".into());
    let userinfo = format!("{base}/application/o/userinfo/");
    let validator = BearerValidator::new(reqwest::Client::new(), userinfo);

    // good token -> principal with UUID sub + effective_groups
    let p = validator.validate("at-api").await.expect("valid token");
    assert_eq!(p.username, "alice");
    assert_eq!(p.uuid.to_string(), ALICE_SUB);
    assert!(p.in_group(&GROUP_A.parse().unwrap()));

    // rejected token -> Rejected (mapped to 401 by callers), never a principal
    match validator.validate("nope").await {
        Err(ValidationError::Rejected) => {}
        other => panic!("expected Rejected, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn bearer_validator_fails_closed_when_userinfo_unreachable() {
    let validator =
        BearerValidator::new(reqwest::Client::new(), "http://127.0.0.1:1/userinfo".to_string());
    match validator.validate("whatever").await {
        Err(ValidationError::Upstream(_)) => {}
        other => panic!("expected Upstream (fail closed), got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn valid_access_token_yields_principal() {
    let (base, mock) = spawn_mock().await;
    mock.valid_access.lock().unwrap().insert("at-live".into());
    let (store, sid) = seed_session("at-live", None).await;
    let app = app(oidc_state(&base, store).await);

    let resp = app
        .oneshot(get_req("/me", &format!("stand_session={sid}"), true))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = String::from_utf8(
        http_body_util::BodyExt::collect(resp.into_body()).await.unwrap().to_bytes().to_vec(),
    )
    .unwrap();
    assert!(body.contains("alice"), "body: {body}");
    assert!(body.contains(GROUP_A), "effective_groups must come through: {body}");
    assert_eq!(mock.userinfo_calls.load(Ordering::SeqCst), 1, "userinfo per request, exactly");
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_access_is_refreshed_server_side_exactly_once() {
    let (base, mock) = spawn_mock().await;
    // access token dead, refresh token alive
    mock.valid_refresh.lock().unwrap().insert("rt-live".into());
    let (store, sid) = seed_session("at-dead", Some("rt-live")).await;
    let app = app(oidc_state(&base, store).await);

    let resp = app
        .clone()
        .oneshot(get_req("/me", &format!("stand_session={sid}"), true))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "refresh must rescue the request");
    assert_eq!(mock.refresh_calls.load(Ordering::SeqCst), 1);

    // second request: refreshed access token is in the session now — no new refresh
    let resp = app
        .oneshot(get_req("/me", &format!("stand_session={sid}"), true))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(mock.refresh_calls.load(Ordering::SeqCst), 1, "no refresh while access is valid");
}

#[tokio::test(flavor = "multi_thread")]
async fn dead_tokens_destroy_session_and_start_silent_login() {
    let (base, mock) = spawn_mock().await;
    let (store, sid) = seed_session("at-dead", Some("rt-dead")).await;
    let app = app(oidc_state(&base, store).await);

    // browser navigation -> redirect into silent authorize
    let resp = app
        .clone()
        .oneshot(get_req("/me", &format!("stand_session={sid}"), true))
        .await
        .unwrap();
    assert!(resp.status().is_redirection(), "got {}", resp.status());
    let loc = resp.headers().get(header::LOCATION).unwrap().to_str().unwrap();
    assert!(loc.contains("/application/o/authorize/"), "location: {loc}");
    assert!(loc.contains("prompt=none"), "first attempt must be silent: {loc}");
    assert!(loc.contains("code_challenge_method=S256"), "PKCE required: {loc}");
    assert!(
        loc.contains("effective_groups") && loc.contains("offline_access"),
        "stand scopes must be requested: {loc}"
    );
    // the dead refresh token was tried once, then the session was dropped
    assert_eq!(mock.refresh_calls.load(Ordering::SeqCst), 1);

    // API caller (no text/html) -> plain 401 carrying the re-auth signal
    let resp = app
        .oneshot(get_req("/me", &format!("stand_session={sid}"), false))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        resp.headers().get("x-common-oidc-reauth").unwrap().to_str().unwrap(),
        "/oidc/login",
        "the shim's 401 contract needs the re-auth signal header"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_shim_and_login_route() {
    let (base, _mock) = spawn_mock().await;
    let app = app(oidc_state(&base, MemoryStore::default()).await);

    // the served shim: baked login path, framework-free module
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/common-oidc.js").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers().get(header::CONTENT_TYPE).unwrap().to_str().unwrap().to_owned();
    assert!(ct.contains("javascript"), "content-type: {ct}");
    let js = String::from_utf8(
        http_body_util::BodyExt::collect(resp.into_body()).await.unwrap().to_bytes().to_vec(),
    )
    .unwrap();
    assert!(js.contains("\"/oidc/login\""), "login path must be baked in");
    assert!(!js.contains("__LOGIN_PATH__"), "placeholder must be replaced");
    assert!(js.contains("installReauthGuard"), "must expose the guard API");

    // login route -> silent authorize, returning to a SAFE next
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/oidc/login?next=/dashboard")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(resp.status().is_redirection());
    let loc = resp.headers().get(header::LOCATION).unwrap().to_str().unwrap();
    assert!(loc.contains("prompt=none"), "login defaults to silent: {loc}");

    // open-redirect guard: a protocol-relative next is dropped to "/"
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/oidc/login?next=//evil.example/x")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let flow = cookie_from(&resp, "so_flow").expect("flow cookie");
    let json = hex_decode_test(flow.trim_start_matches("so_flow="));
    assert!(json.contains("\"n\":\"/\""), "unsafe next must fall back to '/': {json}");
}

// mirror of web.rs hex_decode for asserting flow-cookie contents in tests
// mirror of web.rs `hex_decode` (private there) — keep in sync if that changes
fn hex_decode_test(s: &str) -> String {
    let bytes: Vec<u8> = (0..s.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect();
    String::from_utf8(bytes).unwrap_or_default()
}

fn cookie_from(resp: &axum::http::Response<Body>, name: &str) -> Option<String> {
    resp.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")) && !v.starts_with(&format!("{name}=;")))
        .map(|v| v.split(';').next().unwrap().to_owned())
}

#[tokio::test(flavor = "multi_thread")]
async fn full_login_loop_and_interactive_escalation() {
    let (base, mock) = spawn_mock().await;
    let app = app(oidc_state(&base, MemoryStore::default()).await);

    // 1. anonymous browser hit -> silent authorize + flow cookie
    let resp = app.clone().oneshot(get_req("/me", "", true)).await.unwrap();
    assert!(resp.status().is_redirection());
    let loc = resp.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_owned();
    let flow_cookie = cookie_from(&resp, "so_flow").expect("flow cookie set");
    let state = Url::parse(&loc)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.into_owned())
        .unwrap();

    // 2a. the SSO session was dead: login_required escalates to interactive, once
    let resp = app
        .clone()
        .oneshot(get_req("/oidc/callback?error=login_required", &flow_cookie, true))
        .await
        .unwrap();
    assert!(resp.status().is_redirection(), "escalation expected, got {}", resp.status());
    let loc2 = resp.headers().get(header::LOCATION).unwrap().to_str().unwrap();
    assert!(!loc2.contains("prompt=none"), "escalated attempt must be interactive: {loc2}");
    let flow2 = cookie_from(&resp, "so_flow").expect("new flow cookie");
    let resp = app
        .clone()
        .oneshot(get_req("/oidc/callback?error=login_required", &flow2, true))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "loop breaker: no second escalation");

    // 2b. wrong state on an otherwise-fine callback is rejected
    let resp = app
        .clone()
        .oneshot(get_req("/oidc/callback?code=goodcode&state=WRONG", &flow_cookie, true))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 3. proper callback: code exchange -> session cookie -> original page
    let resp = app
        .clone()
        .oneshot(get_req(
            &format!("/oidc/callback?code=goodcode&state={state}"),
            &flow_cookie,
            true,
        ))
        .await
        .unwrap();
    assert!(resp.status().is_redirection(), "got {}", resp.status());
    let back = resp.headers().get(header::LOCATION).unwrap().to_str().unwrap();
    assert_eq!(back, "/me", "must land on the originally requested page");
    let session_cookie = cookie_from(&resp, "stand_session").expect("session cookie set");
    assert_eq!(mock.exchange_calls.load(Ordering::SeqCst), 1);

    // 4. the session works
    let resp = app.oneshot(get_req("/me", &session_cookie, true)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

/// The delegation seam adopters call from their §3.1 chokepoint must produce
/// the SAME wire contract as the extractor's own rejection — otherwise a
/// service that obeys §3.1 silently emits a different 401 from one that lets
/// the extractor reject, and the shim only works for the second.
#[tokio::test(flavor = "multi_thread")]
async fn unauthorized_response_matches_the_extractor_401() {
    let (base, _mock) = spawn_mock().await;
    let oidc = oidc_state(&base, MemoryStore::default()).await;

    // what an adopter builds by delegating
    let delegated = oidc.unauthorized_response();
    // what the extractor rejects with, for a caller with no session at all
    let from_extractor = app(oidc.clone())
        .oneshot(get_req("/me", "", false))
        .await
        .unwrap();

    assert_eq!(delegated.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(delegated.status(), from_extractor.status());

    let header_of = |r: &axum::response::Response| {
        r.headers().get(common_oidc::REAUTH_HEADER).unwrap().to_str().unwrap().to_owned()
    };
    assert_eq!(header_of(&delegated), "/oidc/login");
    assert_eq!(
        header_of(&delegated),
        header_of(&from_extractor),
        "both paths must advertise the same login path"
    );

    // the contract is more than the header: the dead session cookie is cleared,
    // which is the part an adopter copying only the header string would miss.
    let cookie = delegated
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .expect("the 401 must clear the session cookie")
        .to_str()
        .unwrap()
        .to_owned();
    assert!(cookie.starts_with("stand_session="), "cleared the wrong cookie: {cookie}");
    assert!(
        cookie.contains("stand_session=;") || cookie.to_lowercase().contains("max-age=0"),
        "session cookie must be cleared, got: {cookie}"
    );
}
