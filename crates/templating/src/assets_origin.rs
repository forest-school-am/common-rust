//! The asset origin: the option, the variable it is read from, and the CSP it
//! implies (§12.6). Stamping it into a shell is render.rs; reading or caching
//! an asset from DISK is lib.rs.

use common_logging::{Deployment, Refusal};
use http::header::{HeaderName, HeaderValue, CONTENT_SECURITY_POLICY};
use tower_http::set_header::SetResponseHeaderLayer;

pub const VARIABLE: &str = "ASSETS_ORIGIN";
const ACCEPTED: &str =
    "an https origin and nothing else, such as \"https://assets.dev.local\" — no path, \
     no port, no trailing slash, no credentials";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetsOrigin(String);

impl AssetsOrigin {
    pub fn parse(value: Option<&str>, deployment: Deployment) -> Result<Option<Self>, Refusal> {
        let Some(text) = value.filter(|t| !t.is_empty()) else {
            return match deployment {
                Deployment::Prod => Err(Refusal::new(VARIABLE, "", ACCEPTED)
                    .with_detail("prod-required: unset is allowed only under DEPLOYMENT_TYPE=dev")),
                Deployment::Dev => Ok(None),
            };
        };
        let refuse = |detail: &str| {
            Err(Refusal::new(VARIABLE, text, ACCEPTED).with_detail(detail.to_owned()))
        };
        let Some(host) = text.strip_prefix("https://") else {
            return refuse("must begin with https://");
        };
        if host.is_empty() {
            return refuse("no host after https://");
        }
        if host.ends_with('/') || host.contains('/') {
            return refuse("a path or trailing slash is present");
        }
        if host.contains('@') {
            return refuse("credentials are present");
        }
        if host.contains(':') {
            return refuse("a port is present");
        }
        if host.contains('?') || host.contains('#') {
            return refuse("a query or fragment is present");
        }
        if !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            return refuse("the host has a character that is not a letter, digit, dot or hyphen");
        }
        Ok(Some(Self(text.to_owned())))
    }

    pub fn from_env(deployment: Deployment) -> Result<Option<Self>, Refusal> {
        Self::parse(std::env::var(VARIABLE).ok().as_deref(), deployment)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `data:` is in `img-src` because the shell ships an inline SVG favicon,
    /// which is not `'self'` and fails as an `EncodingError` on decode with no
    /// CSP report of any kind — every byte-level test stays green and the icon
    /// is simply absent.
    pub fn csp(&self) -> String {
        format!(
            "default-src 'self'; script-src 'self' {origin}; style-src 'self' {origin}; \
             img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'self'; \
             form-action 'self'; frame-ancestors 'self'",
            origin = self.0
        )
    }

    pub fn csp_layer(&self) -> SetResponseHeaderLayer<HeaderValue> {
        SetResponseHeaderLayer::overriding(
            HeaderName::from(CONTENT_SECURITY_POLICY),
            HeaderValue::from_str(&self.csp()).expect("an accepted origin makes a header value"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_https_origin_is_accepted_and_kept_verbatim() {
        for good in [
            "https://assets.dev.local",
            "https://assets.dev.redaether",
            "https://a",
            "https://assets-1.example.co.uk",
        ] {
            let origin = AssetsOrigin::parse(Some(good), Deployment::Prod)
                .unwrap_or_else(|r| panic!("{good} must be accepted: {r}"))
                .expect("present");
            assert_eq!(origin.as_str(), good);
        }
    }

    #[test]
    fn anything_that_is_not_a_bare_https_origin_is_refused() {
        for (bad, because) in [
            ("http://assets.dev.local", "must begin with https://"),
            ("assets.dev.local", "must begin with https://"),
            ("https://", "no host after https://"),
            (
                "https://assets.dev.local/",
                "a path or trailing slash is present",
            ),
            (
                "https://assets.dev.local/common-ui",
                "a path or trailing slash is present",
            ),
            ("https://assets.dev.local:8021", "a port is present"),
            ("https://assets.dev.local:8443", "a port is present"),
            (
                "https://assets.dev.local/?v=1",
                "a path or trailing slash is present",
            ),
            (
                "https://user:pw@assets.dev.local",
                "credentials are present",
            ),
            (
                "https://assets.dev.local?x=1",
                "a query or fragment is present",
            ),
            (
                "https://assets.dev.local#f",
                "a query or fragment is present",
            ),
            (
                "https://assets dev local",
                "the host has a character that is not a letter, digit, dot or hyphen",
            ),
        ] {
            let refusal = AssetsOrigin::parse(Some(bad), Deployment::Dev)
                .expect_err(&format!("{bad} must be refused"));
            assert_eq!(refusal.variable, VARIABLE);
            assert_eq!(refusal.value, bad);
            assert!(refusal.accepted.contains("https://assets.dev.local"));
            assert_eq!(
                refusal.detail.as_deref(),
                Some(because),
                "the detail must say WHICH rule {bad} broke"
            );
        }
    }

    #[test]
    fn unset_is_prod_required_and_dev_optional() {
        assert_eq!(AssetsOrigin::parse(None, Deployment::Dev).unwrap(), None);
        assert_eq!(
            AssetsOrigin::parse(Some(""), Deployment::Dev).unwrap(),
            None
        );

        for unset in [None, Some("")] {
            let refusal = AssetsOrigin::parse(unset, Deployment::Prod)
                .expect_err("unset under prod must refuse");
            assert_eq!(refusal.variable, VARIABLE);
            assert!(
                refusal
                    .detail
                    .as_deref()
                    .is_some_and(|d| d.contains("prod-required")),
                "{refusal:?}"
            );
        }
    }

    #[test]
    fn the_csp_is_the_canon_value_with_the_origin_in_both_source_lists() {
        let origin = AssetsOrigin::parse(Some("https://assets.dev.local"), Deployment::Prod)
            .unwrap()
            .unwrap();
        assert_eq!(
            origin.csp(),
            "default-src 'self'; script-src 'self' https://assets.dev.local; \
             style-src 'self' https://assets.dev.local; img-src 'self' data:; \
             connect-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; \
             frame-ancestors 'self'"
        );
        assert!(
            !origin.csp().contains("unsafe"),
            "no unsafe-* directive may ever appear: {}",
            origin.csp()
        );
    }

    /// The shell's favicon is a `data:` URI, and `img-src 'self'` refused it
    /// with no CSP report and no failed request — `Image.decode()` rejects
    /// with an `EncodingError`, which reads as a malformed SVG rather than as
    /// a policy decision, and the icon is just missing. Driven by the URI
    /// SHAPE the shell composes, so this fails if `data:` is dropped again.
    #[test]
    fn the_csp_admits_the_inline_favicon_the_shell_ships() {
        let origin = AssetsOrigin::parse(Some("https://assets.dev.local"), Deployment::Prod)
            .unwrap()
            .unwrap();
        let csp = origin.csp();
        let favicon = "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg'/>";

        let img_src = csp
            .split("; ")
            .find(|directive| directive.starts_with("img-src "))
            .expect("there must be an img-src to reason about");
        let sources: Vec<&str> = img_src["img-src ".len()..].split(' ').collect();

        let scheme = favicon
            .split_once(':')
            .map(|(scheme, _)| format!("{scheme}:"))
            .expect("a data URI has a scheme");
        assert!(
            sources.contains(&scheme.as_str()),
            "the shell's favicon is a {scheme} URI, which no origin in \
             {sources:?} can match — it fails as an EncodingError with no CSP \
             report, so nothing but this notices: {csp}"
        );
        assert!(
            !sources.contains(&"*"),
            "admitting the favicon must not mean admitting everything: {csp}"
        );
    }

    #[test]
    fn the_csp_permits_same_origin_framing() {
        let origin = AssetsOrigin::parse(Some("https://assets.dev.local"), Deployment::Prod)
            .unwrap()
            .unwrap();
        let csp = origin.csp();
        assert!(
            csp.contains("frame-ancestors 'self'"),
            "a page must be able to frame its own origin — les-forms' editor frames \
             /render?preview=1 and cron's 360px harness measures inside a same-origin \
             iframe, and BOTH go blank under 'none': {csp}"
        );
        assert!(
            !csp.contains("frame-ancestors 'none'"),
            "'none' refuses same-origin framing as well as cross-origin: {csp}"
        );
    }

    #[test]
    fn the_exported_name_is_the_variable_an_operator_is_told_to_set() {
        assert_eq!(
            VARIABLE, "ASSETS_ORIGIN",
            "spelled out rather than read off the const: an operator sets this \
             exact string, and a consumer's flag override names it"
        );

        let refusal =
            AssetsOrigin::parse(Some("assets.dev.local"), Deployment::Dev).expect_err("refused");
        assert_eq!(
            refusal.variable, VARIABLE,
            "the refusal must name the variable an operator can actually set"
        );
    }
}
