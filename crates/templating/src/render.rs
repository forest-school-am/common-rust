//! Stamping a built shell with the two values only the request knows: the
//! asset origin and the config block. Build-time values (title, page css,
//! page module, prefix, hashes) are a service's build.rs, not this.
//!
//! The engine is `upon` with no functions, no filters and no escaping: a
//! `{{name}}` expression is the whole grammar a shell uses. Two rules of its
//! matter here and are pinned by tests below:
//!
//! - a `{{name}}` whose value is not supplied is a RENDER error naming the
//!   expression, never a marker quietly shipped to a browser;
//! - a `}}` outside an expression is a COMPILE error, so a build-time value
//!   that happens to contain one is caught when the shell is compiled, not
//!   when a page is served.

use serde::Serialize;

use crate::{Config, RenderError};

pub const ORIGIN_MARKER: &str = "{{assets_origin}}";
pub const CONFIG_MARKER: &str = "{{config}}";

/// The name the shell has in error messages; there is only ever one.
const NAME: &str = "shell";

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

/// Exactly the two values a shell may ask for at runtime. A third field here
/// would be a third runtime marker, which is a shell-contract change first.
#[derive(Serialize)]
struct Values<'a> {
    assets_origin: &'a str,
    config: &'a str,
}

/// A compiled shell: the build-stamped document with only [`ORIGIN_MARKER`]
/// and [`CONFIG_MARKER`] left in it. Compile once at boot, render per request.
pub struct Shell {
    engine: upon::Engine<'static>,
    template: upon::Template<'static>,
}

impl Shell {
    /// Compiles the shell. Fails on anything `upon` cannot parse — including
    /// a lone `}}` — so a stamped shell that breaks the grammar is refused
    /// here rather than at the first request.
    pub fn compile(shell: &str) -> Result<Self, RenderError> {
        let engine = upon::Engine::new();
        let template = engine
            .compile(shell.to_owned())
            .map_err(|e| RenderError::Parse(NAME.to_owned(), format!("{e:#}")))?;
        Ok(Self { engine, template })
    }

    /// Renders with `assets_origin` raw and `config` as the escaped JSON
    /// block. Any other `{{name}}` still in the shell is a render error
    /// whose message quotes the offending line.
    pub fn render(&self, config: &Config) -> Result<String, RenderError> {
        let block = data_block(config);
        let values = Values {
            assets_origin: &config.assets_origin,
            config: &block,
        };
        self.template
            .render(&self.engine, values)
            .to_string()
            .map_err(|e| RenderError::Render(NAME.to_owned(), format!("{e:#}")))
    }
}

/// Compiles and renders in one call, for a shell already validated by the
/// build that stamped it.
///
/// Total by design. It panics only if the shell fails to compile or still
/// holds a `{{name}}` other than the two runtime markers — both are defects
/// in the build-time stamping, not conditions a request can produce. A
/// service that wants to refuse at boot instead compiles a [`Shell`] there.
pub fn render(shell: &str, config: &Config) -> String {
    Shell::compile(shell)
        .and_then(|shell| shell.render(config))
        .unwrap_or_else(|e| panic!("{e}"))
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
    fn an_unstamped_build_time_marker_is_a_render_error_naming_it() {
        let shell = Shell::compile("<title>{{title}}</title>{{config}}").expect("the grammar is fine");
        let err = shell
            .render(&config("/oidc/login"))
            .expect_err("a marker the build did not stamp must not reach a browser");
        assert!(
            matches!(err, RenderError::Render(..)),
            "a missing value is a render error, not a parse error: {err}"
        );
        let text = err.to_string();
        assert!(
            text.contains("title"),
            "the error must name the marker so the build.rs defect is findable: {text}"
        );
    }

    /// upon's rule, and the reason a stamped shell should be compiled at boot:
    /// a build-time value containing `}}` breaks the shell, and this is where
    /// it shows.
    #[test]
    fn a_lone_close_brace_fails_to_compile() {
        let err = Shell::compile("body { a { b } } }} {{config}}")
            .err()
            .expect("a `}}` outside an expression must not compile");
        assert!(matches!(err, RenderError::Parse(..)), "{err}");
    }
}
