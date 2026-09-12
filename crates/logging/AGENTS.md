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
- `src/designator.rs` — common designator consts, `custom!`, and the
  `error!/warn!/info!/debug!/trace!` emission macros (designator as first arg).
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
- THE REFUSAL IS A LOG LINE, NOT PROSE ON STDERR (R50a). Logging still comes
  up — as `LogConfig::default()`, the JSON default, ignoring whatever was set —
  emits exactly ONE ERROR `startup` line in the same shape as every other
  line, with `variable`, `value`, `accepted` and `detail` as FIELDS, then
  exits 1.
- The designator is an event FIELD, never the tracing target (R28). The target
  is the module path, so `RUST_LOG` behaves as standard tracing. Designators
  must stay compile-time `&'static str` (so `custom!` uses `concat!`).
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
