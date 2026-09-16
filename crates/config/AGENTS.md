# AGENTS.md — common-config (+ common-config-derive)

## Purpose
The stand's shared configuration loader (CODESTYLE.md §4.3/§4.4, R114 item 9):
one derived schema, three generated spellings per value (TOML key, env var,
flag), defaults < file < env < args merged per field, one typed parse after
the merge, refusal on anything set-but-invalid. Also the home of `Refusal`,
`Deployment`, the log `Format` and the shared `Common` section, so that this
crate depends on NOTHING else in the workspace and common-logging depends on
it. Every Les binary is to boot through `common_logging::boot::<T>()`
(cron does; registry, les-forms, role-ui are the follow-up).

## Layout
- `crates/config-derive/src/lib.rs` — `#[derive(Config)]`: attribute grammar,
  per-field validation (compile errors), emits `schema`/`from_values` and the
  `Root` impl (APP, BIN, `common()` off the field NAMED `common`). Sees ONE
  struct only.
- `src/lib.rs` — `Config`/`Root` traits, `Outcome`, `load_from` (pure) and
  `load` (argv + env; prints help / print-config and exits 0, RETURNS a
  refusal). `extern crate self as common_config` so the derive works in-crate.
- `src/refusal.rs` — `Refusal { variable: String, value, accepted, detail }`,
  `Display`. Printing one is logging's `refuse!`.
- `src/deployment.rs` — `Deployment` (strum), `parse`/`from_env`,
  `DEPLOYMENT_ACCEPTED`.
- `src/common.rs` — `Format` (strum) and `Common` (derived here: deployment /
  log_format / log_designators under the legacy bare env names).
- `src/path.rs` — `Path` and the spelling rules (`flag`, `env`, `dotted`).
- `src/schema.rs` — `Field`, `Presence`, `Kind`, `Origin`, `Entry`,
  `FileStatus`: data the derive registers and the merge stamps.
- `src/values.rs` — the merged table, the typed parse, the two leaf refusals
  (required-and-unset, wrong type), secret masking in refusals.
- `src/args.rs` — argv grammar; `src/file.rs` — TOML read + flatten;
  `src/layers.rs` — which file, and who overrides whom.
- `src/help.rs` — `--help` and `--print-config` rendering.
- `tests/load.rs` — a consumer in miniature driving `load_from` from outside
  the crate; `examples/cronlike.rs` — the README's captured outputs.

## Invariants
- SPELLINGS ARE GENERATED FROM THE PATH, NEVER PARSED APART: `-`/`_` inside a
  name, `--`/`__` between levels, app prefix once with a single `_`. No code
  may split a flag or env name to discover nesting; the schema is the only
  map. `tests/load.rs::schema_spells_every_field_three_ways` locks it.
- NESTING COMPOSES AT RUNTIME: a `nested` field calls the inner type's
  `schema(prefix + name)` / `from_values(values, prefix + name)`. The derive
  must never read another struct's definition — which is why a root's shared
  section is found by field NAME (`common`), not by type.
- ONE PARSE, AFTER THE MERGE. Every layer contributes text; `FromStr` runs
  once per leaf in `from_values`, so a value has exactly one `Origin` and a
  wrong type names the source that set it. A default is parsed like any other
  text — a bad default is a refusal at boot, not a compile-time value.
- SET-BUT-INVALID REFUSES (§4.3): unknown TOML key, unknown flag, unparsable
  text, malformed or unreadable file, an EMPTY value that does not parse. A
  NOT-FOUND file is allowed (optional file); `--print-config` reports it as
  `(not found)`.
- THE LEGACY WORDS ARE SHARED CONSTANTS: `Deployment::parse` and the derived
  `Common.deployment` refuse with the same `accepted` text
  (`DEPLOYMENT_ACCEPTED`; likewise `LOG_FORMAT_ACCEPTED`), because consumers'
  tests pin the text and must not care which path parsed it. A test in
  common.rs holds the two together.
- SECRETS NEVER PRINT: `--print-config` and wrong-type refusals mask a
  `secret` field's value. `help` shows the default text, so a secret must not
  carry a real default.
- `--help` wins over every refusal (unknown flag, missing required), so a user
  who typed something wrong still sees what is accepted.
- `--k v` refuses a value that starts with `--`; use `--k=v` for such values.
  A bool flag is bare (`--k`) or inline (`--k=no`); `--k no` is a stray
  positional and refuses.
- NO WORKSPACE DEPENDENCY. This crate must not depend on common-logging (the
  arrow points the other way); `load` returns the refusal instead of printing
  it for exactly that reason.

## Run / test
`nix develop --impure -c cargo test -p common-config -p common-config-derive`
at the WORKSPACE root (frozen 1.98.0; `CARGO_TARGET_DIR` from the flake).
`cargo run -p common-config --example cronlike -- --help`. Tests write TOML
fixtures under `std::env::temp_dir()`; no network.

## Stand context
Implements the REVISED answer under "lets use a cli argument parsing library"
in the R114 review file. Done: Refusal + Deployment here, `logging::boot`,
cron (the 3600 s timeout is `sandbox.timeout_secs`). Next: registry,
les-forms, role-ui. Consumers declare this crate by PATH
(`../common-rust/crates/config`), like common-ui-build, until push day: a
patched git source needs its original remote and none exists. Builds are
serialized under R4's disk regime.
