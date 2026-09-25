//! Source precedence: defaults < file < env < args, later wins PER FIELD, and
//! every entry stamped with its `Origin`. Reading argv is args.rs and reading
//! the file is file.rs; this file only decides where the file is and who
//! overrides whom. A fifth source would be added here and nowhere else.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use crate::args::{Parsed, CONFIG_FLAG};
use crate::path::Path;
use crate::schema::{Entry, Field, FileStatus, Origin, Presence};
use crate::values::Values;
use crate::Refusal;

pub(crate) fn config_env(app: &str) -> String {
    format!("{app}_CONFIG")
}

pub(crate) fn merge(
    app: &'static str,
    fields: Vec<Field>,
    args: Parsed,
    env: &[(String, String)],
) -> Result<Values, Refusal> {
    let env: HashMap<&str, &str> = env
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let mut entries: BTreeMap<Path, Entry> = BTreeMap::new();

    for field in &fields {
        if let Presence::Default(text) = field.presence {
            entries.insert(
                field.path.clone(),
                Entry {
                    text: text.to_string(),
                    origin: Origin::Default,
                },
            );
        }
    }

    let file = match args.config {
        Some(path) => Some((path, CONFIG_FLAG.to_string())),
        None => {
            let name = config_env(app);
            env.get(name.as_str())
                .map(|path| (PathBuf::from(path), name))
        }
    };
    let status = match file {
        None => FileStatus::NotConfigured,
        Some((path, source)) => {
            for (field_path, text) in crate::file::read(&fields, &source, &path)? {
                entries.insert(
                    field_path,
                    Entry {
                        text,
                        origin: Origin::File(path.clone()),
                    },
                );
            }
            FileStatus::Read(path)
        }
    };

    for field in &fields {
        let name = field.env(app);
        if let Some(value) = env.get(name.as_str()) {
            entries.insert(
                field.path.clone(),
                Entry {
                    text: value.to_string(),
                    origin: Origin::Env(name),
                },
            );
        }
    }

    for (path, text, flag) in args.values {
        entries.insert(
            path,
            Entry {
                text,
                origin: Origin::Arg(flag),
            },
        );
    }

    Ok(Values::new(app, fields, entries, status))
}
