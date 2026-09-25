//! The public surface and the load pipeline: schema → args → merge → typed parse.
//! Assembly only — spellings are path.rs, sources are layers.rs/args.rs/file.rs,
//! parsing is values.rs, rendering is help.rs. `Deployment` and `Refusal` live
//! here (not a submodule elsewhere) so this crate depends on nothing else in
//! the workspace; what a value DOES (logging, auth) belongs to the crate that does it.
//!
//! ```
//! use common_config::{Config, Deployment, Outcome};
//!
//! #[derive(Config)]
//! struct Log {
//!     /// Log line format.
//!     #[config(default = "json")]
//!     format: String,
//! }
//!
//! #[derive(Config)]
//! #[config(app = "DEMO")]
//! struct Demo {
//!     /// Socket address to listen on.
//!     #[config(default = "0.0.0.0:8080")]
//!     bind: std::net::SocketAddr,
//!     deployment: Deployment,
//!     #[config(nested)]
//!     log: Log,
//! }
//!
//! let args = ["--bind=127.0.0.1:9000".to_string()];
//! let env = [("DEPLOYMENT_TYPE".to_string(), "dev".to_string())];
//! match common_config::load_from::<Demo>(&args, &env).unwrap() {
//!     Outcome::Config(demo) => assert_eq!(demo.bind.port(), 9000),
//!     _ => unreachable!(),
//! }
//! ```

// The derive spells every path as `::common_config::…`, so this crate's own
// tests can use it too.
extern crate self as common_config;

mod args;
mod class;
mod deployment;
mod file;
mod help;
mod layers;
mod path;
mod refusal;
mod schema;
mod values;

pub use common_config_derive::Config;
pub use deployment::{Deployment, DEPLOYMENT_ACCEPTED, DEPLOYMENT_HELP, DEPLOYMENT_VARIABLE};
pub use path::Path;
pub use refusal::Refusal;
pub use schema::{Class, Entry, Field, FileStatus, Kind, Origin, Presence};
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
/// name shown by `--help`. Only a `Root` can be loaded. The derive reads the
/// root's `deployment: Deployment` field (spelled `DEPLOYMENT_TYPE`, required)
/// and its `#[config(nested)] log` field, whose type is whatever the logging
/// crate asks for.
pub trait Root: Config {
    const APP: &'static str;
    const BIN: &'static str;
    type Log;
    fn deployment(&self) -> Deployment;
    fn log(&self) -> &Self::Log;
}

#[derive(Debug)]
pub enum Outcome<T> {
    Config(T),
    Help(String),
    PrintConfig(String),
}

/// `args` is argv without the program name; `env` the process environment.
pub fn load_from<T: Root>(
    args: &[String],
    env: &[(String, String)],
) -> Result<Outcome<T>, Refusal> {
    load_from_with_notices(args, env).map(|(outcome, _)| outcome)
}

/// `load_from`, plus the deprecation lines this load produced (config.6). They
/// are returned rather than logged because config is loaded BEFORE the
/// subscriber exists: `common_logging::boot` emits them once it does.
pub fn load_from_with_notices<T: Root>(
    args: &[String],
    env: &[(String, String)],
) -> Result<(Outcome<T>, Vec<String>), Refusal> {
    let fields = T::schema(&Path::root());
    let parsed = args::parse(&fields, args)?;
    if parsed.help {
        return Ok((
            Outcome::Help(help::help(T::APP, T::BIN, &fields)),
            Vec::new(),
        ));
    }
    let print_config = parsed.print_config;
    let values = layers::merge(T::APP, fields, parsed, env)?;
    let notices = values.deprecations();
    if print_config {
        return Ok((Outcome::PrintConfig(help::print_config(&values)), notices));
    }
    let config = T::from_values(&values, &Path::root())?;
    class::enforce(&values, config.deployment())?;
    Ok((Outcome::Config(config), notices))
}

pub fn load<T: Root>() -> Result<T, Refusal> {
    load_with_notices().map(|(config, _)| config)
}

/// `load`, plus the deprecation lines (config.6); see `load_from_with_notices`.
pub fn load_with_notices<T: Root>() -> Result<(T, Vec<String>), Refusal> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env: Vec<(String, String)> = std::env::vars_os()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        })
        .collect();
    match load_from_with_notices::<T>(&args, &env)? {
        (Outcome::Config(config), notices) => Ok((config, notices)),
        (Outcome::Help(text) | Outcome::PrintConfig(text), _) => {
            print!("{text}");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            std::process::exit(0)
        }
    }
}
