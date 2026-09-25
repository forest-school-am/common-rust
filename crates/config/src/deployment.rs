//! The deployment class: prod or dev. Only the value and its parse live here;
//! what a class IMPLIES (prod-required / dev-only options, the default log
//! verbosity) is each consumer's own rule.

use std::str::FromStr;

use strum::{AsRefStr, Display, EnumString, VariantNames};

use crate::Refusal;

pub const DEPLOYMENT_VARIABLE: &str = "DEPLOYMENT_TYPE";

/// The `accepted` text of a refused `DEPLOYMENT_TYPE`, shared by `parse` and
/// the derived root field so the two spell it alike.
pub const DEPLOYMENT_ACCEPTED: &str = r#"one of ["prod", "dev"]"#;

pub const DEPLOYMENT_HELP: &str =
    "Deployment class: prod or dev. Sets the default log verbosity; which options are \
     prod-required or dev-only is the binary's own rule.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr, Display, EnumString, VariantNames)]
pub enum Deployment {
    #[strum(serialize = "prod")]
    Prod,
    #[strum(serialize = "dev")]
    Dev,
}

impl Deployment {
    /// Unset is refused like any other invalid value: there is no default class.
    pub fn parse(value: Option<&str>) -> Result<Self, Refusal> {
        let Some(text) = value else {
            return Err(
                Refusal::new(DEPLOYMENT_VARIABLE, "unset", DEPLOYMENT_ACCEPTED)
                    .with_detail("required and unset"),
            );
        };
        Deployment::from_str(text)
            .map_err(|_| Refusal::new(DEPLOYMENT_VARIABLE, text, DEPLOYMENT_ACCEPTED))
    }

    pub fn from_env() -> Result<Self, Refusal> {
        Self::parse(std::env::var(DEPLOYMENT_VARIABLE).ok().as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_valid_values_parse() {
        assert_eq!(Deployment::parse(Some("dev")).unwrap(), Deployment::Dev);
        assert_eq!(Deployment::parse(Some("prod")).unwrap(), Deployment::Prod);
    }

    #[test]
    fn unset_refuses_rather_than_defaulting() {
        let r = Deployment::parse(None).expect_err("unset DEPLOYMENT_TYPE must refuse");
        assert_eq!(r.variable, DEPLOYMENT_VARIABLE);
        assert_eq!(r.value, "unset");
        assert_eq!(r.accepted, DEPLOYMENT_ACCEPTED);
    }

    #[test]
    fn set_but_invalid_refuses_rather_than_defaulting_to_dev() {
        for bad in ["Prod", "PROD", "Dev", "production", "prd", ""] {
            let r = Deployment::parse(Some(bad))
                .expect_err("DEPLOYMENT_TYPE set-but-invalid must be refused, not treated as dev");
            assert_eq!(r.variable, DEPLOYMENT_VARIABLE);
            assert_eq!(r.value, bad);
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
        assert_eq!(
            r.to_string(),
            r#"DEPLOYMENT_TYPE="prd" is not valid — expected one of ["prod", "dev"]"#
        );
    }

    #[test]
    fn the_declared_spellings_are_the_ones_on_the_wire() {
        assert_eq!(Deployment::VARIANTS, &["prod", "dev"]);
        assert_eq!(Deployment::Prod.as_ref(), "prod");
        assert_eq!(Deployment::Dev.to_string(), "dev");
    }

    #[test]
    fn as_str_round_trips_through_parse() {
        for d in [Deployment::Prod, Deployment::Dev] {
            assert_eq!(Deployment::parse(Some(d.as_ref())).unwrap(), d);
            assert_eq!(d.to_string(), d.as_ref());
        }
        assert!(Deployment::parse(Some(&format!("{:?}", Deployment::Prod))).is_err());
    }

    #[test]
    fn the_accepted_constant_is_built_from_the_variants() {
        assert_eq!(
            DEPLOYMENT_ACCEPTED,
            format!("one of {:?}", Deployment::VARIANTS)
        );
    }
}
