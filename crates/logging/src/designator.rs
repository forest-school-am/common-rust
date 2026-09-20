//! The designator vocabulary and the field it travels in. A new designator
//! goes here; EMITTING through one is macros.rs; a new way of RENDERING one
//! goes in format.rs; deciding which ones reach the output goes in filter.rs.

use std::convert::Infallible;
use std::str::FromStr;

use strum::{AsRefStr, Display, VariantNames};

/// The emission macros spell this name as a LITERAL: tracing resolves field
/// names at expansion time, so a const cannot be substituted there.
pub const FIELD: &str = "designator";

pub const CUSTOM_PREFIX: &str = "c-";

/// What an event is ABOUT (§8.3). The six unit variants are the stand
/// vocabulary; `Custom` is a project's own.
///
/// `Display` writes the string the log column and `LOG_DESIGNATORS` use —
/// the lowercase name for the stand six, `c-<name>` for a custom one — and
/// `FromStr` reads it back (anything that is not a stand name parses as
/// `Custom`, so filter.rs still checks the prefix). `FromStr` is written by
/// hand, not derived: strum's `EnumString` also emits `TryFrom<&str>`, which
/// core's blanket `TryFrom` makes collide with the `From<impl Into<String>>`
/// below. `Custom` therefore holds
/// the whole column string, prefix included; build one through `From`, which
/// adds the prefix: `Designator::from("scheduler")` displays `c-scheduler`.
/// (`AsRef<str>` is the variant name, `custom` for every custom one — use
/// `Display` for the column.)
#[derive(Debug, Clone, PartialEq, Eq, AsRefStr, Display, VariantNames)]
pub enum Designator {
    #[strum(serialize = "auth")]
    Auth,
    #[strum(serialize = "business")]
    Business,
    #[strum(serialize = "upstream")]
    Upstream,
    #[strum(serialize = "storage")]
    Storage,
    #[strum(serialize = "http")]
    Http,
    #[strum(serialize = "startup")]
    Startup,
    /// §8.3, and nothing here enforces either half: a custom designator MUST
    /// be listed and explained in the owning repo's README.md, and one
    /// proposed by an agent MUST be confirmed by the operator before it lands.
    #[strum(default, serialize = "custom")]
    Custom(String),
}

/// The custom path: any string-ish name becomes `Custom("c-<name>")`. A strum
/// tag enum joins in through `impl From<Tag> for String` (three lines in the
/// owning repo), which `Into<String>` then covers.
impl<T: Into<String>> From<T> for Designator {
    fn from(name: T) -> Self {
        let name = name.into();
        let mut column = String::with_capacity(CUSTOM_PREFIX.len() + name.len());
        column.push_str(CUSTOM_PREFIX);
        column.push_str(&name);
        Designator::Custom(column)
    }
}

impl FromStr for Designator {
    type Err = Infallible;

    fn from_str(column: &str) -> Result<Self, Infallible> {
        Ok(STAND
            .iter()
            .find(|d| d.as_ref() == column)
            .cloned()
            .unwrap_or_else(|| Designator::Custom(column.to_owned())))
    }
}

impl Designator {
    /// A stand designator, or a custom one that carries the prefix. `FromStr`
    /// accepts any string; this is the check the `LOG_DESIGNATORS` parser
    /// applies on top.
    pub fn is_well_formed(&self) -> bool {
        match self {
            Designator::Custom(column) => column.starts_with(CUSTOM_PREFIX),
            _ => true,
        }
    }
}

pub const AUTH: Designator = Designator::Auth;
pub const BUSINESS: Designator = Designator::Business;
pub const UPSTREAM: Designator = Designator::Upstream;
pub const STORAGE: Designator = Designator::Storage;
pub const HTTP: Designator = Designator::Http;
pub const STARTUP: Designator = Designator::Startup;

pub const STAND: [Designator; 6] = [AUTH, BUSINESS, UPSTREAM, STORAGE, HTTP, STARTUP];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stand_variants_display_their_column_string_and_parse_back() {
        for (d, column) in [
            (AUTH, "auth"),
            (BUSINESS, "business"),
            (UPSTREAM, "upstream"),
            (STORAGE, "storage"),
            (HTTP, "http"),
            (STARTUP, "startup"),
        ] {
            assert_eq!(d.to_string(), column);
            assert_eq!(d.as_ref(), column);
            assert_eq!(Designator::from_str(column), Ok(d.clone()));
            assert!(d.is_well_formed());
        }
        assert_eq!(STAND.len(), 6);
    }

    #[test]
    fn custom_prefixes_c_and_round_trips_through_the_column_string() {
        let d = Designator::from("scheduler");
        assert_eq!(d, Designator::Custom("c-scheduler".into()));
        assert_eq!(d.to_string(), "c-scheduler");
        assert_eq!(Designator::from_str("c-scheduler"), Ok(d.clone()));
        assert!(d.is_well_formed());
        assert_eq!(Designator::from(String::from("logs")).to_string(), "c-logs");
    }

    #[test]
    fn an_unprefixed_stranger_parses_as_custom_but_is_not_well_formed() {
        let d = Designator::from_str("scheduler").unwrap();
        assert_eq!(d, Designator::Custom("scheduler".into()));
        assert!(!d.is_well_formed(), "the filter must refuse it");
        assert!(!Designator::from_str("AUTH").unwrap().is_well_formed());
    }
}
