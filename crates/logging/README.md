# common-logging

The Les stand's shared logging crate — the single owner of CODESTYLE.md §8
mechanics, so nothing drifts per repo. Every Les binary and library depends on
it, `common-oidc` included (which logs through here, not raw `tracing`).

- **Version:** `0.3.0` · **Toolchain:** Rust 1.98.0.
- Member of the `common-rust` workspace (`crates/logging`), alongside
  `common-templating` and `common-oidc`.

## Depend on it

Consumer manifests declare a git dependency:

```toml
[dependencies]
common-logging = { git = "https://github.com/forest-school-am/common-rust-logging.git", tag = "v0.1.1" }
```

**That URL and tag are documentation, not a pin, and no such remote exists** —
nothing in this fleet is pushed (R21), and `common-rust` has no remote at all.
The tag predates the workspace merge; do not reason about behaviour from it.

What actually resolves the dependency is the single shared cargo patch at
`/mnt/host/workspace/Les/.cargo/config.toml` (R22a/R22b), which redirects the
URL above to this workspace. Cargo walks up from the build directory and MERGES
that file, so it already applies to every repo under `Les/`: there is nothing to
symlink, and no repo may keep a `.cargo/config.toml` of its own. You therefore
always build whatever `common-rust` currently is.

A missing or wrong path in that file does not fail — cargo silently falls back
to the published crate and rewrites your lockfile to say so. Run
`sh stand/check-cargo-patch.sh` if a build behaves oddly.

## Use

One call at the top of `main`. `boot` loads the binary's `#[derive(Config)]`
tree through common-config (defaults < file < env < args), refuses any config
fault as the `startup` line below, brings the subscriber up from the tree's
`deployment: Deployment` field and its `#[config(nested)] log: Log` section
(`DEPLOYMENT_TYPE`, `LOG_FORMAT`, `LOG_DESIGNATORS`, plus `RUST_LOG` from
the environment) and returns the config:

```rust
fn main() {
    let config: MyConfig = common_logging::boot();
    // …
}
```

`init()` is the environment-only form for a binary that has not moved to
common-config yet (reads the four variables itself; `DEPLOYMENT_TYPE` is
required there too). `Refusal` and `Deployment` are `common_config`'s,
re-exported here under their old paths; `Log` and `Format` are this crate's.

Emit through `log::<level>::<designator>!` — the level is the module, the
**designator** (§8.3) is the macro, and it becomes the event's `designator`
field:

```rust
use common_logging as log;
// tracing idiom: structured fields FIRST, then the message string.
// The human formatter DE-DUPLICATES span-appended fields by name (event
// wins, then inner-most span) — do not hand-dedupe, but DO reduce
// Option/Result before capture: `?opt` prints Rust syntax like Some(0).
log::info::auth!(user = %username, "signed in");
log::warn::upstream!(attempt = n, "retry");
```

Five level modules (`error`, `warn`, `info`, `debug`, `trace`), each with
`auth!`, `business!`, `upstream!`, `storage!`, `http!`, `startup!` and
`custom!` (below). The module is named inside this crate, so `log::` is
whatever you `use … as`; no repo depends on the `log` crate.

Underneath sits one public primitive per level, `log::info!(designator | …)`,
for a designator held in a variable: the `|` separates it from the fields,
and what precedes it must be a path or a string literal (macro_rules permits
`|` after those two fragments only — a call expression needs a `let` first).
It takes anything `Into<Designator>`: the `AUTH`…`STARTUP` consts, a
`Designator`, or a string, which becomes a custom one.

The old first-argument form, `info!(AUTH, …)`, and `custom!("name")` as a
value still compile for ONE release, `#[deprecated]`; the fleet has no uses
left. They go next release.

**Map `Option`/`Result` before capturing them.** A `?`-captured (Debug) value
prints Rust syntax — `exit_code = ?maybe` logs `exit_code=Some(0)`, which reads
badly in a log viewer. Reduce it to a plain value at the call site:
`exit_code = maybe.unwrap_or(-1)`, or use `%` (Display) for types that have it.
common-logging can't intercept tracing's `?`/`%` sigils, so this is on the caller.

Open the **request root span** (§8.2) in your HTTP middleware; it carries
`reqid` and `actor` into every event beneath it:

```rust
let reqid = common_logging::gen_reqid();
let span  = common_logging::request_span!(&reqid);
// once identity resolves (the single resolution point, §5.2):
common_logging::set_actor(&span, &username);
// then run the handler inside the span (enter it, or `.instrument(span)`).
```

Per §8.2, instrument subsystem entry points, every async fn, and every spawned
future (`.instrument(span)`) — not five-line pure helpers.

## Designators (§8.3)

`Designator` is a strum enum: `Auth`, `Business`, `Upstream`, `Storage`,
`Http`, `Startup` and `Custom(String)`. `Display` writes the column string
(`auth`, …, `c-<name>`), `FromStr` reads it back, and `AUTH`…`STARTUP` are
consts for the six. Common vocabulary — prefer it, it covers most events:

| macro | column | for |
|---|---|---|
| `auth!` (`AUTH`) | `auth` | authentication / authorization / identity |
| `business!` (`BUSINESS`) | `business` | domain logic |
| `upstream!` (`UPSTREAM`) | `upstream` | calls to another service (IdP, DB, remote API) |
| `storage!` (`STORAGE`) | `storage` | persistence / caches / files |
| `http!` (`HTTP`) | `http` | request lifecycle |
| `startup!` (`STARTUP`) | `startup` | startup checks: config, classification, boot validation — the process deciding whether it comes up |

### Custom designators

A project MAY add its own where the common set genuinely doesn't fit. The
tag goes before the `|` in `custom!`, as a string literal or as a path — a
repo that uses a tag more than a couple of times declares a small strum enum
for its tags, plus the three-line `String` conversion that puts it on the
`Into<Designator>` path:

```rust
log::info::custom!("scheduler" | run_id = %id, "run started");  // designator "c-scheduler"

#[derive(strum::Display)]
enum Tag { #[strum(serialize = "scheduler")] Scheduler }
impl From<Tag> for String { fn from(t: Tag) -> String { t.to_string() } }
log::debug::custom!(Tag::Scheduler | run_id = %id, "tick");     // designator "c-scheduler"
```

The `c-` prefix is added by the crate (`Designator::from("scheduler")` is
`Custom("c-scheduler")`) and keeps project vocabulary visually distinct from
stand vocabulary. **Rules (§8.3):** every custom designator MUST be listed and
explained in its repo's README; one proposed by an LLM/agent MUST be
operator-confirmed before it lands.

_common-logging itself defines no custom designators. First known customers:
cron-viewer (`c-scheduler`, run-lifecycle); les-registry maps onto
`upstream`+`http` with none needed._

## Formats (§8.4)

`LOG_FORMAT` selects output (default `json`):

- `human`: `timestamp level designator file:row reqid [actor] message` — one
  line per event, nested-span fields appended at the end.
- `json` (default): tracing-subscriber's JSON layer, one object per line,
  event fields flattened, `reqid`/`actor` under `span` — standard-viewer
  readable.

`DEPLOYMENT_TYPE=prod|dev` (default `dev`) sets the default verbosity (`info`
vs `debug`) when `RUST_LOG` is unset; `RUST_LOG` overrides.

## Filtering: two independent axes

The designator is an event FIELD, not the tracing target, so the two axes do
not compete for one slot. An event must pass BOTH.

| variable | selects on | example |
|---|---|---|
| `RUST_LOG` | module path — standard tracing | `RUST_LOG=my_service=debug,sqlx=warn` |
| `LOG_DESIGNATORS` | designator | `LOG_DESIGNATORS=upstream=debug,business=info` |

`LOG_DESIGNATORS` takes `designator=level` pairs and an optional bare level
covering the designators not named (`LOG_DESIGNATORS=warn,auth=debug`). Unset
means everything passes — it has to, or setting only `RUST_LOG` would AND
itself to nothing.

An unknown designator or level is REFUSED rather than ignored, since a filter
that silently matches nothing is the defect this variable exists to remove.

## Set-but-invalid refuses to start (R50)

All four variables behave the same way. **Unset** means the documented default.
**Set to something unrecognised** means the process does not start:

| variable | unset | set-but-invalid |
|---|---|---|
| `LOG_FORMAT` | `json` | refuses |
| `DEPLOYMENT_TYPE` | `dev` | refuses |
| `RUST_LOG` | `info` under prod, `debug` under dev | refuses |
| `LOG_DESIGNATORS` | everything passes | refuses |

**The refusal is a log line, not prose on stderr (R50a/R52, §8.3a).** Exactly
ONE `ERROR` JSON line in the same shape as every other line, then exit 1. The
parts are FIELDS, so anything already parsing this crate's output can read a
refusal with no special case:

| field | |
|---|---|
| `level` | `ERROR` |
| `designator` | `startup` |
| `variable` | the spelling that was rejected: an environment variable, a flag, a `file: key` |
| `value` | what it was set to |
| `accepted` | what would have been accepted |
| `detail` | the underlying parser's own error, or `-` — for `RUST_LOG` it is tracing's message, the only part that says WHERE the filter is wrong |
| `target` | the module that raised it |

A service that wants to handle the refusal itself rather than exit calls
`LogConfig::from_log(&log, deployment, rust_log)` / `LogConfig::from_env()` (or
`Format`/`Deployment::parse`, or `Designators::parse`) and gets a [`Refusal`]
as the `Err`: the same parts as fields, and `Display` renders them as one
sentence.

## Refusing your own boot (R51)

Type and range faults are refused by `boot` before `main` sees the config. A
service's own post-load checks (a taken port, two options that exclude each
other, a directory it cannot create) take the same shape — one `startup`
line, then exit 1 — so build a `Refusal` and hand it to `refuse!` AT THE SITE,
which is what stamps the service's own target on the line:

```rust
fn main() {
    let config: Registry = common_logging::boot();
    let listener = match std::net::TcpListener::bind(&config.bind) {
        Ok(listener) => listener,
        Err(e) => common_logging::refuse!(
            common_logging::Refusal::new(
                "REGISTRY_BIND",
                config.bind.clone(),
                r#"a free socket address such as "0.0.0.0:8080""#,
            )
            .with_detail(e.to_string())
        ),
    };
    // …
}
```

```
{"level":"ERROR","message":"refusing to start: invalid configuration",
 "designator":"startup","variable":"REGISTRY_BIND","value":"not-an-address",
 "accepted":"a socket address such as \"0.0.0.0:8080\"","detail":"-",
 "target":"my_service"}
```

**Neither filter axis can silence it (R52, §8.3a).** `RUST_LOG` scoped to
another module and `LOG_DESIGNATORS` excluding `startup` both leave the
refusal intact: it is written through a scoped unfiltered subscriber, so it is
always JSON whatever `LOG_FORMAT` says. Ordinary `startup` events stay
filterable like everything else.

## Test

`cargo test` — one refuse/default test per environment variable, plus
literal-shape tests for both output formats and the `c-` designator behaviour
(no network). `tests/levels.rs` drives every level module from outside the
crate, the way a consumer does — the thirty static macros are macro-expanded
`macro_export`s, which is the one path rustc lets this crate itself not take.
