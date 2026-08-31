use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

/// The authenticated caller, as authentik's userinfo reported it for THIS
/// request (per-request validation — nothing here is cached).
#[derive(Debug, Clone)]
pub struct Principal {
    /// authentik user UUID (`sub` under the stand-wide `sub_mode=user_uuid`).
    pub uuid: Uuid,
    pub username: String,
    pub email: Option<String>,
    /// Downward closure (direct groups ∪ all descendants) of group UUIDs from
    /// the `effective_groups` claim — parents inherit their children's
    /// access. Gate on UUIDs, never names.
    pub effective_groups: Vec<Uuid>,
}

impl Principal {
    /// The single place the stand's identity contract is enforced: `sub`
    /// MUST be a user UUID (provider `sub_mode=user_uuid`) and every
    /// `effective_groups` entry MUST be a group UUID. Both the BFF client
    /// and the bearer validator build principals through here, so every
    /// stand service agrees on what identity means. Fails closed (`Err`) on
    /// any non-UUID — never a partially-understood principal.
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

    /// Downward-semantics gate: is `group` within the caller's effective
    /// groups? (Members of any ancestor group pass — the closure is already
    /// expanded server-side by authentik.)
    pub fn in_group(&self, group: &Uuid) -> bool {
        self.effective_groups.contains(group)
    }

    /// `?`-friendly gate: `p.require_group(&cron_admins)?` → 403 on failure.
    pub fn require_group(&self, group: &Uuid) -> Result<(), GateDenied> {
        if self.in_group(group) {
            Ok(())
        } else {
            Err(GateDenied)
        }
    }
}

/// 403 response for a failed [`Principal::require_group`] gate.
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
