//! The authenticated caller and the group-membership gate. Pure data and
//! predicates — no IO, no HTTP, no storage.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, Clone)]
pub struct Principal {
    pub username: String,
    pub email: Option<String>,
    pub effective_groups: Vec<String>,
}

impl Principal {
    pub(crate) fn from_userinfo(
        sub: &str,
        username: Option<String>,
        email: Option<String>,
        effective_groups: &[String],
    ) -> Result<Self, String> {
        Ok(Self {
            username: username.unwrap_or_else(|| sub.to_owned()),
            email,
            effective_groups: effective_groups.to_vec(),
        })
    }

    pub fn in_group(&self, group: &str) -> bool {
        self.effective_groups.iter().any(|g| g == group)
    }

    pub fn require_group(&self, group: &str) -> Result<(), GateDenied> {
        if self.in_group(group) {
            Ok(())
        } else {
            Err(GateDenied)
        }
    }
}

#[derive(Debug)]
pub struct GateDenied;

impl IntoResponse for GateDenied {
    fn into_response(self) -> Response {
        (StatusCode::FORBIDDEN, "forbidden: missing required group").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(groups: &[&str]) -> Principal {
        Principal {
            username: "alice".into(),
            email: None,
            effective_groups: groups.iter().map(|g| (*g).to_owned()).collect(),
        }
    }

    #[test]
    fn gate_passes_on_membership_and_denies_otherwise() {
        let gate = "editors";
        assert!(p(&["viewers", gate]).in_group(gate));
        assert!(p(&[gate]).require_group(gate).is_ok());
        assert!(p(&["viewers"]).require_group(gate).is_err());
        assert!(p(&[]).require_group(gate).is_err());
    }
}
