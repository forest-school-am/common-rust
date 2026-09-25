//! What the logging environment says: the `Log` section a binary's config
//! nests, the `Format` enum, and the resolved `LogConfig` (with `RUST_LOG`,
//! which stays tracing's own variable). Resolution and parsing only — nothing
//! here writes a log line; the deployment class is common-config's.

use std::str::FromStr;

use common_config::{Config, Deployment, Refusal};
use strum::{AsRefStr, Display, EnumString, VariantNames};
use tracing_subscriber::EnvFilter;

use crate::filter::Designators;

pub(crate) const DEFAULT_FILTER: &str = "info";

pub const RUST_LOG_VARIABLE: &str = "RUST_LOG";
pub const LOG_FORMAT_VARIABLE: &str = "LOG_FORMAT";
pub const LOG_DESIGNATORS_VARIABLE: &str = "LOG_DESIGNATORS";

/// The `accepted` text of a refused `LOG_FORMAT`, shared by `Format::parse`
/// and the derived `Log` field.
pub const LOG_FORMAT_ACCEPTED: &str = r#"one of ["human", "json"] (unset means json)"#;

const RUST_LOG_ACCEPTED: &str = "comma-separated tracing directives such as \
     \"info\", \"my_crate=debug\" or \"my_crate::module=trace,sqlx=warn\" \
     (unset means \"info\" under DEPLOYMENT_TYPE=prod, \"debug\" under dev)";

#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, Display, EnumString, VariantNames)]
pub enum Format {
    #[strum(serialize = "human")]
    Human,
    #[strum(serialize = "json")]
    Json,
}

impl Format {
    /// `None` is json; a set-but-unknown value refuses rather than degrading.
    pub fn parse(value: Option<&str>) -> Result<Self, Refusal> {
        let Some(text) = value else {
            return Ok(Format::Json);
        };
        Format::from_str(text)
            .map_err(|_| Refusal::new(LOG_FORMAT_VARIABLE, text, LOG_FORMAT_ACCEPTED))
    }

    pub fn from_env() -> Result<Self, Refusal> {
        Self::parse(std::env::var(LOG_FORMAT_VARIABLE).ok().as_deref())
    }
}

/// Nested as `log` in every root (`[log]` in the file, `--log--…` flags); the
/// env spellings are the bare names every operator already sets.
#[derive(Debug, Clone, Config)]
pub struct Log {
    /// Log line format: json (one object per line) or human.
    #[config(
        default = "json",
        env = "LOG_FORMAT",
        accepted = "one of [\"human\", \"json\"] (unset means json)"
    )]
    pub format: Format,
    /// Per-designator level filter such as "auth=debug,c-scheduler=info";
    /// unset passes every designator. ANDed with RUST_LOG.
    #[config(env = "LOG_DESIGNATORS")]
    pub designators: Option<String>,
}

impl Log {
    /// SEALED (config.6, user 2026-09-25): these two variables are the
    /// library's own, so a root need not nest `[log]` at all — `boot_sealed`
    /// reads them here. A nested section still wins wherever one is declared.
    pub fn parse(format: Option<&str>, designators: Option<&str>) -> Result<Self, Refusal> {
        Ok(Self {
            format: Format::parse(format)?,
            designators: designators.map(str::to_owned),
        })
    }

    pub fn from_env() -> Result<Self, Refusal> {
        Self::parse(
            std::env::var(LOG_FORMAT_VARIABLE).ok().as_deref(),
            std::env::var(LOG_DESIGNATORS_VARIABLE).ok().as_deref(),
        )
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
        Self::assemble(format, deployment, rust_log, log_designators)
    }

    /// From a loaded `Log` section and the root's deployment, plus `RUST_LOG`.
    pub fn from_log(
        log: &Log,
        deployment: Deployment,
        rust_log: Option<&str>,
    ) -> Result<Self, Refusal> {
        Self::assemble(log.format, deployment, rust_log, log.designators.as_deref())
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
            get(LOG_FORMAT_VARIABLE).as_deref(),
            get(common_config::DEPLOYMENT_VARIABLE).as_deref(),
            get(RUST_LOG_VARIABLE).as_deref(),
            get(LOG_DESIGNATORS_VARIABLE).as_deref(),
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
    use common_config::Path;

    fn ok(
        format: Option<&str>,
        deployment: Option<&str>,
        rust_log: Option<&str>,
        designators: Option<&str>,
    ) -> LogConfig {
        LogConfig::resolve(format, deployment.or(Some("dev")), rust_log, designators)
            .expect("must resolve")
    }

    #[test]
    fn the_sealed_log_reads_its_own_two_variables() {
        let set = Log::parse(Some("human"), Some("auth=debug")).expect("parse");
        assert_eq!(set.format, Format::Human);
        assert_eq!(set.designators.as_deref(), Some("auth=debug"));

        let unset = Log::parse(None, None).expect("parse");
        assert_eq!(unset.format, Format::Json, "unset LOG_FORMAT is json");
        assert!(unset.designators.is_none());

        let refused = Log::parse(Some("xml"), None).expect_err("a bad format must refuse");
        assert_refusal(&refused, LOG_FORMAT_VARIABLE, "xml", LOG_FORMAT_ACCEPTED);
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
            let r = LogConfig::resolve(Some(bad), Some("dev"), None, None)
                .expect_err("set-but-invalid LOG_FORMAT must refuse, not degrade to json");
            assert_refusal(&r, "LOG_FORMAT", bad, r#"["human", "json"]"#);
        }
    }

    #[test]
    fn the_declared_format_spellings_are_the_ones_on_the_wire() {
        assert_eq!(Format::VARIANTS, &["human", "json"]);
        assert_eq!(
            LOG_FORMAT_ACCEPTED,
            format!("one of {:?} (unset means json)", Format::VARIANTS)
        );
    }

    #[test]
    fn deployment_type_unset_or_unknown_refuses() {
        assert_eq!(
            ok(None, Some("dev"), None, None).deployment,
            Deployment::Dev
        );
        assert_eq!(
            ok(None, Some("prod"), None, None).deployment,
            Deployment::Prod
        );
        let r = LogConfig::resolve(None, None, None, None)
            .expect_err("unset DEPLOYMENT_TYPE must refuse, not default to dev");
        assert_refusal(&r, "DEPLOYMENT_TYPE", "unset", r#"["prod", "dev"]"#);
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
            let r = LogConfig::resolve(None, Some("dev"), Some(bad), None)
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
            let r = LogConfig::resolve(None, Some("dev"), None, Some(bad))
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
        let rendered = LogConfig::resolve(None, Some("dev"), Some("="), None)
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
    fn the_log_section_keeps_the_bare_env_names_under_a_log_table() {
        let spelled: Vec<(String, String, String)> = Log::schema(&Path::root().child("log"))
            .iter()
            .map(|f| (f.flag(), f.env("APP"), f.toml()))
            .collect();
        assert_eq!(
            spelled,
            [
                ("--log--format", "LOG_FORMAT", "log.format"),
                ("--log--designators", "LOG_DESIGNATORS", "log.designators"),
            ]
            .map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string()))
        );
    }

    #[derive(Debug, Config)]
    #[config(app = "DEMO")]
    struct Demo {
        deployment: Deployment,
        #[config(nested)]
        log: Log,
    }

    fn loaded(env: &[(&str, &str)]) -> Result<Demo, Refusal> {
        let env: Vec<(String, String)> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        match common_config::load_from::<Demo>(&[], &env)? {
            common_config::Outcome::Config(demo) => Ok(demo),
            other => panic!("expected a config, got {other:?}"),
        }
    }

    #[test]
    fn the_derived_log_section_refuses_in_the_same_words_as_the_env_parse() {
        let r = loaded(&[("DEPLOYMENT_TYPE", "dev"), ("LOG_FORMAT", "xml")]).unwrap_err();
        assert_eq!(r.variable, "LOG_FORMAT");
        assert_eq!(r.accepted, LOG_FORMAT_ACCEPTED);
        let demo = loaded(&[("DEPLOYMENT_TYPE", "prod")]).unwrap();
        assert_eq!(demo.log.format, Format::Json);
        assert_eq!(demo.log.designators, None);
    }

    #[test]
    fn from_log_takes_the_typed_values_and_rust_log_separately() {
        let log = Log {
            format: Format::Human,
            designators: Some("auth=trace".into()),
        };
        let cfg = LogConfig::from_log(&log, Deployment::Prod, None).expect("resolves");
        assert_eq!(cfg.format, Format::Human);
        assert_eq!(cfg.deployment, Deployment::Prod);
        assert_eq!(cfg.filter, "info", "prod default when RUST_LOG is unset");
        assert_eq!(
            cfg.designators,
            Designators::parse(Some("auth=trace")).unwrap()
        );
        let cfg =
            LogConfig::from_log(&log, Deployment::Prod, Some("mycrate=trace")).expect("resolves");
        assert_eq!(cfg.filter, "mycrate=trace");
        let r = LogConfig::from_log(&log, Deployment::Prod, Some("=")).unwrap_err();
        assert_eq!(r.variable, "RUST_LOG");
        let bad = Log {
            designators: Some("nonsense=info".into()),
            ..log
        };
        let r = LogConfig::from_log(&bad, Deployment::Dev, None).unwrap_err();
        assert_eq!(r.variable, "LOG_DESIGNATORS");
    }
}
