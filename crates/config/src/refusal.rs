//! What a failed boot carries: the variable, its value, what was accepted and
//! the underlying error. Data and its rendering only — PRINTING the refusal
//! and exiting is `common_logging::refuse!`, because that is a log line.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The spelling the operator set: an env name, a flag, a file key.
    pub variable: String,
    pub value: String,
    pub accepted: String,
    pub detail: Option<String>,
}

impl Refusal {
    /// `variable` is `AsRef<str>` rather than `Into<String>` so that a `&&str`
    /// (a name matched out of a table by reference) still passes, as it did
    /// when the field was `&'static str`.
    pub fn new(
        variable: impl AsRef<str>,
        value: impl Into<String>,
        accepted: impl Into<String>,
    ) -> Self {
        Self {
            variable: variable.as_ref().to_owned(),
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

#[cfg(test)]
mod tests {
    use super::Refusal;

    #[test]
    fn the_rendering_names_the_variable_the_value_and_the_accepted_set() {
        let r = Refusal::new("DEPLOYMENT_TYPE", "prd", r#"one of ["prod", "dev"]"#);
        assert_eq!(
            r.to_string(),
            r#"DEPLOYMENT_TYPE="prd" is not valid — expected one of ["prod", "dev"]"#
        );
    }

    #[test]
    fn the_rendering_appends_a_detail_when_there_is_one() {
        let r = Refusal::new("RUST_LOG", "=", "comma-separated directives")
            .with_detail("invalid filter directive");
        assert!(r.to_string().ends_with("(invalid filter directive)"), "{r}");
        assert_eq!(r.detail.as_deref(), Some("invalid filter directive"));
    }

    #[test]
    fn a_variable_may_be_owned_borrowed_or_doubly_borrowed() {
        let owned = Refusal::new(format!("APP_{}", "X"), "", "");
        let borrowed = Refusal::new("APP_X", "", "");
        let table = ["APP_X"];
        let doubly = Refusal::new(table.iter().next().unwrap(), "", "");
        assert_eq!(owned, borrowed);
        assert_eq!(owned, doubly);
    }
}
