# AGENTS.md — common-logging

## Purpose
The Les stand's shared logging crate (CODESTYLE.md §8.5): the single owner of
subscriber setup, designators, emission macros, the `c-` custom helper, and
the request-span helper. Every Les Rust crate depends on it, `common-oidc`
included. Change here ripples fleet-wide — treat the output contract as public.

## Layout
- `src/lib.rs` — public API + `init`/`init_with` (builds the subscriber);
  inline format/designator tests.
- `src/config.rs` — `LogConfig::resolve` (pure `LOG_FORMAT`/`DEPLOYMENT_TYPE`/
  `RUST_LOG` → format/deployment/filter) + matrix tests.
- `src/designator.rs` — common designator consts, `custom!`, and the
  `error!/warn!/info!/debug!/trace!` emission macros (designator as first arg).
- `src/span.rs` — `gen_reqid`, `request_span`, `set_actor`.
- `src/format.rs` — the human `FormatEvent` and the `CaptureLayer` that
  snapshots `reqid`/`actor` for it. (JSON uses the stock layer.)

## Invariants (do not break without a canon change)
- Human layout is EXACTLY `timestamp level designator file:row reqid [actor]
  message`, single-space separated, one line per event. The literal-shape
  tests lock it.
- Default format is `json`; unknown `LOG_FORMAT` degrades to json (logging
  must always come up). Default `DEPLOYMENT_TYPE` is `dev`.
- The designator is the tracing target; it must stay a compile-time
  `&'static str` (so `custom!` uses `concat!`, and consts are `&str`).
- `actor` renders `-` until `set_actor`; `reqid` is per request.
- No network, no IO clients — this crate stays dependency-light (tracing,
  tracing-subscriber, time). Adding a dep here taxes the whole fleet.

## Run / test
`nix develop --impure -c cargo test` (frozen 1.98.0 toolchain;
`CARGO_TARGET_DIR=/home/dev/.cache/common-logging-target`). No binary — library
only; nothing to `nix build`.

## Stand context
Implements DECISIONS.md R6 / CODESTYLE.md §8. Builds are serialized under R4's
disk regime — coordinate a slot with the manager session before compiling.
Every Les Rust crate and binary logs through this crate, `common-oidc` included
(§8.5) — no consumer emits raw `tracing` targets.
