//! Authorization ORCHESTRATION, shared by the stand's apps (R115). The two
//! validated identities live elsewhere and are never re-implemented here: the
//! browser OIDC session in [`OidcState`] (cookie → `resolve_session`) and the
//! bearer service-account token in [`BearerValidator`]. What each app used to
//! hand-roll — a rejecting middleware, an `Auth` extension enum, a `check`/
//! `deny` pair and a handful of extractors — is what this module carries once:
//!
//! 1. [`set_auth_context`], a middleware that only POPULATES state and NEVER
//!    rejects. It resolves bearer-then-session into an [`AuthContext`] in the
//!    request extensions, sets the log actor, owns the request span, and
//!    returns. An invalid token becomes [`AuthContext::Anonymous`], not a 401.
//! 2. A family of extractors a handler names for the level it needs:
//!    [`Authenticated`], [`ServiceAccount`], [`MaybeAuthenticated`], and the
//!    generic [`GatedBy`] driven by a [`Predicate`] the app declares — usually
//!    through the [`HasGroup`] marker and the [`Or`]/[`And`]/[`Not`]
//!    combinators.
//!
//! THE SEAM (R115). The DECISION is shared; the RESPONSE BYTES are the app's.
//! cron answers a refusal with its `ApiError` JSON and a denied HTML shell;
//! registry with plain text; a future adopter with the crate defaults. So
//! [`AuthProviders`] carries an optional pair of [`Refusals`] renderers the
//! extractors call, falling back to a minimal wire JSON when an app supplies
//! none. The HTML login redirect for an anonymous BROWSER is the one refusal
//! the crate always renders itself (via [`OidcState::login_redirect`]), because
//! only it holds the flow store.

use std::marker::PhantomData;
use std::sync::Arc;

use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use tracing::Instrument;
use uuid::Uuid;

use common_logging as log;

use crate::bearer::BearerValidator;
use crate::principal::Principal;
use crate::web::OidcState;

// ---------------------------------------------------------------------------
// The context the middleware resolves.
// ---------------------------------------------------------------------------

/// How a principal proved who they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthVia {
    /// A browser cookie resolved through [`OidcState`].
    Session,
    /// A bearer token validated through [`BearerValidator`].
    Bearer,
    /// The dev-only stub, standing in for a real identity under `--no-auth`.
    Stub,
}

/// What [`set_auth_context`] resolved for a request, stored in its extensions.
/// The middleware NEVER rejects: an absent or invalid credential becomes
/// [`AuthContext::Anonymous`], and the extractor a handler names decides the
/// response.
#[derive(Debug, Clone)]
pub enum AuthContext {
    /// Authentication is switched off for this deployment (`--no-auth`), or a
    /// dev-stub deployment saw no real credential. Gates are NOT consulted; the
    /// actor is the configured [`AuthProviders::dev_stub`], else a synthesized
    /// principal with username `-` and no groups.
    Disabled,
    /// A validated caller.
    Authenticated {
        principal: Arc<Principal>,
        via: AuthVia,
    },
    /// No valid credential. The reason is logged, never returned to the caller.
    Anonymous { reason: String },
}

// ---------------------------------------------------------------------------
// Providers + the response seam.
// ---------------------------------------------------------------------------

type UnauthorizedFn = Arc<dyn Fn(&Parts) -> Response + Send + Sync>;
type DeniedFn = Arc<dyn Fn(&Parts, &Denial) -> Response + Send + Sync>;

/// The two refusal renderers an app supplies so a SHARED extractor keeps that
/// app's exact response bytes. `None` uses the crate's wire-JSON default. This
/// is the documented seam: cron installs both (its `ApiError` shapes and its
/// denied shell); registry installs neither and takes the defaults; role-ui and
/// les-forms will do the same.
#[derive(Clone, Default)]
pub struct Refusals {
    /// The WIRE 401 for an anonymous caller (a machine, or a browser under an
    /// app with no OIDC redirect). The HTML login redirect is handled by the
    /// crate before this is consulted.
    pub unauthorized: Option<UnauthorizedFn>,
    /// The 403 for a signed-in caller a gate refused; receives the [`Denial`]
    /// so it can name the group.
    pub denied: Option<DeniedFn>,
}

impl std::fmt::Debug for Refusals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Refusals")
            .field("unauthorized", &self.unauthorized.is_some())
            .field("denied", &self.denied.is_some())
            .finish()
    }
}

/// Everything the middleware needs to resolve an identity, plus the app's
/// refusal renderers. `#[derive(Clone)]` so it is both the `from_fn_with_state`
/// state and a value stashed in the request extensions for the extractors.
#[derive(Clone)]
pub struct AuthProviders {
    /// The browser-session validator, or `None` for a bearer-only / no-auth app.
    pub oidc: Option<OidcState>,
    /// The bearer validator, or `None` for a session-only / no-auth app.
    pub bearer: Option<BearerValidator>,
    /// The dev-only stub identity, `Some` only under `--no-auth`.
    pub dev_stub: Option<Arc<Principal>>,
    /// App-supplied refusal renderers (the seam above).
    pub refusals: Refusals,
}

impl AuthProviders {
    /// The common case: providers with the crate-default refusals.
    pub fn new(
        oidc: Option<OidcState>,
        bearer: Option<BearerValidator>,
        dev_stub: Option<Arc<Principal>>,
    ) -> Self {
        Self {
            oidc,
            bearer,
            dev_stub,
            refusals: Refusals::default(),
        }
    }

    /// Install the app's 401 renderer (wire callers; the HTML redirect stays
    /// the crate's).
    pub fn on_unauthorized(
        mut self,
        f: impl Fn(&Parts) -> Response + Send + Sync + 'static,
    ) -> Self {
        self.refusals.unauthorized = Some(Arc::new(f));
        self
    }

    /// Install the app's 403 renderer, which receives the [`Denial`].
    pub fn on_denied(
        mut self,
        f: impl Fn(&Parts, &Denial) -> Response + Send + Sync + 'static,
    ) -> Self {
        self.refusals.denied = Some(Arc::new(f));
        self
    }
}

// ---------------------------------------------------------------------------
// The middleware.
// ---------------------------------------------------------------------------

/// Populate [`AuthContext`] and return. Mounted with
/// `axum::middleware::from_fn_with_state(providers, common_oidc::auth::set_auth_context)`.
/// It owns the request span (reqid + `request_span!`) and sets the log actor,
/// exactly as each app's own middleware did, then runs the rest of the stack
/// inside that span. It resolves bearer FIRST (a token means a machine caller),
/// then a browser session; a configured dev stub catches a browser that
/// presented neither.
pub async fn set_auth_context(
    State(providers): State<AuthProviders>,
    req: Request,
    next: Next,
) -> Response {
    let reqid = common_logging::gen_reqid();
    let span = common_logging::request_span!(&reqid);

    // Auth off entirely: no validator can run, so there is nothing to resolve.
    if providers.oidc.is_none() && providers.bearer.is_none() {
        let actor = stub_actor(&providers);
        common_logging::set_actor(&span, &actor);
        let mut req = req;
        req.extensions_mut().insert(AuthContext::Disabled);
        req.extensions_mut().insert(providers);
        return next.run(req).instrument(span).await;
    }

    let (parts, body) = req.into_parts();
    let context = resolve_context(&providers, &parts)
        .instrument(span.clone())
        .await;
    let actor = match &context {
        AuthContext::Disabled => stub_actor(&providers),
        AuthContext::Authenticated { principal, .. } => principal.username.clone(),
        AuthContext::Anonymous { .. } => "-".to_owned(),
    };
    common_logging::set_actor(&span, &actor);

    let mut req = Request::from_parts(parts, body);
    req.extensions_mut().insert(context);
    req.extensions_mut().insert(providers);
    next.run(req).instrument(span).await
}

fn stub_actor(providers: &AuthProviders) -> String {
    providers
        .dev_stub
        .as_ref()
        .map(|p| p.username.clone())
        .unwrap_or_else(|| "-".to_owned())
}

async fn resolve_context(providers: &AuthProviders, parts: &Parts) -> AuthContext {
    // Bearer first: a caller presenting a token is a machine, not a browser.
    if let Some(token) = bearer_token(&parts.headers) {
        return match &providers.bearer {
            Some(validator) => match validator.validate(token).await {
                Ok(principal) => AuthContext::Authenticated {
                    principal: Arc::new(principal),
                    via: AuthVia::Bearer,
                },
                Err(e) => {
                    log::info::auth!(reason = %e, "bearer token rejected");
                    AuthContext::Anonymous {
                        reason: format!("bearer rejected: {e}"),
                    }
                }
            },
            None => {
                log::info::auth!("bearer token presented but no userinfo endpoint is configured");
                AuthContext::Anonymous {
                    reason: "bearer presented but no validator is configured".into(),
                }
            }
        };
    }

    // Then a browser session.
    if let Some(oidc) = &providers.oidc {
        let jar = CookieJar::from_headers(&parts.headers);
        if let Some((principal, _session)) = oidc.resolve_session(&jar).await {
            return AuthContext::Authenticated {
                principal: Arc::new(principal),
                via: AuthVia::Session,
            };
        }
    }

    // No valid credential. A dev-stub deployment treats such a caller as the
    // stub (a preview you can look at); everyone else is anonymous.
    if providers.dev_stub.is_some() {
        return AuthContext::Disabled;
    }

    let reason = if providers.oidc.is_some() {
        "no valid session"
    } else {
        "no credential matched an authenticator"
    };
    log::info::auth!(reason = %reason, "request resolved to anonymous");
    AuthContext::Anonymous {
        reason: reason.into(),
    }
}

/// The single bearer-header parser. `Bearer`/`bearer`, non-empty after trim.
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))?;
    (!token.trim().is_empty()).then_some(token.trim())
}

// ---------------------------------------------------------------------------
// Reading the context, and building refusals.
// ---------------------------------------------------------------------------

fn context(parts: &Parts) -> Result<&AuthContext, Response> {
    parts.extensions.get::<AuthContext>().ok_or_else(|| {
        log::error::http!(
            path = %parts.uri.path(),
            "a route was reached without set_auth_context — mount the middleware"
        );
        internal()
    })
}

fn providers(parts: &Parts) -> Option<&AuthProviders> {
    parts.extensions.get::<AuthProviders>()
}

/// The actor under [`AuthContext::Disabled`]: the configured stub, else a
/// synthesized `-` principal with no groups (documented fallback, R115).
fn disabled_principal(parts: &Parts) -> Arc<Principal> {
    providers(parts)
        .and_then(|p| p.dev_stub.clone())
        .unwrap_or_else(|| {
            Arc::new(Principal {
                uuid: Uuid::nil(),
                username: "-".to_owned(),
                email: None,
                effective_groups: Vec::new(),
            })
        })
}

fn wants_html(parts: &Parts) -> bool {
    parts.method == axum::http::Method::GET
        && parts
            .headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|a| a.contains("text/html"))
}

fn internal() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": "internal" })),
    )
        .into_response()
}

/// The refusal for an anonymous caller. A browser (HTML GET) under an app that
/// has OIDC gets the crate's silent re-auth redirect; everyone else gets the
/// wire 401 — the app's if it installed one, else a minimal JSON body.
async fn anonymous_response(parts: &Parts) -> Response {
    if wants_html(parts) {
        if let Some(oidc) = providers(parts).and_then(|p| p.oidc.as_ref()) {
            return oidc.login_redirect(parts).await;
        }
    }
    unauthorized_wire(parts)
}

/// The wire 401, never a redirect — for machine callers ([`ServiceAccount`])
/// and for the non-HTML branch of [`anonymous_response`].
fn unauthorized_wire(parts: &Parts) -> Response {
    if let Some(f) = providers(parts).and_then(|p| p.refusals.unauthorized.as_ref()) {
        return f(parts);
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "unauthenticated" })),
    )
        .into_response()
}

/// The one FORBIDDEN, naming the gate. The app's renderer if installed, else a
/// minimal JSON body carrying the gate description.
fn denied_response(parts: &Parts, denial: &Denial) -> Response {
    if let Some(f) = providers(parts).and_then(|p| p.refusals.denied.as_ref()) {
        return f(parts, denial);
    }
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({ "error": "denied", "gate": denial.gate })),
    )
        .into_response()
}

/// Emit the shared FORBIDDEN from an app's OWN extractor — so a bespoke,
/// resource-scoped or async authorization check (role-ui's `GroupAccess`: is
/// the caller a leader of THIS group, fetched per request) refuses with the
/// same bytes and honours the same `on_denied` seam as [`GatedBy`], instead of
/// hand-rolling a refusal. The predicate algebra covers pure principal+config
/// rules; this is the door for everything it deliberately does not.
pub fn deny(parts: &Parts, denial: Denial) -> Response {
    denied_response(parts, &denial)
}

/// The shared 401 for an app's own extractor — the machine-caller counterpart
/// of [`deny`]. Most app extractors get this for free by building on
/// [`Authenticated`]/[`ServiceAccount`]; this is for the ones that decide
/// unauthenticated for a reason of their own.
pub fn unauthorized(parts: &Parts) -> Response {
    unauthorized_wire(parts)
}

// ---------------------------------------------------------------------------
// The predicate algebra.
// ---------------------------------------------------------------------------

/// A refused gate, carrying the human description of what membership was
/// required so [`denied_response`] can name it to an operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denial {
    pub gate: String,
}

impl Denial {
    /// An arbitrary gate description — for an app extractor that gates on
    /// something other than a group UUID (a role in a resource, a superuser
    /// bit) and wants to name it to an operator through [`deny`].
    pub fn new(gate: impl Into<String>) -> Self {
        Self { gate: gate.into() }
    }

    /// Names a single group by UUID, or `-` when the group is unconfigured.
    pub fn group(group: Option<Uuid>) -> Self {
        Self {
            gate: match group {
                Some(g) => format!("{g} (effective membership)"),
                None => "-".to_owned(),
            },
        }
    }
}

/// A boolean rule over a principal, evaluated against app state `S`. Apps
/// declare impls — usually via [`HasGroup`] and the combinators — and name them
/// as the type parameter of [`GatedBy`].
pub trait Predicate<S>: 'static {
    fn check(principal: &Principal, state: &S) -> Result<(), Denial>;
}

/// Resolves ONE configured group UUID from app state. Threaded through state
/// (cron's `App.gate`, registry's launcher group), never a global: the
/// gate-proof suites build several states with DIFFERENT groups in one test
/// binary, which a singleton could not serve.
pub trait Group<S>: 'static {
    fn group(state: &S) -> Option<Uuid>;
}

/// Membership in the group `G` resolves from state. An UNCONFIGURED group
/// (`None`) does not pass — the same shape as the apps' previous `is_some_and`.
pub struct HasGroup<G>(PhantomData<G>);

impl<S, G: Group<S>> Predicate<S> for HasGroup<G> {
    fn check(principal: &Principal, state: &S) -> Result<(), Denial> {
        let group = G::group(state);
        match group {
            Some(g) if principal.in_group(&g) => Ok(()),
            _ => Err(Denial::group(group)),
        }
    }
}

/// Passes if EITHER side passes; on refusal, names both required memberships.
pub struct Or<A, B>(PhantomData<(A, B)>);

impl<S, A: Predicate<S>, B: Predicate<S>> Predicate<S> for Or<A, B> {
    fn check(principal: &Principal, state: &S) -> Result<(), Denial> {
        match A::check(principal, state) {
            Ok(()) => Ok(()),
            Err(a) => match B::check(principal, state) {
                Ok(()) => Ok(()),
                Err(b) => Err(Denial {
                    gate: format!("{} or {}", a.gate, b.gate),
                }),
            },
        }
    }
}

/// Passes only if BOTH sides pass; reports the first refusal.
pub struct And<A, B>(PhantomData<(A, B)>);

impl<S, A: Predicate<S>, B: Predicate<S>> Predicate<S> for And<A, B> {
    fn check(principal: &Principal, state: &S) -> Result<(), Denial> {
        A::check(principal, state)?;
        B::check(principal, state)
    }
}

/// Passes only if the inner rule REFUSES.
pub struct Not<A>(PhantomData<A>);

impl<S, A: Predicate<S>> Predicate<S> for Not<A> {
    fn check(principal: &Principal, state: &S) -> Result<(), Denial> {
        match A::check(principal, state) {
            Ok(()) => Err(Denial {
                gate: "must not satisfy the excluded rule".to_owned(),
            }),
            Err(_) => Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------
// The extractor family.
// ---------------------------------------------------------------------------

/// Any signed-in caller. `Disabled` yields the dev stub (or the synthesized
/// `-`); `Anonymous` refuses (login redirect for an HTML browser, else 401).
#[derive(Debug)]
pub struct Authenticated(pub Arc<Principal>);

impl<S: Send + Sync> FromRequestParts<S> for Authenticated {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Response> {
        match context(parts)? {
            AuthContext::Disabled => Ok(Authenticated(disabled_principal(parts))),
            AuthContext::Authenticated { principal, .. } => Ok(Authenticated(principal.clone())),
            AuthContext::Anonymous { .. } => Err(anonymous_response(parts).await),
        }
    }
}

/// A bearer service account and nothing else — no session, no stub, no HTML
/// redirect. Refuses with a plain 401, because the caller has no browser.
#[derive(Debug)]
pub struct ServiceAccount(pub Arc<Principal>);

impl<S: Send + Sync> FromRequestParts<S> for ServiceAccount {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Response> {
        match context(parts)? {
            AuthContext::Authenticated {
                principal,
                via: AuthVia::Bearer,
            } => Ok(ServiceAccount(principal.clone())),
            _ => Err(unauthorized_wire(parts)),
        }
    }
}

/// The caller if there is one, `None` otherwise. Never fails.
pub struct MaybeAuthenticated(pub Option<Arc<Principal>>);

impl<S: Send + Sync> FromRequestParts<S> for MaybeAuthenticated {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(MaybeAuthenticated(
            match parts.extensions.get::<AuthContext>() {
                Some(AuthContext::Disabled) => Some(disabled_principal(parts)),
                Some(AuthContext::Authenticated { principal, .. }) => Some(principal.clone()),
                _ => None,
            },
        ))
    }
}

/// A caller who passes the predicate `P`. `Disabled` SKIPS the predicate and
/// returns the stub (so `--no-auth` = every gate passes); `Authenticated` runs
/// `P::check` and, on `Err(Denial)`, returns the shared denied response;
/// `Anonymous` refuses like [`Authenticated`].
pub struct GatedBy<P>(pub Arc<Principal>, pub PhantomData<P>);

impl<S, P> FromRequestParts<S> for GatedBy<P>
where
    S: Send + Sync,
    P: Predicate<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Response> {
        match context(parts)? {
            AuthContext::Disabled => Ok(GatedBy(disabled_principal(parts), PhantomData)),
            AuthContext::Authenticated { principal, .. } => {
                let principal = principal.clone();
                match P::check(&principal, state) {
                    Ok(()) => Ok(GatedBy(principal, PhantomData)),
                    Err(denial) => Err(denied_response(parts, &denial)),
                }
            }
            AuthContext::Anonymous { .. } => Err(anonymous_response(parts).await),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;

    fn principal(name: &str, groups: &[Uuid]) -> Arc<Principal> {
        Arc::new(Principal {
            uuid: Uuid::new_v4(),
            username: name.to_owned(),
            email: None,
            effective_groups: groups.to_vec(),
        })
    }

    /// Build request parts carrying a context (and, optionally, providers).
    fn parts_with(ctx: AuthContext, providers: Option<AuthProviders>) -> Parts {
        parts_full(ctx, providers, "GET", None)
    }

    fn parts_full(
        ctx: AuthContext,
        providers: Option<AuthProviders>,
        method: &str,
        accept: Option<&str>,
    ) -> Parts {
        let mut b = HttpRequest::builder().method(method).uri("/x");
        if let Some(a) = accept {
            b = b.header(header::ACCEPT, a);
        }
        let (mut parts, _) = b.body(Body::empty()).unwrap().into_parts();
        parts.extensions.insert(ctx);
        if let Some(p) = providers {
            parts.extensions.insert(p);
        }
        parts
    }

    fn empty_providers() -> AuthProviders {
        AuthProviders::new(None, None, None)
    }

    // ----- a tiny app state and two groups for the predicate tests -----

    const GATE: Uuid = Uuid::from_u128(0x1111_1111_1111_4111_8111_1111_1111_1111);
    const INDEX: Uuid = Uuid::from_u128(0x2222_2222_2222_4222_8222_2222_2222_2222);

    struct TestState {
        gate: Option<Uuid>,
        index: Option<Uuid>,
    }

    struct GateGroup;
    struct IndexGroup;
    impl Group<TestState> for GateGroup {
        fn group(s: &TestState) -> Option<Uuid> {
            s.gate
        }
    }
    impl Group<TestState> for IndexGroup {
        fn group(s: &TestState) -> Option<Uuid> {
            s.index
        }
    }

    // ----- context resolution reflected by the extractors -----

    #[tokio::test]
    async fn authenticated_reads_each_context() {
        // Disabled with a stub -> the stub principal.
        let stub = principal("dev-stub", &[]);
        let providers = AuthProviders::new(None, None, Some(stub.clone()));
        let mut parts = parts_with(AuthContext::Disabled, Some(providers));
        let who = Authenticated::from_request_parts(&mut parts, &())
            .await
            .expect("disabled passes");
        assert_eq!(who.0.username, "dev-stub");

        // Disabled with no stub -> synthesized "-".
        let mut parts = parts_with(AuthContext::Disabled, Some(empty_providers()));
        let who = Authenticated::from_request_parts(&mut parts, &())
            .await
            .expect("disabled passes");
        assert_eq!(who.0.username, "-");
        assert!(who.0.effective_groups.is_empty());

        // Authenticated -> the principal.
        let ctx = AuthContext::Authenticated {
            principal: principal("alice", &[]),
            via: AuthVia::Session,
        };
        let mut parts = parts_with(ctx, Some(empty_providers()));
        let who = Authenticated::from_request_parts(&mut parts, &())
            .await
            .expect("authenticated passes");
        assert_eq!(who.0.username, "alice");

        // Anonymous -> 401 (no oidc, so no redirect even for HTML).
        let mut parts = parts_full(
            AuthContext::Anonymous { reason: "x".into() },
            Some(empty_providers()),
            "GET",
            Some("text/html"),
        );
        let err = Authenticated::from_request_parts(&mut parts, &())
            .await
            .expect_err("anonymous refuses");
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn service_account_wants_bearer_only() {
        let bearer = AuthContext::Authenticated {
            principal: principal("svc", &[]),
            via: AuthVia::Bearer,
        };
        let mut parts = parts_with(bearer, Some(empty_providers()));
        assert!(ServiceAccount::from_request_parts(&mut parts, &())
            .await
            .is_ok());

        for via in [AuthVia::Session, AuthVia::Stub] {
            let ctx = AuthContext::Authenticated {
                principal: principal("someone", &[]),
                via,
            };
            let mut parts = parts_with(ctx, Some(empty_providers()));
            let err = ServiceAccount::from_request_parts(&mut parts, &())
                .await
                .expect_err("non-bearer refused");
            assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
        }

        // Disabled is not a bearer either.
        let mut parts = parts_with(AuthContext::Disabled, Some(empty_providers()));
        let err = ServiceAccount::from_request_parts(&mut parts, &())
            .await
            .expect_err("disabled is not a service account");
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn maybe_authenticated_never_fails() {
        let mut parts = parts_with(
            AuthContext::Anonymous { reason: "x".into() },
            Some(empty_providers()),
        );
        let who = MaybeAuthenticated::from_request_parts(&mut parts, &())
            .await
            .expect("infallible");
        assert!(who.0.is_none());

        let ctx = AuthContext::Authenticated {
            principal: principal("alice", &[]),
            via: AuthVia::Session,
        };
        let mut parts = parts_with(ctx, Some(empty_providers()));
        let who = MaybeAuthenticated::from_request_parts(&mut parts, &())
            .await
            .expect("infallible");
        assert_eq!(who.0.expect("some").username, "alice");
    }

    #[tokio::test]
    async fn missing_context_is_an_internal_error_not_a_pass() {
        let (mut parts, _) = HttpRequest::builder()
            .uri("/x")
            .body(Body::empty())
            .unwrap()
            .into_parts();
        let err = Authenticated::from_request_parts(&mut parts, &())
            .await
            .expect_err("no middleware -> refuse");
        assert_eq!(err.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ----- GatedBy: pass / deny / disabled-bypass -----

    async fn gate_status<P>(ctx: AuthContext, state: &TestState) -> StatusCode
    where
        P: Predicate<TestState>,
    {
        let mut parts = parts_with(ctx, Some(empty_providers()));
        match GatedBy::<P>::from_request_parts(&mut parts, state).await {
            Ok(_) => StatusCode::OK,
            Err(resp) => resp.status(),
        }
    }

    #[tokio::test]
    async fn gated_by_passes_denies_and_bypasses_when_disabled() {
        let state = TestState {
            gate: Some(GATE),
            index: Some(INDEX),
        };
        let member = AuthContext::Authenticated {
            principal: principal("alice", &[GATE]),
            via: AuthVia::Session,
        };
        let outsider = AuthContext::Authenticated {
            principal: principal("mallory", &[Uuid::new_v4()]),
            via: AuthVia::Bearer,
        };

        assert_eq!(
            gate_status::<HasGroup<GateGroup>>(member, &state).await,
            StatusCode::OK
        );
        assert_eq!(
            gate_status::<HasGroup<GateGroup>>(outsider, &state).await,
            StatusCode::FORBIDDEN
        );
        // Disabled bypasses the predicate entirely (--no-auth = all pass).
        assert_eq!(
            gate_status::<HasGroup<GateGroup>>(AuthContext::Disabled, &state).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn an_unconfigured_group_never_passes() {
        let state = TestState {
            gate: None,
            index: None,
        };
        let anyone = AuthContext::Authenticated {
            principal: principal("alice", &[GATE]),
            via: AuthVia::Session,
        };
        assert_eq!(
            gate_status::<HasGroup<GateGroup>>(anyone, &state).await,
            StatusCode::FORBIDDEN
        );
    }

    // ----- the combinator truth table via HasGroup -----

    fn check<P: Predicate<TestState>>(groups: &[Uuid], state: &TestState) -> bool {
        P::check(
            &Principal {
                uuid: Uuid::new_v4(),
                username: "t".into(),
                email: None,
                effective_groups: groups.to_vec(),
            },
            state,
        )
        .is_ok()
    }

    #[test]
    fn combinator_truth_table() {
        let state = TestState {
            gate: Some(GATE),
            index: Some(INDEX),
        };
        type G = HasGroup<GateGroup>;
        type I = HasGroup<IndexGroup>;

        // Or: passes if in EITHER.
        assert!(check::<Or<G, I>>(&[GATE], &state));
        assert!(check::<Or<G, I>>(&[INDEX], &state));
        assert!(check::<Or<G, I>>(&[GATE, INDEX], &state));
        assert!(!check::<Or<G, I>>(&[Uuid::new_v4()], &state));

        // And: passes only if in BOTH.
        assert!(check::<And<G, I>>(&[GATE, INDEX], &state));
        assert!(!check::<And<G, I>>(&[GATE], &state));
        assert!(!check::<And<G, I>>(&[INDEX], &state));

        // Not: passes only if NOT in the group.
        assert!(check::<Not<G>>(&[INDEX], &state));
        assert!(!check::<Not<G>>(&[GATE], &state));
    }

    #[test]
    fn or_denial_names_both_required_groups() {
        let state = TestState {
            gate: Some(GATE),
            index: Some(INDEX),
        };
        let denial = <Or<HasGroup<GateGroup>, HasGroup<IndexGroup>>>::check(
            &Principal {
                uuid: Uuid::new_v4(),
                username: "t".into(),
                email: None,
                effective_groups: vec![],
            },
            &state,
        )
        .expect_err("no group -> denied");
        assert!(denial.gate.contains(&GATE.to_string()), "{}", denial.gate);
        assert!(denial.gate.contains(&INDEX.to_string()), "{}", denial.gate);
        assert!(denial.gate.contains(" or "), "{}", denial.gate);
    }

    // ----- the middleware's auth-off short-circuit -----

    #[tokio::test]
    async fn middleware_off_inserts_disabled_and_the_stub_actor() {
        use axum::routing::get;
        use axum::Router;
        use tower::ServiceExt;

        async fn probe(ctx: MaybeAuthenticated) -> Response {
            match ctx.0 {
                Some(p) => (StatusCode::OK, p.username.clone()).into_response(),
                None => (StatusCode::OK, "none".to_owned()).into_response(),
            }
        }

        let providers = AuthProviders::new(None, None, Some(principal("dev-stub", &[])));
        let app: Router =
            Router::new()
                .route("/", get(probe))
                .layer(axum::middleware::from_fn_with_state(
                    providers,
                    set_auth_context,
                ));
        let resp = app
            .oneshot(HttpRequest::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        assert_eq!(&body[..], b"dev-stub");
    }

    // ----- the door stays open for role-ui's kind of predicate -----
    //
    // role-ui's real check is "is the caller a Leader of THIS group", which is
    // async, reads the request path, and fetches the role relation — none of
    // which the pure `Predicate` algebra expresses, ON PURPOSE. This models it
    // as an APP-LOCAL extractor and proves it can still (a) read the principal
    // the shared middleware set, via `Authenticated`, and (b) refuse with the
    // shared bytes, via `deny(Denial::new(..))`. If a future edit privatises
    // either seam, this test stops compiling — which is the guarantee.

    struct Roles {
        leaders: std::collections::HashMap<String, &'static str>,
    }

    /// A bespoke, async, resource-scoped authorization extractor — the shape
    /// `Predicate` deliberately cannot take. It leans only on PUBLIC surface.
    #[derive(Debug)]
    struct LeaderOfResource(#[allow(dead_code)] Arc<Principal>);

    impl FromRequestParts<Roles> for LeaderOfResource {
        type Rejection = Response;

        async fn from_request_parts(parts: &mut Parts, state: &Roles) -> Result<Self, Response> {
            let Authenticated(principal) = Authenticated::from_request_parts(parts, state).await?;
            // Stand-in for an async fetch keyed on a path param.
            let resource = "group-42";
            match state.leaders.get(&principal.username) {
                Some(g) if *g == resource => Ok(LeaderOfResource(principal)),
                _ => Err(deny(parts, Denial::new("must be leader of this group"))),
            }
        }
    }

    #[tokio::test]
    async fn an_app_local_resource_extractor_reuses_authenticated_and_deny() {
        let state = Roles {
            leaders: [("boss".to_owned(), "group-42")].into_iter().collect(),
        };

        // The leader of the resource passes.
        let mut parts = parts_with(
            AuthContext::Authenticated {
                principal: principal("boss", &[]),
                via: AuthVia::Session,
            },
            Some(empty_providers()),
        );
        assert!(LeaderOfResource::from_request_parts(&mut parts, &state)
            .await
            .is_ok());

        // A non-leader is refused with the SHARED forbidden, naming the gate.
        let mut parts = parts_with(
            AuthContext::Authenticated {
                principal: principal("nobody", &[]),
                via: AuthVia::Session,
            },
            Some(empty_providers()),
        );
        let resp = LeaderOfResource::from_request_parts(&mut parts, &state)
            .await
            .expect_err("a non-leader is refused");
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["gate"], "must be leader of this group");
    }
}
