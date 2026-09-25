//! The `[oidc]` section a consumer nests in its `#[derive(Config)]` root: every
//! operator-set value an `OidcConfig` needs, spelled once. What the values DO
//! is config.rs/client.rs; the deployment class stays a root field and is
//! handed in at `to_config`.

use common_config::Config;
use common_logging::Deployment;
use url::Url;

use crate::config::OidcConfig;

#[derive(Debug, Clone, Config)]
pub struct OidcSection {
    /// Issuer URL, the provider's `application/o/<slug>/`; discovery appends
    /// `.well-known/openid-configuration`.
    #[config(required)]
    pub issuer: Url,
    /// Issuer origin for server-to-server calls (discovery, token, userinfo)
    /// when it differs from the browser-facing one; unset means the issuer's.
    pub backchannel: Option<Url>,
    /// The client id registered at the provider.
    #[config(required)]
    pub client_id: String,
    /// Confidential-client secret; unset means a public (PKCE-only) client.
    #[config(secret)]
    pub client_secret: Option<String>,
    /// The redirect URI registered at the provider; its path is the callback route.
    #[config(required)]
    pub redirect_url: Url,
    /// Name of the session cookie.
    #[config(required)]
    pub cookie_name: String,
    /// Send the session cookie only over https.
    #[config(default = "true")]
    pub cookie_secure: bool,
    /// Accept an untrusted TLS certificate on back-channel calls. Dev-only:
    /// discovery refuses it under prod.
    pub danger_accept_invalid_certs: bool,
    /// Ask the provider for a refresh token (`offline_access`).
    pub refresh_tokens: bool,
}

impl OidcSection {
    pub fn to_config(&self, deployment: Deployment) -> OidcConfig {
        let mut config = OidcConfig::new(
            self.issuer.clone(),
            self.client_id.clone(),
            self.redirect_url.clone(),
            self.cookie_name.clone(),
        );
        config.backchannel = self.backchannel.clone();
        config.client_secret = self.client_secret.clone();
        config.cookie_secure = self.cookie_secure;
        config.danger_accept_invalid_certs = self.danger_accept_invalid_certs;
        config.deployment = deployment;
        if self.refresh_tokens {
            config = config.request_refresh_tokens();
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common_config::{Outcome, Path};

    #[derive(Debug, Config)]
    #[config(app = "DEMO")]
    struct Demo {
        #[config(nested)]
        oidc: OidcSection,
        deployment: Deployment,
    }

    fn load(env: &[(&str, &str)]) -> Result<Demo, common_config::Refusal> {
        let env: Vec<(String, String)> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        match common_config::load_from::<Demo>(&[], &env)? {
            Outcome::Config(demo) => Ok(demo),
            other => panic!("expected a config, got {other:?}"),
        }
    }

    const REQUIRED: [(&str, &str); 5] = [
        ("DEPLOYMENT_TYPE", "dev"),
        (
            "DEMO_OIDC__ISSUER",
            "https://auth.dev.local/application/o/x/",
        ),
        ("DEMO_OIDC__CLIENT_ID", "x"),
        (
            "DEMO_OIDC__REDIRECT_URL",
            "https://x.dev.local/oidc/callback",
        ),
        ("DEMO_OIDC__COOKIE_NAME", "x_session"),
    ];

    #[test]
    fn the_section_spells_every_value_under_the_oidc_table() {
        let spelled: Vec<(String, String)> = OidcSection::schema(&Path::root().child("oidc"))
            .iter()
            .map(|f| (f.env("DEMO"), f.toml()))
            .collect();
        assert_eq!(
            spelled,
            [
                ("DEMO_OIDC__ISSUER", "oidc.issuer"),
                ("DEMO_OIDC__BACKCHANNEL", "oidc.backchannel"),
                ("DEMO_OIDC__CLIENT_ID", "oidc.client_id"),
                ("DEMO_OIDC__CLIENT_SECRET", "oidc.client_secret"),
                ("DEMO_OIDC__REDIRECT_URL", "oidc.redirect_url"),
                ("DEMO_OIDC__COOKIE_NAME", "oidc.cookie_name"),
                ("DEMO_OIDC__COOKIE_SECURE", "oidc.cookie_secure"),
                (
                    "DEMO_OIDC__DANGER_ACCEPT_INVALID_CERTS",
                    "oidc.danger_accept_invalid_certs"
                ),
                ("DEMO_OIDC__REFRESH_TOKENS", "oidc.refresh_tokens"),
            ]
            .map(|(a, b)| (a.to_string(), b.to_string()))
        );
    }

    #[test]
    fn the_minimal_section_is_a_public_pkce_client_with_the_crate_defaults() {
        let demo = load(&REQUIRED).unwrap();
        let config = demo.oidc.to_config(Deployment::Prod);
        assert_eq!(config.client_id, "x");
        assert_eq!(config.cookie_name, "x_session");
        assert_eq!(config.backchannel, None);
        assert_eq!(config.client_secret, None);
        assert!(config.cookie_secure);
        assert!(!config.danger_accept_invalid_certs);
        assert!(!config.scopes.iter().any(|s| s == "offline_access"));
        assert_eq!(config.deployment, Deployment::Prod);
        assert_eq!(config.login_path, "/oidc/login");
    }

    #[test]
    fn every_optional_value_reaches_the_config() {
        let mut env = REQUIRED.to_vec();
        env.extend([
            ("DEMO_OIDC__BACKCHANNEL", "http://127.0.0.1:8000"),
            ("DEMO_OIDC__CLIENT_SECRET", "s3cret"),
            ("DEMO_OIDC__COOKIE_SECURE", "false"),
            ("DEMO_OIDC__DANGER_ACCEPT_INVALID_CERTS", "1"),
            ("DEMO_OIDC__REFRESH_TOKENS", "yes"),
        ]);
        let config = load(&env).unwrap().oidc.to_config(Deployment::Dev);
        assert_eq!(
            config.backchannel.as_ref().map(Url::as_str),
            Some("http://127.0.0.1:8000/")
        );
        assert_eq!(config.client_secret.as_deref(), Some("s3cret"));
        assert!(!config.cookie_secure);
        assert!(config.danger_accept_invalid_certs);
        assert!(config.scopes.iter().any(|s| s == "offline_access"));
    }

    #[test]
    fn a_malformed_issuer_refuses_naming_the_variable() {
        let mut env = REQUIRED.to_vec();
        env[1] = ("DEMO_OIDC__ISSUER", "not a url");
        let refusal = load(&env).unwrap_err();
        assert_eq!(refusal.variable, "DEMO_OIDC__ISSUER");
        assert_eq!(refusal.value, "not a url");
    }

    #[test]
    fn the_secret_is_masked_in_a_wrong_type_refusal_and_print_config() {
        let mut env = REQUIRED.to_vec();
        env.push(("DEMO_OIDC__CLIENT_SECRET", "hunter2"));
        let env: Vec<(String, String)> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let out = match common_config::load_from::<Demo>(&["--print-config".to_string()], &env) {
            Ok(Outcome::PrintConfig(text)) => text,
            other => panic!("expected print-config text, got {other:?}"),
        };
        assert!(!out.contains("hunter2"), "{out}");
        assert!(out.contains("oidc.client_secret"), "{out}");
    }
}
