//! The background sync loop and the cursor stores it persists progress in.
//!
//! The feed client and wire types -> feed.rs; applying deltas -> apply.rs.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use common_logging as log;
use sqlx::SqlitePool;

use crate::feed::{NameDeltas, NameFeed};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// `load` returns `None` when no cursor has been stored yet.
pub trait CursorStore: Send + Sync + 'static {
    fn load(&self) -> BoxFuture<'_, anyhow::Result<Option<String>>>;
    fn store(&self, cursor: &str) -> BoxFuture<'_, anyhow::Result<()>>;
}

/// `id` distinguishes several feeds sharing one database.
#[derive(Clone)]
pub struct SqliteCursorStore {
    pool: SqlitePool,
    id: String,
}

impl SqliteCursorStore {
    pub async fn open(pool: SqlitePool, id: impl Into<String>) -> anyhow::Result<Self> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS name_sync_cursor (id TEXT PRIMARY KEY, cursor TEXT)",
        )
        .execute(&pool)
        .await?;
        Ok(Self {
            pool,
            id: id.into(),
        })
    }
}

impl CursorStore for SqliteCursorStore {
    fn load(&self) -> BoxFuture<'_, anyhow::Result<Option<String>>> {
        Box::pin(async move {
            let row: Option<(Option<String>,)> =
                sqlx::query_as("SELECT cursor FROM name_sync_cursor WHERE id = ?")
                    .bind(&self.id)
                    .fetch_optional(&self.pool)
                    .await?;
            Ok(row.and_then(|(c,)| c))
        })
    }

    fn store(&self, cursor: &str) -> BoxFuture<'_, anyhow::Result<()>> {
        let cursor = cursor.to_owned();
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO name_sync_cursor (id, cursor) VALUES (?, ?) \
                 ON CONFLICT(id) DO UPDATE SET cursor = excluded.cursor",
            )
            .bind(&self.id)
            .bind(&cursor)
            .execute(&self.pool)
            .await?;
            Ok(())
        })
    }
}

#[derive(Clone)]
pub struct FileCursorStore {
    path: PathBuf,
}

impl FileCursorStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl CursorStore for FileCursorStore {
    fn load(&self) -> BoxFuture<'_, anyhow::Result<Option<String>>> {
        Box::pin(async move {
            match std::fs::read_to_string(&self.path) {
                Ok(s) => {
                    let s = s.trim();
                    Ok((!s.is_empty()).then(|| s.to_string()))
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(anyhow::anyhow!(
                    "reading cursor file {}: {e}",
                    self.path.display()
                )),
            }
        })
    }

    fn store(&self, cursor: &str) -> BoxFuture<'_, anyhow::Result<()>> {
        let cursor = cursor.to_owned();
        Box::pin(async move {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            // Write-and-rename so a crash mid-write cannot leave a torn cursor.
            let tmp = self.path.with_extension("tmp");
            std::fs::write(&tmp, cursor.as_bytes())
                .and_then(|_| std::fs::rename(&tmp, &self.path))
                .map_err(|e| anyhow::anyhow!("writing cursor file {}: {e}", self.path.display()))
        })
    }
}

pub struct SyncConfig<C: CursorStore> {
    pub feed: NameFeed,
    pub interval: Duration,
    pub cursor: C,
}

/// A cycle's error is logged and swallowed; the loop never exits on error.
pub fn spawn<C, F>(cfg: SyncConfig<C>, apply: F) -> tokio::task::JoinHandle<()>
where
    C: CursorStore,
    F: Fn(&NameDeltas) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync + 'static,
{
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(cfg.interval);
        // tokio's first tick() returns immediately (first sync at start); Delay
        // makes an overrunning cycle push the next tick back rather than burst.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        log::info::startup!(
            interval_secs = cfg.interval.as_secs(),
            "name-sync started"
        );
        loop {
            ticker.tick().await;
            if let Err(e) = run_cycle(&cfg.feed, &cfg.cursor, &apply).await {
                log::error::upstream!(
                    error = %e,
                    "name-sync cycle failed; retrying next interval"
                );
            }
        }
    })
}

async fn run_cycle<C, F>(feed: &NameFeed, cursor: &C, apply: &F) -> anyhow::Result<()>
where
    C: CursorStore,
    F: Fn(&NameDeltas) -> BoxFuture<'static, anyhow::Result<()>>,
{
    let since = cursor.load().await?;
    let deltas = feed.changes_since(since.as_deref()).await?;
    if !deltas.is_empty() {
        apply(&deltas).await?;
        log::info::business!(
            users = deltas.users.len(),
            groups = deltas.groups.len(),
            "name-sync applied renames"
        );
    }
    // Advance the cursor even with no deltas, or the next fetch rescans from here.
    cursor.store(&deltas.now).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn mem_pool() -> SqlitePool {
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn sqlite_cursor_round_trips_and_starts_empty() {
        let pool = mem_pool().await;
        let store = SqliteCursorStore::open(pool, "les-forms").await.unwrap();
        assert_eq!(store.load().await.unwrap(), None);
        store.store("cursor-1").await.unwrap();
        assert_eq!(store.load().await.unwrap(), Some("cursor-1".to_string()));
        store.store("cursor-2").await.unwrap();
        assert_eq!(store.load().await.unwrap(), Some("cursor-2".to_string()));
    }

    #[tokio::test]
    async fn file_cursor_round_trips_and_starts_empty() {
        let path = std::env::temp_dir().join(format!("names-cursor-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = FileCursorStore::new(&path);
        assert_eq!(store.load().await.unwrap(), None);
        store.store("cursor-1").await.unwrap();
        assert_eq!(store.load().await.unwrap(), Some("cursor-1".to_string()));
        std::fs::remove_file(&path).ok();
    }
}
