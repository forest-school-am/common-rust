# common-logging

The Les stand's shared logging crate — the single owner of CODESTYLE.md §8
mechanics, so nothing drifts per repo. Every Les binary and library depends on
it, `common-oidc` included (which logs through here, not raw `tracing`).

- **Version:** `0.1.0` · **Toolchain:** Rust 1.98.0.

## Depend on it

Plain path dep + a NOTE naming the canonical future remote (the stand's
convention until the repo is pushed):

```toml
[dependencies]
# NOTE: canonical remote is https://github.com/rebenkoy/common-logging — switch to
# a git dep once it is pushed. Path dep until then.
common-logging = { path = "../common-logging" }
```

## Use

One call at the top of `main` (reads `LOG_FORMAT` + `DEPLOYMENT_TYPE` +
`RUST_LOG`):

```rust
fn main() {
    common_logging::init();
    // …
}
```

Emit with a **designator** (§8.3) as the first argument — it becomes the
tracing target:

```rust
use common_logging::{info, warn, AUTH, UPSTREAM};
// tracing idiom: structured fields FIRST, then the message string.
// The human formatter DE-DUPLICATES span-appended fields by name (event
// wins, then inner-most span) — do not hand-dedupe, but DO reduce
// Option/Result before capture: `?opt` prints Rust syntax like Some(0).
info!(AUTH, user = %username, "signed in");
warn!(UPSTREAM, attempt = n, "retry");
```

**Map `Option`/`Result` before capturing them.** A `?`-captured (Debug) value
prints Rust syntax — `exit_code = ?maybe` logs `exit_code=Some(0)`, which reads
badly in a log viewer. Reduce it to a plain value at the call site:
`exit_code = maybe.unwrap_or(-1)`, or use `%` (Display) for types that have it.
common-logging can't intercept tracing's `?`/`%` sigils, so this is on the caller.

Open the **request root span** (§8.2) in your HTTP middleware; it carries
`reqid` and `actor` into every event beneath it:

```rust
let reqid = common_logging::gen_reqid();
let span  = common_logging::request_span(&reqid);
// once identity resolves (the single resolution point, §5.2):
common_logging::set_actor(&span, &username);
// then run the handler inside the span (enter it, or `.instrument(span)`).
```

Per §8.2, instrument subsystem entry points, every async fn, and every spawned
future (`.instrument(span)`) — not five-line pure helpers.

## Designators (§8.3)

Common vocabulary — prefer it, it covers most events:

| designator | for |
|---|---|
| `AUTH` (`auth`) | authentication / authorization / identity |
| `BUSINESS` (`business`) | domain logic |
| `UPSTREAM` (`upstream`) | calls to another service (IdP, DB, remote API) |
| `STORAGE` (`storage`) | persistence / caches / files |
| `HTTP` (`http`) | request lifecycle (the request span's target) |

### Custom designators

A project MAY add its own where the common set genuinely doesn't fit, via the
`c-` helper (a compile-time `&'static str`):

```rust
info!(common_logging::custom!("scheduler"), run_id = %id, "run started");  // target "c-scheduler"
```

The `c-` prefix keeps project vocabulary visually distinct from stand
vocabulary. **Rules (§8.3):** every custom designator MUST be listed and
explained in its repo's README; one proposed by an LLM/agent MUST be
operator-confirmed before it lands.

_common-logging itself defines no custom designators. First known customers:
cron-viewer (`c-scheduler`, run-lifecycle, pending operator confirmation);
les-registry maps onto `upstream`+`http` with none needed._

## Formats (§8.4)

`LOG_FORMAT` selects output (default `json`):

- `human`: `timestamp level designator file:row reqid [actor] message` — one
  line per event, nested-span fields appended at the end.
- `json` (default): tracing-subscriber's JSON layer, one object per line,
  event fields flattened, `reqid`/`actor` under `span` — standard-viewer
  readable.

`DEPLOYMENT_TYPE=prod|dev` (default `dev`) sets the default verbosity (`info`
vs `debug`) when `RUST_LOG` is unset; `RUST_LOG` overrides.

## Test

`cargo test` — pure config-matrix tests plus literal-shape tests for both
output formats and the `c-` designator behavior (no network).
