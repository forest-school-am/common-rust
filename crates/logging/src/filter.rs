//! Which designators reach the output, and at what level. Selection only —
//! what a designator MEANS belongs in designator.rs, how a line LOOKS in
//! format.rs.
//!
//! This is the second, independent filtering axis (R28). `RUST_LOG` filters by
//! module path, as standard tracing does; this filters by designator. Neither
//! can express the other and neither overrides the other — an event must pass
//! both.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use tracing::field::{Field, Visit};
use tracing::{Event, Metadata, Subscriber};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::{Context, Filter};

use crate::designator::{CUSTOM_PREFIX, FIELD, STAND};

/// Parsed `LOG_DESIGNATORS`.
///
/// UNSET MEANS EVERYTHING PASSES. Two filters ANDed together will silently
/// resolve to nothing if either defaults to "deny", so the default here has to
/// be permissive or configuring only `RUST_LOG` would go silent — which is the
/// defect R28 exists to remove, rebuilt one layer over.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Designators {
    /// Per-designator levels. `upstream=debug,business=info` must keep
    /// working; it is the fleet's one real use of designator filtering
    /// (searchbase/README.md), so an allowlist alone would be a capability
    /// loss dressed as an improvement.
    rules: BTreeMap<String, LevelFilter>,
    /// A bare level, applying to designators the rules do not name.
    default: Option<LevelFilter>,
    /// Nothing configured at all.
    unset: bool,
}

impl Designators {
    /// Everything passes. Also what an unparseable value degrades to — see
    /// `LogConfig::resolve`.
    pub fn permissive() -> Self {
        Self {
            unset: true,
            ..Default::default()
        }
    }

    /// Strict: an unrecognised designator or level is refused rather than
    /// ignored. This is the one filter input the stand controls end to end,
    /// and silently ignoring a misspelling is exactly the failure R28 removes.
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        let Some(text) = value.map(str::trim).filter(|t| !t.is_empty()) else {
            return Ok(Self::permissive());
        };

        let mut out = Self::default();
        for item in text.split(',').map(str::trim).filter(|i| !i.is_empty()) {
            match item.split_once('=') {
                None => {
                    let level = parse_level(item)?;
                    out.default = Some(level);
                }
                Some((name, level)) => {
                    let name = name.trim();
                    check_designator(name)?;
                    out.rules
                        .insert(name.to_owned(), parse_level(level.trim())?);
                }
            }
        }
        Ok(out)
    }

    pub fn from_env() -> Result<Self, String> {
        Self::parse(std::env::var("LOG_DESIGNATORS").ok().as_deref())
    }

    /// Whether an event carrying `designator` at `level` should be emitted.
    /// An event with NO designator — anything from a dependency — is not this
    /// filter's business and always passes; `RUST_LOG` is the axis for those.
    fn admits(&self, designator: Option<&str>, level: &tracing::Level) -> bool {
        if self.unset {
            return true;
        }
        let Some(designator) = designator else {
            return true;
        };
        match self.rules.get(designator).or(self.default.as_ref()) {
            Some(allowed) => LevelFilter::from_level(*level) <= *allowed,
            None => false,
        }
    }
}

fn parse_level(text: &str) -> Result<LevelFilter, String> {
    LevelFilter::from_str(text).map_err(|_| {
        format!(
            "LOG_DESIGNATORS: {text:?} is not a level — expected one of \
             \"trace\", \"debug\", \"info\", \"warn\", \"error\" or \"off\""
        )
    })
}

fn check_designator(name: &str) -> Result<(), String> {
    if STAND.contains(&name) || name.starts_with(CUSTOM_PREFIX) {
        return Ok(());
    }
    Err(format!(
        "LOG_DESIGNATORS: {name:?} is not a designator — expected one of {STAND:?}, \
         or a project designator carrying the {CUSTOM_PREFIX:?} prefix. Refusing rather \
         than ignoring it: a filter that silently matches nothing is the failure this \
         variable exists to remove."
    ))
}

/// Pulls the designator out of an event's fields. `Filter::enabled` sees only
/// `Metadata`, where field VALUES do not exist, which is why this filtering
/// cannot be done there and `event_enabled` is used instead.
#[derive(Default)]
struct Read(Option<String>);

impl Visit for Read {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == FIELD {
            self.0 = Some(value.to_owned());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == FIELD && self.0.is_none() {
            let rendered = format!("{value:?}");
            self.0 = Some(rendered.trim_matches('"').to_owned());
        }
    }
}

impl<S: Subscriber> Filter<S> for Designators {
    /// Deliberately permissive: field values are not visible here, so a
    /// verdict at this point could only ever be about the module path, which
    /// is `RUST_LOG`'s axis and not this one.
    fn enabled(&self, _meta: &Metadata<'_>, _cx: &Context<'_, S>) -> bool {
        true
    }

    fn event_enabled(&self, event: &Event<'_>, _cx: &Context<'_, S>) -> bool {
        if self.unset {
            return true;
        }
        let mut found = Read::default();
        event.record(&mut found);
        self.admits(found.0.as_deref(), event.metadata().level())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Level;

    /// Spellings are retyped rather than read off the declaration: a test that
    /// reuses it asserts nothing (CODESTYLE 4.5).
    #[test]
    fn unset_and_empty_admit_everything() {
        for unset in [None, Some(""), Some("   ")] {
            let d = Designators::parse(unset).unwrap();
            assert!(d.admits(Some("auth"), &Level::TRACE));
            assert!(d.admits(Some("c-scheduler"), &Level::TRACE));
            assert!(d.admits(None, &Level::TRACE));
        }
    }

    #[test]
    fn per_designator_levels_survive() {
        let d = Designators::parse(Some("upstream=debug,business=info")).unwrap();
        assert!(d.admits(Some("upstream"), &Level::DEBUG));
        assert!(!d.admits(Some("upstream"), &Level::TRACE));
        assert!(d.admits(Some("business"), &Level::INFO));
        assert!(
            !d.admits(Some("business"), &Level::DEBUG),
            "business was pinned at info, so debug must not pass"
        );
        assert!(
            !d.admits(Some("auth"), &Level::ERROR),
            "naming designators restricts to them, as RUST_LOG does"
        );
    }

    #[test]
    fn a_bare_level_covers_the_designators_not_named() {
        let d = Designators::parse(Some("warn,auth=debug")).unwrap();
        assert!(d.admits(Some("auth"), &Level::DEBUG));
        assert!(d.admits(Some("storage"), &Level::WARN));
        assert!(!d.admits(Some("storage"), &Level::INFO));
    }

    /// An event from a dependency carries no designator, so this axis has no
    /// opinion on it — otherwise setting LOG_DESIGNATORS would silently mute
    /// every library the service uses.
    #[test]
    fn events_without_a_designator_are_not_this_filters_business() {
        let d = Designators::parse(Some("auth=error")).unwrap();
        assert!(d.admits(None, &Level::TRACE));
    }

    #[test]
    fn custom_designators_are_accepted_by_prefix() {
        let d = Designators::parse(Some("c-scheduler=debug")).unwrap();
        assert!(d.admits(Some("c-scheduler"), &Level::DEBUG));
    }

    #[test]
    fn a_misspelled_designator_is_refused_not_ignored() {
        for bad in ["athu=debug", "AUTH=debug", "scheduler=debug", "=debug"] {
            let err = Designators::parse(Some(bad)).expect_err("must refuse");
            assert!(
                err.contains("is not a designator"),
                "unhelpful refusal for {bad:?}: {err}"
            );
        }
    }

    #[test]
    fn a_bad_level_is_refused_and_the_message_names_the_valid_ones() {
        let err = Designators::parse(Some("auth=verbose")).expect_err("must refuse");
        assert!(err.contains("is not a level"), "{err}");
        for level in ["trace", "debug", "info", "warn", "error", "off"] {
            assert!(err.contains(level), "refusal must name {level}: {err}");
        }
    }

    #[test]
    fn the_refusal_names_every_stand_designator() {
        let err = Designators::parse(Some("nope=info")).expect_err("must refuse");
        for name in ["auth", "business", "upstream", "storage", "http"] {
            assert!(err.contains(name), "refusal must name {name}: {err}");
        }
    }
}
