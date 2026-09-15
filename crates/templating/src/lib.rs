//! Serving static files from a validated directory, with caching and path
//! safety, and stamping a built shell with the two values only a request
//! knows. Nothing here decides what to serve or when.
//!
//! ```
//! use common_templating::Builder;
//! use sha2::{Digest, Sha256};
//!
//! let dir = std::env::temp_dir().join("common-templating-doc");
//! std::fs::create_dir_all(&dir)?;
//! let logic = "console.log('hello');";
//! std::fs::write(dir.join("logic.js"), logic)?;
//! let expected: [u8; 32] = Sha256::digest(logic.as_bytes()).into();
//!
//! let assets = Builder::new(&dir).pin("logic.js", expected).build()?;
//!
//! assert_eq!(&*assets.static_file("logic.js")?, logic.as_bytes());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod assets_origin;
mod render;

pub use assets_origin::{AssetsOrigin, VARIABLE as ASSETS_ORIGIN_VARIABLE};
pub use render::{render, Shell, CONFIG_MARKER, ORIGIN_MARKER};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("asset dir does not exist or is not a directory: {0}")]
    BadDir(PathBuf),
    #[error("required file not found: {0}")]
    Missing(String),
    #[error("template {0} failed to parse: {1}")]
    Parse(String, String),
    #[error("integrity pin failed for {0}: on-disk content does not match the expected hash")]
    PinMismatch(String),
    #[error("io error on {0}: {1}")]
    Io(String, String),
    #[error("render of {0} failed: {1}")]
    Render(String, String),
    #[error("unsafe asset name (path traversal / escape rejected): {0}")]
    UnsafeName(String),
}

struct Entry {
    mtime: SystemTime,
    bytes: Arc<[u8]>,
}

pub struct AssetCache {
    root: PathBuf,
    canonical_root: PathBuf,
    pins: HashMap<String, [u8; 32]>,
    entries: RwLock<HashMap<String, Entry>>,
}

pub struct Builder {
    root: PathBuf,
    pins: HashMap<String, [u8; 32]>,
}

impl Builder {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            pins: HashMap::new(),
        }
    }

    pub fn pin(mut self, name: impl Into<String>, expected_sha256: [u8; 32]) -> Self {
        self.pins.insert(name.into(), expected_sha256);
        self
    }

    pub fn build(self) -> Result<AssetCache, RenderError> {
        if !self.root.is_dir() {
            return Err(RenderError::BadDir(self.root));
        }
        let canonical_root = self
            .root
            .canonicalize()
            .map_err(|e| RenderError::Io(self.root.display().to_string(), e.to_string()))?;

        let cache = AssetCache {
            root: self.root,
            canonical_root,
            pins: self.pins,
            entries: RwLock::new(HashMap::new()),
        };

        for (name, expected) in &cache.pins {
            let path = cache.safe_path(name)?;
            cache.read_verified(&path, name, Some(expected))?;
        }
        Ok(cache)
    }
}

impl AssetCache {
    fn safe_path(&self, name: &str) -> Result<PathBuf, RenderError> {
        let rel = Path::new(name);
        for component in rel.components() {
            match component {
                std::path::Component::Normal(_) | std::path::Component::CurDir => {}
                _ => return Err(RenderError::UnsafeName(name.to_owned())),
            }
        }
        match self.root.join(rel).canonicalize() {
            Ok(real) if real.starts_with(&self.canonical_root) => Ok(real),
            Ok(_) => Err(RenderError::UnsafeName(name.to_owned())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(RenderError::Missing(name.to_owned()))
            }
            Err(e) => Err(RenderError::Io(name.to_owned(), e.to_string())),
        }
    }

    fn mtime(&self, path: &Path, name: &str) -> Result<SystemTime, RenderError> {
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    RenderError::Missing(name.to_owned())
                } else {
                    RenderError::Io(name.to_owned(), e.to_string())
                }
            })
    }

    fn read_verified(
        &self,
        path: &Path,
        name: &str,
        expected: Option<&[u8; 32]>,
    ) -> Result<Vec<u8>, RenderError> {
        let bytes = std::fs::read(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                RenderError::Missing(name.to_owned())
            } else {
                RenderError::Io(name.to_owned(), e.to_string())
            }
        })?;
        if let Some(expected) = expected {
            let got: [u8; 32] = Sha256::digest(&bytes).into();
            if &got != expected {
                return Err(RenderError::PinMismatch(name.to_owned()));
            }
        }
        Ok(bytes)
    }

    pub fn static_file(&self, name: &str) -> Result<Arc<[u8]>, RenderError> {
        let path = self.safe_path(name)?;
        let mtime = self.mtime(&path, name)?;

        if let Ok(entries) = self.entries.read() {
            if let Some(Entry { mtime: m, bytes }) = entries.get(name) {
                if *m == mtime {
                    return Ok(bytes.clone());
                }
            }
        }

        let bytes: Arc<[u8]> = Arc::from(self.read_verified(&path, name, self.pins.get(name))?);
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(
                name.to_owned(),
                Entry {
                    mtime,
                    bytes: bytes.clone(),
                },
            );
        }
        Ok(bytes)
    }
}

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

    fn tmpdir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("common-templating-{}", uniq()));
        fs::create_dir_all(&d).unwrap();
        d
    }
    fn uniq() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static C: AtomicU64 = AtomicU64::new(0);
        let n = C.fetch_add(1, Ordering::Relaxed);
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
            ^ n
    }
    fn write(dir: &Path, name: &str, body: &str) {
        fs::write(dir.join(name), body).unwrap();
    }
    fn bump_mtime(dir: &Path, name: &str) {
        let t = filetime::FileTime::from_unix_time(2_000_000_000, 0);
        filetime::set_file_mtime(dir.join(name), t).unwrap();
    }

    #[test]
    fn static_file_caches_by_mtime() {
        let d = tmpdir();
        write(&d, "logo.svg", "<svg/>");
        let c = Builder::new(&d).build().unwrap();
        let a = c.static_file("logo.svg").unwrap();
        assert_eq!(&*a, b"<svg/>");
        let b = c.static_file("logo.svg").unwrap();
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn edit_takes_effect_without_restart_via_mtime() {
        let d = tmpdir();
        write(&d, "a.txt", "one");
        let c = Builder::new(&d).build().unwrap();
        assert_eq!(&*c.static_file("a.txt").unwrap(), b"one");
        std::thread::sleep(Duration::from_millis(5));
        write(&d, "a.txt", "two");
        bump_mtime(&d, "a.txt");
        assert_eq!(
            &*c.static_file("a.txt").unwrap(),
            b"two",
            "edit must take effect without restart"
        );
    }

    #[test]
    fn boot_refuses_bad_dir_and_missing_pinned_file() {
        let missing = std::env::temp_dir().join(format!("nope-{}", uniq()));
        assert!(matches!(
            Builder::new(&missing).build(),
            Err(RenderError::BadDir(_))
        ));

        let d = tmpdir();
        assert!(matches!(
            Builder::new(&d).pin("absent.js", sha256(b"")).build(),
            Err(RenderError::Missing(_))
        ));
    }

    #[test]
    fn pin_matches_at_boot_and_mismatches_on_drift() {
        let d = tmpdir();
        write(&d, "logic.js", "authored();");
        let good = sha256(b"authored();");
        let c = Builder::new(&d).pin("logic.js", good).build().unwrap();
        assert_eq!(&*c.static_file("logic.js").unwrap(), b"authored();");

        let wrong = sha256(b"different");
        assert!(matches!(
            Builder::new(&d).pin("logic.js", wrong).build(),
            Err(RenderError::PinMismatch(_))
        ));

        std::thread::sleep(Duration::from_millis(5));
        write(&d, "logic.js", "tampered();");
        bump_mtime(&d, "logic.js");
        assert!(matches!(
            c.static_file("logic.js"),
            Err(RenderError::PinMismatch(_))
        ));
    }

    #[test]
    fn traversal_names_rejected() {
        let d = tmpdir();
        write(&d, "ok.txt", "ok");
        let c = Builder::new(&d).build().unwrap();
        assert_eq!(&*c.static_file("ok.txt").unwrap(), b"ok");

        for bad in [
            "../../etc/passwd",
            "../secret",
            "/etc/passwd",
            "a/../../b",
            "./../x",
        ] {
            assert!(
                matches!(c.static_file(bad), Err(RenderError::UnsafeName(_))),
                "static_file({bad:?}) not rejected"
            );
        }
    }

    #[test]
    fn symlink_escaping_root_is_rejected() {
        let parent = tmpdir();
        fs::write(parent.join("outside.txt"), "SECRET").unwrap();
        let root = parent.join("assets");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("inside.txt"), "ok").unwrap();
        std::os::unix::fs::symlink(&parent, root.join("up")).unwrap();

        let c = Builder::new(&root).build().unwrap();
        assert_eq!(&*c.static_file("inside.txt").unwrap(), b"ok");
        assert!(matches!(
            c.static_file("up/outside.txt"),
            Err(RenderError::UnsafeName(_))
        ));
        assert!(matches!(
            c.static_file("up/does-not-exist.txt"),
            Err(RenderError::Missing(_))
        ));
    }

    #[test]
    fn nonexistent_name_is_missing_not_ok_or_unsafe() {
        let d = tmpdir();
        write(&d, "real.txt", "x");
        let c = Builder::new(&d).build().unwrap();
        assert!(matches!(
            c.static_file("absent.txt"),
            Err(RenderError::Missing(_))
        ));
    }
}
