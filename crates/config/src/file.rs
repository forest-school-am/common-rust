//! Reading the optional TOML file into `(path, text)` pairs: tables are
//! nesting levels, every leaf is a scalar, every key must be in the schema.
//! Deciding WHICH file (`--config` / `<APP>_CONFIG`) is layers.rs; what a text
//! means is values.rs. A not-found file is not an error here — its caller
//! records that as `FileStatus::Missing`.

use std::collections::HashMap;
use std::io::ErrorKind;

use crate::args::HELP_FLAG;
use crate::path::Path;
use crate::schema::Field;
use crate::Refusal;

pub(crate) fn read(
    fields: &[Field],
    source: &str,
    file: &std::path::Path,
) -> Result<Option<Vec<(Path, String)>>, Refusal> {
    let shown = file.display().to_string();
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(
                Refusal::new(source, shown, "the path of a readable TOML file")
                    .with_detail(format!("cannot read: {e}")),
            )
        }
    };
    let table: toml::Table = toml::from_str(&text).map_err(|e| {
        let line = e
            .span()
            .map(|span| text[..span.start].lines().count().max(1))
            .map_or(String::new(), |line| format!(" (line {line})"));
        Refusal::new(source, shown.clone(), "a well-formed TOML file")
            .with_detail(format!("{}{line}", e.message()))
    })?;

    let by_key: HashMap<String, &Path> = fields.iter().map(|f| (f.toml(), &f.path)).collect();
    let mut found = Found::default();
    flatten(&table, String::new(), &by_key, &mut found);
    if !found.non_scalar.is_empty() {
        return Err(Refusal::new(
            source,
            shown,
            "a scalar (string, integer, float, boolean, datetime) for every key",
        )
        .with_detail(format!("not a scalar: {}", found.non_scalar.join(", "))));
    }
    if !found.unknown.is_empty() {
        return Err(Refusal::new(
            source,
            shown,
            format!("only the keys listed by {HELP_FLAG}"),
        )
        .with_detail(format!("unknown keys: {}", found.unknown.join(", "))));
    }
    Ok(Some(found.values))
}

#[derive(Default)]
struct Found {
    values: Vec<(Path, String)>,
    unknown: Vec<String>,
    non_scalar: Vec<String>,
}

fn flatten(
    table: &toml::Table,
    prefix: String,
    by_key: &HashMap<String, &Path>,
    found: &mut Found,
) {
    for (name, value) in table {
        let key = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        let text = match value {
            toml::Value::Table(inner) => {
                flatten(inner, key, by_key, found);
                continue;
            }
            toml::Value::Array(_) => {
                found.non_scalar.push(key);
                continue;
            }
            toml::Value::String(s) => s.clone(),
            toml::Value::Integer(i) => i.to_string(),
            toml::Value::Float(f) => f.to_string(),
            toml::Value::Boolean(b) => b.to_string(),
            toml::Value::Datetime(d) => d.to_string(),
        };
        match by_key.get(&key) {
            Some(path) => found.values.push(((*path).clone(), text)),
            None => found.unknown.push(key),
        }
    }
}
