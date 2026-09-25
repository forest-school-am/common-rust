//! The [`Registration`] route-table entry and its on-disk form. The recording
//! that produces it belongs in `router`.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Registration {
    /// The join key: must equal the handler's `#[client]`-recorded fqname
    /// (`module_path!()::name`), which for a plain `async fn` is its `type_name`.
    pub fqname: String,
    pub method: String,
    pub path: String,
    pub path_params: Vec<String>,
}

/// A doubled brace is axum's escape for a literal `{` and is not a param.
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
