//! The common-ui CORE, carried in a crate (R117).
//!
//! "Nothing vendored; a crate brings the right version." The four core assets
//! a page links (`base.css`, `elements.css`, `common-ui.js`) plus the shell
//! template and the TypeScript type contract ship here as committed consts,
//! the same "built assets in a crate" model as the oidc shim's `SHIM_JS`. A
//! consumer serves them from its own binary via `common_routing`'s static-file
//! mechanism at `/assets/...`; no Garage prefix and no content-hash prefix —
//! the Cargo **version** of this crate IS the pin.
//!
//! # Producer
//!
//! The bytes are authored in the sibling `common-ui-e` repo and emitted here
//! by its `build.sh` (into `src/assets/`, committed). This crate never builds
//! them; it only carries them. The `SRI_*` consts are the `sha384-…` integrity
//! strings of the exact bytes shipped, computed by the same producer over the
//! same files, so a page's `integrity=` attribute always matches the served
//! asset. [`CSP`] is the Content-Security-Policy the shell's pages require.
//!
//! # What is here vs. in `common-theme`
//!
//! Palettes are NOT here — the default palette and the bundled set live in
//! `common-theme`, together with the theme loader. The shell ([`SHELL_HTML`])
//! links `/assets/theme-default/palette.css` and `/assets/theme-loader.js`,
//! both served by the app from `common-theme`. The default palette's SRI is
//! therefore `common_theme::SRI_DEFAULT_PALETTE`, not a const here.

/// The crate version — `env!("CARGO_PKG_VERSION")`. This is the pin: a page's
/// footer prints it so a running page tells you which common-ui it carries.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The core base stylesheet (`@layer theme, page`, design tokens, resets).
/// Served at `/assets/base.css`.
pub const BASE_CSS: &str = include_str!("assets/base.css");

/// The elements stylesheet (the light-DOM classes the components mirror).
/// Served at `/assets/elements.css`.
pub const ELEMENTS_CSS: &str = include_str!("assets/elements.css");

/// The bundled component module (Lit folded in, R56). A `type="module"`
/// script. Served at `/assets/common-ui.js`.
pub const COMMON_UI_JS: &str = include_str!("assets/common-ui.js");

/// The page shell template. Carries BUILD markers `[[name]]` (filled by
/// `common_ui_build::stamp` in a consumer's `build.rs`) and RUNTIME markers
/// `{{name}}` (`assets_origin`, `config`, filled by `common_templating` at
/// request time). Links app-served `/assets/...` core + the default palette +
/// the theme loader.
pub const SHELL_HTML: &str = include_str!("assets/shell.html");

/// The published TypeScript type surface (§12.3). A consumer writes this to
/// its `OUT_DIR` (via `common_ui_build::write_dts`) for its own typecheck and
/// keeps NO vendored copy — the types cannot drift from the bundle.
pub const COMMON_UI_DTS: &str = include_str!("assets/common-ui.d.ts");

/// `sha384-…` integrity of [`BASE_CSS`], for the shell's `integrity=`.
pub const SRI_BASE_CSS: &str = include_str!("assets/base.css.sri");

/// `sha384-…` integrity of [`ELEMENTS_CSS`].
pub const SRI_ELEMENTS_CSS: &str = include_str!("assets/elements.css.sri");

/// `sha384-…` integrity of [`COMMON_UI_JS`].
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
            // Emitted with no trailing newline, so the const is a clean
            // attribute value.
            assert_eq!(sri.trim(), sri, "{what} SRI has surrounding whitespace");
        }
    }

    #[test]
    fn csp_names_the_assets_origin_marker() {
        // A policy without it would block the loader's Garage stylesheet.
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
        // No stylesheet/script is loaded from a Garage prefix: every shared
        // href is app-served /assets, none is `{{assets_origin}}/common-ui@…`.
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
