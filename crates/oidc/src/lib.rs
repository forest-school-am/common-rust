//! Crate root: the module tree and the public surface. A new capability gets
//! its own module here; nothing that decides anything belongs in this file.
//!
//! ```no_run
//! use common_oidc::BearerValidator;
//!
//! # async fn demo() -> Result<(), common_oidc::ValidationError> {
//! let validator = BearerValidator::new(
//!     reqwest::Client::new(),
//!     "https://idp.example/application/o/my-app/userinfo/",
//! );
//! let principal = validator.validate("an-access-token").await?;
//! common_logging::info::auth!(user = %principal.username, "authorised");
//! # Ok(())
//! # }
//! ```

mod auth;
mod bearer;
mod client;
mod config;
mod error;
mod page_config;
mod principal;
mod retry;
mod store;
mod web;

/// The browser shim, stripped from `src/common-oidc.ts` at build time and
/// carried in the binary: it cannot drift from the crate that serves it.
pub const SHIM_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/common-oidc.js"));

/// The shim's TYPE surface (R117), carried in the binary beside [`SHIM_JS`]:
/// `call<T>`, `CallFailure` and `installReauthGuard`, declared for tsc. This is
/// the SINGLE source — consumers write it to their OUT_DIR for the typecheck
/// and keep no vendored copy, so the types cannot drift from the shim.
pub const SHIM_DTS: &str = include_str!("common-oidc.public.d.ts");

pub use auth::{
    deny, set_auth_context, unauthorized, And, AuthContext, AuthProviders, AuthVia, Authenticated,
    Denial, GatedBy, Group, HasGroup, MaybeAuthenticated, Not, Or, Predicate, Refusals,
    ServiceAccount,
};
pub use bearer::{BearerValidator, ValidationError};
pub use client::{OidcClient, TokenBundle};
pub use config::OidcConfig;
pub use error::{OidcError, Upstream};
pub use page_config::{PageConfig, PageUser};
pub use principal::{GateDenied, Principal};
pub use store::{
    BoxFuture, FlowState, FlowStore, MemoryFlowStore, MemoryStore, Session, SessionStore,
};
pub use web::{router, user_portal_url, AuthRedirect, OidcState, REAUTH_HEADER};
