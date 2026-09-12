//! The crate's error types. Variants only; the mapping from error to HTTP
//! response lives with the handlers in web.rs, and what to RETRY in retry.rs.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upstream {
    Rejected(String),
    Unreachable(String),
}

impl std::fmt::Display for Upstream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Upstream::Rejected(why) => write!(f, "rejected by the IdP: {why}"),
            Upstream::Unreachable(why) => write!(f, "no answer from the IdP: {why}"),
        }
    }
}

impl std::error::Error for Upstream {}

#[derive(Debug, thiserror::Error)]
pub enum OidcError {
    #[error("OIDC discovery failed: {0}")]
    Discovery(String),
    #[error("token exchange failed: {0}")]
    Exchange(String),
    #[error("token refresh failed: {0}")]
    Refresh(String),
    #[error("userinfo rejected/unreachable: {0}")]
    Userinfo(String),
    #[error("invalid configuration: {0}")]
    Config(String),
    #[error("asset setup failed: {0}")]
    Assets(String),
}
