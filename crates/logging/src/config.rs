//! What the logging environment says: format, deployment class, filters.
//! Resolution and parsing only — nothing here writes a log line. The value
//! types (`Format`, `Deployment`) and `Refusal` are common-config's, so a
//! binary's derived config carries them without a second parse.

use common_config::{Common, Deployment, Format, Refusal};
use tracing_subscriber::EnvFilter;

use crate::filter::Designators;

pub(crate) const DEFAULT_FILTER: &str = "info";

pub const RUST_LOG_VARIABLE: &str = "RUST_LOG";

const RUST_LOG_ACCEPTED: &str = "comma-separated tracing directives such as \
     \"info\", \"my_crate=debug\" or \"my_crate::module=trace,sqlx=warn\" \
     (unset means \"info\" under DEPLOYMENT_TYPE=prod, \"debug\" under dev)";

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
        Self::assemble(format, deployment, rust_log, log_designators)
    }

    /// From a loaded `Common` section plus `RUST_LOG`, which stays tracing's
    /// own variable rather than a config field.
    pub fn from_common(common: &Common, rust_log: Option<&str>) -> Result<Self, Refusal> {
        Self::assemble(
            common.log_format,
            common.deployment,
            rust_log,
            common.log_designators.as_deref(),
        )
    }

    fn assemble(
        format: Format,
        deployment: Deployment,
        rust_log: Option<&str>,
        log_designators: Option<&str>,
    ) -> Result<Self, Refusal> {
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
            get(common_config::LOG_FORMAT_VARIABLE).as_deref(),
            get(common_config::DEPLOYMENT_VARIABLE).as_deref(),
            get(RUST_LOG_VARIABLE).as_deref(),
            get(common_config::LOG_DESIGNATORS_VARIABLE).as_deref(),
        )
    }

    pub fn env_filter(&self) -> Result<EnvFilter, Refusal> {
        EnvFilter::try_new(&self.filter).map_err(|e| {
            Refusal::new(RUST_LOG_VARIABLE, self.filter.clone(), RUST_LOG_ACCEPTED)
                .with_detail(e.to_string())
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
    fn the_two_filter_axes_do_not_touch_each_other() {
        let cfg = ok(None, None, Some("mycrate=debug"), Some("auth=trace"));
        assert_eq!(cfg.filter, "mycrate=debug");
        assert_eq!(
            cfg.designators,
            Designators::parse(Some("auth=trace")).unwrap()
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
    fn the_documented_default_config_is_json_dev_info_and_permissive() {
        let d = LogConfig::default();
        assert_eq!(d.format, Format::Json);
        assert_eq!(d.deployment, Deployment::Dev);
        assert_eq!(d.filter, "info");
        assert_eq!(d.designators, Designators::permissive());
        d.env_filter().expect("the default filter must be valid");
    }

    #[test]
    fn from_common_takes_the_typed_values_and_rust_log_separately() {
        let common = Common {
            deployment: Deployment::Prod,
            log_format: Format::Human,
            log_designators: Some("auth=trace".into()),
        };
        let cfg = LogConfig::from_common(&common, None).expect("resolves");
        assert_eq!(cfg.format, Format::Human);
        assert_eq!(cfg.deployment, Deployment::Prod);
        assert_eq!(cfg.filter, "info", "prod default when RUST_LOG is unset");
        assert_eq!(
            cfg.designators,
            Designators::parse(Some("auth=trace")).unwrap()
        );
        let cfg = LogConfig::from_common(&common, Some("mycrate=trace")).expect("resolves");
        assert_eq!(cfg.filter, "mycrate=trace");
        let r = LogConfig::from_common(&common, Some("=")).unwrap_err();
        assert_eq!(r.variable, "RUST_LOG");
        let bad = Common {
            log_designators: Some("nonsense=info".into()),
            ..common
        };
        let r = LogConfig::from_common(&bad, None).unwrap_err();
        assert_eq!(r.variable, "LOG_DESIGNATORS");
    }
}
