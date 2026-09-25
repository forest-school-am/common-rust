//! Bearer-token validation for API services that have no browser session:
//! userinfo per request, no discovery, no cookies. If it needs a session
//! store or a redirect, it belongs in web.rs.

use reqwest::header;
use serde::Deserialize;

use crate::principal::Principal;

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("access token rejected by authentik")]
    Rejected,
    #[error("{0}")]
    Upstream(String),
}

#[derive(Debug, Deserialize)]
struct UserInfoResponse {
    sub: String,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    effective_groups: Vec<String>,
}

#[derive(Clone)]
pub struct BearerValidator {
    http: reqwest::Client,
    userinfo_url: String,
}

impl BearerValidator {
    pub fn new(http: reqwest::Client, userinfo_url: impl Into<String>) -> Self {
        Self {
            http,
            userinfo_url: userinfo_url.into(),
        }
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
                Principal::from_userinfo(&body.sub, body.preferred_username, &body.effective_groups)
                    .map_err(ValidationError::Upstream)
            }
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
                Err(ValidationError::Rejected)
            }
            s => Err(ValidationError::Upstream(format!("userinfo returned {s}"))),
        }
    }
}
