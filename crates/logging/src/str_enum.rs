//! The `str_enum!` macro: enums whose variants are mirrored as strings in an
//! env var, a log field or on a wire. Each spelling is written once and both
//! directions are generated from it. A value that never leaves the process as
//! text is a plain enum, not this.

macro_rules! str_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($variant:ident = $text:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        $vis enum $name {
            $($variant),+
        }

        impl $name {
            #[allow(dead_code)]
            $vis const VALUES: &'static [&'static str] = &[$($text),+];

            $vis fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            $vis fn try_from_str(text: &str) -> ::core::option::Option<Self> {
                match text {
                    $($text => ::core::option::Option::Some(Self::$variant),)+
                    _ => ::core::option::Option::None,
                }
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

pub(crate) use str_enum;

pub(crate) fn or_list(values: &[&str]) -> String {
    let quoted: Vec<String> = values.iter().map(|v| format!("{v:?}")).collect();
    match quoted.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    str_enum! {
        enum Colour {
            Red = "red",
            SeaGreen = "sea-green",
        }
    }

    #[test]
    fn every_value_round_trips_and_nothing_else_parses() {
        for value in Colour::VALUES {
            let parsed = Colour::try_from_str(value).expect("a listed value parses");
            assert_eq!(parsed.as_str(), *value);
            assert_eq!(parsed.to_string(), *value);
        }
        assert_eq!(Colour::VALUES, &["red", "sea-green"]);
        for bad in ["Red", "RED", "seagreen", "sea green", ""] {
            assert!(Colour::try_from_str(bad).is_none(), "{bad:?} must not parse");
        }
    }

    #[test]
    fn or_list_quotes_and_joins() {
        assert_eq!(or_list(&[]), "");
        assert_eq!(or_list(&["dev"]), "\"dev\"");
        assert_eq!(or_list(Colour::VALUES), "\"red\" or \"sea-green\"");
        assert_eq!(or_list(&["a", "b", "c"]), "\"a\", \"b\" or \"c\"");
    }
}
