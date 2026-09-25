//! The merged `(path → text, origin)` table and the ONE typed parse of each
//! leaf, with the two refusals a leaf raises: required-and-unset, wrong type.
//! Building the table from sources is layers.rs; rendering it is help.rs. A
//! refusal about a SOURCE (unreadable file, unknown flag) belongs with that
//! source, not here.

use std::collections::BTreeMap;
use std::fmt::Display;
use std::str::FromStr;

use crate::Refusal;

use crate::path::Path;
use crate::schema::{Entry, Field, FileStatus, Origin};

pub struct Values {
    app: &'static str,
    fields: Vec<Field>,
    entries: BTreeMap<Path, Entry>,
    file: FileStatus,
}

pub(crate) const BOOL_ACCEPTED: &str = "true, false, 1, 0, yes, no, on or off";

pub(crate) const MASK: &str = "****";

impl Values {
    pub(crate) fn new(
        app: &'static str,
        fields: Vec<Field>,
        entries: BTreeMap<Path, Entry>,
        file: FileStatus,
    ) -> Self {
        Self {
            app,
            fields,
            entries,
            file,
        }
    }

    /// Values that arrived under a BARE spelling (`env = …` / `flag = …`), each
    /// naming the pattern spelling it will move to (config.6, user 2026-09-25).
    /// Both spellings work today; this is only what the boot log says about it,
    /// and it is computed from the entries so it names what was ACTUALLY used —
    /// a field with a bare spelling that took its value from the file or a
    /// default says nothing.
    pub fn deprecations(&self) -> Vec<String> {
        let mut out = Vec::new();
        for field in &self.fields {
            let Some(entry) = self.entries.get(&field.path) else {
                continue;
            };
            match &entry.origin {
                crate::schema::Origin::Env(used)
                    if field.env.is_some()
                        && *used == field.env(self.app)
                        && *used != field.path.env(self.app)
                        && !crate::schema::FLEET_WIDE_SPELLINGS.contains(&used.as_str()) =>
                {
                    out.push(format!(
                        "{used} is a bare env spelling and will be removed: use {} \
                         (both work today)",
                        field.path.env(self.app)
                    ));
                }
                crate::schema::Origin::Arg(used)
                    if field.flag.is_some()
                        && *used == field.flag()
                        && *used != field.path.flag() =>
                {
                    out.push(format!(
                        "{used} is a bare flag and will be removed: use {} (both work today)",
                        field.path.flag()
                    ));
                }
                _ => {}
            }
        }
        out
    }

    pub fn app(&self) -> &'static str {
        self.app
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    pub fn file(&self) -> &FileStatus {
        &self.file
    }

    pub fn get(&self, path: &Path) -> Option<&Entry> {
        self.entries.get(path)
    }

    pub fn leaf<T>(&self, path: &Path, accepted: Option<&str>) -> Result<T, Refusal>
    where
        T: FromStr,
        T::Err: Display,
    {
        let field = self.field(path);
        match self.entries.get(path) {
            Some(entry) => parse_text(field, entry, accepted),
            None => Err(missing(field, self.app)),
        }
    }

    pub fn leaf_opt<T>(&self, path: &Path, accepted: Option<&str>) -> Result<Option<T>, Refusal>
    where
        T: FromStr,
        T::Err: Display,
    {
        let field = self.field(path);
        match self.entries.get(path) {
            Some(entry) => parse_text(field, entry, accepted).map(Some),
            None => Ok(None),
        }
    }

    pub fn bool(&self, path: &Path) -> Result<bool, Refusal> {
        let field = self.field(path);
        match self.entries.get(path) {
            Some(entry) => parse_bool_entry(field, entry),
            None => Err(missing(field, self.app)),
        }
    }

    pub fn bool_opt(&self, path: &Path) -> Result<Option<bool>, Refusal> {
        let field = self.field(path);
        match self.entries.get(path) {
            Some(entry) => parse_bool_entry(field, entry).map(Some),
            None => Ok(None),
        }
    }

    fn field(&self, path: &Path) -> &Field {
        self.fields
            .iter()
            .find(|f| &f.path == path)
            .unwrap_or_else(|| {
                panic!("{path} was read by from_values but never registered by schema")
            })
    }
}

pub(crate) fn parse_bool(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_bool_entry(field: &Field, entry: &Entry) -> Result<bool, Refusal> {
    parse_bool(&entry.text).ok_or_else(|| {
        wrong_type(
            field,
            entry,
            BOOL_ACCEPTED.to_string(),
            "not a boolean".to_string(),
        )
    })
}

fn parse_text<T>(field: &Field, entry: &Entry, accepted: Option<&str>) -> Result<T, Refusal>
where
    T: FromStr,
    T::Err: Display,
{
    entry.text.parse::<T>().map_err(|e| {
        let accepted = accepted
            .map(str::to_string)
            .unwrap_or_else(|| format!("a {}", short_type_name(std::any::type_name::<T>())));
        wrong_type(field, entry, accepted, e.to_string())
    })
}

fn short_type_name(name: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() || c == '_' || c == ':' {
            word.push(c);
        } else {
            out.push_str(word.rsplit("::").next().unwrap_or(&word));
            word.clear();
            out.push(c);
        }
    }
    out.push_str(word.rsplit("::").next().unwrap_or(&word));
    out
}

fn missing(field: &Field, app: &str) -> Refusal {
    let env = field.env(app);
    let flag = field.flag();
    let place = match field.path.section().as_str() {
        "" => "at the top level of the config file".to_string(),
        section => format!("under [{section}] in the config file"),
    };
    Refusal::new(
        env.clone(),
        "unset",
        format!(
            "a value: {env} in the environment, {flag} on the command line, or `{} = …` {place}",
            field.path.leaf()
        ),
    )
    .with_detail("required and unset")
}

fn wrong_type(field: &Field, entry: &Entry, accepted: String, error: String) -> Refusal {
    let source = match &entry.origin {
        Origin::Env(name) => name.clone(),
        Origin::Arg(flag) => flag.clone(),
        Origin::File(path) => format!("{}: {}", path.display(), field.toml()),
        Origin::Default => format!("default of {}", field.toml()),
    };
    let shown = if field.secret {
        MASK.to_string()
    } else {
        entry.text.clone()
    };
    Refusal::new(source, shown, accepted).with_detail(format!("set by {}: {error}", entry.origin))
}

#[cfg(test)]
mod tests {
    use super::short_type_name;

    #[test]
    fn short_type_name_strips_every_module_path() {
        assert_eq!(
            short_type_name("alloc::vec::Vec<alloc::string::String>"),
            "Vec<String>"
        );
        assert_eq!(short_type_name("u64"), "u64");
        assert_eq!(
            short_type_name("core::net::socket_addr::SocketAddr"),
            "SocketAddr"
        );
    }
}
