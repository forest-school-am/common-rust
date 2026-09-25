//! The rename-feed HTTP client and the wire types of its one response.
//!
//! Applying deltas to storage -> apply.rs; the polling loop -> sync.rs.

use reqwest::header;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Rename {
    pub old: String,
    pub current: String,
}

/// `now` is the server cursor to store and send back as the next `since`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NameDeltas {
    pub now: String,
    #[serde(default)]
    pub users: Vec<Rename>,
    #[serde(default)]
    pub groups: Vec<Rename>,
}

impl NameDeltas {
    pub fn is_empty(&self) -> bool {
        self.users.is_empty() && self.groups.is_empty()
    }
}

#[derive(Clone)]
pub struct NameFeed {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl NameFeed {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self::with_client(reqwest::Client::new(), base_url, token)
    }

    pub fn with_client(
        http: reqwest::Client,
        base_url: impl Into<String>,
        token: impl Into<String>,
    ) -> Self {
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
        }
    }

    /// `None` for `since` fetches the whole feed from the beginning.
    pub async fn changes_since(&self, since: Option<&str>) -> anyhow::Result<NameDeltas> {
        let url = format!("{}/api/v3/forest_school/name_changes/", self.base_url);
        let mut req = self
            .http
            .get(&url)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.token));
        if let Some(since) = since {
            req = req.query(&[("since", since)]);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("name feed unreachable ({url}): {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("name feed returned {status}: {body}");
        }
        resp.json::<NameDeltas>()
            .await
            .map_err(|e| anyhow::anyhow!("name feed returned an invalid payload: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deltas_parse_and_report_emptiness() {
        let empty: NameDeltas = serde_json::from_str(r#"{"now":"2026-09-24T00:00:00Z"}"#).unwrap();
        assert!(empty.is_empty());
        assert_eq!(empty.now, "2026-09-24T00:00:00Z");

        let full: NameDeltas = serde_json::from_str(
            r#"{"now":"t","users":[{"old":"bob","current":"bob2"}],"groups":[{"old":"dev","current":"devs"}]}"#,
        )
        .unwrap();
        assert!(!full.is_empty());
        assert_eq!(full.users, vec![Rename { old: "bob".into(), current: "bob2".into() }]);
        assert_eq!(full.groups, vec![Rename { old: "dev".into(), current: "devs".into() }]);
    }
}
