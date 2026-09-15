# AGENTS.md — common-config (+ common-config-derive)

## Purpose
The stand's shared configuration loader (CODESTYLE.md §4.3/§4.4, R114 item 9):
one derived schema, three generated spellings per value (TOML key, env var,
flag), defaults < file < env < args merged per field, one typed parse after
the merge, refusal on anything set-but-invalid. Every Les binary is to load
its config through here (migration is the follow-up; none has yet).

## Layout
- `crates/config-derive/src/lib.rs` — `#[derive(Config)]`: attribute grammar,
  per-field validation (compile errors), emits `schema`/`from_values` and the
  `Root` impl. Sees ONE struct only.
- `src/lib.rs` — `Config`/`Root` traits, `Outcome`, `load_from` (pure) and
  `load` (argv + env, prints or `refuse!`s).
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
  must never read another struct's definition.
- ONE PARSE, AFTER THE MERGE. Every layer contributes text; `FromStr` runs
  once per leaf in `from_values`, so a value has exactly one `Origin` and a
  wrong type names the source that set it. A default is parsed like any other
  text — a bad default is a refusal at boot, not a compile-time value.
- SET-BUT-INVALID REFUSES (§4.3): unknown TOML key, unknown flag, unparsable
  text, malformed or unreadable file. A NOT-FOUND file is allowed (optional
  file); `--print-config` reports it as `(not found)`.
- SECRETS NEVER PRINT: `--print-config` and wrong-type refusals mask a
  `secret` field's value. `help` shows the default text, so a secret must not
  carry a real default.
- `--help` wins over every refusal (unknown flag, missing required), so a user
  who typed something wrong still sees what is accepted.
- `Refusal` is `common_logging::Refusal` until the follow-up moves it here;
  `values::variable` leaks the generated name into its `&'static str` once on
  the refusal path. Delete that helper when `variable` becomes a `String`.
- `--k v` refuses a value that starts with `--`; use `--k=v` for such values.
  A bool flag is bare (`--k`) or inline (`--k=no`); `--k no` is a stray
  positional and refuses.

## Run / test
`nix develop --impure -c cargo test -p common-config -p common-config-derive`
at the WORKSPACE root (frozen 1.98.0; `CARGO_TARGET_DIR` from the flake).
`cargo run -p common-config --example cronlike -- --help`. Tests write TOML
fixtures under `std::env::temp_dir()`; no network.

## Stand context
Implements the REVISED answer under "lets use a cli argument parsing library"
in the R114 review file. Order after this crate: Refusal + Deployment move
here → cron (16 values; the 3600 s timeout becomes `sandbox.timeout_secs`) →
registry, les-forms, role-ui. Builds are serialized under R4's disk regime.
