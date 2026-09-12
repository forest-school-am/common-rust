//! The asset origin: the option, the CSP it implies, and the template
//! parameter pages substitute it into (§12.6). Anything about reading or
//! caching an asset from DISK belongs in lib.rs.

use common_logging::{Deployment, Refusal};
use http::header::{HeaderName, HeaderValue, CONTENT_SECURITY_POLICY};
use tower_http::set_header::SetResponseHeaderLayer;

const VARIABLE: &str = "ASSETS_ORIGIN";
const ACCEPTED: &str =
    "an https origin and nothing else, such as \"https://assets.dev.local\" — no path, \
     no port, no trailing slash, no credentials";

pub const PARAM: &str = "assets_origin";

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

    pub fn param(&self) -> (&'static str, &str) {
        (PARAM, &self.0)
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
    fn the_template_parameter_is_the_one_pages_write() {
        let origin = AssetsOrigin::parse(Some("https://assets.dev.local"), Deployment::Prod)
            .unwrap()
            .unwrap();
        assert_eq!(
            origin.param(),
            ("assets_origin", "https://assets.dev.local")
        );
    }
}
