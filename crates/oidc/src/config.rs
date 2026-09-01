//! Configuration values and their defaults. Resolution and validation only —
//! nothing here performs IO or decides a response.

use std::path::PathBuf;

use common_logging::Deployment;
use url::Url;

#[derive(Debug, Clone)]
pub struct OidcConfig {
    pub issuer: Url,
    pub backchannel: Option<Url>,
    pub client_id: String,
    pub redirect_url: Url,
    pub scopes: Vec<String>,
    pub login_path: String,
    pub cookie_name: String,
    pub cookie_secure: bool,
    pub danger_accept_invalid_certs: bool,
    pub assets_dir: PathBuf,
    pub deployment: Deployment,
}

impl OidcConfig {
    /// `cookie_name` is required rather than defaulted: a session cookie is
    /// browser-visible and per-application, and a default nobody chose is a
    /// name nobody re-reads. Pass the owning app's own name.
    pub fn new(
        issuer: Url,
        client_id: impl Into<String>,
        redirect_url: Url,
        cookie_name: impl Into<String>,
    ) -> Self {
        Self {
            issuer,
            backchannel: None,
            client_id: client_id.into(),
            redirect_url,
            scopes: ["openid", "profile", "email", "effective_groups"]
                .map(String::from)
                .to_vec(),
            login_path: "/oidc/login".into(),
            cookie_name: cookie_name.into(),
            cookie_secure: true,
            danger_accept_invalid_certs: false,
            assets_dir: PathBuf::from("assets"),
            deployment: Deployment::Dev,
        }
    }

    /// DO NOT call this until the deployment's authentik is confirmed to
    /// revoke refresh tokens at logout — run `tests/live_canary.rs` green
    /// first. Without it, a logged-out identity can be resurrected via the
    /// refresh token for its whole validity (see ruling v5 and the canary).
    pub fn request_refresh_tokens(mut self) -> Self {
        if !self.scopes.iter().any(|s| s == "offline_access") {
            self.scopes.push("offline_access".into());
        }
        self
    }

    pub(crate) fn swap_origin(url: &Url, base: &Url) -> Url {
        let mut out = url.clone();
        out.set_scheme(base.scheme()).expect("valid scheme");
        out.set_host(base.host_str()).expect("valid host");
        out.set_port(base.port()).expect("valid port");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_origin_keeps_path_and_query() {
        let u = Url::parse("https://auth.dev.local/application/o/token/?a=1").unwrap();
        let b = Url::parse("http://server:9000").unwrap();
        assert_eq!(
            OidcConfig::swap_origin(&u, &b).as_str(),
            "http://server:9000/application/o/token/?a=1"
        );
    }

    #[test]
    fn defaults_are_safe_no_refresh_but_request_effective_groups() {
        let c = OidcConfig::new(
            Url::parse("https://auth.dev.local/application/o/x/").unwrap(),
            "x",
            Url::parse("https://x.dev.local/oidc/callback").unwrap(),
            "test_session",
        );
        assert!(!c.scopes.iter().any(|s| s == "offline_access"));
        assert!(c.scopes.iter().any(|s| s == "effective_groups"));
        assert!(c.cookie_secure);
        assert!(!c.danger_accept_invalid_certs);
    }

    #[test]
    fn request_refresh_tokens_opt_in_adds_offline_access_once() {
        let c = OidcConfig::new(
            Url::parse("https://auth.dev.local/application/o/x/").unwrap(),
            "x",
            Url::parse("https://x.dev.local/oidc/callback").unwrap(),
            "test_session",
        )
        .request_refresh_tokens()
        .request_refresh_tokens();
        assert_eq!(c.scopes.iter().filter(|s| *s == "offline_access").count(), 1);
    }
}
