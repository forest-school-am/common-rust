//! Where the bytes come from: the per-prefix cache first, the origin second.
//!
//! The origin is `ASSETS_ORIGIN` (default the stand's asset host) reached
//! over TLS with the CA at `LES_CA` (default the stand CA) as the ONLY root:
//! the origin is ours, and a build that trusted the system store would accept
//! any certificate a proxy on the way could mint. Both variables are already
//! how every consumer names the origin at runtime.
//!
//! The manifest is looked for under the prefix first. Until common publishes
//! per-prefix manifests, the root manifest stands in — but ONLY when the
//! prefix it publishes is the pinned one: the root manifest names the newest
//! prefix's files and nothing else, so for any other prefix it would verify
//! nothing. That case fails naming the missing per-prefix manifest.

use std::cell::OnceCell;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::{Error, Files, Manifest, Pin, Result, MANIFEST, MARKERS, SHELL, TYPES};

pub const ORIGIN_VAR: &str = "ASSETS_ORIGIN";
pub const CA_VAR: &str = "LES_CA";
pub const DEFAULT_ORIGIN: &str = "https://assets.dev.redaether";
pub const DEFAULT_CA: &str = "/mnt/host/workspace/Les/stand/certs/ca.crt";

/// No fetched file is anywhere near this; a response that is signals a
/// misrouted origin, and reading it to the end would only make that slow.
const MAX_BODY: u64 = 16 << 20;
const TIMEOUT: Duration = Duration::from_secs(60);

/// A place to GET the origin's files from. The HTTPS client implements it;
/// tests implement it over a map, so no test touches the network.
pub trait Source {
    fn origin(&self) -> &str;
    /// `GET <origin>/<path>`. `Ok(None)` is a 404 — the one status a caller
    /// decides about (the per-prefix manifest may not exist yet).
    fn get(&self, path: &str) -> Result<Option<Vec<u8>>>;
}

/// `$XDG_CACHE_HOME/common-ui`, or `~/.cache/common-ui`.
pub fn cache_dir() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .ok_or(Error::NoCacheDir)?;
    Ok(base.join("common-ui"))
}

/// The pin for `prefix`: from `cache` when every file is there (digests
/// re-verified), otherwise fetched, verified and stored there.
pub fn load(prefix: &str, cache: &Path, source: &dyn Source) -> Result<Pin> {
    if let Some(pin) = from_cache(prefix, cache)? {
        return Ok(pin);
    }
    let pin = fetch(prefix, source)?;
    store(cache, &pin)?;
    Ok(pin)
}

/// `None` unless all four files are present: a partial directory is an
/// interrupted store, and is simply fetched over.
fn from_cache(prefix: &str, cache: &Path) -> Result<Option<Pin>> {
    let paths: Vec<PathBuf> = [MANIFEST, SHELL, MARKERS, TYPES]
        .iter()
        .map(|n| cache.join(n))
        .collect();
    if !paths.iter().all(|p| p.is_file()) {
        return Ok(None);
    }
    let read = |p: &Path| -> Result<String> {
        let bytes = std::fs::read(p).map_err(|source| Error::Io {
            path: p.to_owned(),
            source,
        })?;
        utf8(bytes, &p.display().to_string())
    };
    let manifest = Manifest::parse(&read(&paths[0])?, &paths[0].display().to_string())?;
    let files = Files {
        shell: read(&paths[1])?,
        markers: read(&paths[2])?,
        dts: read(&paths[3])?,
    };
    // A digest mismatch here is a corrupted cache, reported as such — not
    // silently refetched, because the cache is also what an offline build
    // trusts, and a copy that changed underneath it deserves a look.
    Pin::from_parts(prefix, manifest, files)
        .map(Some)
        .map_err(|e| match e {
            Error::Digest { what, got, want } => Error::Digest {
                what: format!("{} (cached under {})", what, cache.display()),
                got,
                want,
            },
            other => other,
        })
}

fn fetch(prefix: &str, source: &dyn Source) -> Result<Pin> {
    let manifest = manifest_for(prefix, source)?;
    let text = |name: &str| -> Result<String> {
        let path = format!("{prefix}/{name}");
        let bytes = source
            .get(&path)?
            .ok_or_else(|| Error::Http {
                url: format!("{}/{path}", source.origin()),
                status: 404,
            })?;
        utf8(bytes, &path)
    };
    let files = Files {
        shell: text(SHELL)?,
        markers: text(MARKERS)?,
        dts: text(TYPES)?,
    };
    Pin::from_parts(prefix, manifest, files)
}

/// `<prefix>/manifest.json`, else the root manifest when it publishes this
/// very prefix, else the error that names what common has to publish.
pub fn manifest_for(prefix: &str, source: &dyn Source) -> Result<Manifest> {
    let url = format!("{}/{prefix}/{MANIFEST}", source.origin());
    if let Some(bytes) = source.get(&format!("{prefix}/{MANIFEST}"))? {
        return Manifest::parse(&utf8(bytes, &url)?, &url);
    }
    let root_url = format!("{}/{MANIFEST}", source.origin());
    let root = source.get(MANIFEST)?.ok_or_else(|| Error::Http {
        url: root_url.clone(),
        status: 404,
    })?;
    let root = Manifest::parse(&utf8(root, &root_url)?, &root_url)?;
    match root.prefix.as_deref() {
        Some(p) if p == prefix => Ok(root),
        published => Err(Error::NoManifest {
            prefix: prefix.to_owned(),
            url,
            root_url,
            published: published.unwrap_or("no prefix at all").to_owned(),
        }),
    }
}

/// Each file lands under its final name only once fully written, and the
/// manifest last: its presence is what marks the directory complete.
fn store(cache: &Path, pin: &Pin) -> Result<()> {
    std::fs::create_dir_all(cache).map_err(|source| Error::Io {
        path: cache.to_owned(),
        source,
    })?;
    for (name, text) in [
        (SHELL, pin.files.shell.as_str()),
        (MARKERS, pin.files.markers.as_str()),
        (TYPES, pin.files.dts.as_str()),
        (MANIFEST, pin.manifest.raw.as_str()),
    ] {
        let tmp = cache.join(format!("{name}.tmp-{}", std::process::id()));
        let dst = cache.join(name);
        std::fs::write(&tmp, text)
            .and_then(|()| std::fs::rename(&tmp, &dst))
            .map_err(|source| Error::Io { path: dst, source })?;
    }
    Ok(())
}

fn utf8(bytes: Vec<u8>, what: &str) -> Result<String> {
    String::from_utf8(bytes).map_err(|_| Error::NotUtf8 {
        what: what.to_owned(),
    })
}

/// The HTTPS source. The agent — and so the CA file — is built on the first
/// GET, so a build served entirely from the cache never reads the CA at all.
pub struct LazyHttps {
    origin: String,
    ca: PathBuf,
    agent: OnceCell<ureq::Agent>,
}

impl LazyHttps {
    pub fn from_env() -> Self {
        let origin = std::env::var(ORIGIN_VAR)
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_ORIGIN.to_owned());
        let ca = std::env::var_os(CA_VAR)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CA));
        Self::new(origin, ca)
    }

    pub fn new(origin: String, ca: PathBuf) -> Self {
        Self {
            origin: origin.trim_end_matches('/').to_owned(),
            ca,
            agent: OnceCell::new(),
        }
    }

    fn agent(&self) -> Result<&ureq::Agent> {
        if let Some(agent) = self.agent.get() {
            return Ok(agent);
        }
        let agent = ureq::AgentBuilder::new()
            .tls_config(Arc::new(tls_config(&self.ca)?))
            .timeout(TIMEOUT)
            .build();
        Ok(self.agent.get_or_init(|| agent))
    }
}

/// rustls with the stand CA as the only root.
fn tls_config(ca: &Path) -> Result<rustls::ClientConfig> {
    use rustls_pki_types::pem::PemObject as _;
    let pem = std::fs::read(ca).map_err(|source| Error::Ca {
        path: ca.to_owned(),
        source,
    })?;
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pki_types::CertificateDer::pem_slice_iter(&pem) {
        let cert = cert.map_err(|e| Error::Tls(format!("{}: {e}", ca.display())))?;
        roots.add(cert).map_err(|e| Error::Tls(e.to_string()))?;
    }
    if roots.is_empty() {
        return Err(Error::EmptyCa {
            path: ca.to_owned(),
        });
    }
    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Tls(e.to_string()))
        .map(|b| b.with_root_certificates(roots).with_no_client_auth())
}

impl Source for LazyHttps {
    fn origin(&self) -> &str {
        &self.origin
    }

    fn get(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let url = format!("{}/{path}", self.origin);
        let response = match self.agent()?.get(&url).call() {
            Ok(r) => r,
            Err(ureq::Error::Status(404, _)) => return Ok(None),
            Err(ureq::Error::Status(status, _)) => return Err(Error::Http { url, status }),
            Err(ureq::Error::Transport(t)) => {
                return Err(Error::Transport {
                    url,
                    message: t.to_string(),
                })
            }
        };
        let mut body = Vec::new();
        response
            .into_reader()
            .take(MAX_BODY)
            .read_to_end(&mut body)
            .map_err(|e| Error::Transport {
                url,
                message: e.to_string(),
            })?;
        Ok(Some(body))
    }
}
