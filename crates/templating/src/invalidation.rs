//! How the ctx environment decides a loaded template is stale.
//!
//! Four strategies, selected by config (§4.4) and parsed strictly, because a
//! misspelled name that quietly selected a default would produce the RIGHT
//! page by the wrong mechanism — nothing visible to correct (§4.3).
//!
//! What each strategy actually does on this stand is documented in `OPTIONS`
//! rather than checked at boot. Choosing inotify here yields a stale page,
//! which is self-announcing to the one person who just chose it, and under a
//! fresh-binary deploy an empty cache cannot carry it anywhere.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// §4.4 deployment option: the invalidation strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalidation {
    /// Clear the environment on every render.
    PerRequest,
    /// Graph the loader's dependencies; stat the requested template's
    /// upstream set each request and clear everything if any node moved.
    Dag,
    /// Per-directory kernel events via `fcntl(F_NOTIFY)`.
    Dnotify,
    /// Per-inode kernel events via inotify.
    Inotify,
}

/// Help text for the option. Data, not prose: printed by a consumer's
/// `--help` and asserted for presence by a test, so the facts a person needs
/// in order to choose reach them BEFORE they choose.
pub const OPTIONS: &str = "\
TEMPLATE_INVALIDATION — how a loaded template is noticed to have changed.
Unset means per-request. An unrecognised value refuses to start.

  per-request  (default) Clear the environment on every render. Keeps no
               bookkeeping and is the only strategy that cannot silently
               degrade. Costs a reparse per render.

  dag          Build a dependency graph as templates load: every name the
               loader is asked for, so transitive extends/include and
               dynamically-named targets alike. Per request, stat the UPSTREAM
               set of the requested template; if any node changed, clear the
               whole cache. Cost is one stat per upstream node per request —
               a page and its base, typically two — not a walk of the asset
               root. A directory's mtime does NOT move when a file's contents
               change (measured, ext4 and 9p alike), so watching the root
               directory is not a cheaper substitute.

  dnotify      Per-directory kernel events, raw fcntl(F_NOTIFY). This is the
               kernel option that WORKS on this stand's 9p share. Caveats:
               directories only, one fd per directory, no recursion, signal
               driven, and deprecated since Linux 2.6.13 — the `notify` crate
               does not offer it.

  inotify      Per-inode kernel events. INERT on this stand's 9p share:
               inotify_add_watch SUCCEEDS and returns a valid watch
               descriptor, then delivers no events at all (measured, with an
               ext4 control that passed). Selecting it here means templates
               never reload. Use dnotify for kernel events on 9p.

None of this depends on the 9p share existing: the strategies are about how a
running service notices its own assets changing, wherever it runs. The 9p note
is there because it is where inotify silently does nothing.
";

impl Invalidation {
    /// §4.3: absent means the default; set-but-unrecognised refuses.
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value {
            None => Ok(Invalidation::PerRequest),
            Some("per-request") => Ok(Invalidation::PerRequest),
            Some("dag") => Ok(Invalidation::Dag),
            Some("dnotify") => Ok(Invalidation::Dnotify),
            Some("inotify") => Ok(Invalidation::Inotify),
            Some(other) => Err(format!(
                "TEMPLATE_INVALIDATION={other:?} is not valid — expected \"per-request\", \
                 \"dag\", \"dnotify\" or \"inotify\""
            )),
        }
    }

    pub fn from_env() -> Result<Self, String> {
        Self::parse(std::env::var("TEMPLATE_INVALIDATION").ok().as_deref())
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Invalidation::PerRequest => "per-request",
            Invalidation::Dag => "dag",
            Invalidation::Dnotify => "dnotify",
            Invalidation::Inotify => "inotify",
        }
    }
}

impl std::fmt::Display for Invalidation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One flag for the process, not one per watch: dnotify reports through a
/// signal, and demultiplexing by si_fd inside a handler is not worth it here.
/// Two caches under dnotify therefore invalidate together — over-invalidation,
/// which costs a reparse and can never serve something stale.
static DNOTIFY_FIRED: AtomicBool = AtomicBool::new(false);

extern "C" fn dnotify_handler(_sig: libc::c_int) {
    DNOTIFY_FIRED.store(true, Ordering::SeqCst);
}

pub(crate) enum Watch {
    PerRequest,
    Dag,
    Dnotify { fd: libc::c_int },
    Inotify { fd: libc::c_int },
}

impl Drop for Watch {
    fn drop(&mut self) {
        match self {
            Watch::Dnotify { fd } | Watch::Inotify { fd } => unsafe {
                libc::close(*fd);
            },
            _ => {}
        }
    }
}

// Safety: the descriptors are only read through `&self` under the cache's
// existing locking, and the signal handler touches nothing but an atomic.
unsafe impl Send for Watch {}
unsafe impl Sync for Watch {}

/// Not exposed by the `libc` crate; values read from <fcntl.h> on this kernel.
const F_SETSIG: libc::c_int = 10;
const DN_MODIFY: libc::c_int = 0x2;
const DN_CREATE: libc::c_int = 0x4;
const DN_DELETE: libc::c_int = 0x8;
const DN_RENAME: libc::c_int = 0x10;
const DN_MULTISHOT: libc::c_int = 0x8000_0000u32 as libc::c_int;

fn dnotify_signal() -> libc::c_int {
    libc::SIGRTMIN() + 1
}

impl Watch {
    /// Establish the watch. Errors here are setup failures — the directory
    /// cannot be opened, the kernel rejects the request — not a verdict on
    /// whether events will subsequently arrive.
    pub(crate) fn arm(strategy: Invalidation, root: &Path) -> Result<Watch, String> {
        match strategy {
            Invalidation::PerRequest => Ok(Watch::PerRequest),
            Invalidation::Dag => Ok(Watch::Dag),
            Invalidation::Dnotify => arm_dnotify(root),
            Invalidation::Inotify => arm_inotify(root),
        }
    }

    /// Whether the environment must be rebuilt. Called exactly ONCE per
    /// render: the kernel strategies consume what they report.
    pub(crate) fn stale(&self, by_mtime: impl FnOnce() -> bool) -> bool {
        match self {
            Watch::PerRequest => true,
            Watch::Dag => by_mtime(),
            Watch::Dnotify { .. } => DNOTIFY_FIRED.swap(false, Ordering::SeqCst),
            Watch::Inotify { fd } => drain_inotify(*fd),
        }
    }
}

fn arm_dnotify(root: &Path) -> Result<Watch, String> {
    let cpath = std::ffi::CString::new(root.as_os_str().as_encoded_bytes())
        .map_err(|e| format!("dnotify: bad root path: {e}"))?;
    unsafe {
        // Signal context: the handler sets a flag and does nothing else.
        if libc::signal(dnotify_signal(), dnotify_handler as extern "C" fn(libc::c_int) as libc::sighandler_t) == libc::SIG_ERR {
            return Err("dnotify: cannot install signal handler".to_owned());
        }
        let fd = libc::open(cpath.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY);
        if fd < 0 {
            return Err(format!("dnotify: cannot open {} as a directory", root.display()));
        }
        if libc::fcntl(fd, F_SETSIG, dnotify_signal()) < 0 {
            libc::close(fd);
            return Err("dnotify: F_SETSIG failed".to_owned());
        }
        let mask =
            DN_MODIFY | DN_CREATE | DN_DELETE | DN_RENAME | DN_MULTISHOT;
        if libc::fcntl(fd, libc::F_NOTIFY, mask) < 0 {
            libc::close(fd);
            return Err(format!(
                "dnotify: F_NOTIFY failed on {} — the kernel may lack CONFIG_DNOTIFY",
                root.display()
            ));
        }
        Ok(Watch::Dnotify { fd })
    }
}

fn arm_inotify(root: &Path) -> Result<Watch, String> {
    let cpath = std::ffi::CString::new(root.as_os_str().as_encoded_bytes())
        .map_err(|e| format!("inotify: bad root path: {e}"))?;
    unsafe {
        let fd = libc::inotify_init1(libc::IN_NONBLOCK);
        if fd < 0 {
            return Err("inotify: inotify_init1 failed".to_owned());
        }
        if libc::inotify_add_watch(fd, cpath.as_ptr(), libc::IN_ALL_EVENTS) < 0 {
            libc::close(fd);
            return Err(format!("inotify: cannot watch {}", root.display()));
        }
        Ok(Watch::Inotify { fd })
    }
}

fn drain_inotify(fd: libc::c_int) -> bool {
    let mut buf = [0u8; 8192];
    let mut any = false;
    loop {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n > 0 {
            any = true;
            continue;
        }
        return any;
    }
}
