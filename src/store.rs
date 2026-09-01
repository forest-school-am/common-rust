use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, SystemTime};

use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub created: SystemTime,
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait SessionStore: Send + Sync + 'static {
    fn get(&self, id: &str) -> BoxFuture<'_, Option<Session>>;
    fn put(&self, id: String, session: Session) -> BoxFuture<'_, ()>;
    fn remove(&self, id: &str) -> BoxFuture<'_, ()>;
}

pub struct MemoryStore {
    sessions: Mutex<HashMap<String, Session>>,
    max_age: Duration,
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self { sessions: Mutex::new(HashMap::new()), max_age: Duration::from_secs(12 * 3600) }
    }
}

impl MemoryStore {
    pub fn with_max_age(max_age: Duration) -> Self {
        Self { sessions: Mutex::new(HashMap::new()), max_age }
    }

    fn sweep(&self, sessions: &mut HashMap<String, Session>) {
        let now = SystemTime::now();
        sessions.retain(|_, s| {
            now.duration_since(s.created).map(|age| age < self.max_age).unwrap_or(true)
        });
    }
}

impl SessionStore for MemoryStore {
    fn get(&self, id: &str) -> BoxFuture<'_, Option<Session>> {
        let id = id.to_owned();
        Box::pin(async move {
            let mut sessions = self.sessions.lock().await;
            self.sweep(&mut sessions);
            sessions.get(&id).cloned()
        })
    }

    fn put(&self, id: String, session: Session) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let mut sessions = self.sessions.lock().await;
            self.sweep(&mut sessions);
            sessions.insert(id, session);
        })
    }

    fn remove(&self, id: &str) -> BoxFuture<'_, ()> {
        let id = id.to_owned();
        Box::pin(async move {
            self.sessions.lock().await.remove(&id);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(created: SystemTime) -> Session {
        Session { access_token: "at".into(), refresh_token: Some("rt".into()), created }
    }

    #[tokio::test]
    async fn roundtrip_and_remove() {
        let s = MemoryStore::default();
        s.put("a".into(), session(SystemTime::now())).await;
        assert!(s.get("a").await.is_some());
        s.remove("a").await;
        assert!(s.get("a").await.is_none());
    }

    #[tokio::test]
    async fn sweep_drops_sessions_older_than_max_age() {
        let s = MemoryStore::with_max_age(Duration::from_secs(60));
        s.put("old".into(), session(SystemTime::now() - Duration::from_secs(120))).await;
        s.put("new".into(), session(SystemTime::now())).await;
        assert!(s.get("old").await.is_none(), "expired session must be swept");
        assert!(s.get("new").await.is_some());
    }
}
