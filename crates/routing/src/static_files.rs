//! Serving embedded static files in one place: the `name -> Content-Type` map,
//! the traversal guard, and the bytes+type+cache response every service's
//! embedded client (`app.js`/`app.css`, a served shim, a launcher bundle) needs.
//! [`serve_static`] is the one response constructor; each caller keeps only the
//! part that genuinely differs — its auth guard.

use axum::body::Body;
use axum::http::{header, HeaderValue};
use axum::response::Response;

/// How a static response tells caches to treat it. There is no "unset": a
/// caller picks one deliberately, because an unversioned URL served
/// `immutable` hands back stale bytes after a deploy, and a content-hashed URL
/// served `no-cache` throws away the one cache win it was named for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CachePolicy {
    Immutable,
    NoCache,
}

impl CachePolicy {
    fn header(self) -> HeaderValue {
        match self {
            CachePolicy::Immutable => {
                HeaderValue::from_static("public, max-age=31536000, immutable")
            }
            CachePolicy::NoCache => HeaderValue::from_static("no-cache"),
        }
    }
}

pub fn serve_static(bytes: &[u8], content_type: &str, cache: CachePolicy) -> Response {
    let mut resp = Response::new(Body::from(bytes.to_vec()));
    let headers = resp.headers_mut();
    if let Ok(value) = HeaderValue::from_str(content_type) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    headers.insert(header::CACHE_CONTROL, cache.header());
    resp
}

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

/// The traversal guard for an asset path, single-segment OR nested. `Some(path)`
/// if every `/`-separated segment is safe — non-empty and not starting with a
/// dot — and the whole path is at most 512 bytes; `None` otherwise.
///
/// This admits a LEGITIMATE nested path (`sub/dir/app.js`, which role-ui's
/// `/assets/{*path}` and a nested [`crate::Router::static_dir`] serve) while
/// refusing every climb: a `..` segment (`../x`, `a/../b` — a `..` starts with
/// a dot), an absolute path or a `//` (a leading or doubled `/` makes an empty
/// segment), a trailing `/`, and a dotfile (`.hidden`, `.git/config`). It is a
/// syntactic guard over the name only: the bytes it protects are always an
/// exact key lookup in an EMBEDDED set (an [`AssetSet`] or a caller's own map),
/// never a filesystem join, so this refusing a climb is defence in depth over a
/// lookup that already cannot escape.
pub fn safe_asset_path(path: &str) -> Option<&str> {
    let ok = !path.is_empty()
        && path.len() <= 512
        && path
            .split('/')
            .all(|seg| !seg.is_empty() && !seg.starts_with('.'));
    ok.then_some(path)
}

pub struct AssetSet {
    files: &'static [(&'static str, &'static [u8])],
}

impl AssetSet {
    pub const fn new(files: &'static [(&'static str, &'static [u8])]) -> Self {
        Self { files }
    }

    /// The bytes for an EXACT name, or `None`. No traversal decision here —
    /// [`crate::Router::static_dir`] runs [`safe_asset_path`] first.
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
    fn safe_asset_path_accepts_plain_and_nested_files() {
        for good in [
            "app.js",
            "app.css",
            "favicon.svg",
            "index-a1b2c3.js",
            "x",
            "sub/app.js",
            "a/b/c.js",
            "chunks/vendor-9f8e.mjs",
        ] {
            assert_eq!(
                safe_asset_path(good),
                Some(good),
                "{good} should be allowed"
            );
        }
    }

    #[test]
    fn safe_asset_path_refuses_every_climb() {
        for bad in [
            "../x",
            "/x",
            "a/../b",
            "..",
            ".",
            "../../etc/passwd",
            "/etc/passwd",
            ".hidden",
            ".git/config",
            "a//b",
            "a/",
            "",
        ] {
            assert_eq!(safe_asset_path(bad), None, "{bad:?} must be refused");
        }
    }

    async fn body_of(resp: Response) -> Vec<u8> {
        to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body")
            .to_vec()
    }

    #[test]
    fn serve_static_sets_the_type_the_bytes_and_the_cache_policy() {
        let immut = serve_static(
            b"hello",
            "text/plain; charset=utf-8",
            CachePolicy::Immutable,
        );
        assert_eq!(
            immut
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("text/plain; charset=utf-8")
        );
        assert_eq!(
            immut
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("public, max-age=31536000, immutable")
        );

        let no_cache = serve_static(
            b"hi",
            "text/javascript; charset=utf-8",
            CachePolicy::NoCache,
        );
        assert_eq!(
            no_cache
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-cache")
        );
    }

    #[tokio::test]
    async fn static_file_serves_the_bytes_with_the_type_and_the_chosen_cache() {
        let app = Router::<()>::new()
            .static_file(
                "/thing.js",
                b"console.log(1)",
                "text/javascript; charset=utf-8",
                CachePolicy::Immutable,
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
        let r = Router::<()>::new().static_file("/a.css", b"x", "text/css", CachePolicy::NoCache);
        let m = r.manifest();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].method, "GET");
        assert_eq!(m[0].path, "/a.css");
    }

    #[test]
    fn two_static_files_get_distinct_fqnames() {
        let r = Router::<()>::new()
            .static_file("/a.css", b"x", "text/css", CachePolicy::NoCache)
            .static_file("/b.js", b"y", "text/javascript", CachePolicy::NoCache);
        let m = r.manifest();
        assert_eq!(m.len(), 2);
        assert_ne!(m[0].fqname, m[1].fqname, "static routes must not collide");
    }

    static ASSETS: AssetSet = AssetSet::new(&[("app.js", b"CLIENT"), ("app.css", b"SHEET")]);

    #[tokio::test]
    async fn static_dir_serves_a_named_file_with_the_chosen_cache() {
        let app = Router::<()>::new()
            .static_dir("/assets", &ASSETS, CachePolicy::NoCache)
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
        assert_eq!(
            resp.headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-cache")
        );
        assert_eq!(body_of(resp).await, b"SHEET");
    }

    #[tokio::test]
    async fn static_dir_404s_a_traversal_and_an_unknown_name() {
        let app = Router::<()>::new()
            .static_dir("/assets", &ASSETS, CachePolicy::Immutable)
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
