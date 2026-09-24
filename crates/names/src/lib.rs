//! Rename-staleness sweep for Les stand apps.
//!
//! The fleet stores authorization-bearing values as authentik USERNAMES and
//! GROUP NAMES (the R123 migration off UUIDs). Those strings go stale when a
//! principal is renamed. This crate keeps a service's own copies fresh: it polls
//! a rename feed, and rewrites the stale strings in place.
//!
//! Four pieces, each usable on its own:
//!
//! * [`NameColumns`] + `#[derive(NameColumns)]` — a row struct declares which of
//!   its columns hold a username or a group name, as a compile-time
//!   [`NameTarget`] table. Table and column come from the struct, never from
//!   input, so they are safe to format into SQL.
//! * [`NameFeed`] — the feed client: `changes_since(cursor)` returns the
//!   [`NameDeltas`] since a cursor, and a fresh cursor to store.
//! * [`apply_column_renames`] — applies deltas to the plain columns named by a
//!   `&[NameTarget]`.
//! * [`spawn`] — the background runner: load cursor, fetch, apply, store, on a
//!   fixed interval, logging and surviving any cycle's error.
//!
//! ```no_run
//! use common_names::{NameColumns, NameFeed, apply_column_renames};
//!
//! #[derive(NameColumns)]
//! #[names(table = "forms")]
//! struct FormRow {
//!     #[name(user)]
//!     owner: String,
//!     // columns without a #[name(..)] attr are ignored
//!     source_json: String,
//! }
//!
//! # async fn demo(pool: &sqlx::SqlitePool) -> anyhow::Result<()> {
//! let feed = NameFeed::new("http://127.0.0.1:8000", "a-bearer-token");
//! let deltas = feed.changes_since(None).await?;
//! apply_column_renames(pool, FormRow::name_targets(), &deltas).await?;
//! # Ok(())
//! # }
//! ```

// The derive spells every path as `::common_names::…`, so the crate's own tests
// (and the doctest above) can derive `NameColumns` on local structs.
extern crate self as common_names;

mod apply;
mod feed;
mod sync;

pub use apply::apply_column_renames;
pub use common_names_derive::NameColumns;
pub use feed::{NameDeltas, NameFeed, Rename};
pub use sync::{spawn, BoxFuture, CursorStore, FileCursorStore, SqliteCursorStore, SyncConfig};

/// Whether a stored value is a username or a group name. A rename of one kind
/// never touches a column of the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    User,
    Group,
}

/// One column that holds a name: which table, which column, and which kind of
/// name. Produced by `#[derive(NameColumns)]`; both strings are compile-time
/// literals from the struct, so [`apply_column_renames`] may format them into
/// SQL without escaping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameTarget {
    pub table: &'static str,
    pub column: &'static str,
    pub kind: NameKind,
}

/// A row struct's name-bearing columns, as a static table. Implemented by
/// `#[derive(NameColumns)]`.
pub trait NameColumns {
    fn name_targets() -> &'static [NameTarget];
}
