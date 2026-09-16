//! What the derive registers per leaf — `Field`, its presence rule, its kind —
//! and the provenance stamps a merged value carries. Pure data, no parsing:
//! turning an `Entry`'s text into a Rust value belongs in values.rs, and the
//! spelling rules themselves stay in path.rs (a `Field` only applies its
//! legacy overrides on top of them).

use std::fmt;
use std::path::PathBuf;

use crate::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub path: Path,
    pub help: &'static str,
    pub presence: Presence,
    pub secret: bool,
    pub kind: Kind,
    /// Legacy bare env name used INSTEAD of the generated one.
    pub env: Option<&'static str>,
    /// Legacy flag name (no dashes) used INSTEAD of the generated one.
    pub flag: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Required,
    Default(&'static str),
    Optional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Text,
}

impl Field {
    pub fn flag(&self) -> String {
        match self.flag {
            Some(name) => format!("--{name}"),
            None => self.path.flag(),
        }
    }

    pub fn env(&self, app: &str) -> String {
        match self.env {
            Some(name) => name.to_string(),
            None => self.path.env(app),
        }
    }

    pub fn toml(&self) -> String {
        self.path.dotted()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Default,
    File(PathBuf),
    Env(String),
    Arg(String),
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Origin::Default => f.write_str("default"),
            Origin::File(path) => write!(f, "file {}", path.display()),
            Origin::Env(name) => write!(f, "env {name}"),
            Origin::Arg(flag) => write!(f, "arg {flag}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub text: String,
    pub origin: Origin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    NotConfigured,
    Missing(PathBuf),
    Read(PathBuf),
}
