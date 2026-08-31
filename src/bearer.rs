use reqwest::header;
use serde::Deserialize;

use crate::principal::Principal;

/// Why a bearer token did not yield a principal.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// authentik rejected the token (401/403). The caller is unauthenticated.
    #[error("access token rejected by authentik")]
    Rejected,
    /// userinfo was unreachable or answered something we refuse to trust.
    /// Fail closed: never a principal built from partial data.
    #[error("{0}")]
    Upstream(String),
}

#[derive(Debug, Deserialize)]
struct UserInfoResponse {
    sub: String,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    effective_groups: Vec<String>,
}

/// Per-request bearer validator for API services (the mint pattern):
/// userinfo on every call, fail closed, and **no OIDC discovery** — it needs
/// only the userinfo URL, so a bearer-only service keeps its minimal config
/// and lazy (per-request, not boot-time) dependency on authentik. It shares
/// [`Principal::from_userinfo`]'s identity contract with the BFF client, so
/// every stand service parses `sub`/`effective_groups` identically.
///
/// ```ignore
/// let validator = BearerValidator::new(reqwest::Client::new(), userinfo_url);
/// let principal = match validator.validate(bearer).await {
///     Ok(p) => p,
///     Err(ValidationError::Rejected) => return unauthorized(),
///     Err(ValidationError::Upstream(m)) => return bad_gateway(m), // fail closed
/// };
/// ```
pub struct BearerValidator {
    http: reqwest::Client,
    userinfo_url: String,
}

impl BearerValidator {
    pub fn new(http: reqwest::Client, userinfo_url: impl Into<String>) -> Self {
        Self { http, userinfo_url: userinfo_url.into() }
    }

    pub async fn validate(&self, bearer: &str) -> Result<Principal, ValidationError> {
        let resp = self
            .http
            .get(&self.userinfo_url)
            .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
            .send()
            .await
            .map_err(|e| ValidationError::Upstream(format!("userinfo unreachable: {e}")))?;

        match resp.status() {
            s if s.is_success() => {
                let body: UserInfoResponse = resp.json().await.map_err(|e| {
                    ValidationError::Upstream(format!("userinfo returned invalid payload: {e}"))
                })?;
                Principal::from_userinfo(
                    &body.sub,
                    body.preferred_username,
                    body.email,
                    &body.effective_groups,
                )
                .map_err(ValidationError::Upstream)
            }
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
                Err(ValidationError::Rejected)
            }
            s => Err(ValidationError::Upstream(format!("userinfo returned {s}"))),
        }
    }
}
