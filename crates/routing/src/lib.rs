//! Every HTTP path written once, in Rust, and the browser client compiled
//! from it (R114 cron item 4). Three parts, joined by a handler's fully
//! qualified name:
//!
//! 1. [`Router`] — axum's method calls (`.get(path, handler)`, `.post`,
//!    `.nest`, `.merge`, `.layer`, `.with_state`), building an `axum::Router`
//!    underneath and RECORDING each registration: `type_name` of the handler,
//!    method, path template, the params parsed from the template.
//!    [`Router::manifest`] is that table; [`Router::write_manifest`] writes it
//!    as `routes.json`. Nothing about types at the bind.
//! 2. [`client`] — the attribute on each handler. From the SIGNATURE it emits
//!    a descriptor: the extractors that carry a client payload (path, query,
//!    body, multipart) and the response type; guards and state are skipped;
//!    an opaque return is a compile error. The descriptor is written by a
//!    generated export test into `handlers.json` (module [`export`]), with the
//!    TypeScript names resolved through ts-rs, so ts-rs stays for the DTOs.
//! 3. [`generate_client`] — joins the two tables by fqname and writes
//!    `client.ts`: one plain function per handler, path params bound in
//!    template order, then query, then body, on a ~20-line transport
//!    `call(method, url, query?, body?)`. Nothing is joined at run time; tsc
//!    checks ordinary function signatures.

pub mod export;
pub mod generate;
mod manifest;
mod router;
mod static_files;

pub use common_routing_macros::client;
pub use generate::{generate_client, Options as GenerateOptions};
pub use manifest::{parse_path_params, write_manifest, Registration};
pub use router::Router;
pub use static_files::{content_type_for, safe_asset_name, serve_static, AssetSet};

/// Re-exported for the code `#[client]` generates: the consumer's DTOs and
/// this crate must agree on ONE ts-rs, and this is how the macro names it.
pub use ts_rs;
