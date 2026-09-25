//! Crate root: the module tree and the public surface. A new capability gets
//! its own module here; nothing that decides anything belongs in this file.
//!
//! ```no_run
//! # async fn demo() -> Result<(), common_secrets::Error> {
//! use common_secrets::{AppPassword, Config, SecretClient};
//!
//! let config = Config::new(
//!     "https://auth.dev.local/application/o/token/",
//!     "svc-roleui-client-id",
//!     "svc-roleui",
//!     AppPassword::new("the-app-password"),
//!     "http://127.0.0.1:8018",
//!     "roleui",
//! )
//! .ca_path("stand/certs/ca.crt");
//!
//! let client = SecretClient::new(config)?;
//! let token = client.fetch("roleui", "authentik_token").await?;
//! # let _ = token.expose();
//! # Ok(())
//! # }
//! ```

mod client;
mod config;
mod error;
mod secret;

pub use client::SecretClient;
pub use config::{Config, DEFAULT_SCOPE};
pub use error::{Error, Stage};
pub use secret::{AppPassword, Secret};
