//! The recording [`Router`] wrapper over `axum::Router`. The manifest data type
//! belongs in `manifest`; embedded-asset serving belongs in `static_files`.

use std::any::type_name;
use std::convert::Infallible;
use std::path::Path;

use axum::extract::{Path as AssetPath, Request};
use axum::handler::Handler;
use axum::http::{Method, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{MethodRouter, Route};
use tower_layer::Layer;
use tower_service::Service;

use crate::manifest::{parse_path_params, write_manifest, Registration};
use crate::static_files::{content_type_for, safe_asset_path, serve_static, AssetSet, CachePolicy};

pub struct Router<S = ()> {
    inner: axum::Router<S>,
    manifest: Vec<Registration>,
}

impl<S> Default for Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            inner: axum::Router::new(),
            manifest: Vec::new(),
        }
    }

    fn record<H, T>(self, method: Method, path: &str, method_router: MethodRouter<S>) -> Self
    where
        H: Handler<T, S>,
    {
        self.record_raw(type_name::<H>().to_owned(), method, path, method_router)
    }

    /// Takes an explicit fqname because the static-file helpers all share one
    /// closure type, whose `type_name` would collide their mounts; a synthetic
    /// per-path name never matches a `#[client]` fqname, so such routes never
    /// join to a client function.
    fn record_raw(
        mut self,
        fqname: String,
        method: Method,
        path: &str,
        method_router: MethodRouter<S>,
    ) -> Self {
        self.manifest.push(Registration {
            fqname,
            method: method.as_str().to_owned(),
            path: path.to_owned(),
            path_params: parse_path_params(path),
        });
        // axum merges a second method on an already-registered path, so
        // `.get("/x", a).post("/x", b)` is one path with two methods.
        self.inner = self.inner.route(path, method_router);
        self
    }

    pub fn get<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.record::<H, T>(Method::GET, path, axum::routing::get(handler))
    }

    pub fn post<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.record::<H, T>(Method::POST, path, axum::routing::post(handler))
    }

    pub fn put<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.record::<H, T>(Method::PUT, path, axum::routing::put(handler))
    }

    pub fn delete<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.record::<H, T>(Method::DELETE, path, axum::routing::delete(handler))
    }

    pub fn patch<H, T>(self, path: &str, handler: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.record::<H, T>(Method::PATCH, path, axum::routing::patch(handler))
    }

    pub fn static_file(
        self,
        path: &str,
        bytes: &'static [u8],
        content_type: &'static str,
        cache: CachePolicy,
    ) -> Self {
        let fqname = format!("<static_file> GET {path}");
        self.record_raw(
            fqname,
            Method::GET,
            path,
            axum::routing::get(move || async move { serve_static(bytes, content_type, cache) }),
        )
    }

    pub fn static_dir(self, prefix: &str, files: &'static AssetSet, cache: CachePolicy) -> Self {
        let path = format!("{}/{{name}}", prefix.trim_end_matches('/'));
        let fqname = format!("<static_dir> GET {path}");
        self.record_raw(
            fqname,
            Method::GET,
            &path,
            axum::routing::get(move |AssetPath(name): AssetPath<String>| async move {
                match safe_asset_path(&name).and_then(|n| files.get(n)) {
                    Some(bytes) => serve_static(bytes, content_type_for(&name), cache),
                    None => StatusCode::NOT_FOUND.into_response(),
                }
            }),
        )
    }

    /// axum's own `route`, NOT recorded into the manifest.
    pub fn route(mut self, path: &str, method_router: MethodRouter<S>) -> Self {
        self.inner = self.inner.route(path, method_router);
        self
    }

    pub fn nest(mut self, prefix: &str, router: Router<S>) -> Self {
        let base = prefix.trim_end_matches('/');
        for mut reg in router.manifest {
            reg.path = if reg.path == "/" {
                base.to_owned()
            } else {
                format!("{base}{}", reg.path)
            };
            reg.path_params = parse_path_params(&reg.path);
            self.manifest.push(reg);
        }
        self.inner = self.inner.nest(prefix, router.inner);
        self
    }

    pub fn merge(mut self, router: Router<S>) -> Self {
        self.manifest.extend(router.manifest);
        self.inner = self.inner.merge(router.inner);
        self
    }

    pub fn layer<L>(mut self, layer: L) -> Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        self.inner = self.inner.layer(layer);
        self
    }

    pub fn route_layer<L>(mut self, layer: L) -> Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        self.inner = self.inner.route_layer(layer);
        self
    }

    pub fn with_state<S2>(self, state: S) -> Router<S2> {
        Router {
            inner: self.inner.with_state(state),
            manifest: self.manifest,
        }
    }

    pub fn manifest(&self) -> &[Registration] {
        &self.manifest
    }

    pub fn into_axum(self) -> axum::Router<S> {
        self.inner
    }

    pub fn write_manifest(&self, path: &Path) -> std::io::Result<()> {
        write_manifest(&self.manifest, path)
    }
}
