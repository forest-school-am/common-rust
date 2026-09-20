//! Every HTTP path written once, in Rust, and the browser client compiled from
//! it. Three parts joined by a handler's fully-qualified name: [`Router`] records
//! each registration into the manifest; the [`client`] attribute emits a
//! descriptor from each handler's signature; [`generate_client`] joins the two
//! tables into `client.ts`. Nothing is joined at run time.

pub mod export;
pub mod generate;
mod manifest;
mod router;
mod static_files;

pub use common_routing_macros::client;
pub use generate::{generate_client, Options as GenerateOptions};
pub use manifest::{parse_path_params, write_manifest, Registration};
pub use router::Router;
pub use static_files::{content_type_for, safe_asset_path, serve_static, AssetSet, CachePolicy};

/// Re-exported for the code `#[client]` generates: the consumer's DTOs and
/// this crate must agree on ONE ts-rs, and this is how the macro names it.
pub use ts_rs;
