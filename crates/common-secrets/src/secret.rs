//! The redacting newtypes: the caller-supplied `AppPassword` and the fetched
//! `Secret`. Both carry a string that MUST NEVER reach a log or a formatter,
//! so redaction lives here once and both types share it; a type that can print
//! its inner value does not belong in this module.

macro_rules! redacting_newtype {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, PartialEq, Eq)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn expose(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("<redacted>")
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("<redacted>")
            }
        }
    };
}

redacting_newtype!(AppPassword);
redacting_newtype!(Secret);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_display_redact_but_expose_returns_the_value() {
        let s = Secret::new("hunter2");
        assert_eq!(s.expose(), "hunter2");
        assert_eq!(format!("{s:?}"), "<redacted>");
        assert_eq!(format!("{s}"), "<redacted>");
        assert!(!format!("{s:#?}").contains("hunter2"));

        let p = AppPassword::new("hunter2");
        assert_eq!(p.expose(), "hunter2");
        assert_eq!(format!("{p:?}"), "<redacted>");
        assert_eq!(format!("{p}"), "<redacted>");
    }
}
