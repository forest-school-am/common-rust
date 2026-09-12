//! Which designators reach the output, and at what level. Selection only —
//! what a designator MEANS belongs in designator.rs, how a line LOOKS in
//! format.rs.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use tracing::field::{Field, Visit};
use tracing::{Event, Metadata, Subscriber};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::{Context, Filter};

use crate::config::Refusal;
use crate::designator::{CUSTOM_PREFIX, FIELD, STAND};

const LEVELS: [&str; 6] = ["trace", "debug", "info", "warn", "error", "off"];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Designators {
    rules: BTreeMap<String, LevelFilter>,
    default: Option<LevelFilter>,
    unset: bool,
}

impl Designators {
    pub fn permissive() -> Self {
        Self {
            unset: true,
            ..Default::default()
        }
    }

    pub fn parse(value: Option<&str>) -> Result<Self, Refusal> {
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

    pub fn from_env() -> Result<Self, Refusal> {
        Self::parse(std::env::var("LOG_DESIGNATORS").ok().as_deref())
    }

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

fn parse_level(text: &str) -> Result<LevelFilter, Refusal> {
    LevelFilter::from_str(text).map_err(|_| Refusal {
        variable: "LOG_DESIGNATORS",
        value: text.to_owned(),
        accepted: format!("a level, one of {LEVELS:?}"),
        detail: None,
    })
}

fn check_designator(name: &str) -> Result<(), Refusal> {
    if STAND.contains(&name) || name.starts_with(CUSTOM_PREFIX) {
        return Ok(());
    }
    Err(Refusal {
        variable: "LOG_DESIGNATORS",
        value: name.to_owned(),
        accepted: format!(
            "a designator, one of {STAND:?}, or a project designator carrying \
             the {CUSTOM_PREFIX:?} prefix"
        ),
        detail: None,
    })
}

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
    /// `Filter::enabled` is handed only `Metadata`, which carries no field
    /// VALUES, so the designator cannot be read here at all — hence the
    /// verdict is deferred to `event_enabled`.
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
        for (bad, offender) in [
            ("athu=debug", "athu"),
            ("AUTH=debug", "AUTH"),
            ("scheduler=debug", "scheduler"),
            ("=debug", ""),
        ] {
            let r = Designators::parse(Some(bad)).expect_err("must refuse");
            assert_eq!(r.variable, "LOG_DESIGNATORS");
            assert_eq!(r.value, offender, "must name the offending item: {r:?}");
            assert!(
                r.accepted.contains("designator"),
                "must say a designator was expected: {r:?}"
            );
        }
    }

    #[test]
    fn a_bad_level_is_refused_and_the_refusal_names_the_valid_ones() {
        let r = Designators::parse(Some("auth=verbose")).expect_err("must refuse");
        assert_eq!(r.variable, "LOG_DESIGNATORS");
        assert_eq!(r.value, "verbose");
        for level in ["trace", "debug", "info", "warn", "error", "off"] {
            assert!(r.accepted.contains(level), "must name {level}: {r:?}");
        }
    }

    #[test]
    fn the_refusal_names_every_stand_designator() {
        let r = Designators::parse(Some("nope=info")).expect_err("must refuse");
        for name in ["auth", "business", "upstream", "storage", "http"] {
            assert!(r.accepted.contains(name), "must name {name}: {r:?}");
        }
    }
}
