//! The recording router: axum's shape, plus a manifest.

use std::any::type_name;
use std::convert::Infallible;
use std::path::Path;

use axum::extract::Request;
use axum::handler::Handler;
use axum::http::Method;
use axum::response::IntoResponse;
use axum::routing::{MethodRouter, Route};
use tower_layer::Layer;
use tower_service::Service;

use crate::manifest::{parse_path_params, write_manifest, Registration};

/// An `axum::Router<S>` that remembers what was registered on it.
///
/// `.get(path, handler)` and its siblings are the RECORDING form: they take
/// the handler itself, so its `type_name` can be captured. [`Router::route`]
/// with a ready-made `MethodRouter` (axum's `get(handler)`) is passed through
/// unrecorded — the name is gone by then. Register through the method calls
/// and nothing is missing from the manifest.
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

    fn record<H, T>(mut self, method: Method, path: &str, method_router: MethodRouter<S>) -> Self
    where
        H: Handler<T, S>,
    {
        self.manifest.push(Registration {
            fqname: type_name::<H>().to_owned(),
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

    /// axum's own `route`: NOT recorded (see the type docs). For routes that
    /// must not appear in the manifest, or a `MethodRouter` built elsewhere.
    pub fn route(mut self, path: &str, method_router: MethodRouter<S>) -> Self {
        self.inner = self.inner.route(path, method_router);
        self
    }

    /// Nest a recorded router; its registrations join this manifest with
    /// `prefix` applied (the nested `/` is the prefix itself).
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

    /// Merge a recorded router; its registrations join this manifest as they are.
    pub fn merge(mut self, router: Router<S>) -> Self {
        self.manifest.extend(router.manifest);
        self.inner = self.inner.merge(router.inner);
        self
    }

    /// Passthrough of `axum::Router::layer` (applies to routes added so far).
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

    /// Passthrough of `axum::Router::route_layer`.
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

    /// Passthrough of `axum::Router::with_state`; the manifest carries over.
    pub fn with_state<S2>(self, state: S) -> Router<S2> {
        Router {
            inner: self.inner.with_state(state),
            manifest: self.manifest,
        }
    }

    /// Everything registered through the recording methods, in call order.
    pub fn manifest(&self) -> &[Registration] {
        &self.manifest
    }

    /// The axum router, to serve.
    pub fn into_axum(self) -> axum::Router<S> {
        self.inner
    }

    /// `routes.json`: the manifest, sorted and pretty-printed.
    pub fn write_manifest(&self, path: &Path) -> std::io::Result<()> {
        write_manifest(&self.manifest, path)
    }
}
