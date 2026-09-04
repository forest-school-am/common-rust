//! Shared in-app OIDC for stand apps: browser sessions handled server-side
//! (web.rs), the protocol itself (client.rs), and the bearer path below for
//! APIs that are called with an access token rather than a cookie.
//!
//! Identity comes from userinfo on every request — ID tokens are never
//! verified, so a revoked session stops working immediately rather than at
//! token expiry:
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
//!
//! Gate on group UUIDs, never on names — `Principal::require_group` takes a
//! `Uuid` for that reason.

mod bearer;
mod client;
mod config;
mod error;
mod principal;
mod store;
mod web;

include!(concat!(env!("OUT_DIR"), "/shim_hash.rs"));
include!(concat!(env!("OUT_DIR"), "/source_state.rs"));

pub use bearer::{BearerValidator, ValidationError};
pub use client::{OidcClient, TokenBundle};
pub use config::OidcConfig;
pub use error::OidcError;
pub use principal::{GateDenied, Principal};
pub use store::{
    BoxFuture, FlowState, FlowStore, MemoryFlowStore, MemoryStore, Session, SessionStore,
};
pub use web::{router, user_portal_url, AuthRedirect, OidcState, REAUTH_HEADER};
