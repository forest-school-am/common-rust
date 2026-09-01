//! Logging config resolution (CODESTYLE.md §8.4 + §4.4-aware). Two knobs —
//! `LOG_FORMAT` and `DEPLOYMENT_TYPE` — resolve to a format, a deployment
//! class, and an EnvFilter directive. Resolution is a pure function of its
//! inputs ([`LogConfig::resolve`]) so the selection matrix is unit-testable;
//! [`LogConfig::from_env`] is the thin env reader over it.

/// Output format (§8.4). Default is JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `timestamp level designator file:row reqid [actor] message`.
    Human,
    /// tracing-subscriber JSON layer, one object per line.
    Json,
}

/// Deployment class (§4.4). Presence of options never infers this — it is
/// declared. Here it only sets logging defaults (verbosity); services apply
/// the prod-required / dev-only / neutral option rules themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deployment {
    Prod,
    Dev,
}

impl Deployment {
    /// Parse the declared class, REFUSING a set-but-invalid value (§4.3).
    ///
    /// `Prod`, `production` or any typo must not silently become `Dev` — that
    /// is how a mistyped prod deployment quietly activates dev-only
    /// permissiveness. Unset is the one legitimate default.
    ///
    /// This is deliberately STRICTER than [`LogConfig::resolve`]'s handling of
    /// `LOG_FORMAT`, which degrades to a default on an unrecognised value
    /// because logging must always come up. That leniency is correct for a
    /// FORMAT and wrong for a class that gates security behaviour, so the two
    /// do not share a policy (§4.4y).
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

    /// [`Deployment::parse`] against the process environment.
    pub fn from_env() -> Result<Self, String> {
        Self::parse(std::env::var("DEPLOYMENT_TYPE").ok().as_deref())
    }

    /// The env-var spelling — `"prod"` / `"dev"`, round-tripping [`parse`].
    ///
    /// `Debug` yields `Prod`/`Dev`, which do NOT match the values the var
    /// accepts, so logging the class via `{:?}` prints something a reader
    /// cannot paste back into `DEPLOYMENT_TYPE`.
    ///
    /// [`parse`]: Deployment::parse
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

/// Fully-resolved logging configuration.
#[derive(Debug, Clone)]
pub struct LogConfig {
    pub format: Format,
    pub deployment: Deployment,
    /// EnvFilter directive string (from `RUST_LOG`, else a deployment default).
    pub filter: String,
}

impl LogConfig {
    /// Pure resolution — no env access, so the selection matrix is testable.
    ///
    /// - `LOG_FORMAT`: `human` → Human; anything else (incl. unset and
    ///   unrecognized) → Json. Logging must always come up, so an unknown
    ///   value degrades to the default rather than refusing.
    /// - `DEPLOYMENT_TYPE`: `prod` → Prod; anything else → Dev (default dev).
    /// - filter: `RUST_LOG` if non-empty, else `info` (prod) / `debug` (dev).
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

    /// Read the three env vars and resolve. Infallible.
    ///
    /// The `deployment` it returns is resolved LENIENTLY for logging purposes
    /// (see [`LogConfig::resolve`]). It is NOT a §4.3 gate — a service needing
    /// one calls [`Deployment::from_env`], which returns `Err` on a
    /// set-but-invalid value.
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
        // explicit RUST_LOG wins
        assert_eq!(LogConfig::resolve(None, Some("prod"), Some("mycrate=trace")).filter, "mycrate=trace");
        // empty RUST_LOG is ignored -> deployment default
        assert_eq!(LogConfig::resolve(None, Some("prod"), Some("")).filter, "info");
        // deployment defaults
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
        // §4.3: typos must not become defaults. Each of these previously fell
        // through to Dev, silently enabling dev-only permissiveness under what
        // the operator believed was a prod deployment.
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
        // the property that matters: what we PRINT must be what the env var
        // ACCEPTS. Debug does not satisfy this — "Prod" is not a valid value.
        for d in [Deployment::Prod, Deployment::Dev] {
            assert_eq!(Deployment::parse(Some(d.as_str())).unwrap(), d);
            assert_eq!(d.to_string(), d.as_str());
        }
        assert!(Deployment::parse(Some(&format!("{:?}", Deployment::Prod))).is_err());
    }
}
