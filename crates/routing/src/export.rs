//! The handler descriptor data types and their `handlers.json` file. TS type
//! names are resolved here, through ts-rs, at export-test run time. Joining these
//! descriptors to the route table belongs in `generate`; extracting them from a
//! signature belongs in `common-routing-macros`.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const EXPORT_DIR: &str = "COMMON_ROUTING_EXPORT_DIR";
pub const HANDLERS_FILE: &str = "handlers.json";

pub fn dir() -> Option<PathBuf> {
    std::env::var_os(EXPORT_DIR).map(PathBuf::from)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Path,
    Query,
    Body,
    Multipart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub ts_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arg {
    pub name: String,
    pub kind: Kind,
    pub ts_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub positions: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<Field>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    Json(String),
    NoContent,
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handler {
    /// The join key: must equal the router-recorded [`crate::Registration::fqname`].
    pub fqname: String,
    pub name: String,
    pub args: Vec<Arg>,
    pub response: Response,
}

const TS_PRIMITIVES: &[&str] = &["string", "number", "bigint", "boolean", "null"];

fn split_top_level(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '<' | '[' | '{' | '(' => depth += 1,
            '>' | ']' | '}' | ')' => depth -= 1,
            ',' if depth == 0 => {
                let t = cur.trim();
                if !t.is_empty() {
                    out.push(t.to_owned());
                }
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    let t = cur.trim();
    if !t.is_empty() {
        out.push(t.to_owned());
    }
    out
}

pub(crate) fn parse_object_fields(inline: &str) -> Option<Vec<Field>> {
    let body = inline.trim().strip_prefix('{')?.strip_suffix('}')?;
    let mut fields = Vec::new();
    for entry in split_top_level(body) {
        let (name, ty) = entry.split_once(':')?;
        fields.push(Field {
            name: name.trim().trim_end_matches('?').to_owned(),
            ts_type: ty.trim().to_owned(),
        });
    }
    Some(fields)
}

impl Arg {
    pub fn path<T: TS>(name: &str) -> Self {
        let ts_type = T::name();
        let (positions, fields) =
            if let Some(inner) = ts_type.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                (Some(split_top_level(inner)), None)
            } else if TS_PRIMITIVES.contains(&ts_type.as_str()) {
                (None, None)
            } else {
                // ts-rs inlines a struct to `{ … }` and a newtype to its inner
                // primitive, which parse_object_fields returns None for.
                (None, parse_object_fields(&T::inline()))
            };
        Self {
            name: name.to_owned(),
            kind: Kind::Path,
            ts_type,
            positions,
            fields,
        }
    }

    pub fn query<T: TS>(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            kind: Kind::Query,
            ts_type: T::name(),
            positions: None,
            fields: None,
        }
    }

    pub fn body<T: TS>(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            kind: Kind::Body,
            ts_type: T::name(),
            positions: None,
            fields: None,
        }
    }

    pub fn multipart(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            kind: Kind::Multipart,
            ts_type: "FormData".to_owned(),
            positions: None,
            fields: None,
        }
    }
}

impl Response {
    pub fn json<T: TS>() -> Self {
        Self::Json(T::name())
    }
}

/// Under an exclusive file lock: the export tests run in parallel and each
/// appends its own handler to the shared `handlers.json`.
pub fn append(dir: &Path, handler: Handler) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(HANDLERS_FILE))?;
    file.lock()?;
    let result = append_locked(&mut file, handler);
    file.unlock()?;
    result
}

fn append_locked(file: &mut File, handler: Handler) -> std::io::Result<()> {
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    let mut handlers: Vec<Handler> = if text.trim().is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(&text)?
    };
    handlers.retain(|h| h.fqname != handler.fqname);
    handlers.push(handler);
    handlers.sort_by(|a, b| a.fqname.cmp(&b.fqname));
    let mut out = serde_json::to_string_pretty(&handlers)?;
    out.push('\n');
    file.set_len(0)?;
    file.rewind()?;
    file.write_all(out.as_bytes())
}

pub fn read(path: &Path) -> std::io::Result<Vec<Handler>> {
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_fields_parse_flat_and_nested_types() {
        let fields = parse_object_fields("{ id: string, number?: number, tags: Array<string>, }")
            .expect("object");
        assert_eq!(
            fields,
            vec![
                Field {
                    name: "id".into(),
                    ts_type: "string".into()
                },
                Field {
                    name: "number".into(),
                    ts_type: "number".into()
                },
                Field {
                    name: "tags".into(),
                    ts_type: "Array<string>".into()
                },
            ]
        );
        assert_eq!(
            parse_object_fields("{ m: { [key in string]?: number }, n: [string, number], }")
                .expect("object")
                .len(),
            2
        );
        assert!(parse_object_fields("string").is_none());
    }

    #[test]
    fn path_shapes_from_ts_rs() {
        let s = Arg::path::<String>("id");
        assert_eq!(
            (
                s.ts_type.as_str(),
                s.positions.is_none(),
                s.fields.is_none()
            ),
            ("string", true, true)
        );
        // i64 is `bigint` to ts-rs 10; an app that wants `number` uses its
        // own newtype with `#[ts(type = "number")]`, as it does for DTOs.
        let t = Arg::path::<(String, i64)>("p");
        assert_eq!(t.ts_type, "[string, bigint]");
        assert_eq!(
            t.positions.as_deref(),
            Some(&["string".to_owned(), "bigint".to_owned()][..])
        );

        #[derive(TS)]
        #[allow(dead_code)]
        struct RunPath {
            id: String,
            #[ts(type = "number")]
            number: i64,
        }
        let r = Arg::path::<RunPath>("p");
        assert_eq!(r.ts_type, "RunPath");
        assert_eq!(
            r.fields
                .as_deref()
                .map(|f| f.iter().map(|f| f.name.as_str()).collect::<Vec<_>>()),
            Some(vec!["id", "number"])
        );

        #[derive(TS)]
        #[allow(dead_code)]
        struct TaskId(String);
        let n = Arg::path::<TaskId>("id");
        assert_eq!((n.ts_type.as_str(), n.fields.is_none()), ("TaskId", true));
    }

    #[test]
    fn append_replaces_by_fqname_and_sorts() {
        let dir =
            std::env::temp_dir().join(format!("common-routing-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let h = |fq: &str| Handler {
            fqname: fq.into(),
            name: fq.rsplit("::").next().unwrap().into(),
            args: vec![],
            response: Response::NoContent,
        };
        append(&dir, h("app::web::b")).unwrap();
        append(&dir, h("app::web::a")).unwrap();
        append(&dir, h("app::web::b")).unwrap();
        let got = read(&dir.join(HANDLERS_FILE)).unwrap();
        assert_eq!(
            got.iter().map(|h| h.fqname.as_str()).collect::<Vec<_>>(),
            vec!["app::web::a", "app::web::b"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
