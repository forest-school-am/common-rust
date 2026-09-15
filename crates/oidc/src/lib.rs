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
//! common_logging::info!(common_logging::AUTH, user = %principal.username, "authorised");
//! # Ok(())
//! # }
//! ```

mod bearer;
mod client;
mod config;
mod error;
mod principal;
mod retry;
mod store;
mod web;

/// The browser shim, stripped from `src/common-oidc.ts` at build time and
/// carried in the binary: it cannot drift from the crate that serves it.
pub const SHIM_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/common-oidc.js"));

pub use bearer::{BearerValidator, ValidationError};
pub use client::{OidcClient, TokenBundle};
pub use config::OidcConfig;
pub use error::{OidcError, Upstream};
pub use principal::{GateDenied, Principal};
pub use store::{
    BoxFuture, FlowState, FlowStore, MemoryFlowStore, MemoryStore, Session, SessionStore,
};
pub use web::{router, user_portal_url, AuthRedirect, OidcState, REAUTH_HEADER};
