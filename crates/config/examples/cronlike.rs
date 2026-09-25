//! A cron-shaped consumer in miniature: nesting levels, a secret, a boolean,
//! the root's `deployment` and `log` fields. Run it with `--help`,
//! `--print-config`, flags, `CRON_…` variables or `--config file.toml` to see
//! the crate's outputs. A real binary calls `common_logging::boot` instead of
//! `load`; this crate cannot depend on logging, so it prints the refusal.

use std::net::SocketAddr;
use std::path::PathBuf;

use common_config::{Config, Deployment};

#[derive(Debug, Config)]
#[config(app = "CRON", bin = "cron")]
struct Cron {
    /// Directory holding the SQLite database and per-run logs.
    #[config(required)]
    data_dir: PathBuf,
    /// Socket address the HTTP server listens on.
    #[config(default = "0.0.0.0:8080")]
    bind: SocketAddr,
    #[config(nested)]
    auth: Auth,
    #[config(nested)]
    sandbox: Sandbox,
    deployment: Deployment,
    #[config(nested)]
    log: Log,
}

#[derive(Debug, Config)]
struct Auth {
    /// UUID of the authentik group whose members may edit tasks.
    #[config(required, secret)]
    group_uuid: String,
    /// Serve without authentication (dev only).
    no_auth: bool,
}

#[derive(Debug, Config)]
struct Sandbox {
    /// Wall-clock limit for one task run, in seconds.
    #[config(default = "3600")]
    timeout_secs: u64,
    /// Disk budget for one task's working directory, in MiB.
    #[config(default = "100")]
    budget_mb: u64,
    #[config(nested)]
    limits: Limits,
}

#[derive(Debug, Config)]
struct Limits {
    /// CPU seconds one run may consume.
    #[config(default = "600")]
    cpu_secs: u64,
    /// Resident memory one run may hold, in MiB.
    #[config(default = "512")]
    memory_mb: u64,
}

/// Stands in for `common_logging::Log`, which this crate cannot depend on.
#[derive(Debug, Config)]
struct Log {
    /// Log line format: json (one object per line) or human.
    #[config(default = "json", env = "LOG_FORMAT")]
    format: String,
    /// Per-designator level filter such as "auth=debug"; unset passes every designator.
    #[config(env = "LOG_DESIGNATORS")]
    designators: Option<String>,
}

fn main() {
    let cron = match common_config::load::<Cron>() {
        Ok(cron) => cron,
        Err(refusal) => {
            eprintln!("{refusal}");
            std::process::exit(1)
        }
    };
    println!(
        "cron: data_dir={} bind={} auth.group_uuid=<{} chars> auth.no_auth={}",
        cron.data_dir.display(),
        cron.bind,
        cron.auth.group_uuid.len(),
        cron.auth.no_auth
    );
    println!(
        "sandbox: timeout_secs={} budget_mb={} limits.cpu_secs={} limits.memory_mb={}",
        cron.sandbox.timeout_secs,
        cron.sandbox.budget_mb,
        cron.sandbox.limits.cpu_secs,
        cron.sandbox.limits.memory_mb
    );
    println!(
        "deployment={} log.format={} log.designators={}",
        cron.deployment,
        cron.log.format,
        cron.log.designators.as_deref().unwrap_or("-")
    );
}
