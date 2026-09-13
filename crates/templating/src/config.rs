//! What a page is told about its world, and how that reaches the page: one
//! serde struct, serialised into a data block. Anything a page asks for at
//! RUNTIME belongs in a service's own API, not here.

use serde::Serialize;
use ts_rs::TS;

use crate::AssetsOrigin;

/// Who is looking at the page, for `<les-account>` (R72). Server-side, from the
/// session — the element reads no cookie and no token of its own (§12.17).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
pub struct User {
    pub name: String,
    pub portrait: Option<String>,
}

/// The one object page code reads on its first line. Per-request values belong
/// here; there is no `/api/config` and no value passed through JS (R65).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
pub struct Config {
    pub assets_origin: String,
    pub login_path: String,
    /// `None` on an unauthenticated page, where `<les-bar>` renders no account
    /// at all rather than an empty one — registry's launcher is the case.
    pub user: Option<User>,
    /// R75's launcher root. EMPTY means absent, which is what the bar's own
    /// truthiness check treats as "render the app token as text, not a dead
    /// link" — so the Rust side and the element agree without a second
    /// convention.
    pub launcher_url: String,
    /// R73's end-session route. Empty renders no logout control, which is how
    /// the element ships before the route exists.
    pub logout_path: String,
}

impl Config {
    /// The signature is unchanged on purpose: every consumer already calls it
    /// with two arguments, and the bar's three values are set afterwards on the
    /// public fields. Turning this into a builder is the templating window's
    /// job, not a fleet break today.
    pub fn new(assets_origin: &AssetsOrigin, login_path: impl Into<String>) -> Self {
        Self {
            assets_origin: assets_origin.as_str().to_owned(),
            login_path: login_path.into(),
            user: None,
            launcher_url: String::new(),
            logout_path: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common_logging::Deployment;

    fn origin() -> AssetsOrigin {
        AssetsOrigin::parse(Some("https://assets.dev.local"), Deployment::Prod)
            .unwrap()
            .unwrap()
    }

    /// The page reads these names, and `<les-bar>` reads three of them. A
    /// rename here is silent on this side and breaks the bar on the other, so
    /// the keys are spelled out rather than derived from the struct.
    #[test]
    fn the_serialised_keys_are_the_ones_page_code_reads() {
        let json = serde_json::to_value(Config::new(&origin(), "/oidc/login")).unwrap();
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
            "camelCase, and no key added without the element that reads it"
        );
    }

    #[test]
    fn an_unauthenticated_page_serialises_user_as_null() {
        let json = serde_json::to_string(&Config::new(&origin(), "")).unwrap();
        assert!(
            json.contains("\"user\":null"),
            "the bar renders no account for a null user rather than an empty \
             one — registry's launcher is unauthenticated: {json}"
        );
    }

    #[test]
    fn a_user_without_a_portrait_is_null_not_empty() {
        let mut config = Config::new(&origin(), "");
        config.user = Some(User {
            name: "Alice Ackroyd".to_owned(),
            portrait: None,
        });
        let json = serde_json::to_string(&config).unwrap();
        assert!(
            json.contains("\"portrait\":null"),
            "an empty STRING would make the element render an img with src=\"\", \
             which is a broken-image glyph where a face goes: {json}"
        );
    }
}
