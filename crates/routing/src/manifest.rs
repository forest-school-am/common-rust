//! The route table a [`crate::Router`] records, and its file form.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// One `.get(path, handler)`-style registration, after nesting prefixes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Registration {
    /// `std::any::type_name` of the handler: for a plain `async fn` its full
    /// path (`cron::web::run`), which is also what `#[client]` records.
    pub fqname: String,
    /// Upper-case, as on the wire (`GET`).
    pub method: String,
    /// The axum template as registered (`/api/tasks/{id}/runs/{number}`).
    pub path: String,
    /// The `{name}` segments of `path`, in order (`{*rest}` is `rest`).
    pub path_params: Vec<String>,
}

/// `/api/tasks/{id}/runs/{number}` → `["id", "number"]`. A doubled brace is
/// axum's escape for a literal one and is not a param.
pub fn parse_path_params(path: &str) -> Vec<String> {
    path.split('/')
        .filter_map(|seg| {
            let inner = seg.strip_prefix('{')?.strip_suffix('}')?;
            if inner.starts_with('{') || inner.ends_with('}') || inner.is_empty() {
                return None;
            }
            Some(inner.trim_start_matches('*').to_owned())
        })
        .collect()
}

/// Sorted by path then method, pretty-printed, so the file diffs like the
/// ts-rs bindings do.
pub fn write_manifest(regs: &[Registration], path: &Path) -> std::io::Result<()> {
    let mut sorted: Vec<&Registration> = regs.iter().collect();
    sorted.sort_by(|a, b| (&a.path, &a.method).cmp(&(&b.path, &b.method)));
    let mut text = serde_json::to_string_pretty(&sorted)?;
    text.push('\n');
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_come_from_brace_segments_in_order() {
        assert_eq!(
            parse_path_params("/api/tasks/{id}/runs/{number}"),
            vec!["id", "number"]
        );
        assert_eq!(parse_path_params("/api/tasks"), Vec::<String>::new());
        assert_eq!(parse_path_params("/files/{*rest}"), vec!["rest"]);
        assert_eq!(parse_path_params("/lit/{{x}}"), Vec::<String>::new());
    }
}
