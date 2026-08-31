#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("OIDC discovery failed: {0}")]
    Discovery(String),
    #[error("token exchange failed: {0}")]
    Exchange(String),
    #[error("token refresh failed: {0}")]
    Refresh(String),
    /// userinfo rejected the access token or was unreachable. Per the
    /// fail-closed rule the caller is treated as unauthenticated either way.
    #[error("userinfo rejected/unreachable: {0}")]
    Userinfo(String),
    #[error("invalid configuration: {0}")]
    Config(String),
}
