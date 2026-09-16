//! Stamping a built shell with the three values only the request knows: the
//! asset origin, the config block and the theme-override link. Build-time
//! values (title, page css, page module, prefix, hashes) are a service's
//! build.rs, not this; WHAT the config block says is the service's (and
//! common-oidc's) business, not this crate's — it takes any `Serialize` and
//! only escapes it for its block. The theme link is the service's business
//! too: it is built from pattern-validated cookie values and passed RAW,
//! exactly like the asset origin.
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

use crate::{AssetsOrigin, RenderError};

pub const ORIGIN_MARKER: &str = "{{assets_origin}}";
pub const CONFIG_MARKER: &str = "{{config}}";
pub const THEME_MARKER: &str = "{{theme_override}}";

/// The name the shell has in error messages; there is only ever one.
const NAME: &str = "shell";

/// Serialised JSON with `<`, `>` and `&` as `\u00XX`, so no value can close
/// the `<script type="application/json">` it sits in or open a tag inside
/// it. JSON has no syntax use for any of the three, so every occurrence is
/// inside a string literal and the page parses the value back unchanged.
///
/// This is NOT HTML sanitisation and an HTML escaper would be wrong here:
/// browsers do not decode entities inside `<script>`, so `&lt;` would reach
/// `JSON.parse` as a literal `&lt;` and break the block.
fn json_for_script_block(config: &impl Serialize) -> Result<String, RenderError> {
    let json = serde_json::to_string(config)
        .map_err(|e| RenderError::Render(NAME.to_owned(), format!("config: {e}")))?;
    Ok(json
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026"))
}

/// Exactly the three values a shell may ask for at runtime: the asset origin,
/// the config block and the theme-override link. All three are declared in the
/// shell contract (`shell-markers.json`'s `runtime` array), so adding one here
/// followed the contract change rather than driving it. `assets_origin` and
/// `theme_override` are passed RAW; only `config` is escaped, for its block.
#[derive(Serialize)]
struct Values<'a> {
    assets_origin: &'a str,
    config: &'a str,
    theme_override: &'a str,
}

/// A compiled shell: the build-stamped document with only [`ORIGIN_MARKER`],
/// [`CONFIG_MARKER`] and [`THEME_MARKER`] left in it. Compile once at boot,
/// render per request.
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

    /// Renders with `assets_origin` and `theme_override` raw and `config`
    /// serialised and escaped for its block. `theme_override` is a `<link>`
    /// the service built from pattern-validated cookie values (R113), so it is
    /// emitted verbatim — HTML-escaping it would break the tag; an empty slot
    /// is the common case. Any other `{{name}}` still in the shell is a render
    /// error whose message quotes the offending line.
    pub fn render(
        &self,
        origin: &AssetsOrigin,
        config: &impl Serialize,
        theme_override: &str,
    ) -> Result<String, RenderError> {
        let block = json_for_script_block(config)?;
        let values = Values {
            assets_origin: origin.as_str(),
            config: &block,
            theme_override,
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
/// holds a `{{name}}` other than the three runtime markers — both are defects
/// in the build-time stamping, not conditions a request can produce. A
/// service that wants to refuse at boot instead compiles a [`Shell`] there.
pub fn render(
    shell: &str,
    origin: &AssetsOrigin,
    config: &impl Serialize,
    theme_override: &str,
) -> String {
    Shell::compile(shell)
        .and_then(|shell| shell.render(origin, config, theme_override))
        .unwrap_or_else(|e| panic!("{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use common_logging::Deployment;

    /// A config block as a service might shape one. The crate does not know
    /// or care what is in it; the tests need something with a string a hostile
    /// value can land in.
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Page {
        assets_origin: &'static str,
        login_path: String,
        user: Option<Who>,
    }

    #[derive(Serialize)]
    struct Who {
        name: String,
    }

    fn origin() -> AssetsOrigin {
        AssetsOrigin::parse(Some("https://assets.dev.local"), Deployment::Prod)
            .unwrap()
            .unwrap()
    }

    fn config(login_path: &str) -> Page {
        Page {
            assets_origin: "https://assets.dev.local",
            login_path: login_path.to_owned(),
            user: None,
        }
    }

    const SHELL: &str = concat!(
        "<link rel=stylesheet href=\"{{assets_origin}}/common-ui@abc/base.css\">\n",
        "{{theme_override}}\n",
        "<script type=\"module\" src=\"{{assets_origin}}/common-ui@abc/common-ui.js\"></script>\n",
        "<script type=\"application/json\" id=\"config\">{{config}}</script>\n",
    );

    #[test]
    fn every_runtime_marker_is_filled_everywhere_it_appears() {
        let html = render(SHELL, &origin(), &config("/oidc/login"), "");
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

    /// The theme slot is the third runtime marker (the shell contract lists it
    /// in `shell-markers.json`'s `runtime` array). An empty string is the
    /// common case — no cookie, so the default palette stands — and the marker
    /// must still be consumed, never shipped to a browser.
    #[test]
    fn an_empty_theme_override_fills_the_slot_rather_than_leaving_it() {
        let html = render(SHELL, &origin(), &config("/oidc/login"), "");
        assert!(
            !html.contains("{{theme_override}}"),
            "the theme marker must be filled even when empty: {html}"
        );
    }

    /// The theme link is a `<link>` the service built from pattern-validated
    /// cookie values, so it is passed RAW — an HTML escaper here would turn its
    /// quotes and angle brackets into entities and break the tag. The value
    /// reaches the page byte-for-byte.
    #[test]
    fn a_raw_theme_link_passes_through_unescaped() {
        let link = r#"<link rel="stylesheet" href="https://assets.dev.local/common-ui@abc/theme-dusk/palette.css" crossorigin="anonymous">"#;
        let html = render(SHELL, &origin(), &config("/oidc/login"), link);
        assert!(
            html.contains(link),
            "the theme link must reach the page verbatim, not entity-escaped: {html}"
        );
        assert!(
            !html.contains("&lt;link") && !html.contains("&quot;"),
            "no character of the link may be HTML-escaped: {html}"
        );
    }

    #[test]
    fn a_config_value_cannot_close_the_block_it_sits_in() {
        let hostile = "</script><script>alert(1)</script>";
        let html = render(SHELL, &origin(), &config(hostile), "");

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
        let html = render(SHELL, &origin(), &config("/login?a=1&amp;lt;b"), "");
        assert!(
            !html.contains('&'),
            "no raw ampersand may reach the page: {html}"
        );
        assert!(html.contains("\\u0026"), "{html}");
    }

    #[test]
    fn the_escaped_block_is_still_the_json_the_page_parses() {
        let hostile = "</script>&<>";
        let html = render(SHELL, &origin(), &config(hostile), "");
        let start = html.find(r#"id="config">"#).unwrap() + r#"id="config">"#.len();
        let end = html[start..].find("</script>").unwrap() + start;

        let parsed: serde_json::Value = serde_json::from_str(&html[start..end])
            .expect("escaping must leave valid JSON, or the page cannot read it");
        assert_eq!(
            parsed["loginPath"], hostile,
            "the page must get the value back unchanged: {parsed:?}"
        );
    }

    /// A display name arrives from the IdP, so it is the likeliest hostile
    /// string in the whole block — likelier than a login path, which a service
    /// writes itself.
    #[test]
    fn a_hostile_display_name_cannot_close_the_data_block() {
        let mut config = config("/oidc/login");
        config.user = Some(Who {
            name: "</script><script>alert(1)</script>".to_owned(),
        });
        let html = render(SHELL, &origin(), &config, "");

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
        let shell =
            Shell::compile("<title>{{title}}</title>{{config}}").expect("the grammar is fine");
        let err = shell
            .render(&origin(), &config("/oidc/login"), "")
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

    /// The crate takes any `Serialize`; one that cannot serialise (a map with
    /// non-string keys, say) is a render error, not a panic in a handler.
    #[test]
    fn a_config_that_cannot_serialise_is_a_render_error() {
        let shell = Shell::compile(SHELL).unwrap();
        let unserialisable: std::collections::BTreeMap<(u8, u8), u8> =
            [((1, 2), 3)].into_iter().collect();
        let err = shell
            .render(&origin(), &unserialisable, "")
            .expect_err("serde_json refuses a non-string map key");
        assert!(matches!(err, RenderError::Render(..)), "{err}");
    }
}
