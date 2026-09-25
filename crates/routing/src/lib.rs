//! Write each HTTP path once in Rust and compile the browser client from it.
//! Handler signature analysis belongs in `common-routing-macros`; the wire DTOs
//! and the transport the generated client imports belong in the consumer, not
//! here.

pub mod barrel;
pub mod export;
pub mod generate;
mod manifest;
mod router;
mod static_files;

pub use barrel::Barrel;
pub use common_routing_macros::client;
pub use generate::{generate_client, Options as GenerateOptions};
pub use manifest::{parse_path_params, write_manifest, Registration};
pub use router::Router;
pub use static_files::{content_type_for, safe_asset_path, serve_static, AssetSet, CachePolicy};

/// The consumer's DTOs and this crate must resolve to ONE ts-rs; re-exported so
/// both can bind to the same one.
pub use ts_rs;
