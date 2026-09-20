//! The theme palettes and the browser LOADER, carried as committed consts and
//! served by each app at `/assets/theme-*`. The loader (JS) owns the `les_theme`
//! cookie contract and is the security boundary — nothing in this crate reflects
//! a cookie into a URL.

pub const THEME_COOKIE: &str = "les_theme";

pub const LOADER_JS: &str = include_str!("assets/theme-loader.js");

pub const DEFAULT_PALETTE: &str = include_str!("assets/theme-default/palette.css");

pub const SRI_DEFAULT_PALETTE: &str = include_str!("assets/theme-default/palette.css.sri");

// `PALETTES` is generated into `palettes.rs` by build.sh; do not hand-edit.
include!("palettes.rs");

pub fn palette(name: &str) -> Option<&'static str> {
    PALETTES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, css)| *css)
}

/// Reference definition of the hash charset the loader (JS) enforces — this
/// Rust copy is not the enforcement point.
pub fn is_theme_version(v: &str) -> bool {
    v.len() == 12 && v.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Reference definition of the name charset the loader (JS) enforces — this
/// Rust copy is not the enforcement point.
pub fn is_theme_name(n: &str) -> bool {
    (1..=40).contains(&n.len())
        && n.bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_bundled_and_matches_the_const() {
        assert_eq!(palette("default"), Some(DEFAULT_PALETTE));
        assert!(!PALETTES.is_empty());
        assert!(PALETTES.iter().any(|(n, _)| *n == "default"));
        let mut sorted = PALETTES.to_vec();
        sorted.sort();
        assert_eq!(PALETTES, sorted.as_slice(), "PALETTES must be sorted");
    }

    #[test]
    fn every_bundled_name_passes_the_name_charset() {
        for (name, css) in PALETTES {
            assert!(is_theme_name(name), "bundled name {name:?} is not valid");
            assert!(!css.is_empty(), "bundled palette {name:?} is empty");
        }
    }

    #[test]
    fn loader_and_default_sri_are_present() {
        assert!(
            !LOADER_JS.is_empty(),
            "loader is empty — did build.sh emit it?"
        );
        assert!(
            LOADER_JS.contains("les_theme"),
            "loader must read the cookie"
        );
        assert!(
            SRI_DEFAULT_PALETTE.starts_with("sha384-"),
            "default palette SRI is not sha384-…: {SRI_DEFAULT_PALETTE:?}"
        );
        assert_eq!(SRI_DEFAULT_PALETTE.trim(), SRI_DEFAULT_PALETTE);
    }

    #[test]
    fn charset_rules_reject_hostile_values() {
        for bad in ["..", "a/b", "A", "\"><script>", "", &"a".repeat(41)] {
            assert!(!is_theme_name(bad), "{bad:?} must be rejected");
        }
        for bad in ["deadbeef", "DEADBEEF0000", "g00000000000", "../x", ""] {
            assert!(!is_theme_version(bad), "{bad:?} must be rejected");
        }
        assert!(is_theme_name("neo-brutal"));
        assert!(is_theme_version("0123456789ab"));
    }
}
