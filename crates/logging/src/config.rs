//! What the logging environment says: formats, deployment class, filters.
//! Resolution and parsing only — nothing here writes a log line.

use std::str::FromStr;

use strum::{AsRefStr, Display, EnumString, VariantNames};

use crate::filter::Designators;

/// Every spelling below is written once, in `serialize`, and both directions
/// are generated from it (CODESTYLE 4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, Display, EnumString, VariantNames)]
pub enum Format {
    #[strum(serialize = "human")]
    Human,
    #[strum(serialize = "json")]
    Json,
}

/// Deployment class. Presence of options never infers this — it is
/// declared. Here it only sets logging defaults (verbosity); services apply
/// the prod-required / dev-only / neutral option rules themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, Display, EnumString, VariantNames)]
pub enum Deployment {
    #[strum(serialize = "prod")]
    Prod,
    #[strum(serialize = "dev")]
    Dev,
}

impl Deployment {
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        let Some(text) = value else {
            return Ok(Deployment::Dev);
        };
        Deployment::from_str(text).map_err(|_| {
            format!(
                "DEPLOYMENT_TYPE={text:?} is not valid — expected one of {:?} \
                 (unset means dev). Refusing rather than defaulting: a typo here would \
                 silently enable dev-only behaviour under a prod deployment.",
                Deployment::VARIANTS
            )
        })
    }

    pub fn from_env() -> Result<Self, String> {
        Self::parse(std::env::var("DEPLOYMENT_TYPE").ok().as_deref())
    }
}

#[derive(Debug, Clone)]
pub struct LogConfig {
    pub format: Format,
    pub deployment: Deployment,
    /// `RUST_LOG`: the module-path axis, standard tracing semantics.
    pub filter: String,
    /// `LOG_DESIGNATORS`: the designator axis (R28). Independent of `filter`;
    /// an event must satisfy both.
    pub designators: Designators,
}

impl LogConfig {
    /// `log_designators` DEGRADES TO PERMISSIVE when it will not parse, and
    /// says so loudly once the subscriber exists. Refusing to boot is the
    /// §4.3 default, but this is the one option where a wrong value must never
    /// SILENCE anything — the whole point of R28 is removing a filter that
    /// quietly matched nothing. Failing open keeps every event visible and
    /// makes the mistake audible. A service wanting refuse-to-boot calls
    /// [`Designators::parse`] itself, as it already does for `Deployment`.
    pub fn resolve(
        log_format: Option<&str>,
        deployment_type: Option<&str>,
        rust_log: Option<&str>,
        log_designators: Option<&str>,
    ) -> (Self, Option<String>) {
        let format = log_format
            .and_then(|s| Format::from_str(s).ok())
            .unwrap_or(Format::Json);
        let deployment = deployment_type
            .and_then(|s| Deployment::from_str(s).ok())
            .unwrap_or(Deployment::Dev);
        let filter = match rust_log {
            Some(s) if !s.is_empty() => s.to_owned(),
            _ => match deployment {
                Deployment::Prod => "info".to_owned(),
                Deployment::Dev => "debug".to_owned(),
            },
        };
        let (designators, complaint) = match Designators::parse(log_designators) {
            Ok(d) => (d, None),
            Err(why) => (Designators::permissive(), Some(why)),
        };
        (
            Self {
                format,
                deployment,
                filter,
                designators,
            },
            complaint,
        )
    }

    pub fn from_env() -> (Self, Option<String>) {
        let get = |k: &str| std::env::var(k).ok();
        Self::resolve(
            get("LOG_FORMAT").as_deref(),
            get("DEPLOYMENT_TYPE").as_deref(),
            get("RUST_LOG").as_deref(),
            get("LOG_DESIGNATORS").as_deref(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_defaults_to_json_including_unknown() {
        assert_eq!(
            LogConfig::resolve(None, None, None, None).0.format,
            Format::Json
        );
        assert_eq!(
            LogConfig::resolve(Some("json"), None, None, None).0.format,
            Format::Json
        );
        assert_eq!(
            LogConfig::resolve(Some("HUMAN"), None, None, None).0.format,
            Format::Json
        ); // case-sensitive; unknown -> json
        assert_eq!(
            LogConfig::resolve(Some("bogus"), None, None, None).0.format,
            Format::Json
        );
        assert_eq!(
            LogConfig::resolve(Some("human"), None, None, None).0.format,
            Format::Human
        );
    }

    #[test]
    fn deployment_defaults_to_dev_including_unknown() {
        assert_eq!(
            LogConfig::resolve(None, None, None, None).0.deployment,
            Deployment::Dev
        );
        assert_eq!(
            LogConfig::resolve(None, Some("dev"), None, None)
                .0
                .deployment,
            Deployment::Dev
        );
        assert_eq!(
            LogConfig::resolve(None, Some("bogus"), None, None)
                .0
                .deployment,
            Deployment::Dev
        );
        assert_eq!(
            LogConfig::resolve(None, Some("prod"), None, None)
                .0
                .deployment,
            Deployment::Prod
        );
    }

    #[test]
    fn filter_uses_rust_log_else_deployment_default() {
        assert_eq!(
            LogConfig::resolve(None, Some("prod"), Some("mycrate=trace"), None)
                .0
                .filter,
            "mycrate=trace"
        );
        assert_eq!(
            LogConfig::resolve(None, Some("prod"), Some(""), None)
                .0
                .filter,
            "info"
        );
        assert_eq!(
            LogConfig::resolve(None, Some("prod"), None, None).0.filter,
            "info"
        );
        assert_eq!(
            LogConfig::resolve(None, Some("dev"), None, None).0.filter,
            "debug"
        );
        assert_eq!(LogConfig::resolve(None, None, None, None).0.filter, "debug");
        // default dev
    }

    /// The spellings are retyped here on purpose: a test that reads them off
    /// the declaration asserts nothing (CODESTYLE 4.5).
    #[test]
    fn the_declared_spellings_are_the_ones_on_the_wire() {
        assert_eq!(Format::VARIANTS, &["human", "json"]);
        assert_eq!(Deployment::VARIANTS, &["prod", "dev"]);
        assert_eq!(Deployment::Prod.as_ref(), "prod");
        assert_eq!(Deployment::Dev.to_string(), "dev");
    }

    /// The two filtering axes are resolved independently (R28) — a value for
    /// one must never end up governing the other.
    #[test]
    fn the_two_filter_axes_do_not_touch_each_other() {
        let (cfg, complaint) =
            LogConfig::resolve(None, None, Some("mycrate=debug"), Some("auth=trace"));
        assert_eq!(cfg.filter, "mycrate=debug");
        assert_eq!(
            cfg.designators,
            Designators::parse(Some("auth=trace")).unwrap()
        );
        assert!(complaint.is_none());
    }

    /// An unparseable LOG_DESIGNATORS must FAIL OPEN and complain. Failing
    /// closed would silence every event over a typo, which is the exact
    /// failure R28 exists to remove.
    #[test]
    fn an_unparseable_designator_filter_passes_everything_and_complains() {
        let (cfg, complaint) = LogConfig::resolve(None, None, None, Some("nonsense=info"));
        assert_eq!(
            cfg.designators,
            Designators::permissive(),
            "a bad filter must not silence anything"
        );
        let complaint = complaint.expect("the operator must be told");
        assert!(complaint.contains("nonsense"), "{complaint}");
    }

    /// The accepted values are rendered as an ARRAY, not as prose. The point is
    /// the reader can see where the list ends and the sentence resumes, which a
    /// `"prod" or "dev"` join leaves ambiguous. Spelled out here rather than
    /// built from VARIANTS — a test that reuses the declaration asserts nothing.
    #[test]
    fn the_refusal_renders_the_values_as_an_array() {
        let msg = Deployment::parse(Some("prd")).unwrap_err();
        assert!(
            msg.contains(r#"expected one of ["prod", "dev"] (unset means dev)"#),
            "values must read as a delimited array inside the prose: {msg}"
        );
    }
}

#[cfg(test)]
mod deployment_tests {
    use super::*;

    #[test]
    fn unset_is_dev_and_the_two_valid_values_parse() {
        assert_eq!(Deployment::parse(None).unwrap(), Deployment::Dev);
        assert_eq!(Deployment::parse(Some("dev")).unwrap(), Deployment::Dev);
        assert_eq!(Deployment::parse(Some("prod")).unwrap(), Deployment::Prod);
    }

    /// Guards the one thing a generated parser could quietly hand back: the
    /// Rust variant name accepted alongside the declared spelling.
    #[test]
    fn set_but_invalid_refuses_rather_than_defaulting_to_dev() {
        for bad in ["Prod", "PROD", "Dev", "production", "prd", ""] {
            assert!(
                Deployment::parse(Some(bad)).is_err(),
                "DEPLOYMENT_TYPE={bad:?} must be refused, not treated as dev"
            );
        }
    }

    #[test]
    fn the_refusal_names_every_accepted_spelling() {
        let msg = Deployment::parse(Some("prd")).unwrap_err();
        for value in Deployment::VARIANTS {
            assert!(msg.contains(value), "refusal must name {value:?}: {msg}");
        }
    }
}

#[cfg(test)]
mod deployment_str_tests {
    use super::*;

    #[test]
    fn as_str_round_trips_through_parse() {
        for d in [Deployment::Prod, Deployment::Dev] {
            assert_eq!(Deployment::parse(Some(d.as_ref())).unwrap(), d);
            assert_eq!(d.to_string(), d.as_ref());
        }
        assert!(Deployment::parse(Some(&format!("{:?}", Deployment::Prod))).is_err());
    }
}
