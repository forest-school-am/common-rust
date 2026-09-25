//! Build-time glue a common-ui consumer's `build.rs` calls: [`stamp`] fills the
//! shell's `[[build markers]]`, [`shared_markers`] returns the marker values that
//! come straight from the crates, [`write_dts`] emits the d.ts, [`csp`] returns the
//! policy. The assets themselves the consumer serves from this crate's consts and
//! [`common_theme`] — no Garage fetch, no manifest, no prefix pin.
//!
//! This lived in `common-ui-build` (ui-build.1, user 2026-09-25): the reason for
//! a second crate was that ui-core is bytes with no deps, and that is kept by the
//! `build` feature — nothing here is compiled into a runtime binary.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const TYPES: &str = "common-ui.d.ts";

/// The Content-Security-Policy the shell's pages require, `{{assets_origin}}`
/// still in it (a consumer resolves that when it sets the header).
pub fn csp() -> &'static str {
    crate::CSP
}

pub fn shared_markers() -> Vec<(&'static str, String)> {
    vec![
        ("prefix", format!("common-ui {}", crate::VERSION)),
        ("sri_base_css", crate::SRI_BASE_CSS.to_owned()),
        (
            "sri_palette_css",
            common_theme::SRI_DEFAULT_PALETTE.to_owned(),
        ),
        ("sri_elements_css", crate::SRI_ELEMENTS_CSS.to_owned()),
        ("sri_common_ui_js", crate::SRI_COMMON_UI_JS.to_owned()),
    ]
}

pub fn write_dts(out_dir: &Path) -> std::io::Result<PathBuf> {
    let path = out_dir.join(TYPES);
    std::fs::write(&path, crate::COMMON_UI_DTS)?;
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
