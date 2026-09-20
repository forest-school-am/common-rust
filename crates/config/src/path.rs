//! A field's position in the config tree and the spellings GENERATED from it:
//! flag, env name, toml key. Only segment names live here. Anything that needs
//! to know a field's help, default or legacy override belongs in schema.rs;
//! anything that reads a source belongs in layers.rs.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path(Vec<&'static str>);

impl Path {
    pub fn root() -> Self {
        Self(Vec::new())
    }

    pub fn child(&self, name: &'static str) -> Self {
        let mut segments = self.0.clone();
        segments.push(name);
        Self(segments)
    }

    pub fn segments(&self) -> &[&'static str] {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    pub fn leaf(&self) -> &'static str {
        self.0.last().copied().unwrap_or("")
    }

    pub fn section(&self) -> String {
        match self.0.split_last() {
            Some((_, parents)) => parents.join("."),
            None => String::new(),
        }
    }

    pub fn dotted(&self) -> String {
        self.0.join(".")
    }

    pub fn flag(&self) -> String {
        let joined = self
            .0
            .iter()
            .map(|s| s.replace('_', "-"))
            .collect::<Vec<_>>()
            .join("--");
        format!("--{joined}")
    }

    pub fn env(&self, app: &str) -> String {
        let joined = self
            .0
            .iter()
            .map(|s| s.to_ascii_uppercase())
            .collect::<Vec<_>>()
            .join("__");
        format!("{app}_{joined}")
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.dotted())
    }
}

#[cfg(test)]
mod tests {
    use super::Path;

    fn three_deep() -> Path {
        Path::root()
            .child("sandbox")
            .child("limits")
            .child("cpu_secs")
    }

    #[test]
    fn flag_uses_single_dash_inside_and_double_between() {
        assert_eq!(three_deep().flag(), "--sandbox--limits--cpu-secs");
        assert_eq!(Path::root().child("data_dir").flag(), "--data-dir");
    }

    #[test]
    fn env_uses_single_underscore_inside_and_double_between() {
        assert_eq!(three_deep().env("CRON"), "CRON_SANDBOX__LIMITS__CPU_SECS");
        assert_eq!(Path::root().child("data_dir").env("CRON"), "CRON_DATA_DIR");
    }

    #[test]
    fn toml_key_and_section() {
        let p = three_deep();
        assert_eq!(p.dotted(), "sandbox.limits.cpu_secs");
        assert_eq!(p.section(), "sandbox.limits");
        assert_eq!(p.leaf(), "cpu_secs");
        let top = Path::root().child("bind");
        assert_eq!(top.section(), "");
        assert_eq!(top.dotted(), "bind");
    }
}
