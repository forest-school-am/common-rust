//! Stamping a built shell with the two values only the request knows: the
//! asset origin and the config block. Build-time values (title, page css,
//! page module, prefix, hashes) are a service's build.rs, not this.

use crate::Config;

pub const ORIGIN_MARKER: &str = "{{assets_origin}}";
pub const CONFIG_MARKER: &str = "{{config}}";

/// Serialised JSON with `<`, `>` and `&` as `\u00XX`, so no value can close
/// the block it sits in or open a tag inside it. JSON has no syntax use for
/// any of the three, so every occurrence is inside a string literal.
fn data_block(config: &Config) -> String {
    serde_json::to_string(config)
        .expect("Config is strings only, which serde_json cannot fail on")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

pub fn render(shell: &str, config: &Config) -> String {
    shell
        .replace(ORIGIN_MARKER, &config.assets_origin)
        .replace(CONFIG_MARKER, &data_block(config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use common_logging::Deployment;

    fn config(login_path: &str) -> Config {
        let origin = AssetsOrigin::parse(Some("https://assets.dev.local"), Deployment::Prod)
            .unwrap()
            .unwrap();
        Config::new(&origin, login_path)
    }

    use crate::AssetsOrigin;

    const SHELL: &str = concat!(
        "<link rel=stylesheet href=\"{{assets_origin}}/common-ui@abc/base.css\">\n",
        "<script type=\"module\" src=\"{{assets_origin}}/common-ui@abc/common-ui.js\"></script>\n",
        "<script type=\"application/json\" id=\"config\">{{config}}</script>\n",
    );

    #[test]
    fn both_runtime_markers_are_filled_everywhere_they_appear() {
        let html = render(SHELL, &config("/oidc/login"));
        assert_eq!(
            html.matches("https://assets.dev.local/common-ui@abc/")
                .count(),
            2,
            "every occurrence of the origin marker is filled, not just the first: {html}"
        );
        assert!(!html.contains("{{"), "no marker may survive: {html}");
        assert!(
            html.contains(r#"<script type="application/json" id="config">{"assetsOrigin":"#),
            "the config lands inside the data block: {html}"
        );
    }

    #[test]
    fn a_config_value_cannot_close_the_block_it_sits_in() {
        let hostile = "</script><script>alert(1)</script>";
        let html = render(SHELL, &config(hostile));

        assert!(
            !html.contains("</script><script>"),
            "a value closed the data block: {html}"
        );
        assert_eq!(
            html.matches("</script>").count(),
            2,
            "only the shell's own two closing tags may appear: {html}"
        );
        assert!(
            html.contains("\\u003c/script\\u003e"),
            "the angle brackets must be escaped as \\u00XX: {html}"
        );
    }

    #[test]
    fn an_ampersand_is_escaped_so_an_entity_cannot_form() {
        let html = render(SHELL, &config("/login?a=1&amp;lt;b"));
        assert!(
            !html.contains('&'),
            "no raw ampersand may reach the page: {html}"
        );
        assert!(html.contains("\\u0026"), "{html}");
    }

    #[test]
    fn the_escaped_block_is_still_the_json_the_page_parses() {
        let hostile = "</script>&<>";
        let html = render(SHELL, &config(hostile));
        let start = html.find(r#"id="config">"#).unwrap() + r#"id="config">"#.len();
        let end = html[start..].find("</script>").unwrap() + start;

        let parsed: serde_json::Value = serde_json::from_str(&html[start..end])
            .expect("escaping must leave valid JSON, or the page cannot read it");
        assert_eq!(
            parsed["loginPath"], hostile,
            "the page must get the value back unchanged: {parsed:?}"
        );
    }

    /// A display name arrives from authentik, so it is the likeliest hostile
    /// string in the whole block — likelier than a login path, which a service
    /// writes itself.
    #[test]
    fn a_hostile_display_name_cannot_close_the_data_block() {
        let origin = AssetsOrigin::parse(Some("https://assets.dev.local"), Deployment::Prod)
            .unwrap()
            .unwrap();
        let mut config = Config::new(&origin, "/oidc/login");
        config.user = Some(crate::User {
            name: "</script><script>alert(1)</script>".to_owned(),
            portrait: None,
        });
        let html = render(SHELL, &config);

        assert!(
            !html.contains("</script><script>"),
            "a display name closed the data block: {html}"
        );
        assert_eq!(
            html.matches("</script>").count(),
            2,
            "only the shell's own two closing tags may appear: {html}"
        );

        let start = html.find(r#"id="config">"#).unwrap() + r#"id="config">"#.len();
        let end = html[start..].find("</script>").unwrap() + start;
        let parsed: serde_json::Value =
            serde_json::from_str(&html[start..end]).expect("escaping must leave valid JSON");
        assert_eq!(
            parsed["user"]["name"], "</script><script>alert(1)</script>",
            "the page must still get the name back unchanged: {parsed:?}"
        );
    }

    #[test]
    fn a_build_time_marker_render_does_not_own_survives_visibly() {
        let html = render("<title>{{title}}</title>{{config}}", &config("/oidc/login"));
        assert!(
            html.contains("{{title}}"),
            "an unfilled build-time marker stays visible rather than being swallowed: {html}"
        );
    }
}
