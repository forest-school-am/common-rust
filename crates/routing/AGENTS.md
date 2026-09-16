# AGENTS.md — common-routing (+ common-routing-macros)

## Purpose
R114 cron item 4: the HTTP path is written ONCE, in Rust, and the browser
client is generated from it. A recording `Router` (axum's shape) writes
`routes.json`; `#[client]` on each handler writes `handlers.json` through a
generated export test; `generate_client` joins the two by fqname into
`client.ts` — one typed function per handler on a ~25-line transport. ts-rs
stays for the DTOs; this crate names its types and never re-derives them.

## Layout
- `src/router.rs` — `Router<S>`: `.get/.post/.put/.delete/.patch(path, h)`
  record `type_name::<H>()` + method + path + params, then call axum;
  `.route` (unrecorded passthrough), `.nest` (prefix applied to the nested
  manifest), `.merge`, `.layer`, `.route_layer`, `.with_state`, `.manifest`,
  `.into_axum`, `.write_manifest`.
- `src/manifest.rs` — `Registration`, `parse_path_params`, `write_manifest`
  (sorted by path then method, pretty, trailing newline).
- `src/export.rs` — `Handler`/`Arg`/`Response` (the descriptor), `dir()`
  (`COMMON_ROUTING_EXPORT_DIR`), `Arg::path/query/body::<T: TS>` (ts-rs
  names and the path payload's shape), `append` (locked read-modify-write of
  `handlers.json`), `read`.
- `src/generate.rs` — `Options` (transport specifier, types specifier,
  `include` predicate), `generate_client`, the renderer and its errors.
- `../routing-macros/src/parse.rs` — the signature analysis (`describe`,
  `response_of`, `expand`), unit-tested with `parse_quote!`; `lib.rs` is the
  proc-macro shell.
- `tests/router.rs`, `tests/client_macro.rs` — consumer-side tests.

## Invariants
- THE JOIN KEY IS THE FQNAME, AND BOTH SIDES SPELL IT THE SAME WAY:
  `type_name::<H>()` for a fn item equals `module_path!()::name`. Any handler
  shape where that stops holding (a closure, a generic fn) is not supported;
  the generator errors on the unmatched side rather than guessing.
- `.get(path, handler)` RECORDS; `.route(path, get(handler))` DOES NOT. The
  name is gone inside a `MethodRouter`. Document it, do not "fix" it by
  parsing anything.
- THE MACRO SEES ONE SIGNATURE. It never inspects a type's definition: a
  struct path payload's fields, a query type's name, a response's name are
  all resolved in the export TEST at run time through ts-rs. Keep it that way
  — it is why `#[client]` costs one attribute and no registration.
- OPAQUE RETURNS DO NOT COMPILE. `Json<T>` in its wrappings, `StatusCode`,
  `()`; anything else is `compile_error!` naming the handler. A handler that
  serves a file is `#[client(link)]` and gets a URL builder only.
- EXPORT TESTS ARE INERT WITHOUT THE VARIABLE (ts-rs's pattern): `cargo test`
  must never write into a repo. `append` is locked because the tests run in
  parallel.
- PATH PARAMS BIND IN TEMPLATE ORDER: tuple by position, struct by field name
  (must be the template's names — axum deserialises by name too), primitive
  as the one param. A mismatch is a generator error naming the handler.
- THE CLIENT IS FINAL FORM. `client.ts` has the literal template inlined per
  function; nothing is joined in the browser. The transport is the only
  run-time code, and it must call the GLOBAL `fetch` so a shim that wraps
  `window.fetch` (common-oidc's 401 re-auth) covers every call.
- `serde_json` output is deterministic (sorted, pretty, trailing newline) so
  the committed files diff like the ts-rs bindings.

## Run / test
`nix develop --impure -c cargo test -p common-routing -p common-routing-macros`
at the WORKSPACE root (frozen 1.98.0; `CARGO_TARGET_DIR` from the flake).
Tests write under `std::env::temp_dir()`; no network.

## Stand context
Pilot: cron (routes.rs through `Router`, `#[client]` on its API handlers,
`assets/vendor/client-call.ts` as the interim transport, `app.ts` with no
`/api/…` literal). Next: les-forms, registry; role-ui's SPA later. common-ui
is to serve the transport beside the shim (followups in the review file).
