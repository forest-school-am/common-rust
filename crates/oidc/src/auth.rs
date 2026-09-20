//! Authorization orchestration, shared by the stand's apps. The two validated
//! identities are never re-implemented here: the browser OIDC session
//! ([`OidcState`]) and the bearer service-account token ([`BearerValidator`])
//! are resolved by their own modules and only consumed here.

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthVia {
    Session,
    Bearer,
    Stub,
}

/// The middleware NEVER rejects: an absent or invalid credential becomes
/// [`AuthContext::Anonymous`].
#[derive(Debug, Clone)]
pub enum AuthContext {
    Disabled,
    Authenticated {
        principal: Arc<Principal>,
        via: AuthVia,
    },
    /// The reason is logged, never returned to the caller.
    Anonymous {
        reason: String,
    },
}

type UnauthorizedFn = Arc<dyn Fn(&Parts) -> Response + Send + Sync>;
type DeniedFn = Arc<dyn Fn(&Parts, &Denial) -> Response + Send + Sync>;

#[derive(Clone, Default)]
pub struct Refusals {
    pub unauthorized: Option<UnauthorizedFn>,
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

#[derive(Clone)]
pub struct AuthProviders {
    pub oidc: Option<OidcState>,
    pub bearer: Option<BearerValidator>,
    pub dev_stub: Option<Arc<Principal>>,
    pub refusals: Refusals,
}

impl AuthProviders {
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

    pub fn on_unauthorized(
        mut self,
        f: impl Fn(&Parts) -> Response + Send + Sync + 'static,
    ) -> Self {
        self.refusals.unauthorized = Some(Arc::new(f));
        self
    }

    pub fn on_denied(
        mut self,
        f: impl Fn(&Parts, &Denial) -> Response + Send + Sync + 'static,
    ) -> Self {
        self.refusals.denied = Some(Arc::new(f));
        self
    }
}

pub async fn set_auth_context(
    State(providers): State<AuthProviders>,
    req: Request,
    next: Next,
) -> Response {
    let reqid = common_logging::gen_reqid();
    let span = common_logging::request_span!(&reqid);

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

    if let Some(oidc) = &providers.oidc {
        let jar = CookieJar::from_headers(&parts.headers);
        if let Some((principal, _session)) = oidc.resolve_session(&jar).await {
            return AuthContext::Authenticated {
                principal: Arc::new(principal),
                via: AuthVia::Session,
            };
        }
    }

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

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))?;
    (!token.trim().is_empty()).then_some(token.trim())
}

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

async fn anonymous_response(parts: &Parts) -> Response {
    if wants_html(parts) {
        if let Some(oidc) = providers(parts).and_then(|p| p.oidc.as_ref()) {
            return oidc.login_redirect(parts).await;
        }
    }
    unauthorized_wire(parts)
}

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

pub fn deny(parts: &Parts, denial: Denial) -> Response {
    denied_response(parts, &denial)
}

pub fn unauthorized(parts: &Parts) -> Response {
    unauthorized_wire(parts)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denial {
    pub gate: String,
}

impl Denial {
    pub fn new(gate: impl Into<String>) -> Self {
        Self { gate: gate.into() }
    }

    pub fn group(group: Option<Uuid>) -> Self {
        Self {
            gate: match group {
                Some(g) => format!("{g} (effective membership)"),
                None => "-".to_owned(),
            },
        }
    }
}

pub trait Predicate<S>: 'static {
    fn check(principal: &Principal, state: &S) -> Result<(), Denial>;
}

pub trait Group<S>: 'static {
    fn group(state: &S) -> Option<Uuid>;
}

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

pub struct And<A, B>(PhantomData<(A, B)>);

impl<S, A: Predicate<S>, B: Predicate<S>> Predicate<S> for And<A, B> {
    fn check(principal: &Principal, state: &S) -> Result<(), Denial> {
        A::check(principal, state)?;
        B::check(principal, state)
    }
}

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

    #[tokio::test]
    async fn authenticated_reads_each_context() {
        let stub = principal("dev-stub", &[]);
        let providers = AuthProviders::new(None, None, Some(stub.clone()));
        let mut parts = parts_with(AuthContext::Disabled, Some(providers));
        let who = Authenticated::from_request_parts(&mut parts, &())
            .await
            .expect("disabled passes");
        assert_eq!(who.0.username, "dev-stub");

        let mut parts = parts_with(AuthContext::Disabled, Some(empty_providers()));
        let who = Authenticated::from_request_parts(&mut parts, &())
            .await
            .expect("disabled passes");
        assert_eq!(who.0.username, "-");
        assert!(who.0.effective_groups.is_empty());

        let ctx = AuthContext::Authenticated {
            principal: principal("alice", &[]),
            via: AuthVia::Session,
        };
        let mut parts = parts_with(ctx, Some(empty_providers()));
        let who = Authenticated::from_request_parts(&mut parts, &())
            .await
            .expect("authenticated passes");
        assert_eq!(who.0.username, "alice");

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

        assert!(check::<Or<G, I>>(&[GATE], &state));
        assert!(check::<Or<G, I>>(&[INDEX], &state));
        assert!(check::<Or<G, I>>(&[GATE, INDEX], &state));
        assert!(!check::<Or<G, I>>(&[Uuid::new_v4()], &state));

        assert!(check::<And<G, I>>(&[GATE, INDEX], &state));
        assert!(!check::<And<G, I>>(&[GATE], &state));
        assert!(!check::<And<G, I>>(&[INDEX], &state));

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

    struct Roles {
        leaders: std::collections::HashMap<String, &'static str>,
    }

    #[derive(Debug)]
    struct LeaderOfResource(#[allow(dead_code)] Arc<Principal>);

    impl FromRequestParts<Roles> for LeaderOfResource {
        type Rejection = Response;

        async fn from_request_parts(parts: &mut Parts, state: &Roles) -> Result<Self, Response> {
            let Authenticated(principal) = Authenticated::from_request_parts(parts, state).await?;
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
