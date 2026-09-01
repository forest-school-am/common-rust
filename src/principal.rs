//! The authenticated caller and the group-membership gate. Pure data and
//! predicates — no IO, no HTTP, no storage.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Principal {
    pub uuid: Uuid,
    pub username: String,
    pub email: Option<String>,
    pub effective_groups: Vec<Uuid>,
}

impl Principal {
    pub(crate) fn from_userinfo(
        sub: &str,
        username: Option<String>,
        email: Option<String>,
        effective_groups: &[String],
    ) -> Result<Self, String> {
        let uuid = Uuid::parse_str(sub)
            .map_err(|e| format!("sub is not a UUID (need sub_mode=user_uuid): {e}"))?;
        let effective_groups = effective_groups
            .iter()
            .map(|g| Uuid::parse_str(g))
            .collect::<Result<_, _>>()
            .map_err(|e| format!("effective_groups contains a non-UUID entry: {e}"))?;
        Ok(Self {
            uuid,
            username: username.unwrap_or_else(|| uuid.to_string()),
            email,
            effective_groups,
        })
    }

    pub fn in_group(&self, group: &Uuid) -> bool {
        self.effective_groups.contains(group)
    }

    pub fn require_group(&self, group: &Uuid) -> Result<(), GateDenied> {
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

    fn p(groups: &[Uuid]) -> Principal {
        Principal {
            uuid: Uuid::new_v4(),
            username: "alice".into(),
            email: None,
            effective_groups: groups.to_vec(),
        }
    }

    #[test]
    fn gate_passes_on_membership_and_denies_otherwise() {
        let gate = Uuid::new_v4();
        assert!(p(&[Uuid::new_v4(), gate]).in_group(&gate));
        assert!(p(&[gate]).require_group(&gate).is_ok());
        assert!(p(&[Uuid::new_v4()]).require_group(&gate).is_err());
        assert!(p(&[]).require_group(&gate).is_err());
    }
}
