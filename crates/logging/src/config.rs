//! What the logging environment says: formats, deployment class, filters.
//! Resolution and parsing only — nothing here writes a log line.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Human,
    Json,
}

/// Deployment class. Presence of options never infers this — it is
/// declared. Here it only sets logging defaults (verbosity); services apply
/// the prod-required / dev-only / neutral option rules themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deployment {
    Prod,
    Dev,
}

impl Deployment {
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value {
            None => Ok(Deployment::Dev),
            Some("prod") => Ok(Deployment::Prod),
            Some("dev") => Ok(Deployment::Dev),
            Some(other) => Err(format!(
                "DEPLOYMENT_TYPE={other:?} is not valid — expected \"prod\" or \"dev\" \
                 (unset means dev). Refusing rather than defaulting: a typo here would \
                 silently enable dev-only behaviour under a prod deployment."
            )),
        }
    }

    pub fn from_env() -> Result<Self, String> {
        Self::parse(std::env::var("DEPLOYMENT_TYPE").ok().as_deref())
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Deployment::Prod => "prod",
            Deployment::Dev => "dev",
        }
    }
}

impl std::fmt::Display for Deployment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
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
        let format = match log_format {
            Some("human") => Format::Human,
            _ => Format::Json,
        };
        let deployment = match deployment_type {
            Some("prod") => Deployment::Prod,
            _ => Deployment::Dev,
        };
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
        for bad in ["Prod", "PROD", "production", "prd", ""] {
            assert!(
                Deployment::parse(Some(bad)).is_err(),
                "DEPLOYMENT_TYPE={bad:?} must be refused, not treated as dev"
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
            assert_eq!(Deployment::parse(Some(d.as_str())).unwrap(), d);
            assert_eq!(d.to_string(), d.as_str());
        }
        assert!(Deployment::parse(Some(&format!("{:?}", Deployment::Prod))).is_err());
    }
}
