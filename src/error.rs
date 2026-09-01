//! The protocol client's error taxonomy — one variant per distinct failure so
//! a caller maps each to the right response and severity (§3.2, one enum per
//! handling contract). Separate from the bearer validator's `ValidationError`
//! and the extractor's rejection types, which answer to different contracts.

#[derive(Debug, thiserror::Error)]
pub enum OidcError {
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
    /// The served-asset directory / template failed boot validation or its
    /// integrity pin (§9.6/§9.8) — refuse to boot.
    #[error("asset setup failed: {0}")]
    Assets(String),
}
