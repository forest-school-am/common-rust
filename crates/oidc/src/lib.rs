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
mod predicate;
mod principal;
mod retry;
mod section;
mod store;
mod web;

/// The browser shim, stripped from `src/common-oidc.ts` at build time and
/// carried in the binary: it cannot drift from the crate that serves it.
pub const SHIM_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/common-oidc.js"));

/// The shim's TYPE surface, carried in the binary beside [`SHIM_JS`]. This is
/// the SINGLE source — consumers write it to their OUT_DIR for the typecheck
/// and keep no vendored copy, so the types cannot drift from the shim.
pub const SHIM_DTS: &str = include_str!("common-oidc.public.d.ts");

/// The shim a SPA's TEST RUNNER uses in place of [`SHIM_JS`], carried beside it
/// for the same reason: one source, no vendored copy per repo.
///
/// The served shim installs its re-auth guard at module load — it wraps
/// `window.fetch`, reads a `#config` element and can call `location.assign`,
/// which jsdom refuses — and a test runner resolves neither the build's
/// `external` nor the tsconfig `paths` mapping for `/common-oidc.js`. This
/// carries the SAME transport (a test asserts the two do not drift) with the
/// guard as a no-op that is not run at load, so `fetch` stays whatever the test
/// installed. A consumer writes it out and aliases the served specifier to it:
///
/// ```text
/// // vitest.config.ts
/// resolve: { alias: { "/common-oidc.js": resolve(__dirname, "src/test/common-oidc-stub.ts") } }
/// ```
pub const SHIM_TEST_STUB: &str = include_str!("common-oidc.test-stub.ts");

pub use auth::{
    deny, set_auth_context, unauthorized, AuthContext, AuthProviders, AuthVia, Authenticated,
    GatedBy, MaybeAuthenticated, Refusals, ServiceAccount,
};
pub use bearer::{BearerValidator, ValidationError};
pub use client::{OidcClient, TokenBundle};
pub use config::OidcConfig;
pub use error::{OidcError, Upstream};
pub use page_config::{PageConfig, PageUser};
pub use predicate::{And, Denial, Group, HasGroup, Not, Or, Predicate};
pub use principal::{MissingGroup, Principal};
pub use section::OidcSection;
pub use store::{
    BoxFuture, FlowState, FlowStore, MemoryFlowStore, MemoryStore, Session, SessionStore,
};
pub use web::{router, user_portal_url, AuthRedirect, OidcState, REAUTH_HEADER};
