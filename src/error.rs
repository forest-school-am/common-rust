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
