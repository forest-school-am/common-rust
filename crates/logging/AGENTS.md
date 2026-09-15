# AGENTS.md — common-logging

## Purpose
The Les stand's shared logging crate (CODESTYLE.md §8.5): the single owner of
subscriber setup, designators, emission macros, the `c-` custom helper, and
the request-span helper. Every Les Rust crate depends on it, `common-oidc`
included. Change here ripples fleet-wide — treat the output contract as public.

## Layout
- `src/lib.rs` — public API + `init`/`init_with` (builds the subscriber);
  inline format/designator tests.
- `src/refuse.rs` — `refuse!`, `__refusal_line!` and the exit: the refusal
  path only.
- `src/config.rs` — `LogConfig::resolve` (pure `LOG_FORMAT`/`DEPLOYMENT_TYPE`/
  `RUST_LOG`/`LOG_DESIGNATORS` → `Result<LogConfig, Refusal>`) + one
  refuse/default test per variable. `env_filter()` is the `RUST_LOG` check.
- `src/filter.rs` — `Designators`, the `LOG_DESIGNATORS` axis: parsing,
  validation, and the `Filter` impl that reads the designator FIELD. Anything
  about which events pass goes here; what a designator means stays in
  designator.rs.
- `src/designator.rs` — the `Designator` strum enum (six stand variants +
  `Custom(String)`, `Display` = the column string, `From<impl Into<String>>` =
  the custom path), the `AUTH`…`STARTUP` consts and `STAND`.
- `src/macros.rs` — emission: the five level primitives
  (`info!(designator | …)`), the level modules `error`/`warn`/`info`/`debug`/
  `trace` each holding `auth!`…`startup!` + `custom!(tag | …)` (thirty static
  macros from one generating macro, hidden root names re-exported by
  single-segment `pub use`), and the `#[deprecated]` first-argument aliases
  kept for one release.
- `src/span.rs` — `gen_reqid`, the `request_span!` MACRO, `set_actor`.
- `src/format.rs` — the human `FormatEvent` and the `CaptureLayer` that
  snapshots `reqid`/`actor` for it. (JSON uses the stock layer.)

## Invariants (do not break without a canon change)
- Human layout is EXACTLY `timestamp level designator file:row reqid [actor]
  message`, single-space separated, one line per event. The literal-shape
  tests lock it.
- SET-BUT-INVALID REFUSES TO START (R50, canon §4.4y): all four of
  `LOG_FORMAT`, `DEPLOYMENT_TYPE`, `RUST_LOG` and `LOG_DESIGNATORS`. Unset is a
  documented default (`json`, `dev`, deployment-derived, pass-everything).
- `STARTUP` is the sixth STAND designator (R51): startup checks — config,
  classification, boot validation, the process deciding whether it comes up.
  It is the user's own ruling, so no §8.3 operator confirmation. It is ORDINARY
  vocabulary on the designator axis, not an exemption: `LOG_DESIGNATORS` can
  filter it like any other.
- THE REFUSAL IS A LOG LINE, NOT PROSE ON STDERR (R50a/R52, canon §8.3a):
  exactly ONE ERROR `startup` JSON line with `variable`, `value`, `accepted`
  and `detail` as FIELDS, then exit 1. It IGNORES BOTH FILTER AXES — a refusal
  `RUST_LOG` or `LOG_DESIGNATORS` can silence is not a refusal — so it is
  written through a scoped unfiltered subscriber rather than the installed one,
  and is always JSON whatever `LOG_FORMAT` says. Ordinary `startup` lines stay
  filterable. Scope is `__refusal_line!` alone; do not widen it.
  `tests/refusal_bypasses_filters.rs` spawns the `refusal_probe` binary —
  a consumer in miniature, `init()` then `refuse!` from its own module —
  because the window R52 closes only exists AFTER init() installed the real
  filter, so an in-process test or one driving init()'s own refusal passes
  either way.
- The designator is an event FIELD, never the tracing target (R28). The target
  is the module path, so `RUST_LOG` behaves as standard tracing. The field is
  recorded as `%designator` — `Display` of the enum IS the contract (`auth`,
  …, `c-<name>`), and `FromStr` must keep reading exactly what `Display`
  writes, since `LOG_DESIGNATORS` parses through it. `FromStr` is hand-written:
  strum's `EnumString` also emits `TryFrom<&str>`, which collides with the
  blanket `From<impl Into<String>>` through core's `TryFrom`.
- The level primitives `error!`…`trace!` are hand-written and the thirty
  static macros are generated. That split is forced: a macro-expanded
  `#[macro_export]` cannot be reached by absolute path (``, `crate::`)
  from inside this crate, so the generated ones are re-exported by
  single-segment `pub use` (textual scope) and tested only from
  `tests/levels.rs`, and the primitives they expand to must not themselves be
  generated.
- TWO filtering axes, ANDed and independent: `RUST_LOG` (module path) and
  `LOG_DESIGNATORS` (designator). Unset `LOG_DESIGNATORS` must pass
  EVERYTHING, or the two ANDs resolve to silence. An event with no designator
  — anything from a dependency — always passes the designator axis.
- `request_span!` and `refuse!` are MACROS and must stay macros (§1.3b): they
  expand at the ADOPTER's call site, so the span and the refusal carry the
  adopter's module path rather than this crate's.
  `tests/request_span_callsite.rs` asserts both from outside the crate, which
  is the only place either can be asserted.
- `actor` renders `-` until `set_actor`; `reqid` is per request.
- No network, no IO clients — this crate stays dependency-light (tracing,
  tracing-subscriber, time, and strum + strum_macros/heck for the §4.5
  declare-once mechanism, R17a). Adding a dep here taxes the whole fleet.

## Run / test
`nix develop --impure -c cargo test` at the WORKSPACE root (frozen 1.98.0
toolchain; the flake sets `CARGO_TARGET_DIR=/home/dev/.cache/common-rust-target`
for all three members). No binary — library only; `cargo build` is the build
path (R11(a)), there is no `nix build`.

## Stand context
Implements DECISIONS.md R6 / CODESTYLE.md §8. Builds are serialized under R4's
disk regime — coordinate a slot with the manager session before compiling.
Every Les Rust crate and binary logs through this crate, `common-oidc` included
(§8.5) — no consumer emits raw `tracing` targets.
