//! Keep a service's stored authentik usernames and group names fresh against a
//! rename feed. Cross-cutting vocabulary ([`NameKind`], [`NameTarget`],
//! [`NameColumns`]) and re-exports only: feed wire types + client -> feed.rs;
//! applying deltas to storage -> apply.rs; the background poll loop -> sync.rs.

extern crate self as common_names;

mod apply;
mod feed;
mod sync;

pub use apply::apply_column_renames;
pub use common_names_derive::NameColumns;
pub use feed::{NameDeltas, NameFeed, Rename};
pub use sync::{spawn, Apply, BoxFuture, SyncConfig, OVERLAP};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    User,
    Group,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameTarget {
    pub table: &'static str,
    pub column: &'static str,
    pub kind: NameKind,
}

pub trait NameColumns {
    fn name_targets() -> &'static [NameTarget];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    async fn public_surface_compiles(pool: &sqlx::SqlitePool) -> anyhow::Result<()> {
        #[derive(NameColumns)]
        #[names(table = "forms")]
        #[allow(dead_code)]
        struct FormRow {
            #[name(user)]
            owner: String,
            source_json: String,
        }
        let feed = NameFeed::new("http://127.0.0.1:8000", "a-bearer-token");
        let deltas = feed.changes_since(None).await?;
        apply_column_renames(
            &mut *pool.acquire().await?,
            FormRow::name_targets(),
            &deltas,
        )
        .await?;
        Ok(())
    }
}
