//! The common-ui core assets carried as committed consts: the sheets, the
//! component module, the shell template, the d.ts and their SRIs. This crate
//! only CARRIES them (authored in the sibling `common-ui-e` repo); a consumer
//! serves them from its own binary. Palettes are NOT here — they and the theme
//! loader live in `common-theme`. The Cargo version is the pin; no hash prefix.

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const BASE_CSS: &str = include_str!("assets/base.css");

pub const ELEMENTS_CSS: &str = include_str!("assets/elements.css");

pub const COMMON_UI_JS: &str = include_str!("assets/common-ui.js");

pub const SHELL_HTML: &str = include_str!("assets/shell.html");

/// A consumer writes this to its `OUT_DIR` for its typecheck and keeps NO
/// vendored copy, so the types cannot drift from the bundle.
pub const COMMON_UI_DTS: &str = include_str!("assets/common-ui.d.ts");

pub const SRI_BASE_CSS: &str = include_str!("assets/base.css.sri");

pub const SRI_ELEMENTS_CSS: &str = include_str!("assets/elements.css.sri");

pub const SRI_COMMON_UI_JS: &str = include_str!("assets/common-ui.js.sri");

/// The Content-Security-Policy the shell's pages require. Still names
/// `{{assets_origin}}` (so the theme loader may fetch a hashed palette from
/// Garage); a consumer resolves that marker when it sets the header.
pub const CSP: &str = include_str!("assets/csp.txt");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_are_present_and_nonempty() {
        for (what, s) in [
            ("base.css", BASE_CSS),
            ("elements.css", ELEMENTS_CSS),
            ("common-ui.js", COMMON_UI_JS),
            ("shell.html", SHELL_HTML),
            ("common-ui.d.ts", COMMON_UI_DTS),
        ] {
            assert!(!s.is_empty(), "{what} is empty — did build.sh emit it?");
        }
    }

    #[test]
    fn sri_strings_are_sha384_and_untrimmed() {
        for (what, sri) in [
            ("base.css", SRI_BASE_CSS),
            ("elements.css", SRI_ELEMENTS_CSS),
            ("common-ui.js", SRI_COMMON_UI_JS),
        ] {
            assert!(
                sri.starts_with("sha384-"),
                "{what} SRI is not sha384-…: {sri:?}"
            );
            assert_eq!(sri.trim(), sri, "{what} SRI has surrounding whitespace");
        }
    }

    #[test]
    fn csp_names_the_assets_origin_marker() {
        assert!(
            CSP.contains("{{assets_origin}}"),
            "the CSP must keep {{{{assets_origin}}}} for hashed-theme fetches"
        );
    }

    #[test]
    fn shell_links_app_served_core_and_the_loader() {
        assert!(SHELL_HTML.contains("/assets/base.css"));
        assert!(SHELL_HTML.contains("/assets/theme-default/palette.css"));
        assert!(SHELL_HTML.contains("/assets/theme-loader.js"));
        assert!(
            !SHELL_HTML.contains("href=\"{{assets_origin}}"),
            "the shell must not link a Garage-hosted stylesheet"
        );
        assert!(
            !SHELL_HTML.contains("src=\"{{assets_origin}}"),
            "the shell must not load a Garage-hosted script"
        );
    }
}
