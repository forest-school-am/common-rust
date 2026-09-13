//! End-to-end flows against a mock IdP over real sockets: the router, the
//! session lifecycle and the cookie surface. Schedule arithmetic belongs in
//! `retry`'s virtual-time tests; anything only a real authentik can answer
//! belongs in live_canary.rs.

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
    hang_userinfo: std::sync::atomic::AtomicBool,
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
    if m.hang_userinfo.load(Ordering::SeqCst) {
        std::future::pending::<()>().await;
    }
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
    spawn_mock_with(true).await
}

/// `end_session` false serves a discovery document WITHOUT
/// `end_session_endpoint`, which is legal — it is optional in the spec — and
/// is how the fallback path gets exercised instead of being assumed.
async fn spawn_mock_with(end_session: bool) -> (String, Arc<Mock>) {
    let mock = Arc::new(Mock::default());
    let base_path = "/application/o/test";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let disco_base = base.clone();
    let disco = move || {
        let b = disco_base.clone();
        async move {
            let mut doc = json!({
                "issuer": format!("{b}/application/o/test"),
                "authorization_endpoint": format!("{b}/application/o/authorize/"),
                "token_endpoint": format!("{b}/application/o/token/"),
                "userinfo_endpoint": format!("{b}/application/o/userinfo/"),
                "jwks_uri": format!("{b}/application/o/test/jwks/"),
            });
            if end_session {
                doc["end_session_endpoint"] = json!(format!("{b}/application/o/test/end-session/"));
            }
            Json(doc)
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

/// The `reqwest::Client` under test sets NO timeout of its own — if it ever
/// gains one this stops testing the crate's own cancellation and starts
/// passing for reqwest's reason.
#[tokio::test(flavor = "multi_thread")]
async fn a_hung_upstream_is_actually_cancelled_not_merely_wrapped() {
    let (base, mock) = spawn_mock().await;
    let oidc = oidc_state(&base, MemoryStore::default()).await;
    mock.hang_userinfo.store(true, Ordering::SeqCst);

    let started = std::time::Instant::now();
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        oidc.client.principal_from_access_token("at-live"),
    )
    .await;
    let elapsed = started.elapsed();

    assert!(outcome.is_err(), "the hung call must hit the timeout");
    assert!(
        elapsed < std::time::Duration::from_secs(3),
        "a hung upstream must be cut off at the attempt boundary, not run on: {elapsed:?}"
    );
    assert_eq!(
        mock.userinfo_calls.load(Ordering::SeqCst),
        1,
        "the request reached the server and was left hanging there"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_transient_outage_does_not_end_the_session() {
    let (base, mock) = spawn_mock().await;
    mock.valid_access.lock().unwrap().insert("at-live".into());
    let (store, sid) = seed_session("at-live", None).await;
    let app = app(oidc_state(&base, store).await);

    mock.hang_userinfo.store(true, Ordering::SeqCst);
    let recovering = mock.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        recovering.hang_userinfo.store(false, Ordering::SeqCst);
    });

    let resp = app
        .oneshot(get_req("/me", &format!("test_session={sid}"), true))
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "a session must survive an outage that recovers inside the budget"
    );
    assert!(
        mock.userinfo_calls.load(Ordering::SeqCst) >= 2,
        "the first attempt must have been retried, not accepted as a verdict"
    );
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
    let cache = resp
        .headers()
        .get(header::CACHE_CONTROL)
        .map(|v| v.to_str().unwrap().to_owned());
    assert!(ct.contains("javascript"), "content-type: {ct}");
    let cache = cache.expect("a static shim must be cacheable");
    assert!(
        cache.contains("immutable"),
        "the shim is in the binary, so it can be cached forever: {cache}"
    );
    let js = String::from_utf8(
        http_body_util::BodyExt::collect(resp.into_body())
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(
        !js.contains("/oidc/login"),
        "R65: the shim is STATIC — the login path reaches it through the config \
         block, never baked in: {js}"
    );
    assert!(
        js.contains("getElementById(\"config\")") && js.contains("loginPath"),
        "it must read the login path from the config block: {js}"
    );
    assert!(
        js.contains("installReauthGuard"),
        "must expose the guard API"
    );
    assert!(
        !js.contains("interface ") && !js.contains(": string") && !js.contains("as unknown"),
        "the TypeScript must be STRIPPED, not shipped: {js}"
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

    // The escalation above superseded the first flow, so the remaining checks
    // need one that is still live.
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
    let decoded = hex_decode_test(value);
    assert!(
        !decoded.contains('{') && !decoded.contains(':'),
        "cookie value decodes to structured data: {decoded:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forged_flow_cookie_is_inert() {
    let (base, mock) = spawn_mock().await;
    let app = app(oidc_state(&base, MemoryStore::default()).await);

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
async fn unauthorized_response_clears_the_cookie_from_a_jar_it_was_never_in() {
    let (base, _mock) = spawn_mock().await;
    let oidc = oidc_state(&base, MemoryStore::default()).await;

    let cookie = oidc
        .unauthorized_response()
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .expect("an adopter calls this with no jar, so nothing was there to remove")
        .to_str()
        .unwrap()
        .to_owned();
    assert!(
        cookie.to_lowercase().contains("max-age=0"),
        "the removal has to be an expiry the browser acts on, not an absence: {cookie}"
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

/// The les-forms sso_login.sh shape, in-process: hold a live session, POST the
/// logout, then ask for a page again. R73's route exists so the bar's button
/// is not dead the moment a consumer bumps.
#[tokio::test(flavor = "multi_thread")]
async fn a_posted_logout_ends_the_session_and_the_next_page_asks_for_login() {
    let (base, mock) = spawn_mock().await;
    // The IdP has to consider the token live, or "the session works before
    // logout" is vacuous and the whole test proves nothing.
    mock.valid_access.lock().unwrap().insert("at-live".into());
    let (store, sid) = seed_session("at-live", Some("refresh-1")).await;
    let oidc = oidc_state(&base, store).await;
    let cookie = format!("test_session={sid}");

    let before = app(oidc.clone())
        .oneshot(get_req("/me", &cookie, true))
        .await
        .unwrap();
    assert_eq!(
        before.status(),
        StatusCode::OK,
        "the session has to be live BEFORE logout, or this test proves nothing"
    );

    let logout = app(oidc.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/common-oidc/logout")
                .header(header::COOKIE, &cookie)
                // What a real same-origin form POST sends.
                .header("sec-fetch-site", "same-origin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        logout.status(),
        StatusCode::SEE_OTHER,
        "303, so the browser GETs the root rather than re-POSTing to it"
    );
    /* R79 — straight to the IdP's end_session_endpoint, PLAIN: the user's
    ruling is "logout should just send to authentik logout", so no
    id_token_hint and no post_logout_redirect_uri. Authentik's page is the
    end of the trip. */
    let sent = logout
        .headers()
        .get(header::LOCATION)
        .and_then(|l| l.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        sent.contains("/application/o/test/end-session/"),
        "logout must send the browser to the IdP: {sent}"
    );
    assert!(
        !sent.contains("id_token_hint") && !sent.contains("post_logout_redirect_uri"),
        "R79 wants the endpoint plain, with no parameters: {sent}"
    );
    let cleared = logout
        .headers()
        .get(header::SET_COOKIE)
        .expect("the session cookie must be cleared")
        .to_str()
        .unwrap()
        .to_owned();
    assert!(
        cleared.contains("test_session=") && cleared.to_lowercase().contains("max-age=0"),
        "got: {cleared}"
    );

    assert!(
        oidc.store.get(sid).await.is_none(),
        "the SERVER-side session must be gone — clearing only the cookie leaves \
         a live session for anyone who kept the value"
    );

    let after = app(oidc)
        .oneshot(get_req("/me", &cookie, true))
        .await
        .unwrap();
    // 307, which is what start_login already issues (Redirect::temporary) —
    // not the 303 the logout itself uses. Two different redirects for two
    // different jobs: 303 turns a POST into a GET, 307 preserves the method of
    // a page request being sent to login.
    assert_eq!(
        after.status(),
        StatusCode::TEMPORARY_REDIRECT,
        "the next page request must be sent to login, not served: {:?}",
        after.status()
    );
    /* Sent to the IdP to re-authenticate, not to the local login path: the
    extractor starts a SILENT login itself. Which is the thing to understand
    about this commit — the app session is genuinely gone, but with
    authentik's own SSO session still alive that `prompt=none` round trip
    SUCCEEDS and the user is logged straight back in. Ending the IdP session
    is R73's follow-up (the client does not read `end_session_endpoint` from
    discovery yet), and until it lands this route is correct plumbing whose
    user-visible effect is nil. */
    let sent_to = after
        .headers()
        .get(header::LOCATION)
        .and_then(|l| l.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        sent_to.contains("/application/o/authorize/") && sent_to.contains("prompt=none"),
        "the app session must be gone and re-authentication required; sent to {sent_to}"
    );
}

/// THE negative that matters: a logout with no proof of origin must be refused
/// AND must leave the session alive. A check that rejects the response while
/// still destroying the session would be worse than no check — it would be a
/// forced-logout hole that looked closed.
#[tokio::test(flavor = "multi_thread")]
async fn a_cross_site_logout_is_refused_and_leaves_the_session_alive() {
    let (base, _mock) = spawn_mock().await;
    let (store, sid) = seed_session("good-access", Some("refresh-1")).await;
    let oidc = oidc_state(&base, store).await;
    let cookie = format!("test_session={sid}");

    for (label, header_name, value) in [
        ("cross-site fetch metadata", "sec-fetch-site", "cross-site"),
        ("a foreign Origin", "origin", "https://evil.example"),
    ] {
        let refused = app(oidc.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/common-oidc/logout")
                    .header(header::COOKIE, &cookie)
                    .header(header_name, value)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            refused.status(),
            StatusCode::FORBIDDEN,
            "{label} must be refused"
        );
        assert!(
            oidc.store.get(sid).await.is_some(),
            "{label}: the session must SURVIVE a refused logout"
        );
    }

    // No fetch metadata and no Origin at all is a refusal too, not a pass:
    // otherwise a header-less POST from anywhere ends the session.
    let bare = app(oidc.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/common-oidc/logout")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bare.status(), StatusCode::FORBIDDEN);
    assert!(
        oidc.store.get(sid).await.is_some(),
        "still alive after a bare POST"
    );

    // And a GET is not a route at all — a GET logout is an <img src> away.
    let as_get = app(oidc)
        .oneshot(get_req("/common-oidc/logout", &cookie, true))
        .await
        .unwrap();
    assert_eq!(
        as_get.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "GET must not end a session"
    );
}

/// `end_session_endpoint` is OPTIONAL in the spec, so an IdP without it must
/// still log the user out of the app rather than 500 or hang. Exercised by
/// omission from the discovery document, which is why the fallback is not an
/// unverified branch.
#[tokio::test(flavor = "multi_thread")]
async fn without_an_end_session_endpoint_logout_still_ends_the_app_session() {
    let (base, mock) = spawn_mock_with(false).await;
    mock.valid_access.lock().unwrap().insert("at-live".into());
    let (store, sid) = seed_session("at-live", None).await;
    let oidc = oidc_state(&base, store).await;
    let cookie = format!("test_session={sid}");

    assert!(
        oidc.client.end_session_url().is_none(),
        "this mock must NOT advertise the endpoint, or the test proves nothing"
    );

    let logout = app(oidc.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/common-oidc/logout")
                .header(header::COOKIE, &cookie)
                .header("sec-fetch-site", "same-origin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(logout.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        logout.headers().get(header::LOCATION).unwrap(),
        "/",
        "no IdP to send them to, so back to the app root"
    );
    assert!(
        oidc.store.get(sid).await.is_none(),
        "the app session must be gone either way — doing LESS than asked is \
         the failure, doing nothing would be the worse one"
    );
}
