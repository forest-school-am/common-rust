//! Live assertions against a real authentik (§7.4): the load-bearing
//! assumptions about the IdP that only the IdP can answer. Env-gated, so it is
//! a no-op without the stand. Anything provable against a mock belongs in
//! mock_flow.rs.

use serde_json::{json, Value};
use url::Url;

use common_oidc::{OidcClient, OidcConfig};

struct Env {
    ak: String,
    token: String,
    user: String,
    password: String,
}

fn env() -> Option<Env> {
    if std::env::var("COMMON_OIDC_LIVE").as_deref() != Ok("1") {
        eprintln!("live canary skipped: set COMMON_OIDC_LIVE=1 with the teststand running");
        return None;
    }
    let get = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_owned());
    Some(Env {
        ak: get("COMMON_OIDC_AK", "http://127.0.0.1:8000"),
        token: get("COMMON_OIDC_AK_TOKEN", "teststand-api-token"),
        user: get("COMMON_OIDC_USER", "alice"),
        password: get("COMMON_OIDC_PASSWORD", "123456"),
    })
}

async fn sso_login(browser: &reqwest::Client, e: &Env) {
    let exec = format!(
        "{}/api/v3/flows/executor/default-authentication-flow/?query=",
        e.ak
    );
    let mut challenge: Value = browser
        .get(&exec)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .expect("executor challenge");
    for _ in 0..8 {
        let component = challenge["component"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
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
        let flow_url = Url::parse(&format!("{}{}", e.ak, loc)).unwrap();
        let slug = flow_url
            .path()
            .trim_matches('/')
            .rsplit('/')
            .next()
            .unwrap()
            .to_owned();
        let query = flow_url.query().unwrap_or_default();
        let exec = format!(
            "{}/api/v3/flows/executor/{}/?query={}",
            e.ak,
            slug,
            urlenc(query)
        );
        let challenge: Value = browser
            .get(&exec)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .expect("authz executor");
        let component = challenge["component"].as_str().unwrap_or_default();
        assert_eq!(
            component, "xak-flow-redirect",
            "unexpected authorize stage {component}: {challenge}"
        );
        let to = challenge["to"]
            .as_str()
            .expect("redirect target")
            .to_owned();
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

    let jar = std::sync::Arc::new(reqwest::cookie::Jar::default());
    let browser = reqwest::Client::builder()
        .cookie_provider(jar.clone())
        .build()
        .unwrap();
    let http = reqwest::Client::builder()
        .cookie_provider(jar)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    sso_login(&browser, &e).await;

    let config = OidcConfig::new(
        Url::parse(&format!("{}/application/o/common-oidc-canary/", e.ak)).unwrap(),
        "common-oidc-canary",
        Url::parse("http://127.0.0.1:18999/cb").unwrap(),
        "test_session",
    )
    .request_refresh_tokens(); // the whole point is to test refresh revocation
    let client = OidcClient::discover(config)
        .await
        .expect("discovery against live stand");
    let auth = client.authorize_url(false);
    let (auth_url, verifier) = (auth.url, auth.pkce_verifier);
    let code = authorization_code(&http, &browser, &e, &auth_url).await;
    let tokens = client
        .exchange_code(code, verifier)
        .await
        .expect("code exchange");
    let refresh = tokens.refresh_token.clone().expect(
        "no refresh token issued — is offline_access allowed on the canary provider (setup.py)?",
    );

    let p = client
        .principal_from_access_token(&tokens.access_token)
        .await
        .expect("userinfo while session is alive");
    assert_eq!(p.username, e.user);
    assert!(
        !p.effective_groups.is_empty(),
        "effective_groups claim must be populated"
    );
    let refreshed = client
        .refresh(&refresh)
        .await
        .expect("refresh while session is alive");
    let live_access = refreshed.access_token;
    let live_refresh = refreshed.refresh_token.unwrap();
    client
        .principal_from_access_token(&live_access)
        .await
        .expect("refreshed access token must work");

    let admin = reqwest::Client::new();
    let sessions: Value = admin
        .get(format!(
            "{}/api/v3/core/authenticated_sessions/?username={}",
            e.ak, e.user
        ))
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
    assert!(
        !uuids.is_empty(),
        "expected at least one live session for {}",
        e.user
    );
    for uuid in uuids {
        let r = admin
            .delete(format!(
                "{}/api/v3/core/authenticated_sessions/{}/",
                e.ak, uuid
            ))
            .bearer_auth(&e.token)
            .send()
            .await
            .unwrap();
        assert!(
            r.status().is_success(),
            "session delete failed: {}",
            r.status()
        );
    }

    let access_dead = client
        .principal_from_access_token(&live_access)
        .await
        .is_err();
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

#[tokio::test]
async fn effective_groups_is_a_downward_closure_never_an_upward_one() {
    let Some(mut e) = env() else { return };

    let admin = reqwest::Client::new();
    let pk = |name: &'static str, ak: String, tok: String, c: reqwest::Client| async move {
        let v: Value = c
            .get(format!("{ak}/api/v3/core/groups/?name={name}"))
            .bearer_auth(tok)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let pk = v["results"][0]["pk"]
            .as_str()
            .unwrap_or_else(|| panic!("group {name} not in the stand — run stand/setup.py"));
        uuid::Uuid::parse_str(pk).unwrap()
    };
    let g = |n| pk(n, e.ak.clone(), e.token.clone(), admin.clone());
    let (root, ops, dev, dev_junior, search_users) = (
        g("root").await,
        g("ops").await,
        g("dev").await,
        g("dev-junior").await,
        g("search-users").await,
    );

    e.user = "dave".into();
    let mut dave = groups_of(&e).await;
    dave.sort();
    let mut want = vec![ops, dev_junior, search_users];
    want.sort();
    assert_eq!(
        dave, want,
        "dave is a direct member of ops only: the claim must add ops's descendant dev-junior \
         (downward closure) and must not add ops's parent root"
    );

    e.user = "carol".into();
    let mut carol = groups_of(&e).await;
    carol.sort();
    let mut want = vec![dev_junior, search_users];
    want.sort();
    assert_eq!(
        carol, want,
        "carol is a direct member of the leaf dev-junior: the claim must contain no ancestor \
         (dev, ops, root) — an upward closure here would silently widen every group gate"
    );
    assert!(!carol.contains(&root) && !carol.contains(&dev) && !carol.contains(&ops));
}

async fn groups_of(e: &Env) -> Vec<uuid::Uuid> {
    let jar = std::sync::Arc::new(reqwest::cookie::Jar::default());
    let browser = reqwest::Client::builder()
        .cookie_provider(jar.clone())
        .build()
        .unwrap();
    let http = reqwest::Client::builder()
        .cookie_provider(jar)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    sso_login(&browser, e).await;
    let config = OidcConfig::new(
        Url::parse(&format!("{}/application/o/common-oidc-canary/", e.ak)).unwrap(),
        "common-oidc-canary",
        Url::parse("http://127.0.0.1:18999/cb").unwrap(),
        "test_session",
    );
    let client = OidcClient::discover(config)
        .await
        .expect("discovery against live stand");
    let auth = client.authorize_url(false);
    let code = authorization_code(&http, &browser, e, &auth.url).await;
    let tokens = client
        .exchange_code(code, auth.pkce_verifier)
        .await
        .expect("code exchange");
    client
        .principal_from_access_token(&tokens.access_token)
        .await
        .expect("userinfo")
        .effective_groups
}
