//! Rendering only: the `--help` text (grouped by toml table, the three
//! spellings per line) and the `--print-config` dump (effective value and
//! origin per field, secrets masked). Nothing here reads a source or parses a
//! value; a new column is a change here, a new source is layers.rs.

use crate::args::{CONFIG_FLAG, HELP_FLAG, PRINT_CONFIG_FLAG};
use crate::layers::config_env;
use crate::schema::{Field, FileStatus, Kind, Presence};
use crate::values::{Values, MASK};

struct Row {
    flag: String,
    env: String,
    key: String,
    tail: String,
    help: &'static str,
}

pub(crate) fn help(app: &str, bin: &str, fields: &[Field]) -> String {
    let builtins = [
        Row {
            flag: format!("{CONFIG_FLAG} <path>"),
            env: config_env(app),
            key: String::new(),
            tail:
                "TOML file to read; a missing file is fine, an unreadable or malformed one refuses"
                    .into(),
            help: "",
        },
        Row {
            flag: HELP_FLAG.into(),
            env: String::new(),
            key: String::new(),
            tail: "print this text and exit".into(),
            help: "",
        },
        Row {
            flag: PRINT_CONFIG_FLAG.into(),
            env: String::new(),
            key: String::new(),
            tail: "print every value with its origin (secrets masked) and exit".into(),
            help: "",
        },
    ];
    let rows: Vec<(String, Row)> = fields
        .iter()
        .map(|f| {
            let flag = match f.kind {
                Kind::Bool => f.flag(),
                Kind::Text => format!("{} <value>", f.flag()),
            };
            let mut tail = match f.presence {
                Presence::Required => "required".to_string(),
                Presence::Default(d) => format!("default {d}"),
                Presence::Optional => "optional".to_string(),
            };
            if f.secret {
                tail.push_str(", secret");
            }
            (
                f.path.section(),
                Row {
                    flag,
                    env: f.env(app),
                    key: f.path.leaf().to_string(),
                    tail,
                    help: f.help,
                },
            )
        })
        .collect();

    let all = builtins.iter().chain(rows.iter().map(|(_, r)| r));
    let flag_w = all.clone().map(|r| r.flag.len()).max().unwrap_or(0);
    let env_w = all.clone().map(|r| r.env.len()).max().unwrap_or(0);
    let key_w = all.map(|r| r.key.len()).max().unwrap_or(0);
    let line = |r: &Row| {
        let mut s = format!(
            "  {:<flag_w$}  {:<env_w$}  {:<key_w$}  {}",
            r.flag, r.env, r.key, r.tail
        );
        if !r.help.is_empty() {
            s.push_str("\n      ");
            s.push_str(r.help);
        }
        s.push('\n');
        s
    };

    let mut out = format!(
        "{bin}: every value has a flag, an environment variable and a TOML key.\n\
         Later wins: defaults < config file < environment < arguments.\n\n"
    );
    for row in &builtins {
        out.push_str(&line(row));
    }
    let mut section: Option<&str> = None;
    for (sec, row) in &rows {
        if section != Some(sec.as_str()) {
            section = Some(sec);
            out.push('\n');
            if sec.is_empty() {
                out.push_str("top level\n");
            } else {
                out.push_str(&format!("[{sec}]\n"));
            }
        }
        out.push_str(&line(row));
    }
    out
}

pub(crate) fn print_config(values: &Values) -> String {
    let rows: Vec<(String, String, String)> = values
        .fields()
        .iter()
        .map(|f| {
            let key = f.toml();
            match values.get(&f.path) {
                Some(entry) => {
                    let shown = if f.secret {
                        MASK.to_string()
                    } else {
                        entry.text.clone()
                    };
                    (key, shown, entry.origin.to_string())
                }
                None => {
                    let why = match f.presence {
                        Presence::Required => "required",
                        _ => "optional",
                    };
                    (key, "<unset>".to_string(), why.to_string())
                }
            }
        })
        .collect();
    let key_w = rows.iter().map(|r| r.0.len()).max().unwrap_or(0);
    let value_w = rows.iter().map(|r| r.1.len()).max().unwrap_or(0);
    let mut out = match values.file() {
        FileStatus::NotConfigured => format!(
            "config file: none ({CONFIG_FLAG} or {} to set one)\n",
            config_env(values.app())
        ),
        FileStatus::Missing(path) => format!("config file: {} (not found)\n", path.display()),
        FileStatus::Read(path) => format!("config file: {}\n", path.display()),
    };
    for (key, value, origin) in rows {
        out.push_str(&format!("{key:<key_w$} = {value:<value_w$}  {origin}\n"));
    }
    out
}
