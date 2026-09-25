//! The background sync loop: one transaction per cycle holds the consumer's
//! apply AND the cursor advance, so a crash leaves both or neither. The feed
//! client is feed.rs; the column applier is apply.rs; anything that decides
//! WHAT to rewrite is the consumer's `Apply`.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use common_logging as log;
use sqlx::{SqliteConnection, SqlitePool};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::feed::{NameDeltas, NameFeed};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Re-requested before the stored cursor on every fetch: the feed's clock and
/// ours may drift, and applying a rename twice is a no-op.
pub const OVERLAP: Duration = Duration::from_secs(5 * 60);

/// Rewrites a cycle's deltas on `conn`, which is inside the cycle's
/// transaction: return `Err` and nothing of the cycle is kept, cursor included.
pub trait Apply: Send + Sync + 'static {
    fn apply<'c>(
        &'c self,
        conn: &'c mut SqliteConnection,
        deltas: &'c NameDeltas,
    ) -> BoxFuture<'c, anyhow::Result<()>>;
}

pub struct SyncConfig {
    pub feed: NameFeed,
    pub interval: Duration,
    /// Holds the consumer's tables and the cursor, so one transaction covers both.
    pub pool: SqlitePool,
}

/// A cycle's error is logged and swallowed; the loop never exits on error.
pub fn spawn(cfg: SyncConfig, apply: impl Apply) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(cfg.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        log::info::startup!(interval_secs = cfg.interval.as_secs(), "name-sync started");
        loop {
            ticker.tick().await;
            if let Err(e) = run_cycle(&cfg, &apply).await {
                log::error::upstream!(
                    error = %e,
                    "name-sync cycle failed; retrying next interval"
                );
            }
        }
    })
}

async fn run_cycle(cfg: &SyncConfig, apply: &impl Apply) -> anyhow::Result<()> {
    ensure_cursor_table(&cfg.pool).await?;
    let since = load_cursor(&cfg.pool).await?;
    let since = since.as_deref().map(|cursor| match overlapped(cursor) {
        Some(earlier) => earlier,
        None => {
            log::warn::upstream!(cursor, "cursor is not RFC 3339; fetching without overlap");
            cursor.to_owned()
        }
    });
    let deltas = cfg.feed.changes_since(since.as_deref()).await?;
    commit_cycle(&cfg.pool, &deltas, apply).await
}

async fn commit_cycle(
    pool: &SqlitePool,
    deltas: &NameDeltas,
    apply: &impl Apply,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    if !deltas.is_empty() {
        apply.apply(&mut *tx, deltas).await?;
        log::info::business!(
            users = deltas.users.len(),
            groups = deltas.groups.len(),
            "name-sync applied renames"
        );
    }
    store_cursor(&mut *tx, &deltas.now).await?;
    tx.commit().await?;
    Ok(())
}

fn overlapped(cursor: &str) -> Option<String> {
    let at = OffsetDateTime::parse(cursor, &Rfc3339).ok()?;
    (at - OVERLAP).format(&Rfc3339).ok()
}

async fn ensure_cursor_table(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS name_sync_cursor \
         (id INTEGER PRIMARY KEY CHECK (id = 1), cursor TEXT NOT NULL)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn load_cursor(pool: &SqlitePool) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT cursor FROM name_sync_cursor WHERE id = 1")
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(c,)| c))
}

async fn store_cursor(conn: &mut SqliteConnection, cursor: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO name_sync_cursor (id, cursor) VALUES (1, ?) \
         ON CONFLICT(id) DO UPDATE SET cursor = excluded.cursor",
    )
    .bind(cursor)
    .execute(conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rename;

    async fn mem_pool() -> SqlitePool {
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    fn deltas(now: &str, users: &[(&str, &str)]) -> NameDeltas {
        NameDeltas {
            now: now.into(),
            users: users
                .iter()
                .map(|(o, c)| Rename {
                    old: (*o).into(),
                    current: (*c).into(),
                })
                .collect(),
            groups: vec![],
        }
    }

    struct RenameOwners;

    impl Apply for RenameOwners {
        fn apply<'c>(
            &'c self,
            conn: &'c mut SqliteConnection,
            deltas: &'c NameDeltas,
        ) -> BoxFuture<'c, anyhow::Result<()>> {
            Box::pin(async move {
                for r in &deltas.users {
                    sqlx::query("UPDATE forms SET owner = ? WHERE owner = ?")
                        .bind(&r.current)
                        .bind(&r.old)
                        .execute(&mut *conn)
                        .await?;
                }
                Ok(())
            })
        }
    }

    struct Failing;

    impl Apply for Failing {
        fn apply<'c>(
            &'c self,
            conn: &'c mut SqliteConnection,
            _: &'c NameDeltas,
        ) -> BoxFuture<'c, anyhow::Result<()>> {
            Box::pin(async move {
                sqlx::query("UPDATE forms SET owner = 'half-done'")
                    .execute(conn)
                    .await?;
                anyhow::bail!("consumer apply failed")
            })
        }
    }

    async fn forms_with_alice(pool: &SqlitePool) {
        sqlx::query("CREATE TABLE forms (owner TEXT)")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO forms (owner) VALUES ('alice')")
            .execute(pool)
            .await
            .unwrap();
    }

    async fn owner(pool: &SqlitePool) -> String {
        let (o,): (String,) = sqlx::query_as("SELECT owner FROM forms")
            .fetch_one(pool)
            .await
            .unwrap();
        o
    }

    #[test]
    fn overlap_moves_the_cursor_back_and_keeps_rfc3339() {
        assert_eq!(
            overlapped("2026-09-24T12:05:00Z").as_deref(),
            Some("2026-09-24T12:00:00Z")
        );
        assert_eq!(
            overlapped("2026-09-24T00:02:30.123456Z").as_deref(),
            Some("2026-09-23T23:57:30.123456Z")
        );
        assert_eq!(overlapped("not a timestamp"), None);
        assert_eq!(overlapped(""), None);
    }

    #[tokio::test]
    async fn the_cursor_starts_absent_and_advances_with_every_cycle() {
        let pool = mem_pool().await;
        ensure_cursor_table(&pool).await.unwrap();
        assert_eq!(load_cursor(&pool).await.unwrap(), None);
        commit_cycle(&pool, &deltas("c1", &[]), &RenameOwners)
            .await
            .unwrap();
        assert_eq!(load_cursor(&pool).await.unwrap().as_deref(), Some("c1"));
        commit_cycle(&pool, &deltas("c2", &[]), &RenameOwners)
            .await
            .unwrap();
        assert_eq!(load_cursor(&pool).await.unwrap().as_deref(), Some("c2"));
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM name_sync_cursor")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1, "the cursor is one row, not one row per cycle");
    }

    #[tokio::test]
    async fn a_cycle_applies_and_advances_together() {
        let pool = mem_pool().await;
        ensure_cursor_table(&pool).await.unwrap();
        forms_with_alice(&pool).await;
        commit_cycle(&pool, &deltas("c1", &[("alice", "alicia")]), &RenameOwners)
            .await
            .unwrap();
        assert_eq!(owner(&pool).await, "alicia");
        assert_eq!(load_cursor(&pool).await.unwrap().as_deref(), Some("c1"));
    }

    #[tokio::test]
    async fn a_failed_apply_keeps_neither_the_rewrite_nor_the_cursor() {
        let pool = mem_pool().await;
        ensure_cursor_table(&pool).await.unwrap();
        forms_with_alice(&pool).await;
        commit_cycle(&pool, &deltas("c0", &[]), &Failing)
            .await
            .unwrap();
        commit_cycle(&pool, &deltas("c1", &[("alice", "alicia")]), &Failing)
            .await
            .expect_err("the consumer's failure must surface");
        assert_eq!(
            owner(&pool).await,
            "alice",
            "the half-done rewrite rolled back"
        );
        assert_eq!(
            load_cursor(&pool).await.unwrap().as_deref(),
            Some("c0"),
            "the cursor did not advance past an unapplied delta"
        );
    }
}
