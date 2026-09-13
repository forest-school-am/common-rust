//! What a page is told about its world, and how that reaches the page: one
//! serde struct, serialised into a data block. Anything a page asks for at
//! RUNTIME belongs in a service's own API, not here.

use serde::Serialize;
use ts_rs::TS;

use crate::AssetsOrigin;

/// The one object page code reads on its first line. Per-request values belong
/// here; there is no `/api/config` and no value passed through JS (R65).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Config {
    pub assets_origin: String,
    pub login_path: String,
}

impl Config {
    pub fn new(assets_origin: &AssetsOrigin, login_path: impl Into<String>) -> Self {
        Self {
            assets_origin: assets_origin.as_str().to_owned(),
            login_path: login_path.into(),
        }
    }
}
