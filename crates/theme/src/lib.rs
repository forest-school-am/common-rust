//! The `les_theme` / `les_theme_v` cookie contract, in ONE place (R117).
//!
//! Four apps hand-rolled this parser (cron, les-registry, les-forms,
//! authentik-role-UI) — same intent, four spellings, four build-time theme
//! tables. This crate owns it once: the two charsets, the `name != "default"`
//! rule, and the single override `<link>` the shell drops into its
//! `{{theme_override}}` slot per request.
//!
//! # The contract (R113, R113.1, R117)
//!
//! The stand-wide theme picker (on another host) writes two cookies on the
//! shared `Domain`, so every app reads the same choice:
//! - `les_theme=<name>` — the theme directory name.
//! - `les_theme_v=<12 lowercase hex>` — the common-ui release the theme was
//!   picked from.
//!
//! Both are UNTRUSTED input. [`theme_override_link`] validates each by charset
//! and returns at most one link, or `None`:
//!
//! - valid `les_theme_v` **and** a valid name (not `default`) →
//!   `{origin}/common-ui@{version}/theme-{name}/palette.css` — the picked
//!   release, so a theme published after this build still applies (no table
//!   lookup, no rebuild).
//! - a valid name (not `default`) with **no** valid `les_theme_v` → the app's
//!   one pinned prefix: `{origin}/common-ui@{pinned_prefix}/theme-{name}/palette.css`.
//!   R117 retires the per-app compiled theme table: the link points straight
//!   at the palette asset, built from origin + prefix + name and nothing else.
//! - no valid name, or `name == "default"` → `None` (the shell's own default
//!   palette shows; `default` is already linked by the shell).
//!
//! No `integrity` rides the override: the cookie can only pick among published,
//! immutable files on our own content-addressed Garage over TLS — the same
//! trust R113 gave the versioned link. The security property is the CHARSET:
//! neither value can hold a byte that leaves the `href` attribute or the path
//! segment, so the cookie can only ever name a file, never inject markup or
//! escape the origin.

use common_templating::AssetsOrigin;

/// The name cookie the stand-wide theme picker sets (untrusted).
pub const THEME_COOKIE: &str = "les_theme";
/// The release the theme was picked from (untrusted). A SECOND cookie rather
/// than a suffix on `les_theme`, so readers of the bare name keep working.
pub const THEME_VERSION_COOKIE: &str = "les_theme_v";

/// The theme override `<link>` for this request's cookies, or `None`.
///
/// `pinned_prefix` is the app's compiled common-ui pin as the bare hex that
/// follows `common-ui@` (interchangeable with a valid `les_theme_v` in the
/// same slot) — NOT the whole `common-ui@<hex>` string. `origin` is the
/// resolved, trailing-slash-free asset origin (not the `{{assets_origin}}`
/// marker: this runs as the shell's per-request render value, after which no
/// marker pass remains).
pub fn theme_override_link(
    cookie_header: Option<&str>,
    origin: &AssetsOrigin,
    pinned_prefix: &str,
) -> Option<String> {
    // Validation is the whole defence, so it runs before any formatting. An
    // invalid name, or `default` (already linked by the shell), is no link.
    let name =
        cookie(cookie_header, THEME_COOKIE).filter(|n| is_theme_name(n) && *n != "default")?;
    // A valid version selects the picked release; anything else falls back to
    // the app's one pinned prefix (R117: no compiled table in between).
    let prefix = cookie(cookie_header, THEME_VERSION_COOKIE)
        .filter(|v| is_theme_version(v))
        .unwrap_or(pinned_prefix);
    Some(format!(
        "<link rel=\"stylesheet\" \
         href=\"{origin}/common-ui@{prefix}/theme-{name}/palette.css\" \
         crossorigin=\"anonymous\">",
        origin = origin.as_str(),
    ))
}

/// `^[0-9a-f]{12}$` — a common-ui release hash, lowercase hex, exactly 12.
pub fn is_theme_version(v: &str) -> bool {
    v.len() == 12 && v.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `^[a-z0-9-]{1,40}$` — a theme directory name. No dot, slash, quote,
/// uppercase or angle bracket survives this, which is what keeps the value
/// inside its path segment and the `href` attribute.
pub fn is_theme_name(n: &str) -> bool {
    (1..=40).contains(&n.len())
        && n.bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

/// A cookie's value, if the header carries it. Each `;`-separated pair is split
/// on the FIRST `=` and matched by its EXACT trimmed name, so `other_les_theme`
/// is not `les_theme` and `les_theme_v` is not `les_theme`.
fn cookie<'a>(cookie_header: Option<&'a str>, wanted: &str) -> Option<&'a str> {
    cookie_header?
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == wanted)
        .map(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use common_logging::Deployment;

    const PINNED: &str = "b9f049d05d44";
    const V: &str = "d9cdfe232098";

    fn origin() -> AssetsOrigin {
        AssetsOrigin::parse(Some("https://assets.dev.local"), Deployment::Prod)
            .expect("a valid https origin parses")
            .expect("the value is present")
    }

    fn link(header: &str) -> Option<String> {
        theme_override_link(Some(header), &origin(), PINNED)
    }

    // ---- charset: is_theme_version -----------------------------------------

    #[test]
    fn version_charset_accept_and_reject() {
        assert!(is_theme_version("d9cdfe232098"));
        assert!(is_theme_version("000000000000"));
        assert!(is_theme_version("abcdef012345"));
        for bad in [
            "d9cdfe23209",            // 11: too short
            "d9cdfe2320981",          // 13: too long
            "D9CDFE232098",           // uppercase
            "d9cdfe23209g",           // g is not hex
            "../x",                   // traversal
            "d9cdfe2320\"8",          // quote
            "d9cdfe23209>",           // angle bracket
            "common-ui@d9cdfe232098", // the prefix, not the version
            "",                       // empty
        ] {
            assert!(!is_theme_version(bad), "{bad:?} must be rejected");
        }
    }

    // ---- charset: is_theme_name --------------------------------------------

    #[test]
    fn name_charset_accept_and_reject() {
        assert!(is_theme_name("neumorph"));
        assert!(is_theme_name("neo-brutal"));
        assert!(is_theme_name("aurora-7"));
        assert!(is_theme_name("a"));
        assert!(
            is_theme_name(&"a".repeat(40)),
            "40 is the last valid length"
        );
        for bad in [
            "Neumorph".to_string(),    // uppercase
            "..".to_string(),          // traversal
            "neu/morph".to_string(),   // slash
            "neu\"morph".to_string(),  // quote
            "neu morph".to_string(),   // space
            "neu.morph".to_string(),   // dot
            "\"><script>".to_string(), // XSS attempt
            "default\"><script>alert(1)".to_string(),
            "../../../evil".to_string(),
            "a".repeat(41), // one over the limit
            String::new(),  // empty
        ] {
            assert!(!is_theme_name(&bad), "{bad:?} must be rejected");
        }
    }

    // ---- the three outcomes: versioned / pinned-prefix / none ---------------

    #[test]
    fn a_valid_pair_links_the_picked_release() {
        let want = format!(
            "<link rel=\"stylesheet\" \
             href=\"https://assets.dev.local/common-ui@{V}/theme-aurora-7/palette.css\" \
             crossorigin=\"anonymous\">"
        );
        // Order and neighbouring cookies do not matter.
        for header in [
            format!("{THEME_COOKIE}=aurora-7; {THEME_VERSION_COOKIE}={V}"),
            format!("{THEME_VERSION_COOKIE}={V}; session=x; {THEME_COOKIE}=aurora-7"),
        ] {
            assert_eq!(link(&header).as_deref(), Some(want.as_str()), "{header}");
        }
        assert!(!want.contains("integrity="), "the override carries no SRI");
    }

    #[test]
    fn a_valid_name_with_no_version_links_the_pinned_prefix() {
        // R117: no compiled table between the name and the palette — a valid
        // name links the app's ONE pinned prefix, straight at the asset.
        let want = format!(
            "<link rel=\"stylesheet\" \
             href=\"https://assets.dev.local/common-ui@{PINNED}/theme-neumorph/palette.css\" \
             crossorigin=\"anonymous\">"
        );
        assert_eq!(
            link(&format!("{THEME_COOKIE}=neumorph")).as_deref(),
            Some(want.as_str())
        );
        assert!(
            !want.contains("integrity="),
            "no SRI on the pinned-prefix link either"
        );
    }

    #[test]
    fn an_invalid_version_falls_back_to_the_pinned_prefix() {
        for bad in [
            "d9cdfe23209",            // 11 hex
            "d9cdfe2320981",          // 13 hex
            "D9CDFE232098",           // uppercase
            "../x",                   // traversal
            "d9cdfe2320\"8",          // quote
            "d9cdfe23209>",           // angle bracket
            "common-ui@d9cdfe232098", // the prefix, not the version
            "",                       // empty
        ] {
            let want = format!(
                "<link rel=\"stylesheet\" \
                 href=\"https://assets.dev.local/common-ui@{PINNED}/theme-dusk/palette.css\" \
                 crossorigin=\"anonymous\">"
            );
            assert_eq!(
                link(&format!(
                    "{THEME_COOKIE}=dusk; {THEME_VERSION_COOKIE}={bad}"
                ))
                .as_deref(),
                Some(want.as_str()),
                "{bad:?} did not fall back to the pinned prefix"
            );
        }
    }

    #[test]
    fn no_name_is_no_link() {
        assert_eq!(
            theme_override_link(None, &origin(), PINNED),
            None,
            "no header"
        );
        assert_eq!(link(""), None, "empty header");
        assert_eq!(link("session=abc; other=1"), None, "no theme cookie");
        assert_eq!(
            link(&format!("{THEME_VERSION_COOKIE}={V}")),
            None,
            "version but no name"
        );
    }

    #[test]
    fn default_is_never_linked() {
        assert_eq!(
            link(&format!("{THEME_COOKIE}=default")),
            None,
            "default, no version"
        );
        assert_eq!(
            link(&format!(
                "{THEME_COOKIE}=default; {THEME_VERSION_COOKIE}={V}"
            )),
            None,
            "default, with a valid version"
        );
    }

    // ---- the cookie is found only by its EXACT name -------------------------

    #[test]
    fn a_similarly_named_cookie_is_not_this_one() {
        // A suffix match would let another cookie choose the theme.
        assert_eq!(link(&format!("other_{THEME_COOKIE}=neumorph")), None);
        assert_eq!(link(&format!("{THEME_COOKIE}_extra=neumorph")), None);
        // A prefix match would let another cookie supply the version: with the
        // real version cookie misspelled, the name falls back to the pinned
        // prefix rather than following the bogus `les_theme_vx`.
        let got = link(&format!(
            "{THEME_COOKIE}_vx=deadbeef0000; {THEME_COOKIE}=neumorph"
        ));
        assert!(
            got.as_deref()
                .is_some_and(|h| h.contains(&format!("common-ui@{PINNED}/theme-neumorph"))),
            "a bogus les_theme_vx must not supply the version: {got:?}"
        );
    }

    // ---- the security boundary: nothing hostile is ever reflected -----------

    #[test]
    fn xss_and_traversal_names_are_never_reflected() {
        for hostile in [
            "\"><script>alert(1)</script>",
            "default\"><script>",
            "../../../evil",
            "https://evil.example/palette.css",
            "default'",
            "<!--",
            "neu/morph",
            "neu morph",
            "Neumorph",
            &"a".repeat(41),
        ] {
            // Both with and without a valid version: neither path may reflect.
            assert_eq!(
                link(&format!("{THEME_COOKIE}={hostile}")),
                None,
                "name-only path reflected {hostile:?}"
            );
            assert_eq!(
                link(&format!(
                    "{THEME_COOKIE}={hostile}; {THEME_VERSION_COOKIE}={V}"
                )),
                None,
                "versioned path reflected {hostile:?}"
            );
        }
    }

    #[test]
    fn a_hostile_version_never_reaches_the_href() {
        // A version that fails the charset is simply ignored; the name (valid)
        // then links the pinned prefix, and the hostile bytes never appear.
        for hostile in ["\"><script>", "../../../x", "a/b", "abcdefABCDEF"] {
            let got = link(&format!(
                "{THEME_COOKIE}=dusk; {THEME_VERSION_COOKIE}={hostile}"
            ));
            assert!(
                got.as_deref().is_some_and(|h| !h.contains(hostile)
                    && h.contains(&format!("common-ui@{PINNED}/theme-dusk"))),
                "hostile version {hostile:?} leaked into {got:?}"
            );
        }
    }
}
