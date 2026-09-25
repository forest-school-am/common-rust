# common-config

Layered configuration for Les stand binaries (CODESTYLE.md §4.3/§4.4): ONE
`#[derive(Config)]` schema, from which every value's three spellings — TOML
key, environment variable, command-line flag — are GENERATED, never parsed
apart. R114 item 9.

- **Version:** `0.4.0` · **Toolchain:** Rust 1.98.0.
- Members of the `common-rust` workspace: `crates/config` (this crate) and
  `crates/config-derive` (the proc macro, re-exported as
  `common_config::Config`).
- Depends on NOTHING else in the workspace. `Refusal` and `Deployment` live
  here; `common-logging` depends on this crate, re-exports both under their
  old paths, reads its own two log variables from the environment, and owns the
  one call a binary makes: `common_logging::boot_sealed::<T>()`.

## Design

**Schema.** Plain structs derive `Config`. Field attributes: the doc comment is
the help text; `#[config(default = "…")]` (a string, parsed at load like any
other source); `#[config(required)]`; `Option<T>` fields are optional; a `bool`
defaults to `false`; `#[config(secret)]` masks the value in `--print-config`
and in a wrong-type refusal; `#[config(nested)]` for a field whose type also
derives `Config`; `#[config(accepted = "…")]` names the
accepted values in a refusal (for strum enums and anything whose `FromStr`
error is not self-explanatory). The root carries `#[config(app = "CRON", bin =
"cron")]` — env prefix and help title — plus ONE field the derive reads by
NAME:

- `deployment: Deployment` — spelled `DEPLOYMENT_TYPE` / `--deployment` /
  `deployment`, REQUIRED (there is no default class), help text and accepted
  values supplied by this crate. It is the one spelling that keeps its bare
  name instead of taking the app prefix, because one variable across every
  service is the point of it; no field attribute can ask for the same.

There is NO `log` section. `LOG_FORMAT` and `LOG_DESIGNATORS` are
common-logging's own and it reads them from the environment itself, so no app
declares or threads them. `RUST_LOG` is likewise tracing's own.

**Deployment classes (§4.4).** An optional value is classified in code:
`#[config(prod_required)]` on an `Option<T>` — unset under `prod` refuses,
under `dev` it is simply absent; `#[config(dev_only)]` on an `Option<T>` or
`bool` — set (a bool: true) under `prod` refuses. Everything else is neutral.
The class is data on the `Field` (`Class`), prints in `--help` as the tail
(`prod-required` / `dev-only`), and is checked once after the merge against
the root's `deployment`, so a consumer's own `validate` is left with cross-field
rules only.

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
`<APP>_CONFIG`; a named file that cannot be read — not found included — or
does not parse refuses) < environment < arguments. Each layer yields `(path,
text, origin)`; leaves are parsed ONCE after the merge with `FromStr` (`bool`
also takes `1/0/yes/no/on/off`; strum `EnumString` types work as-is), so every
value knows its `Origin` (`Default` / `File(path)` / `Env(NAME)` /
`Arg(--flag)`) and there is no nested-deserialize problem. Unknown TOML keys
and unknown flags refuse, naming them. An EMPTY value is a set value
(`FOO=` is not "unset"): set-but-invalid refuses.

**Arguments** are parsed by this crate (`--k=v`, `--k v`, bare `--k` for
bools, `--help`/`-h`, `--print-config`, `--config PATH`); clap is not used,
for rendering either: the help's whole point is a flag / env / TOML-key /
default column per line grouped by TOML table, which clap's template cannot
lay out, and the renderer is shorter than the customisation would be.

**Refusals** are `common_config::Refusal { variable, value, accepted, detail
}`: a missing required value names all three spellings; a wrong-typed value
names the SOURCE that set it (`--sandbox--timeout-secs`, `CRON_BIND`,
`cron.toml: sandbox.timeout_secs`) and the offending text. Printing one is a
log line, so it is `common_logging::refuse!` (one `startup` ERROR JSON line,
exit 1) — which is what `boot_sealed` does. `--help` and `--print-config` print and
exit 0.

**API.** `common_logging::boot_sealed::<T>()` for `main` (load, refuse, init
logging, return `T`); `common_config::load::<T>() -> Result<T, Refusal>`
underneath it (prints help / print-config and exits 0, RETURNS a refusal);
`load_from::<T>(args, env) -> Result<Outcome<T>, Refusal>` is the pure
pipeline tests drive.

## The example struct

`examples/cronlike.rs` (an example cannot depend on logging, so it declares a
stand-in `Log` and prints the refusal's `Display` form; a binary uses `boot`
and gets the JSON line):

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
    deployment: Deployment,
    #[config(nested)]
    log: common_logging::Log,
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

fn main() {
    let cron: Cron = common_logging::boot_sealed(); // in a real binary
    // …
}
```

## Captured outputs

Captured verbatim from `cargo run --example cronlike -- …` (the binary
invoked directly, `env -i` so only the variables shown are set), 2026-09-25.

### `--help`

```
$ cronlike --help
cron: every value has a flag, an environment variable and a TOML key.
Later wins: defaults < config file < environment < arguments.

  --config <path>                       CRON_CONFIG                                    TOML file to read; a missing, unreadable or malformed one refuses
  --help                                                                               print this text and exit
  --print-config                                                                       print every value with its origin (secrets masked) and exit

top level
  --data-dir <value>                    CRON_DATA_DIR                    data_dir      required
      Directory holding the SQLite database and per-run logs.
  --bind <value>                        CRON_BIND                        bind          default 0.0.0.0:8080
      Socket address the HTTP server listens on.
  --deployment <value>                  DEPLOYMENT_TYPE                  deployment    required
      Deployment class: prod or dev. Sets the default log verbosity; which options are prod-required or dev-only is the binary's own rule.

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
$ DEPLOYMENT_TYPE=dev CRON_AUTH__GROUP_UUID=3f2b0c1e-9d1a-4f5e-8f41-2c1a9b0e7d55 cronlike --config cron.toml --data-dir=/srv/cron --print-config
config file: cron.toml
data_dir                 = /srv/cron     arg --data-dir
bind                     = 0.0.0.0:8080  default
auth.group_uuid          = ****          env CRON_AUTH__GROUP_UUID
auth.no_auth             = false         default
sandbox.timeout_secs     = 60            file cron.toml
sandbox.budget_mb        = 100           default
sandbox.limits.cpu_secs  = 600           default
sandbox.limits.memory_mb = 1024          file cron.toml
deployment               = dev           env DEPLOYMENT_TYPE
log.format               = json          default
log.designators          = <unset>       optional
[exit 0]
```

### Refusals (the example's `Display` form)

```
$ DEPLOYMENT_TYPE=dev cronlike --data-dir=/srv/cron
CRON_AUTH__GROUP_UUID="unset" is not valid — expected a value: CRON_AUTH__GROUP_UUID in the environment, --auth--group-uuid on the command line, or `group_uuid = …` under [auth] in the config file (required and unset)
[exit 1]

$ DEPLOYMENT_TYPE=dev CRON_AUTH__GROUP_UUID=… cronlike --data-dir=/srv/cron --sandbox--timeout-secs=abc
--sandbox--timeout-secs="abc" is not valid — expected a u64 (set by arg --sandbox--timeout-secs: invalid digit found in string)
[exit 1]

$ DEPLOYMENT_TYPE=staging CRON_AUTH__GROUP_UUID=… cronlike --data-dir=/srv/cron
DEPLOYMENT_TYPE="staging" is not valid — expected one of ["prod", "dev"] (set by env DEPLOYMENT_TYPE: Matching variant not found)
[exit 1]

$ CRON_AUTH__GROUP_UUID=… cronlike --data-dir=/srv/cron
DEPLOYMENT_TYPE="unset" is not valid — expected a value: DEPLOYMENT_TYPE in the environment, --deployment on the command line, or `deployment = …` at the top level of the config file (required and unset)
[exit 1]

$ DEPLOYMENT_TYPE=dev CRON_AUTH__GROUP_UUID=… cronlike --data-dir=/srv/cron --config=/nonexistent.toml
--config="/nonexistent.toml" is not valid — expected the path of a readable TOML file (cannot read: No such file or directory (os error 2))
[exit 1]
```

### The same refusal through `common_logging::boot_sealed` (the real cron binary)

The four parts are FIELDS of one `startup` ERROR line, written through the
unfiltered subscriber so no `RUST_LOG` / `LOG_DESIGNATORS` can silence it;
the target is `common_logging`, where `boot` refuses (a binary's own
post-load checks refuse at their site and carry the binary's target):

```
$ CRON_AUTH__NO_AUTH=1 ASSETS_ORIGIN=https://assets.example cron --storage--task-budget-mb=heaps
{"timestamp":"2026-09-16T00:20:18.625688Z","level":"ERROR","message":"refusing to start: invalid configuration","designator":"startup","variable":"--storage--task-budget-mb","value":"heaps","accepted":"a whole number of MiB, 0 for unlimited","detail":"set by arg --storage--task-budget-mb: invalid digit found in string","target":"common_logging"}
[exit 1]
```

## Run / test

`nix develop --impure -c cargo test -p common-config -p common-config-derive`
at the workspace root; `cargo run -p common-config --example cronlike -- --help`.
