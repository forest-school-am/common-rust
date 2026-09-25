//! The `index.ts` re-export barrel over a bindings directory. Every repo grew
//! its own shell one-liner for this (`ls *.ts | grep -v | awk …`) and they had
//! already drifted apart — one emitting `export *`, another `export type *`,
//! each with its own idea of what to leave out. The rule belongs in one place,
//! called from the same step that writes `client.ts`.

use std::collections::BTreeSet;
use std::path::Path;

/// How to write the barrel. `type_only` is the default because ts-rs emits pure
/// type aliases, and `export type *` is what survives `verbatimModuleSyntax`.
#[derive(Debug, Clone)]
pub struct Barrel {
    type_only: bool,
    exclude: BTreeSet<String>,
    header: Option<String>,
}

impl Default for Barrel {
    fn default() -> Self {
        Self {
            type_only: true,
            // The barrel itself, and the client — whose exports are functions,
            // not types, and which an app imports by name rather than through
            // the barrel.
            exclude: ["index", "client"].iter().map(|s| s.to_string()).collect(),
            header: Some("// Generated — do not edit.".to_string()),
        }
    }
}

impl Barrel {
    pub fn new() -> Self {
        Self::default()
    }

    /// `export *` instead of `export type *`, for a directory that carries
    /// values as well as types.
    pub fn values(mut self) -> Self {
        self.type_only = false;
        self
    }

    /// Leave a module out of the barrel. The stem, with no `.ts`.
    pub fn exclude(mut self, stem: &str) -> Self {
        self.exclude.insert(stem.to_string());
        self
    }

    /// Put `client` back in — for an app that re-exports the client through the
    /// barrel rather than importing it directly.
    pub fn with_client(mut self) -> Self {
        self.exclude.remove("client");
        self
    }

    pub fn header(mut self, line: &str) -> Self {
        self.header = Some(line.to_string());
        self
    }

    pub fn no_header(mut self) -> Self {
        self.header = None;
        self
    }

    /// Render the barrel for `dir` and write it as `index.ts`. Sorted, so the
    /// file does not churn on directory order.
    pub fn write(&self, dir: &Path) -> std::io::Result<()> {
        let text = self.render(dir)?;
        std::fs::write(dir.join("index.ts"), text)
    }

    /// The barrel's text, for a caller that wants to place it itself.
    pub fn render(&self, dir: &Path) -> std::io::Result<String> {
        let mut stems: Vec<String> = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("ts") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            // `.d.ts` is a declaration, never a module to re-export.
            if stem.ends_with(".d") || self.exclude.contains(stem) {
                continue;
            }
            stems.push(stem.to_string());
        }
        stems.sort();

        let keyword = if self.type_only {
            "export type *"
        } else {
            "export *"
        };
        let mut out = String::new();
        if let Some(header) = &self.header {
            out.push_str(header);
            out.push('\n');
        }
        for stem in stems {
            out.push_str(&format!("{keyword} from \"./{stem}\";\n"));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(names: &[&str]) -> tempdir::Dir {
        let dir = tempdir::Dir::new();
        for n in names {
            std::fs::write(dir.path().join(n), "export type X = string;\n").unwrap();
        }
        dir
    }

    #[test]
    fn the_barrel_is_sorted_and_leaves_itself_and_the_client_out() {
        let dir = dir_with(&["Zebra.ts", "Apple.ts", "index.ts", "client.ts"]);
        let out = Barrel::new().render(dir.path()).unwrap();
        assert_eq!(
            out,
            "// Generated — do not edit.\n\
             export type * from \"./Apple\";\n\
             export type * from \"./Zebra\";\n"
        );
    }

    #[test]
    fn non_ts_files_and_declarations_are_not_modules() {
        let dir = dir_with(&["Keep.ts", "routes.json", "handlers.json", "shim.d.ts"]);
        let out = Barrel::new().no_header().render(dir.path()).unwrap();
        assert_eq!(out, "export type * from \"./Keep\";\n");
    }

    #[test]
    fn a_directory_carrying_values_asks_for_the_value_form() {
        let dir = dir_with(&["Thing.ts"]);
        let out = Barrel::new()
            .values()
            .no_header()
            .render(dir.path())
            .unwrap();
        assert_eq!(out, "export * from \"./Thing\";\n");
    }

    #[test]
    fn the_client_can_be_put_back_in_and_anything_else_left_out() {
        let dir = dir_with(&["client.ts", "Secret.ts", "Public.ts"]);
        let out = Barrel::new()
            .with_client()
            .exclude("Secret")
            .no_header()
            .render(dir.path())
            .unwrap();
        assert_eq!(
            out,
            "export type * from \"./Public\";\nexport type * from \"./client\";\n"
        );
    }

    #[test]
    fn write_puts_it_beside_the_modules() {
        let dir = dir_with(&["A.ts"]);
        Barrel::new().no_header().write(dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join("index.ts")).unwrap();
        assert_eq!(text, "export type * from \"./A\";\n");
    }

    /// A scratch directory that removes itself — the crate has no dev-dependency
    /// on tempfile and this is the only test here that needs one.
    mod tempdir {
        use std::path::{Path, PathBuf};

        pub struct Dir(PathBuf);

        impl Dir {
            pub fn new() -> Self {
                let base = std::env::temp_dir().join(format!(
                    "common-routing-barrel-{}-{:?}",
                    std::process::id(),
                    std::thread::current().id()
                ));
                let _ = std::fs::remove_dir_all(&base);
                std::fs::create_dir_all(&base).unwrap();
                Self(base)
            }
            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}
