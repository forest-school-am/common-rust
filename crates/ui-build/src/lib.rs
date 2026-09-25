//! DEPRECATED, and about to be deleted (ui-build.1, user 2026-09-25): the glue
//! moved into `common_ui_core::build`, behind that crate's `build` feature. This
//! crate is now the re-export that keeps every consumer's `build.rs` compiling
//! until it switches the import; the tests below still exercise the moved code
//! through it.

pub use common_ui_core::build::{csp, shared_markers, stamp, write_dts, TYPES};

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
