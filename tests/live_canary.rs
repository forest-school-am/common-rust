//! LIVE canary against the real teststand authentik: pins the
//! **instant-logout assumption** — authentik revokes access AND refresh
//! tokens when the SSO session ends. This behavior is version-dependent
//! (observed on 2026.5.2; goauthentik#13780 claims otherwise for other
//! versions), and the whole "server-side refresh cannot outlive logout"
//! guarantee of ruling v5 rests on it. **Run this against any authentik
//! upgrade — the 2026.8 migration in particular — before trusting it.**
//!
//! Gated: only runs with STAND_LIVE=1 (needs the teststand up and
//! setup.py's `stand-oidc-canary` provider). Overridable env:
//!   STAND_AK        base URL          (default http://127.0.0.1:8000)
//!   STAND_AK_TOKEN  admin API token   (default teststand-api-token)
//!   STAND_USER / STAND_PASSWORD       (default alice / 123456)

use serde_json::{json, Value};
use url::Url;

use stand_oidc::{OidcConfig, StandClient};

struct Env {
    ak: String,
    token: String,
    user: String,
    password: String,
}

fn env() -> Option<Env> {
    if std::env::var("STAND_LIVE").as_deref() != Ok("1") {
        eprintln!("live canary skipped: set STAND_LIVE=1 with the teststand running");
        return None;
    }
    let get = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_owned());
    Some(Env {
        ak: get("STAND_AK", "http://127.0.0.1:8000"),
        token: get("STAND_AK_TOKEN", "teststand-api-token"),
        user: get("STAND_USER", "alice"),
        password: get("STAND_PASSWORD", "123456"),
    })
}

/// Log in through the flow executor like a browser would, leaving the SSO
/// session cookie in the shared jar. `browser` follows redirects (the
/// executor uses POST-redirect-GET chains).
async fn sso_login(browser: &reqwest::Client, e: &Env) {
    let exec = format!("{}/api/v3/flows/executor/default-authentication-flow/?query=", e.ak);
    let mut challenge: Value =
        browser.get(&exec).send().await.unwrap().json().await.expect("executor challenge");
    for _ in 0..8 {
        let component = challenge["component"].as_str().unwrap_or_default().to_owned();
        let answer = match component.as_str() {
            "ak-stage-identification" => json!({ "uid_field": e.user }),
            "ak-stage-password" => json!({ "password": e.password }),
            "xak-flow-redirect" => return,
            other => panic!("unexpected auth stage {other}: {challenge}"),
        };
        challenge = browser
            .post(&exec)
            .json(&answer)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .expect("executor step");
    }
    panic!("authentication flow did not converge: {challenge}");
}

/// Drive the authorize redirect + implicit-consent flow to an authorization
/// code, using the crate's own authorize URL (PKCE and scopes included).
/// `http` has redirects DISABLED (we sniff Location for the code);
/// `browser` follows them (executor PRG chains).
async fn authorization_code(
    http: &reqwest::Client,
    browser: &reqwest::Client,
    e: &Env,
    authorize_url: &Url,
) -> String {
    const REDIRECT: &str = "http://127.0.0.1:18999/cb";
    let code_of = |u: &str| -> Option<String> {
        u.starts_with(REDIRECT).then(|| {
            Url::parse(u)
                .unwrap()
                .query_pairs()
                .find(|(k, _)| k == "code")
                .map(|(_, v)| v.into_owned())
                .expect("redirect back carries no code")
        })
    };

    let mut current = authorize_url.clone();
    for _ in 0..5 {
        let resp = http.get(current.clone()).send().await.unwrap();
        let loc = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        if let Some(code) = code_of(&loc) {
            return code;
        }
        assert!(
            loc.contains("/if/flow/"),
            "authorize did not enter a flow (status {}): {loc}",
            resp.status()
        );
        // same flow, executor API form
        let flow_url = Url::parse(&format!("{}{}", e.ak, loc)).unwrap();
        let slug = flow_url.path().trim_matches('/').rsplit('/').next().unwrap().to_owned();
        let query = flow_url.query().unwrap_or_default();
        let exec = format!("{}/api/v3/flows/executor/{}/?query={}", e.ak, slug, urlenc(query));
        let challenge: Value =
            browser.get(&exec).send().await.unwrap().json().await.expect("authz executor");
        for _ in 0..8 {
            match challenge["component"].as_str().unwrap_or_default() {
                "xak-flow-redirect" => break,
                other => panic!("unexpected authorize stage {other}: {challenge}"),
            }
        }
        let to = challenge["to"].as_str().expect("redirect target").to_owned();
        if let Some(code) = code_of(&to) {
            return code;
        }
        current = Url::parse(&to)
            .or_else(|_| Url::parse(&format!("{}{}", e.ak, to)))
            .expect("absolute redirect");
    }
    panic!("authorization never redirected back to {REDIRECT}");
}

fn urlenc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[tokio::test]
async fn instant_logout_kills_access_and_refresh_tokens() {
    let Some(e) = env() else { return };

    // one cookie jar, two views of it: `browser` follows redirects (flow
    // executor), `http` does not (Location sniffing on authorize)
    let jar = std::sync::Arc::new(reqwest::cookie::Jar::default());
    let browser = reqwest::Client::builder().cookie_provider(jar.clone()).build().unwrap();
    let http = reqwest::Client::builder()
        .cookie_provider(jar)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    sso_login(&browser, &e).await;

    // the crate under test does discovery, PKCE, exchange, refresh, userinfo
    let config = OidcConfig::new(
        Url::parse(&format!("{}/application/o/stand-oidc-canary/", e.ak)).unwrap(),
        "stand-oidc-canary",
        Url::parse("http://127.0.0.1:18999/cb").unwrap(),
    )
    .request_refresh_tokens(); // the whole point is to test refresh revocation
    let client = StandClient::discover(config).await.expect("discovery against live stand");
    let (auth_url, _state, verifier) = client.authorize_url(false);
    let code = authorization_code(&http, &browser, &e, &auth_url).await;
    let tokens = client.exchange_code(code, verifier).await.expect("code exchange");
    let refresh = tokens.refresh_token.clone().expect(
        "no refresh token issued — is offline_access allowed on the canary provider (setup.py)?",
    );

    // sanity: both tokens work while the SSO session lives
    let p = client
        .principal_from_access_token(&tokens.access_token)
        .await
        .expect("userinfo while session is alive");
    assert_eq!(p.username, e.user);
    assert!(!p.effective_groups.is_empty(), "effective_groups claim must be populated");
    let refreshed = client.refresh(&refresh).await.expect("refresh while session is alive");
    let live_access = refreshed.access_token;
    let live_refresh = refreshed.refresh_token.unwrap();
    client
        .principal_from_access_token(&live_access)
        .await
        .expect("refreshed access token must work");

    // end the SSO session administratively (same effect as user logout)
    let admin = reqwest::Client::new();
    let sessions: Value = admin
        .get(format!("{}/api/v3/core/authenticated_sessions/?username={}", e.ak, e.user))
        .bearer_auth(&e.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let uuids: Vec<String> = sessions["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["uuid"].as_str().map(String::from))
        .collect();
    assert!(!uuids.is_empty(), "expected at least one live session for {}", e.user);
    for uuid in uuids {
        let r = admin
            .delete(format!("{}/api/v3/core/authenticated_sessions/{}/", e.ak, uuid))
            .bearer_auth(&e.token)
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success(), "session delete failed: {}", r.status());
    }

    // THE CANARY: both tokens must be dead IMMEDIATELY — no grace, no TTL.
    let access_dead = client.principal_from_access_token(&live_access).await.is_err();
    let refresh_dead = client.refresh(&live_refresh).await.is_err();
    assert!(
        access_dead,
        "ACCESS token survived session end — instant logout is BROKEN on this authentik; \
         ruling v5's refresh model is unsafe here"
    );
    assert!(
        refresh_dead,
        "REFRESH token survived session end — instant logout is BROKEN on this authentik; \
         ruling v5's refresh model is unsafe here"
    );
}
