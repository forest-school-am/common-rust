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
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid_grant"})),
                )
                    .into_response()
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
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid_grant"})),
                )
                    .into_response()
            }
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "unsupported_grant_type"})),
        )
            .into_response(),
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
        .route(
            &format!("{base_path}/.well-known/openid-configuration"),
            get(disco),
        )
        .route("/application/o/token/", post(mock_token))
        .route("/application/o/userinfo/", get(mock_userinfo))
        .with_state(mock.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, mock)
}

async fn oidc_state(base: &str, store: MemoryStore) -> OidcState {
    let mut config = OidcConfig::new(
        Url::parse(&format!("{base}/application/o/test/")).unwrap(),
        "test-client",
        Url::parse("http://app.example/oidc/callback").unwrap(),
        "test_session",
    )
    .request_refresh_tokens();
    config.cookie_secure = false;
    config.assets_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/templates").into();
    OidcState::discover(config, store)
        .await
        .expect("discovery against mock")
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

    let p = validator.validate("at-api").await.expect("valid token");
    assert_eq!(p.username, "alice");
    assert_eq!(p.uuid.to_string(), ALICE_SUB);
    assert!(p.in_group(&GROUP_A.parse().unwrap()));

    match validator.validate("nope").await {
        Err(ValidationError::Rejected) => {}
        other => panic!("expected Rejected, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn bearer_validator_fails_closed_when_userinfo_unreachable() {
    let validator = BearerValidator::new(
        reqwest::Client::new(),
        "http://127.0.0.1:1/userinfo".to_string(),
    );
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
        .oneshot(get_req("/me", &format!("test_session={sid}"), true))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = String::from_utf8(
        http_body_util::BodyExt::collect(resp.into_body())
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("alice"), "body: {body}");
    assert!(
        body.contains(GROUP_A),
        "effective_groups must come through: {body}"
    );
    assert_eq!(
        mock.userinfo_calls.load(Ordering::SeqCst),
        1,
        "userinfo per request, exactly"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_access_is_refreshed_server_side_exactly_once() {
    let (base, mock) = spawn_mock().await;
    mock.valid_refresh.lock().unwrap().insert("rt-live".into());
    let (store, sid) = seed_session("at-dead", Some("rt-live")).await;
    let app = app(oidc_state(&base, store).await);

    let resp = app
        .clone()
        .oneshot(get_req("/me", &format!("test_session={sid}"), true))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "refresh must rescue the request"
    );
    assert_eq!(mock.refresh_calls.load(Ordering::SeqCst), 1);

    let resp = app
        .oneshot(get_req("/me", &format!("test_session={sid}"), true))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        mock.refresh_calls.load(Ordering::SeqCst),
        1,
        "no refresh while access is valid"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dead_tokens_destroy_session_and_start_silent_login() {
    let (base, mock) = spawn_mock().await;
    let (store, sid) = seed_session("at-dead", Some("rt-dead")).await;
    let app = app(oidc_state(&base, store).await);

    let resp = app
        .clone()
        .oneshot(get_req("/me", &format!("test_session={sid}"), true))
        .await
        .unwrap();
    assert!(resp.status().is_redirection(), "got {}", resp.status());
    let loc = resp
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(loc.contains("/application/o/authorize/"), "location: {loc}");
    assert!(
        loc.contains("prompt=none"),
        "first attempt must be silent: {loc}"
    );
    assert!(
        loc.contains("code_challenge_method=S256"),
        "PKCE required: {loc}"
    );
    assert!(
        loc.contains("effective_groups") && loc.contains("offline_access"),
        "stand scopes must be requested: {loc}"
    );
    assert_eq!(mock.refresh_calls.load(Ordering::SeqCst), 1);

    let resp = app
        .oneshot(get_req("/me", &format!("test_session={sid}"), false))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        resp.headers()
            .get("x-common-oidc-reauth")
            .unwrap()
            .to_str()
            .unwrap(),
        "/oidc/login",
        "the shim's 401 contract needs the re-auth signal header"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_shim_and_login_route() {
    let (base, _mock) = spawn_mock().await;
    let app = app(oidc_state(&base, MemoryStore::default()).await);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/common-oidc.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(ct.contains("javascript"), "content-type: {ct}");
    let js = String::from_utf8(
        http_body_util::BodyExt::collect(resp.into_body())
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(
        js.contains("\"/oidc/login\""),
        "login path must be baked in"
    );
    assert!(
        !js.contains("__LOGIN_PATH__"),
        "placeholder must be replaced"
    );
    assert!(
        js.contains("installReauthGuard"),
        "must expose the guard API"
    );

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
    let loc = resp
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        loc.contains("prompt=none"),
        "login defaults to silent: {loc}"
    );

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/oidc/login?next=//evil.example/x")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // `next` is server-side now, so the sanitised value cannot be read off the
    // cookie — it is asserted where it becomes observable, by finishing the
    // login and seeing where the browser is sent.
    let flow_cookie = cookie_from(&resp, "oidc_flow").expect("flow cookie");
    let state = state_from_location(&resp);
    let resp = app
        .oneshot(get_req(
            &format!("/oidc/callback?code=goodcode&state={state}"),
            &flow_cookie,
            true,
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap(),
        "/",
        "an unsafe ?next= must land on '/' after the flow completes"
    );
}

fn state_from_location(resp: &axum::http::Response<Body>) -> String {
    let loc = resp
        .headers()
        .get(header::LOCATION)
        .expect("redirect to the IdP")
        .to_str()
        .unwrap();
    Url::parse(loc)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.into_owned())
        .expect("authorize URL carries state")
}

fn hex_encode_test(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

/// What an attacker who can set a cookie for this origin actually has: the
/// flow cookie carries no MAC, so every field is theirs to choose. Built here
/// by hand rather than through the crate, with the wire keys spelled out.
fn forged_flow_cookie(state: &str, next: &str) -> String {
    let json = format!(r#"{{"s":"{state}","v":"forged-verifier","n":"{next}","i":false}}"#);
    format!("oidc_flow={}", hex_encode_test(&json))
}

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

    let resp = app.clone().oneshot(get_req("/me", "", true)).await.unwrap();
    assert!(resp.status().is_redirection());
    let flow_cookie = cookie_from(&resp, "oidc_flow").expect("flow cookie set");

    let resp = app
        .clone()
        .oneshot(get_req(
            "/oidc/callback?error=login_required",
            &flow_cookie,
            true,
        ))
        .await
        .unwrap();
    assert!(
        resp.status().is_redirection(),
        "escalation expected, got {}",
        resp.status()
    );
    let loc2 = resp
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        !loc2.contains("prompt=none"),
        "escalated attempt must be interactive: {loc2}"
    );
    let flow2 = cookie_from(&resp, "oidc_flow").expect("new flow cookie");
    let resp = app
        .clone()
        .oneshot(get_req("/oidc/callback?error=login_required", &flow2, true))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "loop breaker: no second escalation"
    );

    // A fresh flow for the remaining two checks: the escalation above
    // SUPERSEDED the first one, and a superseded flow is now genuinely gone
    // rather than still decodable from a self-contained cookie.
    let resp = app.clone().oneshot(get_req("/me", "", true)).await.unwrap();
    let flow_cookie = cookie_from(&resp, "oidc_flow").expect("flow cookie set");
    let state = state_from_location(&resp);

    let resp = app
        .clone()
        .oneshot(get_req(
            "/oidc/callback?code=goodcode&state=WRONG",
            &flow_cookie,
            true,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

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
    let back = resp
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(back, "/me", "must land on the originally requested page");
    let session_cookie = cookie_from(&resp, "test_session").expect("session cookie set");
    assert_eq!(mock.exchange_calls.load(Ordering::SeqCst), 1);

    let resp = app
        .oneshot(get_req("/me", &session_cookie, true))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

/// The property this whole design exists for, asserted against the real
/// Set-Cookie header rather than against the struct: the browser is handed an
/// opaque id, and the CSRF state, the PKCE verifier and the redirect target
/// stay on the server.
#[tokio::test(flavor = "multi_thread")]
async fn the_flow_cookie_carries_no_secrets() {
    let (base, _mock) = spawn_mock().await;
    let app = app(oidc_state(&base, MemoryStore::default()).await);

    let resp = app
        .oneshot(get_req("/oidc/login?next=/deep/page", "", true))
        .await
        .unwrap();

    let state = state_from_location(&resp);
    let cookie = cookie_from(&resp, "oidc_flow").expect("flow cookie");
    let value = cookie.trim_start_matches("oidc_flow=");

    assert!(
        !cookie.contains(&state),
        "the CSRF state reached the browser: {cookie}"
    );
    assert!(
        !cookie.contains("/deep/page"),
        "the redirect target reached the browser: {cookie}"
    );
    assert_eq!(value.len(), 64, "expected an opaque id, got {value:?}");
    assert!(
        value.chars().all(|c| c.is_ascii_hexdigit()),
        "cookie value must be opaque: {value:?}"
    );
    // Whatever the value is, it must not decode to anything structured — the
    // old form was hex-encoded JSON and looked equally opaque at a glance.
    let decoded = hex_decode_test(value);
    assert!(
        !decoded.contains('{') && !decoded.contains(':'),
        "cookie value decodes to structured data: {decoded:?}"
    );
}

/// Replaces `a_next_from_the_flow_cookie_cannot_leave_the_origin`, which
/// forged the cookie payload to steer the post-login redirect. That attack is
/// no longer expressible — the cookie carries an opaque id and the flow lives
/// server-side — so the property to assert is the stronger one: a payload the
/// attacker writes names no flow and therefore does NOTHING.
#[tokio::test(flavor = "multi_thread")]
async fn a_forged_flow_cookie_is_inert() {
    let (base, mock) = spawn_mock().await;
    let app = app(oidc_state(&base, MemoryStore::default()).await);

    // Byte-for-byte the old attack: the pre-store wire form, hex-encoded,
    // naming an off-origin `next` and a state the attacker also supplies.
    let payload = r#"{"s":"forged-state","v":"forged-verifier","n":"//evil.example","i":false}"#;
    let forged = format!(
        "oidc_flow={}",
        payload
            .bytes()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );

    let resp = app
        .clone()
        .oneshot(get_req(
            "/oidc/callback?code=goodcode&state=forged-state",
            &forged,
            true,
        ))
        .await
        .unwrap();

    let loc = resp
        .headers()
        .get(header::LOCATION)
        .expect("a redirect")
        .to_str()
        .unwrap();
    assert!(
        !loc.contains("evil.example"),
        "forged next reached the Location header: {loc}"
    );
    assert!(
        loc.contains("/application/o/authorize/"),
        "an unknown flow id must restart login, not complete one: {loc}"
    );
    assert!(
        cookie_from(&resp, "test_session").is_none(),
        "a forged flow cookie must not yield a session"
    );
    assert_eq!(
        mock.exchange_calls.load(Ordering::SeqCst),
        0,
        "no token exchange may happen for a flow the server never issued"
    );

    // And the id space is not guessable-by-shape either: a plausible-looking
    // opaque id that was never issued is equally inert.
    let resp = app
        .oneshot(get_req(
            "/oidc/callback?code=goodcode&state=forged-state",
            &format!("oidc_flow={}", "a".repeat(64)),
            true,
        ))
        .await
        .unwrap();
    assert!(
        cookie_from(&resp, "test_session").is_none(),
        "an unissued flow id must not yield a session"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn unauthorized_response_matches_the_extractor_401() {
    let (base, _mock) = spawn_mock().await;
    let oidc = oidc_state(&base, MemoryStore::default()).await;

    let delegated = oidc.unauthorized_response();
    let from_extractor = app(oidc.clone())
        .oneshot(get_req("/me", "", false))
        .await
        .unwrap();

    assert_eq!(delegated.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(delegated.status(), from_extractor.status());

    let header_of = |r: &axum::response::Response| {
        r.headers()
            .get(common_oidc::REAUTH_HEADER)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(header_of(&delegated), "/oidc/login");
    assert_eq!(
        header_of(&delegated),
        header_of(&from_extractor),
        "both paths must advertise the same login path"
    );

    let cookie = delegated
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .expect("the 401 must clear the session cookie")
        .to_str()
        .unwrap()
        .to_owned();
    assert!(
        cookie.starts_with("test_session="),
        "cleared the wrong cookie: {cookie}"
    );
    assert!(
        cookie.contains("test_session=;") || cookie.to_lowercase().contains("max-age=0"),
        "session cookie must be cleared, got: {cookie}"
    );
}
