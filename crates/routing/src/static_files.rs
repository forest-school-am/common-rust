//! Serving embedded static files, ONCE (review followup #9). Every service in
//! the fleet embeds its compiled client (`app.js`/`app.css`, a served shim, a
//! launcher bundle) and hand-rolled the same three things to serve it: a
//! `name -> Content-Type` map, a path-traversal guard, and a bytes+type
//! response. They live here now, in one place, and each caller keeps only the
//! part that genuinely differs — its cache policy and its auth guard.
//!
//! Two shapes:
//!
//! * The FREE helpers ([`serve_static`], [`content_type_for`],
//!   [`safe_asset_name`]) are for a RAW axum handler — a caller that already
//!   has a handler (because it carries an auth extractor, a wildcard path, or
//!   a bespoke 404 body) and only wants the shared pieces.
//! * [`crate::Router::static_file`] / [`crate::Router::static_dir`] are the
//!   turnkey form for the plain case: they register a GET route through the
//!   recording path, so the file shows in the manifest like any route, and
//!   serve with a long-lived immutable cache.

use axum::body::Body;
use axum::http::{header, HeaderValue};
use axum::response::Response;

/// The immutable cache header for a fixed embedded file served through
/// [`crate::Router::static_file`] / [`crate::Router::static_dir`]. A caller
/// whose bytes change under a stable URL across restarts (a non-hashed
/// `launcher.js`, say) must NOT use these; it serves through [`serve_static`]
/// and sets `no-cache` itself.
const IMMUTABLE: HeaderValue = HeaderValue::from_static("public, max-age=31536000, immutable");

/// A bytes-with-a-Content-Type response, and NOTHING else — no cache header,
/// no status but 200. The one construction every static handler shared. The
/// caller layers on whatever cache policy (or `Vary`, or auth) it needs.
pub fn serve_static(bytes: &[u8], content_type: &str) -> Response {
    let mut resp = Response::new(Body::from(bytes.to_vec()));
    if let Ok(value) = HeaderValue::from_str(content_type) {
        resp.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    resp
}

/// As [`serve_static`], plus the immutable cache header. Used by the [`Router`]
/// conveniences; kept crate-private because a caller that reaches for a cache
/// header should pick it deliberately.
///
/// [`Router`]: crate::Router
pub(crate) fn serve_static_immutable(bytes: &[u8], content_type: &str) -> Response {
    let mut resp = serve_static(bytes, content_type);
    resp.headers_mut().insert(header::CACHE_CONTROL, IMMUTABLE);
    resp
}

/// `name -> Content-Type`, by file extension. The union of what the fleet
/// served (lifted from authentik-role-UI's `render::content_type_for`); an
/// unknown extension is `application/octet-stream`, never a guess.
pub fn content_type_for(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "map" | "json" => "application/json",
        "html" => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The traversal guard for a SINGLE-SEGMENT asset name (lifted from cron's
/// `bundles::valid_name` and its `/assets/{name}` refusal). `Some(name)` if the
/// name is a plain file — ASCII alphanumerics and `-_.`, non-empty, not longer
/// than 120, not starting with a dot — and `None` otherwise.
///
/// This refuses every case cron's `asset_names_that_climb_out_are_refused`
/// covers: a `/` (so `has/slash`, and a decoded `%2Fetc%2Fpasswd`), a leading
/// dot (so `..`, `.hidden`, and a decoded `..%2FCargo.toml`), and a literal
/// `%` (so the still-encoded `..%2F…` and a leading `%2F` are refused even if a
/// caller never decodes them). It is NOT for a nested path: a `{*path}` handler
/// that legitimately serves `sub/dir/file.js` must keep a guard that permits
/// `/` (e.g. `common_templating`'s), which this deliberately does not.
pub fn safe_asset_name(name: &str) -> Option<&str> {
    let ok = !name.is_empty()
        && name.len() <= 120
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    ok.then_some(name)
}

/// A fixed set of embedded files, keyed by name, for [`crate::Router::static_dir`].
/// Ordered (a slice), so iteration and the manifest are stable. The
/// Content-Type is inferred from the name by [`content_type_for`].
pub struct AssetSet {
    files: &'static [(&'static str, &'static [u8])],
}

impl AssetSet {
    /// `AssetSet::new(&[("app.js", APP_JS), ("app.css", APP_CSS)])`.
    pub const fn new(files: &'static [(&'static str, &'static [u8])]) -> Self {
        Self { files }
    }

    /// The bytes for an EXACT name, or `None`. No traversal decision here —
    /// [`crate::Router::static_dir`] runs [`safe_asset_name`] first.
    pub fn get(&self, name: &str) -> Option<&'static [u8]> {
        self.files
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, bytes)| *bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Router;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[test]
    fn content_type_is_by_extension_and_unknown_is_octet_stream() {
        for (name, ct) in [
            ("app.js", "text/javascript; charset=utf-8"),
            ("m.mjs", "text/javascript; charset=utf-8"),
            ("app.css", "text/css; charset=utf-8"),
            ("logo.svg", "image/svg+xml"),
            ("x.png", "image/png"),
            ("favicon.ico", "image/x-icon"),
            ("f.woff2", "font/woff2"),
            ("app.js.map", "application/json"),
            ("data.json", "application/json"),
            ("page.html", "text/html; charset=utf-8"),
            ("noext", "application/octet-stream"),
            ("thing.bin", "application/octet-stream"),
        ] {
            assert_eq!(content_type_for(name), ct, "{name}");
        }
    }

    #[test]
    fn safe_asset_name_accepts_plain_files() {
        for good in ["app.js", "app.css", "favicon.svg", "index-a1b2c3.js", "x"] {
            assert_eq!(
                safe_asset_name(good),
                Some(good),
                "{good} should be allowed"
            );
        }
    }

    #[test]
    fn safe_asset_name_refuses_the_names_cron_refuses() {
        // The three from cron's `asset_names_that_climb_out_are_refused`, both
        // still-encoded and decoded, plus the plain climbers.
        for bad in [
            "..%2FCargo.toml",
            "..%2F..%2Fetc%2Fpasswd",
            "%2Fetc%2Fpasswd",
            "../Cargo.toml",
            "../../etc/passwd",
            "/etc/passwd",
            "..",
            ".hidden",
            "has/slash",
            "has space",
            "",
        ] {
            assert_eq!(safe_asset_name(bad), None, "{bad:?} must be refused");
        }
    }

    async fn body_of(resp: Response) -> Vec<u8> {
        to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body")
            .to_vec()
    }

    #[test]
    fn serve_static_sets_the_type_and_the_bytes() {
        let resp = serve_static(b"hello", "text/plain; charset=utf-8");
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("text/plain; charset=utf-8")
        );
        assert!(resp.headers().get(header::CACHE_CONTROL).is_none());
    }

    #[tokio::test]
    async fn static_file_serves_the_bytes_with_the_type_and_a_cache_header() {
        let app = Router::<()>::new()
            .static_file(
                "/thing.js",
                b"console.log(1)",
                "text/javascript; charset=utf-8",
            )
            .into_axum();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/thing.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("text/javascript; charset=utf-8")
        );
        assert_eq!(
            resp.headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("public, max-age=31536000, immutable")
        );
        assert_eq!(body_of(resp).await, b"console.log(1)");
    }

    #[test]
    fn static_file_shows_in_the_manifest() {
        let r = Router::<()>::new().static_file("/a.css", b"x", "text/css");
        let m = r.manifest();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].method, "GET");
        assert_eq!(m[0].path, "/a.css");
    }

    static ASSETS: AssetSet = AssetSet::new(&[("app.js", b"CLIENT"), ("app.css", b"SHEET")]);

    #[tokio::test]
    async fn static_dir_serves_a_named_file() {
        let app = Router::<()>::new()
            .static_dir("/assets", &ASSETS)
            .into_axum();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/assets/app.css")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("text/css; charset=utf-8")
        );
        assert_eq!(body_of(resp).await, b"SHEET");
    }

    #[tokio::test]
    async fn static_dir_404s_a_traversal_and_an_unknown_name() {
        let app = Router::<()>::new()
            .static_dir("/assets", &ASSETS)
            .into_axum();
        for name in ["..%2FCargo.toml", "nope.js"] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/assets/{name}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{name}");
            let body = body_of(resp).await;
            assert!(!body.windows(7).any(|w| w == b"[packag"), "leaked file");
        }
    }
}
