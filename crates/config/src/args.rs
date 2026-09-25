//! The argv grammar: `--k=v`, `--k v`, bare boolean `--k`, `--help`,
//! `--print-config`, `--config PATH`; anything else refuses. Tokenising and
//! flag lookup only — what a value MEANS (type, default) is values.rs, and
//! where args rank against env and file is layers.rs.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::path::Path;
use crate::schema::{Field, Kind};
use crate::Refusal;

pub(crate) const CONFIG_FLAG: &str = "--config";
pub(crate) const HELP_FLAG: &str = "--help";
pub(crate) const HELP_FLAG_SHORT: &str = "-h";
pub(crate) const PRINT_CONFIG_FLAG: &str = "--print-config";

#[derive(Debug, Default)]
pub(crate) struct Parsed {
    pub help: bool,
    pub print_config: bool,
    pub config: Option<PathBuf>,
    /// `(field, text, flag as written)` in argv order.
    pub values: Vec<(Path, String, String)>,
}

pub(crate) fn parse(fields: &[Field], args: &[String]) -> Result<Parsed, Refusal> {
    let by_flag: HashMap<String, &Field> = fields.iter().map(|f| (f.flag(), f)).collect();
    let mut parsed = Parsed::default();
    let mut unknown = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let token = &args[i];
        i += 1;
        let (name, inline) = match token.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value)),
            _ => (token.as_str(), None),
        };
        match name {
            HELP_FLAG | HELP_FLAG_SHORT => parsed.help = true,
            PRINT_CONFIG_FLAG => parsed.print_config = true,
            CONFIG_FLAG => {
                let value = take_value(name, inline, args, &mut i)?;
                parsed.config = Some(PathBuf::from(value));
            }
            _ => match by_flag.get(name) {
                Some(field) if field.kind == Kind::Bool => {
                    let text = inline.map_or_else(|| "true".to_string(), str::to_string);
                    parsed
                        .values
                        .push((field.path.clone(), text, name.to_string()));
                }
                Some(field) => {
                    let text = take_value(name, inline, args, &mut i)?;
                    parsed
                        .values
                        .push((field.path.clone(), text, name.to_string()));
                }
                None => unknown.push(token.clone()),
            },
        }
    }
    if parsed.help {
        return Ok(parsed);
    }
    if !unknown.is_empty() {
        return Err(Refusal::new(
            "arguments",
            unknown.join(" "),
            format!("flags listed by {HELP_FLAG}"),
        )
        .with_detail(format!("unknown: {}", unknown.join(", "))));
    }
    Ok(parsed)
}

fn take_value(
    name: &str,
    inline: Option<&str>,
    args: &[String],
    i: &mut usize,
) -> Result<String, Refusal> {
    if let Some(value) = inline {
        return Ok(value.to_string());
    }
    match args.get(*i) {
        Some(next) if !next.starts_with("--") => {
            *i += 1;
            Ok(next.clone())
        }
        _ => Err(Refusal::new(
            name,
            "",
            format!("a value: {name}=<value> or {name} <value>"),
        )
        .with_detail("flag given without a value")),
    }
}
