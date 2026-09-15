# common-config

Layered configuration for Les stand binaries (CODESTYLE.md §4.3/§4.4): ONE
`#[derive(Config)]` schema, from which every value's three spellings — TOML
key, environment variable, command-line flag — are GENERATED, never parsed
apart. R114 item 9.

- **Version:** `0.3.0` · **Toolchain:** Rust 1.98.0.
- Members of the `common-rust` workspace: `crates/config` (this crate) and
  `crates/config-derive` (the proc macro, re-exported as
  `common_config::Config`).
- Follow-up, not yet done: `Refusal` and `Deployment` move here from
  `common-logging` (which then re-exports them and takes its `LogConfig` from
  the shared `Common` section); consumers (cron, registry, les-forms, role-ui)
  migrate afterwards. Until then this crate DEPENDS on `common-logging` for
  `Refusal`, whose `&'static str` variable is leaked once on the refusal path.

## Design

**Schema.** Plain structs derive `Config`. Field attributes: the doc comment is
the help text; `#[config(default = "…")]` (a string, parsed at load like any
other source); `#[config(required)]`; `Option<T>` fields are optional; a `bool`
defaults to `false`; `#[config(secret)]` masks the value in `--print-config`
and in a wrong-type refusal; `#[config(nested)]` for a field whose type also
derives `Config`; `#[config(env = "NAME")]` / `#[config(flag = "name")]`
replace ONE generated spelling with a legacy bare name; `#[config(accepted =
"…")]` names the accepted values in a refusal (for strum enums). The root
carries `#[config(app = "CRON", bin = "cron")]`: env prefix and help title.

**Composition is at runtime, not in the macro.** The derive emits
`schema(prefix) -> Vec<Field>` and `from_values(values, prefix) -> Result<Self,
Refusal>`; a nested field calls the inner type's impl with `prefix + name`. No
macro ever inspects another struct, which is what defeated serde+clap (flatten
does not prefix; splitting env names on `_` cannot tell nesting from multi-word
names).

**Spellings** come from the field path by the separator rule: `-` inside a
name and `--` between levels for flags (`--data-dir`,
`--sandbox--timeout-secs`); `_` inside and `__` between for env, with the app
prefix once and a single `_` (`CRON_DATA_DIR`, `CRON_SANDBOX__TIMEOUT_SECS`);
TOML tables for levels (`[sandbox] timeout_secs = 3600`). Every existing
root-level env name is therefore unchanged, and every spelling is unambiguous
without the schema.

**Sources, later wins per field:** defaults < TOML file (`--config PATH` or
`<APP>_CONFIG`; optional — a missing file is fine, an unreadable or malformed
one refuses) < environment < arguments. Each layer yields `(path, text,
origin)`; leaves are parsed ONCE after the merge with `FromStr` (`bool` also
takes `1/0/yes/no`; strum `EnumString` types work as-is), so every value knows
its `Origin` (`Default` / `File(path)` / `Env(NAME)` / `Arg(--flag)`) and there
is no nested-deserialize problem. Unknown TOML keys and unknown flags refuse,
naming them.

**Arguments** are parsed by this crate (`--k=v`, `--k v`, bare `--k` for
bools, `--help`/`-h`, `--print-config`, `--config PATH`); clap is not used,
for rendering either: the help's whole point is a flag / env / TOML-key /
default column per line grouped by TOML table, which clap's template cannot
lay out, and the renderer is shorter than the customisation would be.

**Refusals** are `common_logging::Refusal` (one `startup` ERROR line, exit 1
via `refuse!`): a missing required value names all three spellings; a
wrong-typed value names the source that set it and the offending text.
`--help` and `--print-config` print and exit 0.

**API.** `common_config::load::<T>()` for `main`; `load_from::<T>(args, env) ->
Result<Outcome<T>, Refusal>` is the pure pipeline tests drive.

## The example struct

`examples/cronlike.rs`:

```rust
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
    #[config(nested)]
    common: Common,
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

#[derive(Debug, Config)]
struct Common {
    /// Deployment class, prod or dev.
    #[config(default = "dev", env = "DEPLOYMENT_TYPE")]
    deployment: String,
    /// Log line format, json or human.
    #[config(default = "json", env = "LOG_FORMAT")]
    log_format: String,
}

fn main() {
    let cron = common_config::load::<Cron>();
    // …
}
```

## Captured outputs

All four captured verbatim from `cargo run --example cronlike -- …` (the
binary invoked directly, `env -i` so only the variables shown are set).

### `--help`

```
$ cronlike --help
cron: every value has a flag, an environment variable and a TOML key.
Later wins: defaults < config file < environment < arguments.

  --config <path>                       CRON_CONFIG                                    TOML file to read; a missing file is fine, an unreadable or malformed one refuses
  --help                                                                               print this text and exit
  --print-config                                                                       print every value with its origin (secrets masked) and exit

top level
  --data-dir <value>                    CRON_DATA_DIR                    data_dir      required
      Directory holding the SQLite database and per-run logs.
  --bind <value>                        CRON_BIND                        bind          default 0.0.0.0:8080
      Socket address the HTTP server listens on.

[auth]
  --auth--group-uuid <value>            CRON_AUTH__GROUP_UUID            group_uuid    required, secret
      UUID of the authentik group whose members may edit tasks.
  --auth--no-auth                       CRON_AUTH__NO_AUTH               no_auth       default false
      Serve without authentication (dev only).

[sandbox]
  --sandbox--timeout-secs <value>       CRON_SANDBOX__TIMEOUT_SECS       timeout_secs  default 3600
      Wall-clock limit for one task run, in seconds.
  --sandbox--budget-mb <value>          CRON_SANDBOX__BUDGET_MB          budget_mb     default 100
      Disk budget for one task's working directory, in MiB.

[sandbox.limits]
  --sandbox--limits--cpu-secs <value>   CRON_SANDBOX__LIMITS__CPU_SECS   cpu_secs      default 600
      CPU seconds one run may consume.
  --sandbox--limits--memory-mb <value>  CRON_SANDBOX__LIMITS__MEMORY_MB  memory_mb     default 512
      Resident memory one run may hold, in MiB.

[common]
  --common--deployment <value>          DEPLOYMENT_TYPE                  deployment    default dev
      Deployment class, prod or dev.
  --common--log-format <value>          LOG_FORMAT                       log_format    default json
      Log line format, json or human.
[exit 0]
```

### `--print-config` with a file, one env var and one flag on different fields

`cron.toml`:

```toml
[sandbox]
timeout_secs = 60

[sandbox.limits]
memory_mb = 1024
```

```
$ CRON_AUTH__GROUP_UUID=3f2b0c1e-9d1a-4f5e-8f41-2c1a9b0e7d55 cronlike --config cron.toml --data-dir=/srv/cron --print-config
config file: cron.toml
data_dir                 = /srv/cron     arg --data-dir
bind                     = 0.0.0.0:8080  default
auth.group_uuid          = ****          env CRON_AUTH__GROUP_UUID
auth.no_auth             = false         default
sandbox.timeout_secs     = 60            file cron.toml
sandbox.budget_mb        = 100           default
sandbox.limits.cpu_secs  = 600           default
sandbox.limits.memory_mb = 1024          file cron.toml
common.deployment        = dev           default
common.log_format        = json          default
[exit 0]
```

### Refusal: a required value is missing

```
$ cronlike --data-dir=/srv/cron
{"timestamp":"2026-09-15T23:56:35.137562Z","level":"ERROR","message":"refusing to start: invalid configuration","designator":"startup","variable":"CRON_AUTH__GROUP_UUID","value":"unset","accepted":"a value: CRON_AUTH__GROUP_UUID in the environment, --auth--group-uuid on the command line, or `group_uuid = …` under [auth] in the config file","detail":"required and unset","target":"common_config"}
[exit 1]
```

### Refusal: `--sandbox--timeout-secs=abc`

```
$ CRON_AUTH__GROUP_UUID=3f2b0c1e-9d1a-4f5e-8f41-2c1a9b0e7d55 cronlike --data-dir=/srv/cron --sandbox--timeout-secs=abc
{"timestamp":"2026-09-15T23:56:35.140513Z","level":"ERROR","message":"refusing to start: invalid configuration","designator":"startup","variable":"--sandbox--timeout-secs","value":"abc","accepted":"a u64","detail":"set by arg --sandbox--timeout-secs: invalid digit found in string","target":"common_config"}
[exit 1]
```

## Run / test

`nix develop --impure -c cargo test -p common-config -p common-config-derive`
at the workspace root; `cargo run -p common-config --example cronlike -- --help`.
