//! `client.ts` from `routes.json` + `handlers.json`, joined by fqname.
//!
//! One plain function per handler:
//!
//! ```ts
//! export function run(id: string, number: number): Promise<RunDetail> {
//!   return call<RunDetail>("GET", `/api/tasks/${enc(id)}/runs/${enc(number)}`);
//! }
//! ```
//!
//! Path params come first, in TEMPLATE order (a tuple payload by position, a
//! struct payload by field name, a primitive as the one param), then `query`,
//! then `body`. Nothing is joined at run time: the browser gets this file plus
//! the transport it imports. A `Link` handler gets `<name>Url(...)` only.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::export::{self, Handler, Kind, Response};
use crate::manifest::Registration;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("handler {0} has #[client] but no route registered it (is it mounted through the recording `.get(path, handler)` form?)")]
    HandlerWithoutRoute(String),
    #[error("route {method} {path} → {fqname} has no #[client] descriptor (annotate the handler, or exclude the route in Options::include)")]
    RouteWithoutHandler {
        method: String,
        path: String,
        fqname: String,
    },
    #[error(
        "handler {0} is mounted on more than one route; the client needs one path per function"
    )]
    HandlerMountedTwice(String),
    #[error("handler {fqname}: {detail}")]
    Shape { fqname: String, detail: String },
    #[error("handlers {0} and {1} would both become `{2}` in TypeScript")]
    NameClash(String, String, String),
}

/// What the generator can be told.
pub struct Options {
    /// The module specifier `client.ts` imports `call` from.
    pub transport: String,
    /// The module specifier the DTO types are imported from.
    pub types: String,
    /// Which routes must have a client function. A route this rejects is
    /// left out silently; a route it accepts without a descriptor is an error.
    pub include: Box<dyn Fn(&Registration) -> bool>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            transport: "./client-call".to_owned(),
            types: "./index".to_owned(),
            include: Box::new(|_| true),
        }
    }
}

impl Options {
    pub fn transport(mut self, specifier: &str) -> Self {
        self.transport = specifier.to_owned();
        self
    }

    pub fn types(mut self, specifier: &str) -> Self {
        self.types = specifier.to_owned();
        self
    }

    pub fn include(mut self, f: impl Fn(&Registration) -> bool + 'static) -> Self {
        self.include = Box::new(f);
        self
    }

    /// Reads the two tables, writes `out_ts`, returns the number of
    /// functions written.
    pub fn generate(
        &self,
        routes_json: &Path,
        handlers_json: &Path,
        out_ts: &Path,
    ) -> Result<usize, Error> {
        let routes: Vec<Registration> =
            serde_json::from_str(&std::fs::read_to_string(routes_json)?)?;
        let handlers = export::read(handlers_json)?;
        let (text, count) = render(&routes, &handlers, self)?;
        std::fs::write(out_ts, text)?;
        Ok(count)
    }
}

/// [`Options::generate`] with the defaults.
pub fn generate_client(
    routes_json: &Path,
    handlers_json: &Path,
    out_ts: &Path,
) -> Result<usize, Error> {
    Options::default().generate(routes_json, handlers_json, out_ts)
}

/// `task_delete_packed` → `taskDeletePacked`.
pub fn camel(snake: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for ch in snake.chars() {
        if ch == '_' {
            upper = true;
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// The exported type names a TS type expression refers to: capitalised
/// identifiers that are not TypeScript's own generics. ts-rs spells
/// primitives in lower case, so this is exactly its named types.
fn named_types(ts: &str, into: &mut BTreeSet<String>) {
    const BUILTIN: &[&str] = &[
        "Array", "Record", "Partial", "Map", "Set", "Promise", "Date",
    ];
    let mut cur = String::new();
    for ch in ts.chars().chain(std::iter::once(' ')) {
        if ch.is_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else {
            if cur.chars().next().is_some_and(|c| c.is_uppercase())
                && !BUILTIN.contains(&cur.as_str())
            {
                into.insert(cur.clone());
            }
            cur.clear();
        }
    }
}

const RESERVED: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "let",
    "static",
    "implements",
    "interface",
    "package",
    "private",
    "protected",
    "public",
    "await",
    "query",
    "body",
];

struct Fn_ {
    name: String,
    params: Vec<(String, String)>,
    url: String,
    method: String,
    has_query: bool,
    has_body: bool,
    response: Response,
    fqname: String,
    path: String,
}

fn plan(reg: &Registration, h: &Handler) -> Result<Fn_, Error> {
    let shape = |detail: String| Error::Shape {
        fqname: h.fqname.clone(),
        detail,
    };
    let path_args: Vec<&export::Arg> = h.args.iter().filter(|a| a.kind == Kind::Path).collect();
    if path_args.len() > 1 {
        return Err(shape("more than one Path extractor".into()));
    }
    // Template params, in template order, each with its TS type.
    let mut params: Vec<(String, String)> = Vec::new();
    match path_args.first() {
        None => {
            if !reg.path_params.is_empty() {
                return Err(shape(format!(
                    "the route {} has params {:?} but the handler takes no Path<T>",
                    reg.path, reg.path_params
                )));
            }
        }
        Some(arg) => {
            if let Some(fields) = &arg.fields {
                for p in &reg.path_params {
                    let Some(f) = fields.iter().find(|f| &f.name == p) else {
                        return Err(shape(format!(
                            "path param {{{p}}} of {} is not a field of {}",
                            reg.path, arg.ts_type
                        )));
                    };
                    params.push((p.clone(), f.ts_type.clone()));
                }
            } else {
                let types: Vec<String> = match &arg.positions {
                    Some(p) => p.clone(),
                    None => vec![arg.ts_type.clone()],
                };
                if types.len() != reg.path_params.len() {
                    return Err(shape(format!(
                        "the route {} has {} params {:?} but Path<{}> carries {}",
                        reg.path,
                        reg.path_params.len(),
                        reg.path_params,
                        arg.ts_type,
                        types.len()
                    )));
                }
                for (p, t) in reg.path_params.iter().zip(types) {
                    params.push((p.clone(), t));
                }
            }
        }
    }
    for (p, _) in &params {
        if RESERVED.contains(&p.as_str()) {
            return Err(shape(format!(
                "path param {{{p}}} is not usable as a TypeScript parameter name"
            )));
        }
    }
    let query = h
        .args
        .iter()
        .filter(|a| a.kind == Kind::Query)
        .collect::<Vec<_>>();
    let body = h
        .args
        .iter()
        .filter(|a| matches!(a.kind, Kind::Body | Kind::Multipart))
        .collect::<Vec<_>>();
    if query.len() > 1 || body.len() > 1 {
        return Err(shape(
            "more than one Query, or more than one body, extractor".into(),
        ));
    }
    if let Some(q) = query.first() {
        params.push(("query".into(), q.ts_type.clone()));
    }
    if let Some(b) = body.first() {
        params.push(("body".into(), b.ts_type.clone()));
    }
    // The URL: the template with each `{p}` replaced by `${enc(p)}`.
    let mut is_template = false;
    let url = reg
        .path
        .split('/')
        .map(
            |seg| match seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                Some(inner) if !inner.starts_with('{') => {
                    is_template = true;
                    format!("${{enc({})}}", inner.trim_start_matches('*'))
                }
                _ => seg.to_owned(),
            },
        )
        .collect::<Vec<_>>()
        .join("/");
    let url = if is_template {
        format!("`{url}`")
    } else {
        format!("\"{url}\"")
    };
    Ok(Fn_ {
        name: camel(&h.name),
        params,
        url,
        method: reg.method.clone(),
        has_query: !query.is_empty(),
        has_body: !body.is_empty(),
        response: h.response.clone(),
        fqname: h.fqname.clone(),
        path: reg.path.clone(),
    })
}

fn render(
    routes: &[Registration],
    handlers: &[Handler],
    opts: &Options,
) -> Result<(String, usize), Error> {
    let mut by_fqname: BTreeMap<&str, &Registration> = BTreeMap::new();
    for r in routes {
        if by_fqname.insert(r.fqname.as_str(), r).is_some() {
            return Err(Error::HandlerMountedTwice(r.fqname.clone()));
        }
    }
    let described: BTreeSet<&str> = handlers.iter().map(|h| h.fqname.as_str()).collect();
    for r in routes {
        if (opts.include)(r) && !described.contains(r.fqname.as_str()) {
            return Err(Error::RouteWithoutHandler {
                method: r.method.clone(),
                path: r.path.clone(),
                fqname: r.fqname.clone(),
            });
        }
    }
    let mut fns = Vec::new();
    for h in handlers {
        let Some(reg) = by_fqname.get(h.fqname.as_str()) else {
            return Err(Error::HandlerWithoutRoute(h.fqname.clone()));
        };
        fns.push(plan(reg, h)?);
    }
    fns.sort_by(|a, b| (&a.path, &a.method).cmp(&(&b.path, &b.method)));
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for f in &fns {
        let ts_name = match f.response {
            Response::Link => format!("{}Url", f.name),
            _ => f.name.clone(),
        };
        if let Some(prev) = seen.insert(ts_name.clone(), f.fqname.clone()) {
            return Err(Error::NameClash(prev, f.fqname.clone(), ts_name));
        }
    }

    let mut imports = BTreeSet::new();
    for f in &fns {
        for (_, t) in &f.params {
            named_types(t, &mut imports);
        }
        if let Response::Json(t) = &f.response {
            named_types(t, &mut imports);
        }
    }
    imports.remove("FormData");

    let mut out = String::new();
    out.push_str(
        "// Generated by common-routing from routes.json and handlers.json — do not edit.\n",
    );
    out.push_str("// One function per #[client] handler; the path template lives in Rust only.\n");
    out.push_str(&format!("import {{ call }} from \"{}\";\n", opts.transport));
    if !imports.is_empty() {
        out.push_str("import type {\n");
        for t in &imports {
            out.push_str(&format!("  {t},\n"));
        }
        out.push_str(&format!("}} from \"{}\";\n", opts.types));
    }
    out.push_str(
        "\nconst enc = (v: string | number | boolean) => encodeURIComponent(String(v));\n",
    );
    for f in &fns {
        let sig = f
            .params
            .iter()
            .map(|(n, t)| format!("{n}: {t}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "\n/** {} {} — {} */\n",
            f.method, f.path, f.fqname
        ));
        match &f.response {
            Response::Link => {
                out.push_str(&format!(
                    "export function {}Url({sig}): string {{\n  return {};\n}}\n",
                    f.name, f.url
                ));
            }
            resp => {
                let ret = match resp {
                    Response::Json(t) => t.clone(),
                    _ => "void".to_owned(),
                };
                let mut call_args = vec![format!("\"{}\"", f.method), f.url.clone()];
                if f.has_query || f.has_body {
                    call_args.push(if f.has_query {
                        "query".into()
                    } else {
                        "undefined".into()
                    });
                }
                if f.has_body {
                    call_args.push("body".into());
                }
                out.push_str(&format!(
                    "export function {}({sig}): Promise<{ret}> {{\n  return call<{ret}>({});\n}}\n",
                    f.name,
                    call_args.join(", ")
                ));
            }
        }
    }
    Ok((out, fns.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{Arg, Field};
    use crate::manifest::parse_path_params;

    fn reg(method: &str, path: &str, fq: &str) -> Registration {
        Registration {
            fqname: fq.into(),
            method: method.into(),
            path: path.into(),
            path_params: parse_path_params(path),
        }
    }

    fn handler(fq: &str, args: Vec<Arg>, response: Response) -> Handler {
        Handler {
            fqname: fq.into(),
            name: fq.rsplit("::").next().unwrap().into(),
            args,
            response,
        }
    }

    fn arg(kind: Kind, ts: &str) -> Arg {
        Arg {
            name: "x".into(),
            kind,
            ts_type: ts.into(),
            positions: None,
            fields: None,
        }
    }

    #[test]
    fn camel_case() {
        assert_eq!(camel("task_delete_packed"), "taskDeletePacked");
        assert_eq!(camel("run"), "run");
    }

    #[test]
    fn functions_bind_path_then_query_then_body() {
        let routes = vec![
            reg("GET", "/api/tasks/{id}/runs/{number}", "app::web::run"),
            reg("GET", "/api/runs", "app::web::runs"),
            reg("POST", "/api/tasks/create", "app::web::task_create"),
            reg(
                "POST",
                "/api/batches/{name}/upload",
                "app::web::batch_upload",
            ),
            reg("POST", "/api/tasks/{id}/delete", "app::web::task_delete"),
            reg(
                "GET",
                "/api/batches/{name}/download/{version}",
                "app::web::batch_download",
            ),
            reg("GET", "/api/things/{id}", "app::web::thing"),
        ];
        let handlers = vec![
            handler(
                "app::web::run",
                vec![Arg {
                    positions: Some(vec!["string".into(), "number".into()]),
                    ..arg(Kind::Path, "[string, number]")
                }],
                Response::Json("RunDetail".into()),
            ),
            handler(
                "app::web::runs",
                vec![arg(Kind::Query, "RunQuery")],
                Response::Json("RunPage".into()),
            ),
            handler(
                "app::web::task_create",
                vec![arg(Kind::Body, "CreateTaskRequest")],
                Response::Json("TaskCreated".into()),
            ),
            handler(
                "app::web::batch_upload",
                vec![arg(Kind::Path, "string"), arg(Kind::Multipart, "FormData")],
                Response::Json("VersionUploaded".into()),
            ),
            handler(
                "app::web::task_delete",
                vec![arg(Kind::Path, "string")],
                Response::NoContent,
            ),
            handler(
                "app::web::batch_download",
                vec![Arg {
                    positions: Some(vec!["string".into(), "string".into()]),
                    ..arg(Kind::Path, "[string, string]")
                }],
                Response::Link,
            ),
            handler(
                "app::web::thing",
                vec![Arg {
                    fields: Some(vec![Field {
                        name: "id".into(),
                        ts_type: "ThingId".into(),
                    }]),
                    ..arg(Kind::Path, "ThingPath")
                }],
                Response::Json("Array<Thing>".into()),
            ),
        ];
        let (ts, n) = render(&routes, &handlers, &Options::default()).expect("renders");
        assert_eq!(n, 7);
        assert!(
            ts.contains("import { call } from \"./client-call\";"),
            "{ts}"
        );
        assert!(ts.contains("  CreateTaskRequest,\n"), "{ts}");
        assert!(ts.contains("  Thing,\n  ThingId,\n"), "{ts}");
        assert!(!ts.contains("  Array,"), "{ts}");
        assert!(!ts.contains("FormData,"), "{ts}");
        assert!(ts.contains(
            "export function run(id: string, number: number): Promise<RunDetail> {\n  return call<RunDetail>(\"GET\", `/api/tasks/${enc(id)}/runs/${enc(number)}`);\n}"
        ), "{ts}");
        assert!(ts.contains("export function runs(query: RunQuery): Promise<RunPage> {\n  return call<RunPage>(\"GET\", \"/api/runs\", query);"), "{ts}");
        assert!(ts.contains("export function taskCreate(body: CreateTaskRequest): Promise<TaskCreated> {\n  return call<TaskCreated>(\"POST\", \"/api/tasks/create\", undefined, body);"), "{ts}");
        assert!(ts.contains("export function batchUpload(name: string, body: FormData): Promise<VersionUploaded>"), "{ts}");
        assert!(ts.contains("export function taskDelete(id: string): Promise<void> {\n  return call<void>(\"POST\", `/api/tasks/${enc(id)}/delete`);"), "{ts}");
        assert!(ts.contains("export function batchDownloadUrl(name: string, version: string): string {\n  return `/api/batches/${enc(name)}/download/${enc(version)}`;"), "{ts}");
        assert!(
            ts.contains("export function thing(id: ThingId): Promise<Array<Thing>>"),
            "{ts}"
        );
    }

    #[test]
    fn unmatched_either_way_is_an_error_naming_it() {
        let routes = vec![reg("GET", "/api/a", "app::a")];
        let handlers = vec![handler("app::b", vec![], Response::NoContent)];
        let err = render(&routes, &handlers, &Options::default())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("GET /api/a") && err.contains("app::a"),
            "{err}"
        );

        let routes = vec![reg("GET", "/api/a", "app::a")];
        let handlers = vec![
            handler("app::a", vec![], Response::NoContent),
            handler("app::b", vec![], Response::NoContent),
        ];
        let err = render(&routes, &handlers, &Options::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("app::b") && err.contains("no route"), "{err}");
    }

    #[test]
    fn excluded_routes_need_no_handler() {
        let routes = vec![
            reg("GET", "/", "app::shell"),
            reg("GET", "/api/a", "app::a"),
        ];
        let handlers = vec![handler("app::a", vec![], Response::NoContent)];
        let opts = Options::default().include(|r| r.path.starts_with("/api/"));
        let (ts, n) = render(&routes, &handlers, &opts).expect("renders");
        assert_eq!(n, 1);
        assert!(!ts.contains("shell"));
    }

    #[test]
    fn param_count_and_field_mismatches_are_errors() {
        let routes = vec![reg("GET", "/api/x/{a}/{b}", "app::x")];
        let handlers = vec![handler(
            "app::x",
            vec![arg(Kind::Path, "string")],
            Response::NoContent,
        )];
        let err = render(&routes, &handlers, &Options::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("app::x") && err.contains("2 params"), "{err}");

        let handlers = vec![handler(
            "app::x",
            vec![Arg {
                fields: Some(vec![Field {
                    name: "a".into(),
                    ts_type: "string".into(),
                }]),
                ..arg(Kind::Path, "XPath")
            }],
            Response::NoContent,
        )];
        let err = render(&routes, &handlers, &Options::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("{b}") && err.contains("XPath"), "{err}");
    }
}
