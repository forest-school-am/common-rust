//! Build-time glue a common-ui consumer's `build.rs` calls: [`stamp`] fills the
//! shell's `[[build markers]]`, [`shared_markers`] returns the marker values that
//! come straight from the crates, [`write_dts`] emits the d.ts, [`csp`] returns the
//! policy. The assets themselves the consumer serves from [`common_ui_core`] /
//! [`common_theme`] — no Garage fetch, no manifest, no prefix pin.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const TYPES: &str = "common-ui.d.ts";

/// The Content-Security-Policy the shell's pages require, `{{assets_origin}}`
/// still in it (a consumer resolves that when it sets the header).
pub fn csp() -> &'static str {
    common_ui_core::CSP
}

pub fn shared_markers() -> Vec<(&'static str, String)> {
    vec![
        ("prefix", format!("common-ui {}", common_ui_core::VERSION)),
        ("sri_base_css", common_ui_core::SRI_BASE_CSS.to_owned()),
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

pub fn write_dts(out_dir: &Path) -> std::io::Result<PathBuf> {
    let path = out_dir.join(TYPES);
    std::fs::write(&path, common_ui_core::COMMON_UI_DTS)?;
    Ok(path)
}

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
