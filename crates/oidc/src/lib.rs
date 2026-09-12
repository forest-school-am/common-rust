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
mod source_state;
mod store;
mod web;

use source_state::SourceState;

include!(concat!(env!("OUT_DIR"), "/shim_hash.rs"));
include!(concat!(env!("OUT_DIR"), "/source_state.rs"));

pub use bearer::{BearerValidator, ValidationError};
pub use client::{OidcClient, TokenBundle};
pub use config::OidcConfig;
pub use error::{OidcError, Upstream};
pub use principal::{GateDenied, Principal};
pub use store::{
    BoxFuture, FlowState, FlowStore, MemoryFlowStore, MemoryStore, Session, SessionStore,
};
pub use web::{router, user_portal_url, AuthRedirect, OidcState, REAUTH_HEADER};
