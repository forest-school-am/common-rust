//! Client configuration and its defaults. Resolution only — nothing here
//! performs a request or decides a response; the CA is read from disk lazily
//! (in `ca_pem_bytes`) so a `Config` can be constructed before its file exists.

use std::path::PathBuf;

use crate::error::Error;
use crate::secret::AppPassword;

/// The scope the browserless jwt-login binds on: OpenBao reads claims from the
/// token, not from userinfo, so `effective_groups` is present ONLY when asked
/// for here.
pub const DEFAULT_SCOPE: &str = "openid effective_groups";

#[derive(Clone)]
enum Ca {
    None,
    Pem(Vec<u8>),
    Path(PathBuf),
}

#[derive(Clone)]
pub struct Config {
    pub authentik_token_url: String,
    pub client_id: String,
    pub service_account: String,
    pub app_password: AppPassword,
    pub scope: String,
    pub bao_addr: String,
    pub jwt_role: String,
    ca: Ca,
}

impl Config {
    pub fn new(
        authentik_token_url: impl Into<String>,
        client_id: impl Into<String>,
        service_account: impl Into<String>,
        app_password: AppPassword,
        bao_addr: impl Into<String>,
        jwt_role: impl Into<String>,
    ) -> Self {
        Self {
            authentik_token_url: authentik_token_url.into(),
            client_id: client_id.into(),
            service_account: service_account.into(),
            app_password,
            scope: DEFAULT_SCOPE.into(),
            bao_addr: bao_addr.into(),
            jwt_role: jwt_role.into(),
            ca: Ca::None,
        }
    }

    pub fn scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = scope.into();
        self
    }

    pub fn ca_pem(mut self, pem: impl Into<Vec<u8>>) -> Self {
        self.ca = Ca::Pem(pem.into());
        self
    }

    pub fn ca_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.ca = Ca::Path(path.into());
        self
    }

    pub(crate) fn ca_pem_bytes(&self) -> Result<Option<Vec<u8>>, Error> {
        match &self.ca {
            Ca::None => Ok(None),
            Ca::Pem(bytes) => Ok(Some(bytes.clone())),
            Ca::Path(path) => std::fs::read(path)
                .map(Some)
                .map_err(|e| Error::Config(format!("reading CA at {}: {e}", path.display()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults_scope_and_leaves_ca_unset() {
        let c = Config::new(
            "u",
            "cid",
            "svc",
            AppPassword::new("pw"),
            "http://b",
            "role",
        );
        assert_eq!(c.scope, DEFAULT_SCOPE);
        assert_eq!(c.ca_pem_bytes().unwrap(), None);
    }

    #[test]
    fn ca_pem_is_returned_verbatim_and_scope_overrides() {
        let c = Config::new(
            "u",
            "cid",
            "svc",
            AppPassword::new("pw"),
            "http://b",
            "role",
        )
        .scope("openid")
        .ca_pem(b"-----BEGIN-----".to_vec());
        assert_eq!(c.scope, "openid");
        assert_eq!(c.ca_pem_bytes().unwrap(), Some(b"-----BEGIN-----".to_vec()));
    }

    #[test]
    fn ca_path_that_does_not_exist_is_a_config_error() {
        let c = Config::new(
            "u",
            "cid",
            "svc",
            AppPassword::new("pw"),
            "http://b",
            "role",
        )
        .ca_path("/no/such/ca.crt");
        assert!(matches!(c.ca_pem_bytes(), Err(Error::Config(_))));
    }
}
