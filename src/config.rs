//! Deployment configuration for the BFF client. `OidcConfig::new` takes the
//! three required inputs (issuer, client id, redirect URL); everything else is
//! defaulted, with opt-ins as builder methods (behavior carrying an invariant)
//! or plain fields (toggles). Refresh is default-OFF (DECISIONS.md R2 Track A).

use url::Url;

/// Deployment configuration. Everything else (endpoints, keys) comes from
/// OIDC discovery at [`crate::OidcState::discover`] time.
#[derive(Debug, Clone)]
pub struct OidcConfig {
    /// Browser-canonical issuer, e.g.
    /// `https://auth.dev.local/application/o/my-app/`. The SSO cookie jar
    /// lives on this host; authorize redirects always go here.
    pub issuer: Url,
    /// Origin override for server→authentik calls (token, userinfo) when the
    /// backend cannot reach the browser hostname, e.g.
    /// `http://server:9000`. Path is taken from discovery; only
    /// scheme/host/port are swapped. `None` = same origin as `issuer`.
    pub backchannel: Option<Url>,
    /// Public client id (uniform stand model: public + PKCE, no secret).
    pub client_id: String,
    /// Absolute redirect URL registered on the provider, e.g.
    /// `https://my-app.dev.local/oidc/callback`. Its path is also the
    /// callback route mounted by [`crate::router`].
    pub redirect_url: Url,
    /// Requested scopes. Default: `openid profile email effective_groups`
    /// — deliberately WITHOUT `offline_access`, so no refresh token is
    /// issued. Server-side refresh (ruling v5) is safe only if authentik
    /// revokes refresh tokens at logout; the live canary
    /// (`tests/live_canary.rs`) found it does NOT on 2026.5.2, so refresh is
    /// opt-in via [`OidcConfig::request_refresh_tokens`] pending that fix.
    pub scopes: Vec<String>,
    /// Path of the login-start route mounted by [`crate::router`] and baked
    /// into the served `/stand-oidc.js` shim (default `/oidc/login`). A
    /// browser hits it to (re)establish a session via silent `prompt=none`.
    pub login_path: String,
    /// Session cookie name (default `stand_session`).
    pub cookie_name: String,
    /// Mark cookies `Secure` (default true — stand and prod are https-only;
    /// disable only for plain-http localhost experiments).
    pub cookie_secure: bool,
    /// Accept untrusted TLS certificates on server→authentik calls (the dev
    /// stand's self-signed CA). Never enable in prod.
    pub danger_accept_invalid_certs: bool,
}

impl OidcConfig {
    pub fn new(issuer: Url, client_id: impl Into<String>, redirect_url: Url) -> Self {
        Self {
            issuer,
            backchannel: None,
            client_id: client_id.into(),
            redirect_url,
            scopes: ["openid", "profile", "email", "effective_groups"]
                .map(String::from)
                .to_vec(),
            login_path: "/oidc/login".into(),
            cookie_name: "stand_session".into(),
            cookie_secure: true,
            danger_accept_invalid_certs: false,
        }
    }

    /// Opt into server-side refresh tokens by requesting `offline_access`.
    ///
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

    /// Replace scheme/host/port of `url` with those of `base`, keeping path
    /// and query. Used to steer discovery-provided endpoints onto the
    /// backchannel origin (and the authorize endpoint onto the browser one).
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
        );
        // canary red -> refresh must be OFF unless explicitly opted in
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
        )
        .request_refresh_tokens()
        .request_refresh_tokens();
        assert_eq!(c.scopes.iter().filter(|s| *s == "offline_access").count(), 1);
    }
}
