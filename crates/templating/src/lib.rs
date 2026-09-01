//! Rendering templates from a validated directory, with caching and path
//! safety. Everything here is about turning a template plus parameters into
//! bytes; nothing here decides what to serve or when.
//!
//! An asset cache is built once at boot. `require_template` makes a missing
//! file a startup failure rather than a 500 on first request; `pin` adds an
//! integrity hash so a copy that has drifted from the one this binary was
//! built against is detected (§9.8):
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
//! let assets = Builder::new(&dir)
//!     .require_template("logic.js")
//!     .pin("logic.js", expected)
//!     .build()?;
//!
//! assert_eq!(&*assets.static_file("logic.js")?, logic.as_bytes());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Pins are per-file: pinning a page that `extends` a base does not cover the
//! base. Pin every file whose content matters.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use minijinja::{AutoEscape, Environment};
use sha2::{Digest, Sha256};

mod invalidation;
pub use invalidation::{Invalidation, OPTIONS as INVALIDATION_OPTIONS};
use invalidation::Watch;

thread_local! {
    /// Names the loader is asked for during one render. `None` outside a
    /// recording render, so nothing accumulates when the graph is not wanted.
    static RECORDING: std::cell::RefCell<Option<Vec<String>>> =
        const { std::cell::RefCell::new(None) };
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("asset dir does not exist or is not a directory: {0}")]
    BadDir(PathBuf),
    #[error("required template not found: {0}")]
    Missing(String),
    #[error("template {0} failed to parse: {1}")]
    Parse(String, String),
    #[error("integrity pin failed for {0}: on-disk content does not match the expected hash")]
    PinMismatch(String),
    #[error("io error on {0}: {1}")]
    Io(String, String),
    #[error("render of {0} failed: {1}")]
    Render(String, String),
    #[error("cannot arm template invalidation: {0}")]
    Invalidation(String),
    #[error("unsafe asset name (path traversal / escape rejected): {0}")]
    UnsafeName(String),
}

enum Entry {
    Template { mtime: SystemTime, key: u64, out: Arc<str> },
    Static { mtime: SystemTime, bytes: Arc<[u8]> },
}

fn autoescape(name: &str) -> AutoEscape {
    if name.ends_with(".html") || name.ends_with(".htm") {
        AutoEscape::Html
    } else {
        AutoEscape::None
    }
}

struct CtxEnv {
    env: Environment<'static>,
    /// Upstream set per template, as the loader reported it: transitive
    /// extends/include and dynamically-named targets alike.
    graph: HashMap<String, Vec<String>>,
    mtimes: HashMap<String, SystemTime>,
    loads: u64,
}

pub struct AssetCache {
    root: PathBuf,
    canonical_root: PathBuf,
    env: Environment<'static>,
    ctx_env: RwLock<CtxEnv>,
    pins: HashMap<String, [u8; 32]>,
    watch: Watch,
    entries: RwLock<HashMap<String, Entry>>,
}

pub struct Builder {
    root: PathBuf,
    required: Vec<String>,
    invalidation: Invalidation,
    pins: HashMap<String, [u8; 32]>,
}

impl Builder {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            required: Vec::new(),
            invalidation: Invalidation::PerRequest,
            pins: HashMap::new(),
        }
    }

    pub fn require_template(mut self, name: impl Into<String>) -> Self {
        self.required.push(name.into());
        self
    }

    /// Selects the strategy (§4.4). Unset is `PerRequest`; see
    /// `INVALIDATION_OPTIONS` for what each one does on this stand.
    pub fn invalidation(mut self, strategy: Invalidation) -> Self {
        self.invalidation = strategy;
        self
    }

    /// Pins are per-file: a pinned template that `extends` or `include`s an
    /// unpinned one gets no coverage of that dependency. Pin every file whose
    /// content matters.
    pub fn pin(mut self, name: impl Into<String>, expected_sha256: [u8; 32]) -> Self {
        let name = name.into();
        self.pins.insert(name.clone(), expected_sha256);
        self.required.push(name);
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
        let mut env = Environment::new();
        env.set_auto_escape_callback(autoescape);

        let mut ctx_env = Environment::new();
        ctx_env.set_auto_escape_callback(autoescape);
        let inner = minijinja::path_loader(&canonical_root);
        ctx_env.set_loader(move |name| {
            RECORDING.with(|r| {
                if let Some(v) = r.borrow_mut().as_mut() {
                    v.push(name.to_owned());
                }
            });
            inner(name)
        });

        let watch = Watch::arm(self.invalidation, &canonical_root).map_err(RenderError::Invalidation)?;
        let cache = AssetCache {
            root: self.root,
            canonical_root,
            env,
            ctx_env: RwLock::new(CtxEnv {
                env: ctx_env,
                graph: HashMap::new(),
                mtimes: HashMap::new(),
                loads: 0,
            }),
            watch,
            pins: self.pins,
            entries: RwLock::new(HashMap::new()),
        };

        for (name, expected) in &cache.pins {
            let path = cache.safe_path(name)?;
            cache.read_verified(&path, name, Some(expected))?;
        }
        for name in &self.required {
            let path = cache.safe_path(name)?;
            let src = String::from_utf8(cache.read_verified(&path, name, cache.pins.get(name))?)
                .map_err(|e| RenderError::Parse(name.clone(), e.to_string()))?;
            cache
                .env
                .template_from_named_str(name, &src)
                .map_err(|e| RenderError::Parse(name.clone(), e.to_string()))?;
        }
        Ok(cache)
    }
}

impl AssetCache {
    /// Resolve an untrusted asset `name` to a filesystem path that is proven to
    /// stay under the asset root (§9.5b). Rejects absolute paths and any
    /// non-`Normal`/`CurDir` component (`..`, root, prefix) BEFORE touching the
    /// filesystem, then canonicalizes and requires containment under
    /// `canonical_root` — which catches a symlink INSIDE the root pointing out
    /// (the component check alone would not). `Ok` therefore always means
    /// "safe AND real": a name that does not resolve to an existing file
    /// returns `Missing` here rather than an unvalidated path, so no caller
    /// ever touches a path safe_path hasn't cleared. This also closes the
    /// intermediate-symlink gap by construction — a `link/newfile` where
    /// `link` escapes the root but `newfile` is absent is `Missing`, never a
    /// path a later read would follow out. (`canonicalize` is realpath: a
    /// permissions error surfaces as `Io`, so `NotFound` genuinely means
    /// absent.)
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
            Ok(_) => Err(RenderError::UnsafeName(name.to_owned())), // symlink escaped the root
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

    pub fn render(&self, name: &str, params: &[(&str, &str)]) -> Result<Arc<str>, RenderError> {
        let path = self.safe_path(name)?; // §9.5b: reject traversal before any FS/cache touch
        let mtime = self.mtime(&path, name)?;
        let key = params_key(params);

        if let Ok(entries) = self.entries.read() {
            if let Some(Entry::Template { mtime: m, key: k, out }) = entries.get(name) {
                if *m == mtime && *k == key {
                    return Ok(out.clone());
                }
            }
        }

        let src = String::from_utf8(self.read_verified(&path, name, self.pins.get(name))?)
            .map_err(|e| RenderError::Render(name.to_owned(), e.to_string()))?;
        let ctx: BTreeMap<&str, &str> = params.iter().copied().collect();
        let tmpl = self
            .env
            .template_from_named_str(name, &src)
            .map_err(|e| RenderError::Parse(name.to_owned(), e.to_string()))?;
        let out: Arc<str> =
            Arc::from(tmpl.render(ctx).map_err(|e| RenderError::Render(name.to_owned(), e.to_string()))?);

        if let Ok(mut entries) = self.entries.write() {
            entries.insert(name.to_owned(), Entry::Template { mtime, key, out: out.clone() });
        }
        Ok(out)
    }

    /// `extends`/`include` targets reach minijinja through the path loader,
    /// which never calls `read_verified` — so a pin on a base template held
    /// only until boot finished. Every pin is re-checked on every render.
    fn verify_all_pins(&self) -> Result<(), RenderError> {
        for (pinned, expected) in &self.pins {
            let path = self.safe_path(pinned)?;
            self.read_verified(&path, pinned, Some(expected))?;
        }
        Ok(())
    }

    pub fn render_ctx<S: serde::Serialize>(
        &self,
        name: &str,
        ctx: &S,
    ) -> Result<String, RenderError> {
        let path = self.safe_path(name)?;
        let _ = self.read_verified(&path, name, self.pins.get(name))?;
        self.verify_all_pins()?;

        // Consulted once per render: a kernel strategy CONSUMES what it reports.
        let stale;

        {
            let g = self.ctx_env.read().unwrap_or_else(|e| e.into_inner());
            stale = self.watch.stale(|| self.graph_stale(name, &g));
            if !stale && g.mtimes.contains_key(name) {
                return self.ctx_render(&g.env, name, ctx);
            }
        }

        let mut g = self.ctx_env.write().unwrap_or_else(|e| e.into_inner());
        g.loads += 1;
        if stale {
            // Whole cache, not the affected subtree: no partial-invalidation
            // bookkeeping and no chance of a stale sibling. The graph goes too,
            // so a removed edge cannot outlive the templates that had it.
            g.env.clear_templates();
            g.graph.clear();
            g.mtimes.clear();
        } else if !g.graph.contains_key(name) {
            // minijinja memoizes by name, so the loader is not consulted for a
            // template a previous render already pulled in. Recording a NEW
            // template's upstream set against a populated environment would
            // therefore miss exactly the shared bases. Drop the compiled
            // templates so this render sees its whole set.
            g.env.clear_templates();
        }

        RECORDING.with(|r| *r.borrow_mut() = Some(Vec::new()));
        let rendered = self.ctx_render(&g.env, name, ctx);
        let mut upstream = RECORDING.with(|r| r.borrow_mut().take()).unwrap_or_default();
        let out = rendered?;

        upstream.sort();
        upstream.dedup();
        for node in &upstream {
            if let Ok(mt) = self.canonical_root.join(node).metadata().and_then(|m| m.modified()) {
                g.mtimes.insert(node.clone(), mt);
            }
        }
        g.graph.insert(name.to_owned(), upstream);
        Ok(out)
    }

    fn ctx_render<S: serde::Serialize>(
        &self,
        env: &Environment<'static>,
        name: &str,
        ctx: &S,
    ) -> Result<String, RenderError> {
        let tmpl = env.get_template(name).map_err(|e| {
            if e.kind() == minijinja::ErrorKind::TemplateNotFound {
                RenderError::Missing(name.to_owned())
            } else {
                RenderError::Parse(name.to_owned(), e.to_string())
            }
        })?;
        tmpl.render(minijinja::value::Value::from_serialize(ctx))
            .map_err(|e| RenderError::Render(name.to_owned(), e.to_string()))
    }

    /// One stat per upstream node of the REQUESTED template — typically the
    /// page and its base — rather than a stat of everything ever recorded.
    fn graph_stale(&self, name: &str, g: &CtxEnv) -> bool {
        let Some(upstream) = g.graph.get(name) else { return false };
        upstream.iter().any(|node| {
            match self.canonical_root.join(node).metadata().and_then(|m| m.modified()) {
                Ok(now) => g.mtimes.get(node).is_none_or(|rec| now != *rec),
                Err(_) => true,
            }
        })
    }

    pub fn static_file(&self, name: &str) -> Result<Arc<[u8]>, RenderError> {
        let path = self.safe_path(name)?; // §9.5b: reject traversal before any FS/cache touch
        let mtime = self.mtime(&path, name)?;

        if let Ok(entries) = self.entries.read() {
            if let Some(Entry::Static { mtime: m, bytes }) = entries.get(name) {
                if *m == mtime {
                    return Ok(bytes.clone());
                }
            }
        }

        let bytes: Arc<[u8]> = Arc::from(self.read_verified(&path, name, self.pins.get(name))?);
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(name.to_owned(), Entry::Static { mtime, bytes: bytes.clone() });
        }
        Ok(bytes)
    }
}

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn params_key(params: &[(&str, &str)]) -> u64 {
    let sorted: BTreeMap<&str, &str> = params.iter().copied().collect();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (k, v) in sorted {
        k.hash(&mut h);
        v.hash(&mut h);
    }
    h.finish()
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
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos() as u64 ^ n
    }
    fn write(dir: &Path, name: &str, body: &str) {
        fs::write(dir.join(name), body).unwrap();
    }
    fn bump_mtime(dir: &Path, name: &str) {
        let t = filetime::FileTime::from_unix_time(2_000_000_000, 0);
        filetime::set_file_mtime(dir.join(name), t).unwrap();
    }

    #[test]
    fn render_substitutes_and_caches_by_params_and_mtime() {
        let d = tmpdir();
        write(&d, "shim.js.jinja", "const P = {{ login_path }};");
        let c = Builder::new(&d).require_template("shim.js.jinja").build().unwrap();

        let a = c.render("shim.js.jinja", &[("login_path", "\"/oidc/login\"")]).unwrap();
        assert_eq!(&*a, "const P = \"/oidc/login\";");
        let b = c.render("shim.js.jinja", &[("login_path", "\"/oidc/login\"")]).unwrap();
        assert!(Arc::ptr_eq(&a, &b), "same params+mtime must be a cache hit");
        let e = c.render("shim.js.jinja", &[("login_path", "\"/x\"")]).unwrap();
        assert_eq!(&*e, "const P = \"/x\";");
        assert!(!Arc::ptr_eq(&a, &e));
    }

    #[test]
    fn edit_takes_effect_without_restart_via_mtime() {
        let d = tmpdir();
        write(&d, "a.txt.jinja", "one {{ x }}");
        let c = Builder::new(&d).build().unwrap();
        let first = c.render("a.txt.jinja", &[("x", "!")]).unwrap();
        assert_eq!(&*first, "one !");
        std::thread::sleep(Duration::from_millis(5));
        write(&d, "a.txt.jinja", "two {{ x }}");
        bump_mtime(&d, "a.txt.jinja");
        let second = c.render("a.txt.jinja", &[("x", "!")]).unwrap();
        assert_eq!(&*second, "two !", "edit must take effect without restart");
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
    fn boot_refuses_bad_dir_and_missing_template() {
        let missing = std::env::temp_dir().join(format!("nope-{}", uniq()));
        assert!(matches!(Builder::new(&missing).build(), Err(RenderError::BadDir(_))));

        let d = tmpdir();
        assert!(matches!(
            Builder::new(&d).require_template("absent.jinja").build(),
            Err(RenderError::Missing(_))
        ));
    }

    #[test]
    fn boot_refuses_unparseable_template() {
        let d = tmpdir();
        write(&d, "bad.jinja", "{{ unclosed ");
        assert!(matches!(
            Builder::new(&d).require_template("bad.jinja").build(),
            Err(RenderError::Parse(..))
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
        assert!(matches!(c.static_file("logic.js"), Err(RenderError::PinMismatch(_))));
    }

    #[test]
    fn render_ctx_takes_structured_context_and_iterates() {
        let d = tmpdir();
        write(&d, "list.html", "{% for t in tasks %}<li>{{ t.name }}</li>{% endfor %}");
        let c = Builder::new(&d).require_template("list.html").build().unwrap();
        #[derive(serde::Serialize)]
        struct Ctx {
            tasks: Vec<Row>,
        }
        #[derive(serde::Serialize)]
        struct Row {
            name: String,
        }
        let out = c
            .render_ctx(
                "list.html",
                &Ctx { tasks: vec![Row { name: "a".into() }, Row { name: "<b>".into() }] },
            )
            .unwrap();
        assert_eq!(out, "<li>a</li><li>&lt;b&gt;</li>", "iteration + autoescape");
    }

    #[test]
    fn render_ctx_supports_extends_and_edits_without_restart() {
        let d = tmpdir();
        write(&d, "base.html", "[{% block body %}{% endblock %}]");
        write(&d, "page.html", "{% extends \"base.html\" %}{% block body %}{{ n }}{% endblock %}");
        let c = Builder::new(&d).require_template("page.html").build().unwrap();
        #[derive(serde::Serialize)]
        struct Ctx {
            n: u32,
        }
        assert_eq!(c.render_ctx("page.html", &Ctx { n: 1 }).unwrap(), "[1]");
        std::thread::sleep(Duration::from_millis(5));
        write(&d, "base.html", "({% block body %}{% endblock %})");
        bump_mtime(&d, "base.html");
        assert_eq!(
            c.render_ctx("page.html", &Ctx { n: 2 }).unwrap(),
            "(2)",
            "template edits must take effect without restart on the uncached path"
        );
    }

    #[test]
    fn render_ctx_verifies_pins_and_reports_missing() {
        let d = tmpdir();
        write(&d, "pinned.html", "ok {{ x }}");
        let good = sha256(b"ok {{ x }}");
        let c = Builder::new(&d).pin("pinned.html", good).build().unwrap();
        #[derive(serde::Serialize)]
        struct Ctx {
            x: u32,
        }
        assert_eq!(c.render_ctx("pinned.html", &Ctx { x: 7 }).unwrap(), "ok 7");
        std::thread::sleep(Duration::from_millis(5));
        write(&d, "pinned.html", "tampered {{ x }}");
        bump_mtime(&d, "pinned.html");
        assert!(matches!(
            c.render_ctx("pinned.html", &Ctx { x: 7 }),
            Err(RenderError::PinMismatch(_))
        ));
        assert!(matches!(
            c.render_ctx("absent.html", &Ctx { x: 7 }),
            Err(RenderError::Missing(_))
        ));
    }

    #[test]
    fn html_autoescapes_but_js_does_not() {
        let d = tmpdir();
        write(&d, "p.html", "<b>{{ v }}</b>");
        write(&d, "p.js.jinja", "x = {{ v }}");
        let c = Builder::new(&d).build().unwrap();
        assert_eq!(&*c.render("p.html", &[("v", "<x>")]).unwrap(), "<b>&lt;x&gt;</b>");
        assert_eq!(&*c.render("p.js.jinja", &[("v", "<x>")]).unwrap(), "x = <x>");
    }

    #[test]
    fn traversal_names_rejected_on_every_entry_point() {
        let d = tmpdir();
        write(&d, "ok.txt", "ok");
        write(&d, "t.js.jinja", "x = {{ v }}");
        let c = Builder::new(&d).build().unwrap();
        assert_eq!(&*c.static_file("ok.txt").unwrap(), b"ok");

        for bad in ["../../etc/passwd", "../secret", "/etc/passwd", "a/../../b", "./../x"] {
            assert!(
                matches!(c.static_file(bad), Err(RenderError::UnsafeName(_))),
                "static_file({bad:?}) not rejected"
            );
            assert!(
                matches!(c.render(bad, &[]), Err(RenderError::UnsafeName(_))),
                "render({bad:?}) not rejected"
            );
            assert!(
                matches!(c.render_ctx(bad, &()), Err(RenderError::UnsafeName(_))),
                "render_ctx({bad:?}) not rejected"
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
        assert!(matches!(c.static_file("absent.txt"), Err(RenderError::Missing(_))));
        assert!(matches!(c.render("absent.js.jinja", &[]), Err(RenderError::Missing(_))));
    }

    #[test]
    fn render_ctx_steady_state_no_reparse_but_partial_edit_invalidates() {
        use std::collections::BTreeMap;
        let d = tmpdir();
        write(&d, "base.html", "<html>{% block body %}{% endblock %}</html>");
        write(
            &d,
            "page.html",
            "{% extends \"base.html\" %}{% block body %}v{{ n }}{% endblock %}",
        );
        let c = Builder::new(&d).invalidation(Invalidation::Dag).build().unwrap();
        let loads = || c.ctx_env.read().unwrap().loads;

        assert_eq!(&c.render_ctx("page.html", &BTreeMap::from([("n", 1)])).unwrap(), "<html>v1</html>");
        assert_eq!(loads(), 1);

        assert_eq!(&c.render_ctx("page.html", &BTreeMap::from([("n", 2)])).unwrap(), "<html>v2</html>");
        assert_eq!(loads(), 1, "steady-state render must not reload");

        std::thread::sleep(Duration::from_millis(5));
        write(&d, "base.html", "<div>{% block body %}{% endblock %}</div>");
        bump_mtime(&d, "base.html");
        assert_eq!(&c.render_ctx("page.html", &BTreeMap::from([("n", 3)])).unwrap(), "<div>v3</div>");
        assert_eq!(loads(), 2, "a parent-partial edit must trigger exactly one reload");

        assert_eq!(&c.render_ctx("page.html", &BTreeMap::from([("n", 4)])).unwrap(), "<div>v4</div>");
        assert_eq!(loads(), 2);
    }

    #[test]
    fn pinned_base_template_is_verified_on_every_render_not_just_at_boot() {
        #[derive(serde::Serialize)]
        struct Ctx { n: u32 }
        let d = tmpdir();
        let base = "BASE {% block body %}{% endblock %}";
        let page = "{% extends \"base.html\" %}{% block body %}{{ n }}{% endblock %}";
        write(&d, "base.html", base);
        write(&d, "page.html", page);
        let bs: [u8; 32] = Sha256::digest(base.as_bytes()).into();
        let pg: [u8; 32] = Sha256::digest(page.as_bytes()).into();
        let c = Builder::new(&d).pin("page.html", pg).pin("base.html", bs).build().unwrap();
        assert_eq!(c.render_ctx("page.html", &Ctx { n: 1 }).unwrap(), "BASE 1");

        write(&d, "base.html", "TAMPERED {% block body %}{% endblock %}");
        bump_mtime(&d, "base.html");
        assert!(
            matches!(c.render_ctx("page.html", &Ctx { n: 1 }), Err(RenderError::PinMismatch(n)) if n == "base.html"),
            "tampering a pinned base template must fail the render"
        );

        write(&d, "base.html", base);
        bump_mtime(&d, "base.html");
        assert_eq!(c.render_ctx("page.html", &Ctx { n: 2 }).unwrap(), "BASE 2");
    }

    #[test]
    fn invalidation_parses_strictly_and_round_trips() {
        assert_eq!(Invalidation::parse(None).unwrap(), Invalidation::PerRequest);
        for s in ["per-request", "dag", "dnotify", "inotify"] {
            let v = Invalidation::parse(Some(s)).expect("valid strategy");
            assert_eq!(v.as_str(), s, "as_str must round-trip the accepted spelling");
        }
        // §4.3: set-but-invalid refuses rather than falling back, and the
        // message names the alternatives.
        for bad in ["", "PerRequest", "per_request", "notify", "true"] {
            let e = Invalidation::parse(Some(bad)).expect_err("must refuse");
            assert!(e.contains("per-request") && e.contains("dnotify"), "unhelpful: {e}");
        }
    }

    #[test]
    fn options_help_states_what_a_chooser_needs() {
        for s in ["per-request", "dag", "dnotify", "inotify"] {
            assert!(INVALIDATION_OPTIONS.contains(s), "help omits {s}");
        }
        // The two measured facts that change which option a person picks.
        assert!(INVALIDATION_OPTIONS.contains("INERT"), "help must say inotify is inert on 9p");
        assert!(
            INVALIDATION_OPTIONS.contains("UPSTREAM"),
            "help must say dag checks the requested template's upstream set"
        );
    }

    #[test]
    fn per_request_reloads_every_render_and_dag_does_not() {
        let mk = |strategy| {
            let d = tmpdir();
            write(&d, "base.html", "<b>{% block body %}{% endblock %}</b>");
            write(&d, "page.html", "{% extends \"base.html\" %}{% block body %}{{ n }}{% endblock %}");
            let c = Builder::new(&d).invalidation(strategy).build().unwrap();
            for i in 0..3 {
                c.render_ctx("page.html", &BTreeMap::from([("n", i)])).unwrap();
            }
            let n = c.ctx_env.read().unwrap().loads;
            n
        };
        assert_eq!(mk(Invalidation::PerRequest), 3, "per-request must rebuild on every render");
        assert_eq!(mk(Invalidation::Dag), 1, "dag must not rebuild while nothing changed");
    }

    #[test]
    fn dnotify_sees_an_edit_to_an_extended_base() {
        let d = tmpdir();
        write(&d, "base.html", "<b>v1 {% block body %}{% endblock %}</b>");
        write(&d, "page.html", "{% extends \"base.html\" %}{% block body %}{{ n }}{% endblock %}");
        let c = match Builder::new(&d).invalidation(Invalidation::Dnotify).build() {
            Ok(c) => c,
            // A kernel without CONFIG_DNOTIFY cannot run this; that is a
            // property of the host, not a failure of the code under test.
            Err(RenderError::Invalidation(e)) => {
                eprintln!("skipped: dnotify unavailable here ({e})");
                return;
            }
            Err(e) => panic!("unexpected build failure: {e}"),
        };
        let ctx = BTreeMap::from([("n", 7)]);
        assert_eq!(c.render_ctx("page.html", &ctx).unwrap(), "<b>v1 7</b>");

        write(&d, "base.html", "<b>v2 {% block body %}{% endblock %}</b>");
        bump_mtime(&d, "base.html");
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(
            c.render_ctx("page.html", &ctx).unwrap(),
            "<b>v2 7</b>",
            "an edit to a BASE template must be picked up — editing the child was always caught"
        );
    }

    #[test]
    fn dag_records_a_shared_base_for_a_page_first_rendered_after_it_was_loaded() {
        let d = tmpdir();
        write(&d, "base.html", "<b>v1 {% block body %}{% endblock %}</b>");
        for p in ["one.html", "two.html"] {
            write(&d, p, "{% extends \"base.html\" %}{% block body %}{{ n }}{% endblock %}");
        }
        let c = Builder::new(&d).invalidation(Invalidation::Dag).build().unwrap();
        let ctx = BTreeMap::from([("n", 1)]);

        // one.html loads base.html. two.html is rendered afterwards, when the
        // loader would be memoized past base.html.
        assert_eq!(c.render_ctx("one.html", &ctx).unwrap(), "<b>v1 1</b>");
        assert_eq!(c.render_ctx("two.html", &ctx).unwrap(), "<b>v1 1</b>");
        {
            let g = c.ctx_env.read().unwrap();
            assert!(
                g.graph["two.html"].contains(&"base.html".to_owned()),
                "two.html's upstream set must include the shared base, not just itself: {:?}",
                g.graph["two.html"]
            );
        }

        write(&d, "base.html", "<b>v2 {% block body %}{% endblock %}</b>");
        bump_mtime(&d, "base.html");
        assert_eq!(
            c.render_ctx("two.html", &ctx).unwrap(),
            "<b>v2 1</b>",
            "editing the shared base must invalidate a page that never loaded it itself"
        );
    }
}
