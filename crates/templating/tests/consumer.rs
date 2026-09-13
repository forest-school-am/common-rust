//! An asset-serving service in miniature: the §12.6 layer and parameter as a
//! consumer wires them. Anything assertable without a router belongs in
//! `src/assets_origin.rs`'s own tests.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use common_logging::Deployment;
use common_templating::{AssetCache, AssetsOrigin, Builder};
use http_body_util::BodyExt;
use tower::util::ServiceExt;

const PAGE: &str = "<!doctype html>\n\
<script type=\"module\" src=\"{{ assets_origin }}/common-ui@abc123/common-ui.js\"\n\
        integrity=\"sha384-xyz\" crossorigin=\"anonymous\"></script>\n\
<link rel=\"stylesheet\" href=\"{{ assets_origin }}/common-ui@abc123/palette.css\">\n";

const LOGIC: &[u8] = b"export const answer = 42;\n";

struct App {
    assets: Arc<AssetCache>,
    origin: AssetsOrigin,
}

fn dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "common-templating-consumer-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("index.html"), PAGE).unwrap();
    std::fs::write(d.join("page.js"), LOGIC).unwrap();
    d
}

fn app() -> Router {
    let d = dir();
    let assets = Builder::new(&d)
        .require_template("index.html")
        .build()
        .expect("boot");
    let origin = AssetsOrigin::parse(Some("https://assets.dev.local"), Deployment::Prod)
        .expect("accepted")
        .expect("present");
    let layer = origin.csp_layer();
    let state = Arc::new(App {
        assets: Arc::new(assets),
        origin,
    });

    Router::new()
        .route(
            "/",
            get(|State(app): State<Arc<App>>| async move {
                match app
                    .assets
                    .render("index.html", &[("assets_origin", app.origin.as_str())])
                {
                    Ok(html) => (
                        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                        html.to_string(),
                    )
                        .into_response(),
                    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
                }
            }),
        )
        .route(
            "/page.js",
            get(|State(app): State<Arc<App>>| async move {
                match app.assets.static_file("page.js") {
                    Ok(bytes) => ([(header::CONTENT_TYPE, "text/javascript")], bytes.to_vec())
                        .into_response(),
                    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
                }
            }),
        )
        .layer(layer)
        .with_state(state)
}

async fn get_path(path: &str) -> (StatusCode, Vec<(String, String)>, Vec<u8>) {
    let response = app()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_owned()))
        .collect();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, body.to_vec())
}

fn header_of(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.clone())
}

#[tokio::test]
async fn a_served_page_carries_the_csp_and_the_substituted_origin() {
    let (status, headers, body) = get_path("/").await;
    assert_eq!(status, StatusCode::OK);

    let csp = header_of(&headers, "content-security-policy").expect("the layer must set the CSP");
    assert_eq!(
        csp,
        "default-src 'self'; script-src 'self' https://assets.dev.local; \
         style-src 'self' https://assets.dev.local; img-src 'self' data:; connect-src 'self'; \
         object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'self'"
    );

    let html = String::from_utf8(body).unwrap();
    assert!(
        html.contains("src=\"https://assets.dev.local/common-ui@abc123/common-ui.js\""),
        "the origin must be substituted into the script tag: {html}"
    );
    assert!(
        html.contains("href=\"https://assets.dev.local/common-ui@abc123/palette.css\""),
        "and into the stylesheet link: {html}"
    );
    assert!(
        !html.contains("{{"),
        "no parameter may survive unsubstituted: {html}"
    );
}

#[tokio::test]
async fn a_static_asset_is_byte_identical_and_still_carries_the_csp() {
    let (status, headers, body) = get_path("/page.js").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body, LOGIC,
        "static mode must not touch the bytes: template substitution is the other mode"
    );
    assert!(
        header_of(&headers, "content-security-policy").is_some(),
        "the layer covers every response the service serves, not only pages"
    );
}

#[tokio::test]
async fn the_layer_never_emits_an_unsafe_directive() {
    let (_, headers, _) = get_path("/").await;
    let csp = header_of(&headers, "content-security-policy").expect("csp");
    assert!(!csp.contains("unsafe"), "{csp}");
    for required in [
        "object-src 'none'",
        "base-uri 'self'",
        "form-action 'self'",
        "frame-ancestors 'self'",
    ] {
        assert!(
            csp.contains(required),
            "{required} does not fall back to default-src, so it has to be present: {csp}"
        );
    }
}
