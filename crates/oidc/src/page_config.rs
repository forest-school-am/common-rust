//! What a page is told about its world: the block a service serialises into
//! its shell, read by page code on its first line and by the shim (`SHIM_JS`)
//! for `loginPath`. The paths come off the [`OidcConfig`] the router was
//! mounted with, so a page cannot carry a literal that drifts from a route.
//!
//! The serialised KEY NAMES are a contract with the readers in common-ui; the
//! test below spells them out so a rename here is loud.

use serde::Serialize;

use crate::config::{OidcConfig, LOGIN_PATH};

/// Who is looking at the page. `portrait` is `None`, never `""`: the reader
/// renders initials for a missing image and a broken `<img>` for an empty one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageUser {
    name: String,
    portrait: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageConfig {
    assets_origin: String,
    login_path: String,
    launcher_url: String,
    logout_path: String,
    /// `None` on an unauthenticated page: the reader renders no account at
    /// all rather than an empty one.
    user: Option<PageUser>,
}

impl PageConfig {
    /// `user` is the display name the service resolved for this request, or
    /// `None` when nobody is signed in. No deployment on the stand has a
    /// portrait source (userinfo carries no picture claim), so the portrait is
    /// `None` for every user.
    pub fn new(
        cfg: &OidcConfig,
        user: Option<&str>,
        launcher_url: &str,
        assets_origin: &str,
    ) -> Self {
        Self::build(
            &cfg.login_path,
            &cfg.logout_path,
            user,
            launcher_url,
            assets_origin,
        )
    }

    /// For a deployment that mounts no OIDC router and so has no
    /// [`OidcConfig`] (a `--no-auth` build, a cookie-login stub). The login
    /// path keeps the crate's default spelling as a placeholder; the logout
    /// path is EMPTY, which the reader takes as "render no logout control" —
    /// better than a control that would POST into a 404.
    pub fn without_oidc(user: Option<&str>, launcher_url: &str, assets_origin: &str) -> Self {
        Self::build(LOGIN_PATH, "", user, launcher_url, assets_origin)
    }

    fn build(
        login_path: &str,
        logout_path: &str,
        user: Option<&str>,
        launcher_url: &str,
        assets_origin: &str,
    ) -> Self {
        Self {
            assets_origin: assets_origin.to_owned(),
            login_path: login_path.to_owned(),
            launcher_url: launcher_url.to_owned(),
            logout_path: logout_path.to_owned(),
            user: user.map(|name| PageUser {
                name: name.to_owned(),
                portrait: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn oidc() -> OidcConfig {
        OidcConfig::new(
            Url::parse("https://auth.dev.local/application/o/x/").unwrap(),
            "x",
            Url::parse("https://x.dev.local/oidc/callback").unwrap(),
            "test_session",
        )
    }

    /// The names page code and the shim read, spelled out rather than derived
    /// from the struct: a rename here is silent on this side and breaks the
    /// reader on the other.
    #[test]
    fn the_serialised_keys_are_the_ones_page_code_reads() {
        let json = serde_json::to_value(PageConfig::new(
            &oidc(),
            Some("Alice Ackroyd"),
            "https://launcher.dev.local",
            "https://assets.dev.local",
        ))
        .unwrap();
        let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "assetsOrigin",
                "launcherUrl",
                "loginPath",
                "logoutPath",
                "user"
            ],
            "camelCase, and no key added without a reader for it"
        );
        assert_eq!(json["loginPath"], "/oidc/login", "taken off the OidcConfig");
        assert_eq!(
            json["logoutPath"], "/common-oidc/logout",
            "taken off the OidcConfig"
        );
        assert_eq!(json["assetsOrigin"], "https://assets.dev.local");
        assert_eq!(json["launcherUrl"], "https://launcher.dev.local");
        assert_eq!(json["user"]["name"], "Alice Ackroyd");
        assert!(
            json["user"]["portrait"].is_null(),
            "no portrait source: null, never an empty string — {json}"
        );
    }

    #[test]
    fn an_unauthenticated_page_serialises_user_as_null() {
        let json = serde_json::to_string(&PageConfig::new(&oidc(), None, "", "")).unwrap();
        assert!(
            json.contains("\"user\":null"),
            "the reader renders no account for a null user rather than an empty one: {json}"
        );
    }

    #[test]
    fn without_a_router_the_logout_path_is_empty_and_the_login_path_is_the_default() {
        let json = serde_json::to_value(PageConfig::without_oidc(None, "", "")).unwrap();
        assert_eq!(json["loginPath"], "/oidc/login");
        assert_eq!(json["logoutPath"], "", "no route exists to POST to");
    }
}
