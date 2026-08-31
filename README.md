# stand-log

The Les stand's shared logging crate — the single owner of CODESTYLE.md §8
mechanics, so nothing drifts per repo. Every Les binary and library depends on
it, `stand-oidc` included (which logs through here, not raw `tracing`).

- **Version:** `0.1.0` · **Toolchain:** Rust 1.98.0.

## Depend on it

Plain path dep + a NOTE naming the canonical future remote (the stand's
convention until the repo is pushed):

```toml
[dependencies]
# NOTE: canonical remote is https://github.com/rebenkoy/stand-log — switch to
# a git dep once it is pushed. Path dep until then.
stand-log = { path = "../stand-log" }
```

## Use

One call at the top of `main` (reads `LOG_FORMAT` + `DEPLOYMENT_TYPE` +
`RUST_LOG`):

```rust
fn main() {
    stand_log::init();
    // …
}
```

Emit with a **designator** (§8.3) as the first argument — it becomes the
tracing target:

```rust
use stand_log::{info, warn, AUTH, UPSTREAM};
// tracing idiom: structured fields FIRST, then the message string.
info!(AUTH, user = %username, "signed in");
warn!(UPSTREAM, attempt = n, "retry");
```

Open the **request root span** (§8.2) in your HTTP middleware; it carries
`reqid` and `actor` into every event beneath it:

```rust
let reqid = stand_log::gen_reqid();
let span  = stand_log::request_span(&reqid);
// once identity resolves (the single resolution point, §5.2):
stand_log::set_actor(&span, &username);
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
info!(stand_log::custom!("scheduler"), run_id = %id, "run started");  // target "c-scheduler"
```

The `c-` prefix keeps project vocabulary visually distinct from stand
vocabulary. **Rules (§8.3):** every custom designator MUST be listed and
explained in its repo's README; one proposed by an LLM/agent MUST be
operator-confirmed before it lands.

_stand-log itself defines no custom designators. First known customers:
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
