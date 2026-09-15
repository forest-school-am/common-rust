//! The common-ui pin for a consumer's `build.rs` (R114, "vendored" item).
//!
//! A consumer pins common-ui with ONE line in its `Cargo.toml`:
//!
//! ```toml
//! [package.metadata.common-ui]
//! prefix = "common-ui@b9f049d05d44"
//! ```
//!
//! [`Pin::load`] reads that line, fetches the prefix's manifest and the three
//! files every build needs — `shell.html` (the template), `shell-markers.json`
//! (the marker/CSP contract) and `common-ui.d.ts` (the typecheck) — verifies
//! every byte string against the manifest's sha384, and caches them under
//! `$XDG_CACHE_HOME/common-ui/<prefix>/` so every later build is offline.
//! Digests are verified again on every read from the cache: a pin nobody
//! checks is a comment.
//!
//! Trust is unchanged from the vendor directories this replaces: those lock
//! files were copied from the same manifest over the same TLS to the same
//! origin. A prefix is content-addressed and immutable, so pinning its name
//! pins its bytes.
//!
//! What a `build.rs` then does with the pin: [`stamp`] the shell's build-time
//! markers, [`check_markers`] the result both ways against the contract,
//! embed the [`Pin::csp`] and the [`Pin::sri_table`], and [`Pin::write_dts`]
//! for the typecheck. Nothing here runs at request time.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use sha2::{Digest, Sha384};

pub mod fetch;

/// The three files fetched per prefix.
pub const SHELL: &str = "shell.html";
pub const MARKERS: &str = "shell-markers.json";
pub const TYPES: &str = "common-ui.d.ts";
/// The manifest's name at the origin, under the prefix (and at the root).
pub const MANIFEST: &str = "manifest.json";
/// What [`Pin::write_manifest`] writes into `OUT_DIR`: the manifest the build
/// used, for the consumer's own tests to read digests from.
pub const OUT_MANIFEST: &str = "common-ui.manifest.json";

/// The metadata table in the consumer's `Cargo.toml`.
pub const METADATA_TABLE: &str = "common-ui";
pub const METADATA_KEY: &str = "prefix";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} is not set — this crate runs inside a cargo build script")]
    Env(&'static str),
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not TOML: {source}")]
    Toml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error(
        "{path} has no `[package.metadata.{METADATA_TABLE}] {METADATA_KEY} = \"common-ui@<hex>\"` — \
         that one line is the whole common-ui pin"
    )]
    NoPrefix { path: PathBuf },
    #[error("prefix {0:?} is not of the form common-ui@<hex>")]
    BadPrefix(String),
    #[error("{what} is not JSON: {source}")]
    Json {
        what: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("the manifest lists no digest for {key}")]
    NotInManifest { key: String },
    #[error("{what} is not UTF-8 — every pinned file is text by contract")]
    NotUtf8 { what: String },
    #[error(
        "{what} does not match the manifest.\n  got  {got}\n  want {want}\n  \
         A prefix is immutable, so this is a corrupted copy or a served file that differs \
         from what the manifest says — never something to edit around."
    )]
    Digest {
        what: String,
        got: String,
        want: String,
    },
    #[error(
        "no manifest for {prefix}: {url} is 404 and the root manifest {root_url} publishes \
         {published} — common must publish a per-prefix manifest.json under each prefix \
         (the root manifest's entries for that prefix), and backfill {prefix}"
    )]
    NoManifest {
        prefix: String,
        url: String,
        root_url: String,
        published: String,
    },
    #[error("{url} answered {status}")]
    Http { url: String, status: u16 },
    #[error("fetching {url}: {message}")]
    Transport { url: String, message: String },
    #[error("cannot read the CA at {path} ({source}) — set LES_CA to the stand CA certificate")]
    Ca {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the CA at {path} holds no certificate")]
    EmptyCa { path: PathBuf },
    #[error("tls setup: {0}")]
    Tls(String),
    #[error("no cache directory: neither XDG_CACHE_HOME nor HOME is set")]
    NoCacheDir,
    #[error("shell-markers.json: {0}")]
    Markers(String),
    #[error("{0}")]
    MarkerCheck(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// `sha384-` + standard base64: the spelling an `integrity` attribute and the
/// manifest both use, so the two compare by eye.
pub fn sha384(bytes: &[u8]) -> String {
    format!(
        "sha384-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha384::digest(bytes))
    )
}

/// The one pinned value, read from a consumer's `Cargo.toml` text.
pub fn read_prefix(cargo_toml: &str, path: &Path) -> Result<String> {
    let doc: toml::Value = toml::from_str(cargo_toml).map_err(|source| Error::Toml {
        path: path.to_owned(),
        source,
    })?;
    let prefix = doc
        .get("package")
        .and_then(|p| p.get("metadata"))
        .and_then(|m| m.get(METADATA_TABLE))
        .and_then(|c| c.get(METADATA_KEY))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| Error::NoPrefix {
            path: path.to_owned(),
        })?;
    validate_prefix(prefix)?;
    Ok(prefix.to_owned())
}

/// `common-ui@<hex>`. The prefix names a cache directory and a URL segment,
/// so its shape is checked before either is built from it.
pub fn validate_prefix(prefix: &str) -> Result<()> {
    let hex = prefix
        .strip_prefix("common-ui@")
        .filter(|h| (6..=40).contains(&h.len()) && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .filter(|h| h.bytes().all(|b| !b.is_ascii_uppercase()));
    match hex {
        Some(_) => Ok(()),
        None => Err(Error::BadPrefix(prefix.to_owned())),
    }
}

/// The origin's manifest: `files` is keyed `<prefix>/<path>` for every file
/// the origin serves under that prefix. The root manifest also names the
/// newest `prefix`; a per-prefix manifest carries that prefix's entries.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub prefix: Option<String>,
    pub files: BTreeMap<String, String>,
    /// The bytes as fetched, written verbatim by [`Pin::write_manifest`].
    pub raw: String,
}

impl Manifest {
    pub fn parse(raw: &str, what: &str) -> Result<Self> {
        #[derive(serde::Deserialize)]
        struct Wire {
            prefix: Option<String>,
            files: BTreeMap<String, String>,
        }
        let wire: Wire = serde_json::from_str(raw).map_err(|source| Error::Json {
            what: what.to_owned(),
            source,
        })?;
        Ok(Self {
            prefix: wire.prefix,
            files: wire.files,
            raw: raw.to_owned(),
        })
    }

    /// The digest the manifest pins for `<prefix>/<name>`.
    pub fn integrity(&self, prefix: &str, name: &str) -> Result<&str> {
        let key = format!("{prefix}/{name}");
        self.files
            .get(&key)
            .map(String::as_str)
            .ok_or(Error::NotInManifest { key })
    }
}

/// The three fetched files, as text (all three are UTF-8 by contract).
#[derive(Debug, Clone)]
pub struct Files {
    pub shell: String,
    pub markers: String,
    pub dts: String,
}

/// The shell's marker contract (`shell-markers.json`): which markers a build
/// fills, which survive to the request, and the CSP its pages need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Markers {
    pub build: Vec<String>,
    pub runtime: Vec<String>,
    pub csp: String,
}

impl Markers {
    pub fn parse(json: &str) -> Result<Self> {
        #[derive(serde::Deserialize)]
        struct Wire {
            build: Vec<String>,
            runtime: Vec<String>,
            csp: String,
        }
        let wire: Wire = serde_json::from_str(json).map_err(|source| Error::Json {
            what: MARKERS.to_owned(),
            source,
        })?;
        // A policy without the origin marker would block the shell's own
        // stylesheets; `csp` is a field, not a marker, so it must not appear
        // in the marker table either.
        if !wire.csp.contains("{{assets_origin}}") {
            return Err(Error::Markers(
                "the csp has no {{assets_origin}}: it would block the shell's own stylesheets"
                    .into(),
            ));
        }
        if wire.build.iter().any(|m| m == "csp") {
            return Err(Error::Markers(
                "`csp` is declared as a build marker — it is a field".into(),
            ));
        }
        Ok(Self {
            build: wire.build,
            runtime: wire.runtime,
            csp: wire.csp,
        })
    }
}

/// The verified pin: the prefix, its manifest, and the three files.
#[derive(Debug, Clone)]
pub struct Pin {
    pub prefix: String,
    pub files: Files,
    pub manifest: Manifest,
    pub markers: Markers,
}

impl Pin {
    /// The whole build-script entry point. Reads the prefix from the calling
    /// package's `Cargo.toml` (`CARGO_MANIFEST_DIR`), serves it from the cache
    /// when present, fetches and caches it otherwise. Emits the `cargo:` lines
    /// for the pin's inputs.
    pub fn load() -> Result<Self> {
        let manifest_dir =
            PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").ok_or(Error::Env("CARGO_MANIFEST_DIR"))?);
        let cargo_toml = manifest_dir.join("Cargo.toml");
        println!("cargo:rerun-if-changed={}", cargo_toml.display());
        println!("cargo:rerun-if-env-changed={}", fetch::ORIGIN_VAR);
        println!("cargo:rerun-if-env-changed={}", fetch::CA_VAR);

        let text = std::fs::read_to_string(&cargo_toml).map_err(|source| Error::Io {
            path: cargo_toml.clone(),
            source,
        })?;
        let prefix = read_prefix(&text, &cargo_toml)?;
        let cache = fetch::cache_dir()?.join(&prefix);
        fetch::load(&prefix, &cache, &fetch::LazyHttps::from_env())
    }

    /// Verifies the three files against the manifest and parses the contract.
    /// Every constructor goes through here: the cache, the fetch, a test.
    pub fn from_parts(prefix: &str, manifest: Manifest, files: Files) -> Result<Self> {
        validate_prefix(prefix)?;
        for (name, bytes) in [
            (SHELL, files.shell.as_bytes()),
            (MARKERS, files.markers.as_bytes()),
            (TYPES, files.dts.as_bytes()),
        ] {
            verify(&manifest, prefix, name, bytes)?;
        }
        let markers = Markers::parse(&files.markers)?;
        Ok(Self {
            prefix: prefix.to_owned(),
            files,
            manifest,
            markers,
        })
    }

    /// The digest the page stamps as `integrity=` for one asset under the
    /// prefix, e.g. `base.css` or `theme-default/palette.css`.
    pub fn sri(&self, name: &str) -> Result<&str> {
        self.manifest.integrity(&self.prefix, name)
    }

    /// Every asset the prefix publishes that a page LINKS — the four
    /// sheets/script and every theme palette — as `(name, integrity)`, sorted
    /// by name. The three fetched files are not in it: they are embedded, not
    /// linked. See [`sri_table`].
    pub fn sri_table(&self) -> Vec<(String, String)> {
        sri_table(&self.manifest, &self.prefix)
    }

    /// `(name, integrity)` for every `theme-<name>/palette.css` the prefix
    /// ships, the default included, sorted by name. Derived from the same
    /// manifest the `sri_*` markers come from (R91): a hash is never
    /// hand-copied, and a prefix that adds a palette adds a row here on the
    /// next build with no edit at all.
    pub fn themes(&self) -> Vec<(String, String)> {
        let mut rows: Vec<(String, String)> = self
            .sri_table()
            .into_iter()
            .filter_map(|(name, sri)| {
                let theme = name.strip_prefix("theme-")?.strip_suffix("/palette.css")?;
                Some((theme.to_owned(), sri))
            })
            .collect();
        rows.sort();
        rows
    }

    /// The policy the prefix declares, `{{assets_origin}}` still in it.
    pub fn csp(&self) -> &str {
        &self.markers.csp
    }

    /// Writes `common-ui.d.ts` into `out_dir` for the consumer's typecheck to
    /// point its tsconfig `paths` at. Returns the path written.
    pub fn write_dts(&self, out_dir: &Path) -> Result<PathBuf> {
        write(out_dir.join(TYPES), self.files.dts.as_bytes())
    }

    /// Writes the manifest the build used into `out_dir` as
    /// [`OUT_MANIFEST`], so the consumer's tests can compare what the build
    /// embedded against what the origin pinned without a second fetch.
    pub fn write_manifest(&self, out_dir: &Path) -> Result<PathBuf> {
        write(out_dir.join(OUT_MANIFEST), self.manifest.raw.as_bytes())
    }
}

fn write(path: PathBuf, bytes: &[u8]) -> Result<PathBuf> {
    std::fs::write(&path, bytes).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// `bytes` must hash to what the manifest pins for `<prefix>/<name>`.
pub fn verify(manifest: &Manifest, prefix: &str, name: &str, bytes: &[u8]) -> Result<()> {
    let want = manifest.integrity(prefix, name)?;
    let got = sha384(bytes);
    if got == want {
        Ok(())
    } else {
        Err(Error::Digest {
            what: format!("{prefix}/{name}"),
            got,
            want: want.to_owned(),
        })
    }
}

/// Every `files` entry under `<prefix>/` except the three fetched files, as
/// `(name, integrity)` sorted by name — the same set the retired lock files
/// held under `assets`.
pub fn sri_table(manifest: &Manifest, prefix: &str) -> Vec<(String, String)> {
    let head = format!("{prefix}/");
    manifest
        .files
        .iter()
        .filter_map(|(key, sri)| {
            let name = key.strip_prefix(&head)?;
            (![SHELL, MARKERS, TYPES].contains(&name)).then(|| (name.to_owned(), sri.clone()))
        })
        .collect()
}

/// The CSP out of raw `shell-markers.json`.
pub fn csp(markers_json: &str) -> Result<String> {
    Ok(Markers::parse(markers_json)?.csp)
}

/// Fills `{{marker}}` for each `(marker, value)` by plain substitution, in
/// order. Nothing is escaped: the values are the consumer's own constants
/// and the digests the manifest pins.
pub fn stamp(shell: &str, values: &[(&str, &str)]) -> String {
    let mut stamped = shell.to_owned();
    for (marker, value) in values {
        stamped = stamped.replace(&format!("{{{{{marker}}}}}"), value);
    }
    stamped
}

/// The marker contract, checked BOTH WAYS (§12.24):
///
/// - the set this build filled equals the shell's declared `build` set — one
///   direction alone is half a check: "nothing left unfilled" misses a
///   marker the shell GAINED that this build knows nothing about, and that
///   ships as literal braces; a marker filled but not declared is dead code
///   pretending to fill something;
/// - the markers that survive stamping are exactly the declared `runtime`
///   set — every one of them still present, and nothing else left.
pub fn check_markers(stamped: &str, markers: &Markers, filled: &[&str]) -> Result<()> {
    let filled: BTreeSet<&str> = filled.iter().copied().collect();
    let declared: BTreeSet<&str> = markers.build.iter().map(String::as_str).collect();
    if filled != declared {
        return Err(Error::MarkerCheck(format!(
            "build.rs and shell-markers.json disagree about the shell.\n  \
             declared but not filled: {:?}\n  filled but not declared: {:?}\n  \
             The first list is the dangerous one: those markers are really in the shell \
             and would reach a browser as literal text.",
            declared.difference(&filled).collect::<Vec<_>>(),
            filled.difference(&declared).collect::<Vec<_>>(),
        )));
    }

    let runtime: BTreeSet<String> = markers
        .runtime
        .iter()
        .map(|m| format!("{{{{{m}}}}}"))
        .collect();
    let survivors: BTreeSet<String> = stamped
        .match_indices("{{")
        .map(|(i, _)| {
            let rest = &stamped[i..];
            rest[..rest.find("}}").map(|j| j + 2).unwrap_or(rest.len().min(24))].to_owned()
        })
        .collect();
    let left: Vec<&String> = survivors.difference(&runtime).collect();
    if !left.is_empty() {
        return Err(Error::MarkerCheck(format!(
            "the stamped page still carries markers the shell does not declare as runtime: {left:?}"
        )));
    }
    let consumed: Vec<&String> = runtime.difference(&survivors).collect();
    if !consumed.is_empty() {
        return Err(Error::MarkerCheck(format!(
            "stamping consumed the runtime markers {consumed:?} — a build value must not fill \
             what the request fills"
        )));
    }
    Ok(())
}
