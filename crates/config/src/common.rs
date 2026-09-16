//! The section every Les binary carries: deployment class and the logging
//! knobs, under their legacy bare env names. The struct and the log FORMAT
//! enum live here; what the values DO (bringing a subscriber up) is
//! `common_logging::boot`, and `RUST_LOG` stays tracing's own variable, read
//! by logging, not a field here.

use std::str::FromStr;

use strum::{AsRefStr, Display, EnumString, VariantNames};

use crate::{Config, Deployment, Refusal};

pub const LOG_FORMAT_VARIABLE: &str = "LOG_FORMAT";
pub const LOG_DESIGNATORS_VARIABLE: &str = "LOG_DESIGNATORS";

/// The `accepted` text of a refused `LOG_FORMAT`, shared by `Format::parse`
/// and the derived field.
pub const LOG_FORMAT_ACCEPTED: &str = r#"one of ["human", "json"] (unset means json)"#;

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

/// Nested as `common` in every root (`[common]` in the file,
/// `--common--…` flags); the env spellings are the legacy bare names.
#[derive(Debug, Clone, Config)]
pub struct Common {
    /// Deployment class: prod or dev. Sets the default log verbosity; which
    /// options are prod-required or dev-only is the binary's own rule.
    #[config(
        default = "dev",
        env = "DEPLOYMENT_TYPE",
        accepted = "one of [\"prod\", \"dev\"] (unset means dev)"
    )]
    pub deployment: Deployment,
    /// Log line format: json (one object per line) or human.
    #[config(
        default = "json",
        env = "LOG_FORMAT",
        accepted = "one of [\"human\", \"json\"] (unset means json)"
    )]
    pub log_format: Format,
    /// Per-designator level filter such as "auth=debug,c-scheduler=info";
    /// unset passes every designator. ANDed with RUST_LOG.
    #[config(env = "LOG_DESIGNATORS")]
    pub log_designators: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deployment::DEPLOYMENT_ACCEPTED;
    use crate::Path;

    #[test]
    fn log_format_defaults_to_json_unset_and_refuses_anything_it_does_not_know() {
        assert_eq!(Format::parse(None).unwrap(), Format::Json);
        assert_eq!(Format::parse(Some("json")).unwrap(), Format::Json);
        assert_eq!(Format::parse(Some("human")).unwrap(), Format::Human);
        for bad in ["HUMAN", "Human", "Json", "bogus", ""] {
            let r = Format::parse(Some(bad))
                .expect_err("set-but-invalid LOG_FORMAT must refuse, not degrade to json");
            assert_eq!(r.variable, LOG_FORMAT_VARIABLE);
            assert_eq!(r.value, bad);
            assert!(r.accepted.contains(r#"["human", "json"]"#), "{r:?}");
        }
    }

    #[test]
    fn the_declared_spellings_are_the_ones_on_the_wire() {
        assert_eq!(Format::VARIANTS, &["human", "json"]);
        assert_eq!(
            LOG_FORMAT_ACCEPTED,
            format!("one of {:?} (unset means json)", Format::VARIANTS)
        );
    }

    #[test]
    fn the_three_fields_keep_their_legacy_env_names_under_a_common_table() {
        let fields = Common::schema(&Path::root().child("common"));
        let spelled: Vec<(String, String, String)> = fields
            .iter()
            .map(|f| (f.flag(), f.env("APP"), f.toml()))
            .collect();
        assert_eq!(
            spelled,
            [
                (
                    "--common--deployment",
                    "DEPLOYMENT_TYPE",
                    "common.deployment"
                ),
                ("--common--log-format", "LOG_FORMAT", "common.log_format"),
                (
                    "--common--log-designators",
                    "LOG_DESIGNATORS",
                    "common.log_designators"
                ),
            ]
            .map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string()))
        );
    }

    #[derive(Debug, Config)]
    #[config(app = "DEMO")]
    struct Demo {
        #[config(nested)]
        common: Common,
    }

    fn refused(name: &str, value: &str) -> Refusal {
        let env = [(name.to_string(), value.to_string())];
        match crate::load_from::<Demo>(&[], &env) {
            Err(refusal) => refusal,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// The derived field and the legacy `parse` must refuse in the same words,
    /// because consumers' tests pin the text and do not care which path ran.
    #[test]
    fn the_derived_accepted_texts_are_the_shared_constants() {
        let r = refused("DEPLOYMENT_TYPE", "staging");
        assert_eq!(r.variable, "DEPLOYMENT_TYPE");
        assert_eq!(r.value, "staging");
        assert_eq!(r.accepted, DEPLOYMENT_ACCEPTED);
        let r = refused("LOG_FORMAT", "xml");
        assert_eq!(r.accepted, LOG_FORMAT_ACCEPTED);
        let r = refused("DEPLOYMENT_TYPE", "");
        assert_eq!(
            r.value, "",
            "an empty value is set, and set-but-invalid refuses"
        );
    }

    #[test]
    fn unset_gives_the_documented_defaults_and_the_root_reaches_its_section() {
        let demo = match crate::load_from::<Demo>(&[], &[]) {
            Ok(crate::Outcome::Config(demo)) => demo,
            other => panic!("expected a config, got {other:?}"),
        };
        let common = crate::Root::common(&demo);
        assert_eq!(common.deployment, Deployment::Dev);
        assert_eq!(common.log_format, Format::Json);
        assert_eq!(common.log_designators, None);
    }
}
