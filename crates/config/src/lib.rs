//! The public surface and the load pipeline: schema → args → merge → typed
//! parse, and the process-level `load` that prints or refuses. Assembly only —
//! spellings are path.rs, sources are layers.rs/args.rs/file.rs, parsing is
//! values.rs, rendering is help.rs. A consumer's own config STRUCT never
//! lives in this crate; only the shared `Common` section will (follow-up).
//!
//! ```
//! use common_config::{Config, Outcome};
//!
//! #[derive(Config)]
//! #[config(app = "DEMO")]
//! struct Demo {
//!     /// Socket address to listen on.
//!     #[config(default = "0.0.0.0:8080")]
//!     bind: std::net::SocketAddr,
//! }
//!
//! let args = ["--bind=127.0.0.1:9000".to_string()];
//! match common_config::load_from::<Demo>(&args, &[]).unwrap() {
//!     Outcome::Config(demo) => assert_eq!(demo.bind.port(), 9000),
//!     _ => unreachable!(),
//! }
//! ```

mod args;
mod file;
mod help;
mod layers;
mod path;
mod schema;
mod values;

pub use common_config_derive::Config;
pub use common_logging::Refusal;
pub use path::Path;
pub use schema::{Entry, Field, FileStatus, Kind, Origin, Presence};
pub use values::Values;

/// Implemented by `#[derive(Config)]`. `schema` registers this struct's leaves
/// under `prefix`; `from_values` reads them back. A `#[config(nested)]` field
/// calls the inner type's impl with `prefix.child(name)` — that is the whole
/// composition mechanism, so no derive ever sees another struct's fields.
pub trait Config: Sized {
    fn schema(prefix: &Path) -> Vec<Field>;
    fn from_values(values: &Values, prefix: &Path) -> Result<Self, Refusal>;
}

/// The struct carrying `#[config(app = "…")]`: the env prefix and the binary
/// name shown by `--help`. Only a `Root` can be loaded.
pub trait Root: Config {
    const APP: &'static str;
    const BIN: &'static str;
}

#[derive(Debug)]
pub enum Outcome<T> {
    Config(T),
    /// `--help` text; print it and exit 0.
    Help(String),
    /// `--print-config` text; print it and exit 0.
    PrintConfig(String),
}

/// The pure pipeline: `args` is argv without the program name; `env` is the
/// process environment. Tests drive this directly.
pub fn load_from<T: Root>(
    args: &[String],
    env: &[(String, String)],
) -> Result<Outcome<T>, Refusal> {
    let fields = T::schema(&Path::root());
    let parsed = args::parse(&fields, args)?;
    if parsed.help {
        return Ok(Outcome::Help(help::help(T::APP, T::BIN, &fields)));
    }
    let print_config = parsed.print_config;
    let values = layers::merge(T::APP, fields, parsed, env)?;
    if print_config {
        return Ok(Outcome::PrintConfig(help::print_config(&values)));
    }
    T::from_values(&values, &Path::root()).map(Outcome::Config)
}

/// Loads from the real argv and environment. `--help` and `--print-config`
/// print to stdout and exit 0; any refusal goes through
/// `common_logging::refuse!` (one `startup` ERROR line, exit 1).
pub fn load<T: Root>() -> T {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env: Vec<(String, String)> = std::env::vars_os()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        })
        .collect();
    match load_from::<T>(&args, &env) {
        Ok(Outcome::Config(config)) => config,
        Ok(Outcome::Help(text)) | Ok(Outcome::PrintConfig(text)) => {
            print!("{text}");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            std::process::exit(0)
        }
        Err(refusal) => common_logging::refuse!(refusal),
    }
}
