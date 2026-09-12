//! What the logging environment says: formats, deployment class, filters.
//! Resolution and parsing only — nothing here writes a log line.

use std::str::FromStr;

use strum::{AsRefStr, Display, EnumString, VariantNames};
use tracing_subscriber::EnvFilter;

use crate::filter::Designators;

#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, Display, EnumString, VariantNames)]
pub enum Format {
    #[strum(serialize = "human")]
    Human,
    #[strum(serialize = "json")]
    Json,
}

impl Format {
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        let Some(text) = value else {
            return Ok(Format::Json);
        };
        Format::from_str(text).map_err(|_| {
            format!(
                "LOG_FORMAT={text:?} is not valid — expected one of {:?} \
                 (unset means json). Refusing rather than defaulting (R50): \
                 coming up in the other format silently changes every line a \
                 downstream parser reads.",
                Format::VARIANTS
            )
        })
    }

    pub fn from_env() -> Result<Self, String> {
        Self::parse(std::env::var("LOG_FORMAT").ok().as_deref())
    }
}

/// Only the logging verbosity default is decided from this here; the
/// prod-required / dev-only / neutral option rules are each service's own.
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
    pub filter: String,
    pub designators: Designators,
}

impl LogConfig {
    pub fn resolve(
        log_format: Option<&str>,
        deployment_type: Option<&str>,
        rust_log: Option<&str>,
        log_designators: Option<&str>,
    ) -> Result<Self, String> {
        let format = Format::parse(log_format)?;
        let deployment = Deployment::parse(deployment_type)?;
        let filter = match rust_log {
            Some(s) if !s.is_empty() => s.to_owned(),
            _ => match deployment {
                Deployment::Prod => "info".to_owned(),
                Deployment::Dev => "debug".to_owned(),
            },
        };
        let resolved = Self {
            format,
            deployment,
            filter,
            designators: Designators::parse(log_designators)?,
        };
        resolved.env_filter()?;
        Ok(resolved)
    }

    pub fn from_env() -> Result<Self, String> {
        let get = |k: &str| std::env::var(k).ok();
        Self::resolve(
            get("LOG_FORMAT").as_deref(),
            get("DEPLOYMENT_TYPE").as_deref(),
            get("RUST_LOG").as_deref(),
            get("LOG_DESIGNATORS").as_deref(),
        )
    }

    /// `RUST_LOG` is a filter DSL rather than a set of spellings, so only
    /// tracing can say whether a value parses. Built here so `resolve` and
    /// `init_with` refuse identically and the message is written once.
    pub fn env_filter(&self) -> Result<EnvFilter, String> {
        EnvFilter::try_new(&self.filter).map_err(|e| {
            format!(
                "RUST_LOG={:?} is not a valid tracing filter: {e}. Expected \
                 comma-separated directives such as \"info\", \
                 \"my_crate=debug\" or \"my_crate::module=trace,sqlx=warn\" \
                 (unset means \"info\" under DEPLOYMENT_TYPE=prod, \"debug\" \
                 under dev). Refusing rather than defaulting (R50).",
                self.filter
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(
        format: Option<&str>,
        deployment: Option<&str>,
        rust_log: Option<&str>,
        designators: Option<&str>,
    ) -> LogConfig {
        LogConfig::resolve(format, deployment, rust_log, designators).expect("must resolve")
    }

    /// The refusal has to name all three things an operator needs, and a
    /// message is the part nobody checks: the VARIABLE, the VALUE they set,
    /// and what would have been accepted.
    fn assert_refusal(msg: &str, variable: &str, value: &str, accepted: &str) {
        assert!(msg.contains(variable), "must name {variable}: {msg}");
        assert!(
            msg.contains(&format!("{value:?}")),
            "must quote the rejected value {value:?}: {msg}"
        );
        assert!(
            msg.contains(accepted),
            "must name what is accepted ({accepted}): {msg}"
        );
    }

    #[test]
    fn log_format_defaults_to_json_unset_and_refuses_anything_it_does_not_know() {
        assert_eq!(ok(None, None, None, None).format, Format::Json);
        assert_eq!(ok(Some("json"), None, None, None).format, Format::Json);
        assert_eq!(ok(Some("human"), None, None, None).format, Format::Human);

        for bad in ["HUMAN", "Human", "Json", "bogus", ""] {
            let msg = LogConfig::resolve(Some(bad), None, None, None)
                .expect_err("set-but-invalid LOG_FORMAT must refuse, not degrade to json");
            assert_refusal(&msg, "LOG_FORMAT", bad, r#"["human", "json"]"#);
        }
    }

    #[test]
    fn deployment_type_defaults_to_dev_unset_and_refuses_anything_it_does_not_know() {
        assert_eq!(ok(None, None, None, None).deployment, Deployment::Dev);
        assert_eq!(
            ok(None, Some("dev"), None, None).deployment,
            Deployment::Dev
        );
        assert_eq!(
            ok(None, Some("prod"), None, None).deployment,
            Deployment::Prod
        );

        for bad in ["PROD", "Prod", "production", "prd", ""] {
            let msg = LogConfig::resolve(None, Some(bad), None, None)
                .expect_err("set-but-invalid DEPLOYMENT_TYPE must refuse, not degrade to dev");
            assert_refusal(&msg, "DEPLOYMENT_TYPE", bad, r#"["prod", "dev"]"#);
        }
    }

    #[test]
    fn rust_log_refuses_a_filter_tracing_cannot_parse() {
        for bad in [
            "=",
            "=info",
            "foo=notalevel",
            "a=b=c",
            "foo=99",
            "[[[",
            "!!!",
            " ",
        ] {
            let msg = LogConfig::resolve(None, None, Some(bad), None)
                .expect_err("a malformed RUST_LOG must refuse, not fall back to \"info\"");
            assert_refusal(&msg, "RUST_LOG", bad, "my_crate=debug");
        }

        for fine in [
            "info",
            "bogus",
            "my_crate=debug",
            "foo::bar=trace,sqlx=warn",
        ] {
            assert_eq!(
                ok(None, None, Some(fine), None).filter,
                fine,
                "{fine} is a valid tracing filter and must be taken verbatim"
            );
        }
    }

    #[test]
    fn log_designators_passes_everything_unset_and_refuses_a_value_it_does_not_know() {
        assert_eq!(
            ok(None, None, None, None).designators,
            Designators::permissive()
        );

        for (bad, accepted) in [
            ("nonsense=info", "auth"),
            ("athu=debug", "auth"),
            ("auth=verbose", "trace"),
        ] {
            let msg = LogConfig::resolve(None, None, None, Some(bad))
                .expect_err("set-but-invalid LOG_DESIGNATORS must refuse, not pass everything");
            assert!(
                msg.contains("LOG_DESIGNATORS"),
                "must name the variable: {msg}"
            );
            assert!(msg.contains(accepted), "must name what is accepted: {msg}");
        }
    }

    #[test]
    fn filter_uses_rust_log_else_deployment_default() {
        assert_eq!(
            ok(None, Some("prod"), Some("mycrate=trace"), None).filter,
            "mycrate=trace"
        );
        assert_eq!(ok(None, Some("prod"), Some(""), None).filter, "info");
        assert_eq!(ok(None, Some("prod"), None, None).filter, "info");
        assert_eq!(ok(None, Some("dev"), None, None).filter, "debug");
        assert_eq!(ok(None, None, None, None).filter, "debug");
    }

    #[test]
    fn the_declared_spellings_are_the_ones_on_the_wire() {
        assert_eq!(Format::VARIANTS, &["human", "json"]);
        assert_eq!(Deployment::VARIANTS, &["prod", "dev"]);
        assert_eq!(Deployment::Prod.as_ref(), "prod");
        assert_eq!(Deployment::Dev.to_string(), "dev");
    }

    #[test]
    fn the_two_filter_axes_do_not_touch_each_other() {
        let cfg = ok(None, None, Some("mycrate=debug"), Some("auth=trace"));
        assert_eq!(cfg.filter, "mycrate=debug");
        assert_eq!(
            cfg.designators,
            Designators::parse(Some("auth=trace")).unwrap()
        );
    }

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
