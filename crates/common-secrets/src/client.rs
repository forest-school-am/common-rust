//! The login->jwt-login->read chain and the vault-token cache. This owns the
//! HTTP contract with authentik and OpenBao; the redacting value types live in
//! `secret`, config resolution in `config`, the error taxonomy in `error`.
//! `fetch` reads a single field; the `*_doc` methods read, replace, and delete a
//! whole kv-v2 document at a logical path, all through the one re-auth path.

use std::collections::BTreeMap;
use std::future::Future;
use std::time::{Duration, Instant};

use reqwest::StatusCode;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::config::Config;
use crate::error::{Error, Stage};
use crate::secret::Secret;

// authentik's OAuth2 token endpoint answers 400 (invalid_client / invalid_grant)
// for bad credentials rather than 401, and OpenBao's jwt/login answers 400 for
// an unknown role or an unverifiable JWT. All three statuses are credential
// rejections, not transport or upstream faults.
const AUTH_REJECT: [StatusCode; 3] = [
    StatusCode::BAD_REQUEST,
    StatusCode::UNAUTHORIZED,
    StatusCode::FORBIDDEN,
];

// Treat a vault token as spent this long before its lease actually ends, so a
// read never races the expiry it was authorised under.
const LEASE_SKEW: Duration = Duration::from_secs(10);

// authentik returns more token fields (expires_in, token_type, ...); only the
// JWT is used, and it is spent immediately on the vault login rather than cached.
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

// OpenBao wraps the issued token and its lease under `auth`.
#[derive(Deserialize)]
struct VaultLoginResponse {
    auth: VaultAuth,
}

#[derive(Deserialize)]
struct VaultAuth {
    client_token: String,
    #[serde(default)]
    lease_duration: u64,
}

// kv-v2 nests the stored fields one level down: `data.data.<field>`.
#[derive(Deserialize)]
struct KvRead {
    data: KvData,
}

#[derive(Deserialize)]
struct KvData {
    data: serde_json::Map<String, serde_json::Value>,
}

struct Cached {
    value: String,
    expires_at: Instant,
}

pub struct SecretClient {
    http: reqwest::Client,
    config: Config,
    cache: Mutex<Option<Cached>>,
}

impl SecretClient {
    pub fn new(config: Config) -> Result<Self, Error> {
        let mut builder = reqwest::Client::builder();
        if let Some(pem) = config.ca_pem_bytes()? {
            let cert = reqwest::Certificate::from_pem(&pem)
                .map_err(|e| Error::Config(format!("CA certificate: {e}")))?;
            builder = builder.add_root_certificate(cert);
        }
        let http = builder
            .build()
            .map_err(|e| Error::Config(format!("http client: {e}")))?;
        Ok(Self {
            http,
            config,
            cache: Mutex::new(None),
        })
    }

    pub async fn fetch(&self, service: &str, key: &str) -> Result<Secret, Error> {
        let path = format!("services/{service}/{key}");
        self.with_reauth(move |token| {
            let path = path.clone();
            async move { extract(&self.get(&token, &path).await?, key) }
        })
        .await
    }

    /// Read every field of the kv-v2 document at `path` (a logical path under the
    /// kv mount, e.g. `tasks/<slug>`; a leading `/` is trimmed). A missing
    /// document is `Error::NotFound`, not an empty map — the caller distinguishes
    /// "no secrets set" from "set but empty" by catching NotFound.
    pub async fn read_doc(&self, path: &str) -> Result<BTreeMap<String, Secret>, Error> {
        self.with_reauth(move |token| async move {
            collect(&self.get(&token, path).await?)
        })
        .await
    }

    /// Replace the kv-v2 document at `path` with exactly `data` (kv-v2 write
    /// semantics: fields absent from `data` are gone after this). Values are the
    /// plaintext to store; they are never logged or Displayed.
    pub async fn write_doc(&self, path: &str, data: &BTreeMap<String, String>) -> Result<(), Error> {
        self.with_reauth(move |token| async move { self.put(&token, path, data).await })
            .await
    }

    /// Delete every version of the document at `path`. A 404 is success, so a
    /// delete of an already-absent document is a no-op.
    pub async fn delete_doc(&self, path: &str) -> Result<(), Error> {
        self.with_reauth(move |token| async move { self.remove(&token, path).await })
            .await
    }

    // The shared token->attempt->re-auth-on-401->retry-once wrapper every kv
    // data-plane op runs through: `op` is handed a vault token and re-run once
    // with a fresh one if the first attempt is rejected at the read stage.
    async fn with_reauth<F, Fut, T>(&self, op: F) -> Result<T, Error>
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Result<T, Error>>,
    {
        let token = self.cached_token().await?;
        match op(token).await {
            Err(Error::AuthRejected(Stage::VaultRead)) => {
                let token = self.reauthenticate().await?;
                op(token).await
            }
            result => result,
        }
    }

    async fn cached_token(&self) -> Result<String, Error> {
        let mut guard = self.cache.lock().await;
        if let Some(cached) = guard.as_ref() {
            if cached.expires_at > Instant::now() {
                return Ok(cached.value.clone());
            }
        }
        let fresh = self.login().await?;
        let value = fresh.value.clone();
        *guard = Some(fresh);
        Ok(value)
    }

    async fn reauthenticate(&self) -> Result<String, Error> {
        let mut guard = self.cache.lock().await;
        *guard = None;
        let fresh = self.login().await?;
        let value = fresh.value.clone();
        *guard = Some(fresh);
        Ok(value)
    }

    async fn login(&self) -> Result<Cached, Error> {
        let jwt = self.authentik_token().await?;
        let (token, lease) = self.vault_login(&jwt).await?;
        let ttl = Duration::from_secs(lease).saturating_sub(LEASE_SKEW);
        Ok(Cached {
            value: token,
            expires_at: Instant::now() + ttl,
        })
    }

    async fn authentik_token(&self) -> Result<String, Error> {
        let form = token_form(
            &self.config.client_id,
            &self.config.service_account,
            self.config.app_password.expose(),
            &self.config.scope,
        );
        let resp = self
            .http
            .post(&self.config.authentik_token_url)
            .form(&form)
            .send()
            .await
            .map_err(|e| Error::Network {
                stage: Stage::AuthentikToken,
                detail: e.to_string(),
            })?;
        let status = resp.status();
        if status.is_success() {
            let body: TokenResponse = resp.json().await.map_err(|e| Error::Upstream {
                stage: Stage::AuthentikToken,
                detail: format!("invalid token payload: {e}"),
            })?;
            Ok(body.access_token)
        } else if AUTH_REJECT.contains(&status) {
            Err(Error::AuthRejected(Stage::AuthentikToken))
        } else {
            Err(Error::Upstream {
                stage: Stage::AuthentikToken,
                detail: format!("token endpoint returned {status}"),
            })
        }
    }

    async fn vault_login(&self, jwt: &str) -> Result<(String, u64), Error> {
        let url = login_url(&self.config.bao_addr);
        let body = serde_json::json!({ "role": self.config.jwt_role, "jwt": jwt });
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Network {
                stage: Stage::VaultLogin,
                detail: e.to_string(),
            })?;
        let status = resp.status();
        if status.is_success() {
            let body: VaultLoginResponse = resp.json().await.map_err(|e| Error::Upstream {
                stage: Stage::VaultLogin,
                detail: format!("invalid login payload: {e}"),
            })?;
            Ok((body.auth.client_token, body.auth.lease_duration))
        } else if AUTH_REJECT.contains(&status) {
            Err(Error::AuthRejected(Stage::VaultLogin))
        } else {
            Err(Error::Upstream {
                stage: Stage::VaultLogin,
                detail: format!("jwt login returned {status}"),
            })
        }
    }

    async fn get(
        &self,
        token: &str,
        path: &str,
    ) -> Result<serde_json::Map<String, serde_json::Value>, Error> {
        let url = data_url(&self.config.bao_addr, path);
        let resp = self
            .http
            .get(&url)
            .header("X-Vault-Token", token)
            .send()
            .await
            .map_err(|e| Error::Network {
                stage: Stage::VaultRead,
                detail: e.to_string(),
            })?;
        let status = resp.status();
        if status.is_success() {
            let body: KvRead = resp.json().await.map_err(|e| Error::Upstream {
                stage: Stage::VaultRead,
                detail: format!("invalid kv payload: {e}"),
            })?;
            Ok(body.data.data)
        } else if status == StatusCode::NOT_FOUND {
            Err(Error::NotFound)
        } else if AUTH_REJECT.contains(&status) {
            Err(Error::AuthRejected(Stage::VaultRead))
        } else {
            Err(Error::Upstream {
                stage: Stage::VaultRead,
                detail: format!("kv read returned {status}"),
            })
        }
    }

    async fn put(
        &self,
        token: &str,
        path: &str,
        data: &BTreeMap<String, String>,
    ) -> Result<(), Error> {
        let url = data_url(&self.config.bao_addr, path);
        let body = serde_json::json!({ "data": data });
        let resp = self
            .http
            .post(&url)
            .header("X-Vault-Token", token)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Network {
                stage: Stage::VaultRead,
                detail: e.to_string(),
            })?;
        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else if AUTH_REJECT.contains(&status) {
            Err(Error::AuthRejected(Stage::VaultRead))
        } else {
            Err(Error::Upstream {
                stage: Stage::VaultRead,
                detail: format!("kv write returned {status}"),
            })
        }
    }

    async fn remove(&self, token: &str, path: &str) -> Result<(), Error> {
        let url = metadata_url(&self.config.bao_addr, path);
        let resp = self
            .http
            .delete(&url)
            .header("X-Vault-Token", token)
            .send()
            .await
            .map_err(|e| Error::Network {
                stage: Stage::VaultRead,
                detail: e.to_string(),
            })?;
        let status = resp.status();
        if status.is_success() || status == StatusCode::NOT_FOUND {
            Ok(())
        } else if AUTH_REJECT.contains(&status) {
            Err(Error::AuthRejected(Stage::VaultRead))
        } else {
            Err(Error::Upstream {
                stage: Stage::VaultRead,
                detail: format!("kv delete returned {status}"),
            })
        }
    }
}

pub(crate) fn token_form<'a>(
    client_id: &'a str,
    username: &'a str,
    password: &'a str,
    scope: &'a str,
) -> [(&'a str, &'a str); 5] {
    [
        ("grant_type", "client_credentials"),
        ("client_id", client_id),
        ("username", username),
        ("password", password),
        ("scope", scope),
    ]
}

pub(crate) fn login_url(bao_addr: &str) -> String {
    format!("{}/v1/auth/jwt/login", bao_addr.trim_end_matches('/'))
}

// A logical kv path (`tasks/<slug>`, `services/<service>/<key>`) maps to the
// kv-v2 data plane for reads and writes and the metadata plane for deletes. A
// leading slash is caller noise and is trimmed so the path joins cleanly.
pub(crate) fn data_url(bao_addr: &str, path: &str) -> String {
    format!(
        "{}/v1/kv/data/{}",
        bao_addr.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

pub(crate) fn metadata_url(bao_addr: &str, path: &str) -> String {
    format!(
        "{}/v1/kv/metadata/{}",
        bao_addr.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn extract(
    fields: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Secret, Error> {
    match fields.get(key) {
        Some(serde_json::Value::String(s)) => Ok(Secret::new(s.clone())),
        Some(_) => Err(Error::Upstream {
            stage: Stage::VaultRead,
            detail: format!("value at {key} is not a string"),
        }),
        None => Err(Error::NotFound),
    }
}

fn collect(
    fields: &serde_json::Map<String, serde_json::Value>,
) -> Result<BTreeMap<String, Secret>, Error> {
    let mut out = BTreeMap::new();
    for (name, value) in fields {
        match value {
            serde_json::Value::String(s) => {
                out.insert(name.clone(), Secret::new(s.clone()));
            }
            _ => {
                return Err(Error::Upstream {
                    stage: Stage::VaultRead,
                    detail: format!("value at {name} is not a string"),
                })
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_url_trims_a_trailing_slash() {
        assert_eq!(
            login_url("http://127.0.0.1:8018"),
            "http://127.0.0.1:8018/v1/auth/jwt/login"
        );
        assert_eq!(
            login_url("http://127.0.0.1:8018/"),
            "http://127.0.0.1:8018/v1/auth/jwt/login"
        );
    }

    #[test]
    fn data_and_metadata_urls_trim_addr_slash_and_path_slash() {
        assert_eq!(
            data_url("http://127.0.0.1:8018/", "services/roleui/authentik_token"),
            "http://127.0.0.1:8018/v1/kv/data/services/roleui/authentik_token"
        );
        assert_eq!(
            data_url("http://127.0.0.1:8018", "/tasks/connector-principals"),
            "http://127.0.0.1:8018/v1/kv/data/tasks/connector-principals"
        );
        assert_eq!(
            metadata_url("http://127.0.0.1:8018/", "/tasks/connector-principals"),
            "http://127.0.0.1:8018/v1/kv/metadata/tasks/connector-principals"
        );
    }

    #[test]
    fn token_form_carries_the_client_credentials_grant_and_scope() {
        let form = token_form("cid", "svc-roleui", "pw", "openid effective_groups");
        assert_eq!(form[0], ("grant_type", "client_credentials"));
        assert!(form.contains(&("client_id", "cid")));
        assert!(form.contains(&("username", "svc-roleui")));
        assert!(form.contains(&("password", "pw")));
        assert!(form.contains(&("scope", "openid effective_groups")));
    }

    #[test]
    fn token_response_ignores_extra_fields() {
        let body: TokenResponse = serde_json::from_str(
            r#"{"access_token":"jwt-xyz","token_type":"Bearer","expires_in":300}"#,
        )
        .unwrap();
        assert_eq!(body.access_token, "jwt-xyz");
    }

    #[test]
    fn vault_login_response_parses_token_and_lease() {
        let body: VaultLoginResponse = serde_json::from_str(
            r#"{"auth":{"client_token":"vault-tok","lease_duration":900,"policies":["p"]}}"#,
        )
        .unwrap();
        assert_eq!(body.auth.client_token, "vault-tok");
        assert_eq!(body.auth.lease_duration, 900);
    }

    #[test]
    fn extract_reads_the_named_field_from_kv_v2_nesting() {
        let body: KvRead = serde_json::from_str(
            r#"{"data":{"data":{"authentik_token":"s3cr3t"},"metadata":{"version":1}}}"#,
        )
        .unwrap();
        let secret = extract(&body.data.data, "authentik_token").unwrap();
        assert_eq!(secret.expose(), "s3cr3t");
    }

    #[test]
    fn extract_missing_field_is_not_found_not_empty() {
        let body: KvRead = serde_json::from_str(r#"{"data":{"data":{"other":"x"}}}"#).unwrap();
        assert!(matches!(
            extract(&body.data.data, "authentik_token"),
            Err(Error::NotFound)
        ));
    }

    #[test]
    fn extract_non_string_field_is_upstream() {
        let body: KvRead =
            serde_json::from_str(r#"{"data":{"data":{"authentik_token":42}}}"#).unwrap();
        assert!(matches!(
            extract(&body.data.data, "authentik_token"),
            Err(Error::Upstream { .. })
        ));
    }

    #[test]
    fn collect_maps_every_field_to_a_redacting_secret() {
        let body: KvRead = serde_json::from_str(
            r#"{"data":{"data":{"alpha":"a","beta":"b"},"metadata":{"version":3}}}"#,
        )
        .unwrap();
        let map = collect(&body.data.data).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("alpha").unwrap().expose(), "a");
        assert_eq!(map.get("beta").unwrap().expose(), "b");
    }

    #[test]
    fn collect_of_an_empty_document_is_an_empty_map() {
        let body: KvRead = serde_json::from_str(r#"{"data":{"data":{}}}"#).unwrap();
        assert!(collect(&body.data.data).unwrap().is_empty());
    }

    #[test]
    fn collect_non_string_field_is_upstream() {
        let body: KvRead = serde_json::from_str(r#"{"data":{"data":{"alpha":7}}}"#).unwrap();
        assert!(matches!(
            collect(&body.data.data),
            Err(Error::Upstream { .. })
        ));
    }

    #[test]
    fn auth_reject_set_drives_the_reauth_decision() {
        for s in [
            StatusCode::BAD_REQUEST,
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
        ] {
            assert!(AUTH_REJECT.contains(&s));
        }
        for s in [
            StatusCode::OK,
            StatusCode::NOT_FOUND,
            StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            assert!(!AUTH_REJECT.contains(&s));
        }
    }
}
