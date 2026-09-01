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
pub use store::{BoxFuture, MemoryStore, Session, SessionStore};
pub use web::{router, user_portal_url, AuthRedirect, OidcState, REAUTH_HEADER};
