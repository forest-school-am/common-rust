//! What the logging environment says: formats, deployment class, filters.
//! Resolution and parsing only — nothing here writes a log line.

use std::fmt;
use std::str::FromStr;

use strum::{AsRefStr, Display, EnumString, VariantNames};
use tracing_subscriber::EnvFilter;

use crate::filter::Designators;

pub(crate) const DEFAULT_FILTER: &str = "info";

const RUST_LOG_ACCEPTED: &str = "comma-separated tracing directives such as \
     \"info\", \"my_crate=debug\" or \"my_crate::module=trace,sqlx=warn\" \
     (unset means \"info\" under DEPLOYMENT_TYPE=prod, \"debug\" under dev)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub variable: &'static str,
    pub value: String,
    pub accepted: String,
    pub detail: Option<String>,
}

impl Refusal {
    pub fn new(
        variable: &'static str,
        value: impl Into<String>,
        accepted: impl Into<String>,
    ) -> Self {
        Self {
            variable,
            value: value.into(),
            accepted: accepted.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}={:?} is not valid — expected {}",
            self.variable, self.value, self.accepted
        )?;
        match &self.detail {
            Some(detail) => write!(f, " ({detail})"),
            None => Ok(()),
        }
    }
}

impl std::error::Error for Refusal {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, Display, EnumString, VariantNames)]
pub enum Format {
    #[strum(serialize = "human")]
    Human,
    #[strum(serialize = "json")]
    Json,
}

impl Format {
    pub fn parse(value: Option<&str>) -> Result<Self, Refusal> {
        let Some(text) = value else {
            return Ok(Format::Json);
        };
        Format::from_str(text).map_err(|_| Refusal {
            variable: "LOG_FORMAT",
            value: text.to_owned(),
            accepted: format!("one of {:?} (unset means json)", Format::VARIANTS),
            detail: None,
        })
    }

    pub fn from_env() -> Result<Self, Refusal> {
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
    pub fn parse(value: Option<&str>) -> Result<Self, Refusal> {
        let Some(text) = value else {
            return Ok(Deployment::Dev);
        };
        Deployment::from_str(text).map_err(|_| Refusal {
            variable: "DEPLOYMENT_TYPE",
            value: text.to_owned(),
            accepted: format!("one of {:?} (unset means dev)", Deployment::VARIANTS),
            detail: None,
        })
    }

    pub fn from_env() -> Result<Self, Refusal> {
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
    ) -> Result<Self, Refusal> {
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

    pub fn from_env() -> Result<Self, Refusal> {
        let get = |k: &str| std::env::var(k).ok();
        Self::resolve(
            get("LOG_FORMAT").as_deref(),
            get("DEPLOYMENT_TYPE").as_deref(),
            get("RUST_LOG").as_deref(),
            get("LOG_DESIGNATORS").as_deref(),
        )
    }

    pub fn env_filter(&self) -> Result<EnvFilter, Refusal> {
        EnvFilter::try_new(&self.filter).map_err(|e| Refusal {
            variable: "RUST_LOG",
            value: self.filter.clone(),
            accepted: RUST_LOG_ACCEPTED.to_owned(),
            detail: Some(e.to_string()),
        })
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            format: Format::Json,
            deployment: Deployment::Dev,
            filter: DEFAULT_FILTER.to_owned(),
            designators: Designators::permissive(),
        }
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

    fn assert_refusal(r: &Refusal, variable: &str, value: &str, accepted: &str) {
        assert_eq!(r.variable, variable, "wrong variable in {r:?}");
        assert_eq!(r.value, value, "must carry the rejected value: {r:?}");
        assert!(
            r.accepted.contains(accepted),
            "must name what is accepted ({accepted}): {r:?}"
        );
    }

    #[test]
    fn log_format_defaults_to_json_unset_and_refuses_anything_it_does_not_know() {
        assert_eq!(ok(None, None, None, None).format, Format::Json);
        assert_eq!(ok(Some("json"), None, None, None).format, Format::Json);
        assert_eq!(ok(Some("human"), None, None, None).format, Format::Human);

        for bad in ["HUMAN", "Human", "Json", "bogus", ""] {
            let r = LogConfig::resolve(Some(bad), None, None, None)
                .expect_err("set-but-invalid LOG_FORMAT must refuse, not degrade to json");
            assert_refusal(&r, "LOG_FORMAT", bad, r#"["human", "json"]"#);
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
            let r = LogConfig::resolve(None, Some(bad), None, None)
                .expect_err("set-but-invalid DEPLOYMENT_TYPE must refuse, not degrade to dev");
            assert_refusal(&r, "DEPLOYMENT_TYPE", bad, r#"["prod", "dev"]"#);
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
            let r = LogConfig::resolve(None, None, Some(bad), None)
                .expect_err("a malformed RUST_LOG must refuse, not fall back to \"info\"");
            assert_refusal(&r, "RUST_LOG", bad, "my_crate=debug");
            assert!(
                r.detail.is_some(),
                "RUST_LOG is a DSL, so tracing's own parse error is the only \
                 thing that says WHERE it is wrong: {r:?}"
            );
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

        for (bad, offender, accepted) in [
            ("nonsense=info", "nonsense", "auth"),
            ("athu=debug", "athu", "auth"),
            ("auth=verbose", "verbose", "trace"),
        ] {
            let r = LogConfig::resolve(None, None, None, Some(bad))
                .expect_err("set-but-invalid LOG_DESIGNATORS must refuse, not pass everything");
            assert_refusal(&r, "LOG_DESIGNATORS", offender, accepted);
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
    fn the_rendered_refusal_names_the_variable_the_value_and_the_array() {
        let rendered = Deployment::parse(Some("prd")).unwrap_err().to_string();
        assert_eq!(
            rendered,
            r#"DEPLOYMENT_TYPE="prd" is not valid — expected one of ["prod", "dev"] (unset means dev)"#
        );
    }

    #[test]
    fn the_rendered_refusal_appends_a_detail_when_there_is_one() {
        let rendered = LogConfig::resolve(None, None, Some("="), None)
            .unwrap_err()
            .to_string();
        assert!(
            rendered.starts_with(r#"RUST_LOG="=" is not valid — expected comma-separated"#),
            "{rendered}"
        );
        assert!(
            rendered.ends_with("(invalid filter directive)"),
            "tracing's own error must survive into the rendering: {rendered}"
        );
    }

    #[test]
    fn the_refusal_fallback_config_is_the_documented_default() {
        let d = LogConfig::default();
        assert_eq!(d.format, Format::Json);
        assert_eq!(d.deployment, Deployment::Dev);
        assert_eq!(d.filter, "info");
        assert_eq!(d.designators, Designators::permissive());
        d.env_filter()
            .expect("the fallback filter must itself be valid, or init() cannot refuse");
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
        let r = Deployment::parse(Some("prd")).unwrap_err();
        for value in Deployment::VARIANTS {
            assert!(
                r.accepted.contains(value),
                "refusal must name {value:?}: {r:?}"
            );
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
