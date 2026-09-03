//! What the logging environment says: formats, deployment class, filters.
//! Resolution and parsing only — nothing here writes a log line.

use std::str::FromStr;

use strum::{AsRefStr, Display, EnumString, VariantNames};

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
                "DEPLOYMENT_TYPE={text:?} is not valid — expected {} \
                 (unset means dev). Refusing rather than defaulting: a typo here would \
                 silently enable dev-only behaviour under a prod deployment.",
                or_list(Deployment::VARIANTS)
            )
        })
    }

    pub fn from_env() -> Result<Self, String> {
        Self::parse(std::env::var("DEPLOYMENT_TYPE").ok().as_deref())
    }
}

/// `["a", "b", "c"]` -> `"a", "b" or "c"`. For refusal messages that must name
/// every accepted spelling without any of them being retyped here.
fn or_list(values: &[&str]) -> String {
    let quoted: Vec<String> = values.iter().map(|v| format!("{v:?}")).collect();
    match quoted.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

#[derive(Debug, Clone)]
pub struct LogConfig {
    pub format: Format,
    pub deployment: Deployment,
    pub filter: String,
}

impl LogConfig {
    pub fn resolve(
        log_format: Option<&str>,
        deployment_type: Option<&str>,
        rust_log: Option<&str>,
    ) -> Self {
        let format = log_format.and_then(|s| Format::from_str(s).ok()).unwrap_or(Format::Json);
        let deployment =
            deployment_type.and_then(|s| Deployment::from_str(s).ok()).unwrap_or(Deployment::Dev);
        let filter = match rust_log {
            Some(s) if !s.is_empty() => s.to_owned(),
            _ => match deployment {
                Deployment::Prod => "info".to_owned(),
                Deployment::Dev => "debug".to_owned(),
            },
        };
        Self { format, deployment, filter }
    }

    pub fn from_env() -> Self {
        let get = |k: &str| std::env::var(k).ok();
        Self::resolve(
            get("LOG_FORMAT").as_deref(),
            get("DEPLOYMENT_TYPE").as_deref(),
            get("RUST_LOG").as_deref(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_defaults_to_json_including_unknown() {
        assert_eq!(LogConfig::resolve(None, None, None).format, Format::Json);
        assert_eq!(LogConfig::resolve(Some("json"), None, None).format, Format::Json);
        assert_eq!(LogConfig::resolve(Some("HUMAN"), None, None).format, Format::Json); // case-sensitive; unknown -> json
        assert_eq!(LogConfig::resolve(Some("bogus"), None, None).format, Format::Json);
        assert_eq!(LogConfig::resolve(Some("human"), None, None).format, Format::Human);
    }

    #[test]
    fn deployment_defaults_to_dev_including_unknown() {
        assert_eq!(LogConfig::resolve(None, None, None).deployment, Deployment::Dev);
        assert_eq!(LogConfig::resolve(None, Some("dev"), None).deployment, Deployment::Dev);
        assert_eq!(LogConfig::resolve(None, Some("bogus"), None).deployment, Deployment::Dev);
        assert_eq!(LogConfig::resolve(None, Some("prod"), None).deployment, Deployment::Prod);
    }

    #[test]
    fn filter_uses_rust_log_else_deployment_default() {
        assert_eq!(LogConfig::resolve(None, Some("prod"), Some("mycrate=trace")).filter, "mycrate=trace");
        assert_eq!(LogConfig::resolve(None, Some("prod"), Some("")).filter, "info");
        assert_eq!(LogConfig::resolve(None, Some("prod"), None).filter, "info");
        assert_eq!(LogConfig::resolve(None, Some("dev"), None).filter, "debug");
        assert_eq!(LogConfig::resolve(None, None, None).filter, "debug"); // default dev
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

    #[test]
    fn or_list_quotes_and_joins() {
        assert_eq!(or_list(&[]), "");
        assert_eq!(or_list(&["dev"]), "\"dev\"");
        assert_eq!(or_list(Deployment::VARIANTS), "\"prod\" or \"dev\"");
        assert_eq!(or_list(&["a", "b", "c"]), "\"a\", \"b\" or \"c\"");
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
