//! Build-script helper for common-ui consumers (R117, crate-carried delivery).
//!
//! Since R117 there is no Garage fetch, no manifest and no prefix pin: the core
//! assets are carried in [`common_ui_core`] and the palettes + loader in
//! [`common_theme`], both as committed consts. This crate is the thin
//! build-time glue a consumer's `build.rs` calls:
//!
//! - [`stamp`] fills the shell's `[[build markers]]` (via `upon`), leaving the
//!   `{{runtime markers}}` for `common_templating`'s per-request render.
//! - [`shared_markers`] returns the build markers that are a pure function of
//!   the shipped assets and identical in every consumer: the version-derived
//!   `prefix` and the four SRI strings each page links. The SRIs come straight
//!   from the crates, so a hash is never hand-copied.
//! - [`write_dts`] writes [`common_ui_core::COMMON_UI_DTS`] to `OUT_DIR` for
//!   the consumer's typecheck; the consumer keeps no vendored copy.
//! - [`csp`] returns the policy the pages require ([`common_ui_core::CSP`]).
//!
//! A consumer serves the assets themselves — `common_ui_core::{BASE_CSS,
//! ELEMENTS_CSS, COMMON_UI_JS}`, `common_theme::{PALETTES, LOADER_JS}` — at
//! `/assets/...` through `common_routing`'s static-file mechanism.
//!
//! # The marker styles
//!
//! The shell carries TWO marker styles. BUILD markers are `[[name]]`, filled
//! here by [`stamp`] through `upon`, which ERRORS on any `[[marker]]` left
//! unfilled — a forgotten or misspelled build marker is a build-time failure,
//! not a raw marker on the page. RUNTIME markers are `{{name}}`
//! (`assets_origin`, `config`); [`stamp`] leaves them for the app's boot,
//! which renders the stamped shell through `common_templating::Shell` and
//! refuses any `{{marker}}` left unfilled, naming it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The name the d.ts is written under in `OUT_DIR`.
pub const TYPES: &str = "common-ui.d.ts";

/// The Content-Security-Policy the shell's pages require, `{{assets_origin}}`
/// still in it (a consumer resolves that when it sets the header).
pub fn csp() -> &'static str {
    common_ui_core::CSP
}

/// The build markers that are a pure function of the shipped assets and
/// identical in EVERY consumer: the `prefix` (the common-ui-core version, so a
/// page's footer names which common-ui it runs) and the SRI of the four
/// sheets/script each page links. A `build.rs` extends its app-specific marker
/// list with this. Every SRI is read from the crate the bytes ship in, so a
/// hash is never hand-copied and cannot drift from the served file.
pub fn shared_markers() -> Vec<(&'static str, String)> {
    vec![
        ("prefix", format!("common-ui {}", common_ui_core::VERSION)),
        ("sri_base_css", common_ui_core::SRI_BASE_CSS.to_owned()),
        // The default palette lives in common-theme, so its SRI does too.
        (
            "sri_palette_css",
            common_theme::SRI_DEFAULT_PALETTE.to_owned(),
        ),
        (
            "sri_elements_css",
            common_ui_core::SRI_ELEMENTS_CSS.to_owned(),
        ),
        (
            "sri_common_ui_js",
            common_ui_core::SRI_COMMON_UI_JS.to_owned(),
        ),
    ]
}

/// Writes [`common_ui_core::COMMON_UI_DTS`] into `out_dir` as [`TYPES`] for the
/// consumer's typecheck to point its tsconfig `paths` at. Returns the path.
pub fn write_dts(out_dir: &Path) -> std::io::Result<PathBuf> {
    let path = out_dir.join(TYPES);
    std::fs::write(&path, common_ui_core::COMMON_UI_DTS)?;
    Ok(path)
}

/// Fills the shell's `[[marker]]` BUILD markers from `values`, leaving every
/// `{{marker}}` RUNTIME marker (`assets_origin`, `config`) for
/// common-templating's per-request render.
///
/// The engine is `upon` with `[[ ]]` expression delimiters and no escaping:
/// the values are the consumer's own constants and the SRIs the crates carry,
/// so they reach the page verbatim. `{{ }}` is not this engine's delimiter, so
/// runtime markers pass through untouched.
///
/// This runs inside a `build.rs`, so a defect must fail the build. `upon`
/// ERRORS on any `[[marker]]` the shell holds that `values` does not fill —
/// naming it — and on a shell that does not parse as a `[[ ]]` template.
/// Either is a PANIC here, the correct build failure: a forgotten or
/// misspelled build marker is caught at build time, never shipped raw to a
/// browser. (Runtime `{{ }}` markers are still refused at boot by
/// common-templating's `Shell`, which names them.)
pub fn stamp(shell: &str, values: &[(&str, &str)]) -> String {
    let syntax = upon::Syntax::builder().expr("[[", "]]").build();
    let mut engine = upon::Engine::new();
    engine.set_syntax(syntax);
    let template = engine.compile(shell).unwrap_or_else(|e| {
        panic!("the common-ui shell does not parse as a [[ ]] template: {e:#}\n\nshell:\n{shell}")
    });
    let map: BTreeMap<&str, &str> = values.iter().copied().collect();
    template
        .render(&engine, &map)
        .to_string()
        .unwrap_or_else(|e| {
            panic!(
            "stamping the common-ui shell failed — an unfilled or misspelled [[build marker]]?: \
             {e:#}\n\nshell:\n{shell}"
        )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_markers_are_the_five_from_the_crates() {
        let markers = shared_markers();
        let names: Vec<&str> = markers.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            vec![
                "prefix",
                "sri_base_css",
                "sri_palette_css",
                "sri_elements_css",
                "sri_common_ui_js",
            ]
        );
        // The SRIs are the crates' own, not hand-copied.
        let by = |k: &str| {
            markers
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.as_str())
                .unwrap()
        };
        assert_eq!(by("sri_base_css"), common_ui_core::SRI_BASE_CSS);
        assert_eq!(by("sri_palette_css"), common_theme::SRI_DEFAULT_PALETTE);
        assert_eq!(by("sri_elements_css"), common_ui_core::SRI_ELEMENTS_CSS);
        assert_eq!(by("sri_common_ui_js"), common_ui_core::SRI_COMMON_UI_JS);
        assert!(by("prefix").starts_with("common-ui "));
    }

    #[test]
    fn stamp_fills_build_markers_and_leaves_runtime_markers() {
        let shell = "<x>[[prefix]]</x>{{assets_origin}}";
        let out = stamp(shell, &[("prefix", "common-ui 0.3.0")]);
        assert_eq!(out, "<x>common-ui 0.3.0</x>{{assets_origin}}");
    }

    #[test]
    fn stamp_stamps_the_real_shell_leaving_only_runtime_markers() {
        // Every [[build marker]] the real shell carries must be fillable, and
        // the {{runtime markers}} must survive — the two-engine contract.
        let mut values: Vec<(&str, String)> = shared_markers();
        for (k, v) in [
            ("title", "T"),
            ("app_name", "app"),
            ("app_icon", "x"),
            ("bar_pages", ""),
            ("page_css", "/p.css"),
            ("page_module", "/p.js"),
            ("root_class", ""),
            ("footer_app", "app"),
            ("footer_commit", "abc"),
            ("legal", "L"),
        ] {
            values.push((k, v.to_owned()));
        }
        let refs: Vec<(&str, &str)> = values.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let out = stamp(common_ui_core::SHELL_HTML, &refs);
        assert!(
            !out.contains("[["),
            "an unfilled build marker remains: {out}"
        );
        assert!(
            out.contains("{{assets_origin}}"),
            "runtime marker was eaten"
        );
        assert!(out.contains("{{config}}"), "runtime marker was eaten");
    }
}
