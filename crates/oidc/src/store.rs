//! Server-side state the browser holds only an opaque id for: established
//! sessions, and logins still in flight. Traits plus the in-memory backends.
//! What a session MEANS belongs in principal.rs; who may see it, web.rs.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::RwLock;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub created: SystemTime,
}

#[derive(Clone)]
pub struct FlowState {
    pub state: String,
    pub verifier: String,
    pub next: String,
    pub interactive_tried: bool,
    pub created: SystemTime,
}

impl std::fmt::Debug for FlowState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlowState")
            .field("state", &"<redacted>")
            .field("verifier", &"<redacted>")
            .field("next", &self.next)
            .field("interactive_tried", &self.interactive_tried)
            .field("created", &self.created)
            .finish()
    }
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait FlowStore: Send + Sync + 'static {
    fn get(&self, id: &str) -> BoxFuture<'_, Option<FlowState>>;
    fn put(&self, id: String, flow: FlowState) -> BoxFuture<'_, ()>;
    fn remove(&self, id: &str) -> BoxFuture<'_, ()>;
}

pub trait SessionStore: Send + Sync + 'static {
    fn get(&self, id: &str) -> BoxFuture<'_, Option<Session>>;
    fn put(&self, id: String, session: Session) -> BoxFuture<'_, ()>;
    fn remove(&self, id: &str) -> BoxFuture<'_, ()>;
}

/// Readers share the lock; a `put` takes it exclusively and sweeps expired
/// entries. The lock is never held across an await.
struct Expiring<T> {
    entries: RwLock<HashMap<String, T>>,
    max_age: Duration,
    created: fn(&T) -> SystemTime,
}

impl<T: Clone> Expiring<T> {
    fn new(max_age: Duration, created: fn(&T) -> SystemTime) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            max_age,
            created,
        }
    }

    fn live(&self, value: &T, now: SystemTime) -> bool {
        now.duration_since((self.created)(value))
            .map(|age| age < self.max_age)
            .unwrap_or(true)
    }

    fn get(&self, id: &str) -> Option<T> {
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        entries
            .get(id)
            .filter(|value| self.live(value, SystemTime::now()))
            .cloned()
    }

    fn put(&self, id: String, value: T) {
        let now = SystemTime::now();
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        entries.retain(|_, value| self.live(value, now));
        entries.insert(id, value);
    }

    fn remove(&self, id: &str) {
        self.entries
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
    }
}

pub struct MemoryFlowStore(Expiring<FlowState>);

impl Default for MemoryFlowStore {
    fn default() -> Self {
        Self::with_max_age(Duration::from_secs(10 * 60))
    }
}

impl MemoryFlowStore {
    pub fn with_max_age(max_age: Duration) -> Self {
        Self(Expiring::new(max_age, |f| f.created))
    }
}

impl FlowStore for MemoryFlowStore {
    fn get(&self, id: &str) -> BoxFuture<'_, Option<FlowState>> {
        let found = self.0.get(id);
        Box::pin(async move { found })
    }

    fn put(&self, id: String, flow: FlowState) -> BoxFuture<'_, ()> {
        self.0.put(id, flow);
        Box::pin(async {})
    }

    fn remove(&self, id: &str) -> BoxFuture<'_, ()> {
        self.0.remove(id);
        Box::pin(async {})
    }
}

pub struct MemoryStore(Expiring<Session>);

impl Default for MemoryStore {
    fn default() -> Self {
        Self::with_max_age(Duration::from_secs(12 * 3600))
    }
}

impl MemoryStore {
    pub fn with_max_age(max_age: Duration) -> Self {
        Self(Expiring::new(max_age, |s| s.created))
    }
}

impl SessionStore for MemoryStore {
    fn get(&self, id: &str) -> BoxFuture<'_, Option<Session>> {
        let found = self.0.get(id);
        Box::pin(async move { found })
    }

    fn put(&self, id: String, session: Session) -> BoxFuture<'_, ()> {
        self.0.put(id, session);
        Box::pin(async {})
    }

    fn remove(&self, id: &str) -> BoxFuture<'_, ()> {
        self.0.remove(id);
        Box::pin(async {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(created: SystemTime) -> Session {
        Session {
            access_token: "at".into(),
            refresh_token: Some("rt".into()),
            created,
        }
    }

    #[tokio::test]
    async fn roundtrip_and_remove() {
        let s = MemoryStore::default();
        s.put("a".into(), session(SystemTime::now())).await;
        assert!(s.get("a").await.is_some());
        s.remove("a").await;
        assert!(s.get("a").await.is_none());
    }

    fn flow(created: SystemTime) -> FlowState {
        FlowState {
            state: "csrf".into(),
            verifier: "pkce".into(),
            next: "/somewhere".into(),
            interactive_tried: false,
            created,
        }
    }

    #[tokio::test]
    async fn flow_roundtrip_and_remove() {
        let s = MemoryFlowStore::default();
        s.put("f".into(), flow(SystemTime::now())).await;
        assert_eq!(s.get("f").await.expect("stored").next, "/somewhere");
        s.remove("f").await;
        assert!(
            s.get("f").await.is_none(),
            "a consumed flow must not be reusable"
        );
    }

    #[tokio::test]
    async fn abandoned_flows_expire() {
        let s = MemoryFlowStore::with_max_age(Duration::from_secs(600));
        s.put(
            "stale".into(),
            flow(SystemTime::now() - Duration::from_secs(1200)),
        )
        .await;
        s.put("live".into(), flow(SystemTime::now())).await;
        assert!(
            s.get("stale").await.is_none(),
            "an abandoned login must not stay resumable forever"
        );
        assert!(s.get("live").await.is_some());
    }

    #[test]
    fn debug_redacts_the_secrets_but_keeps_the_diagnostics() {
        let rendered = format!("{:?}", flow(SystemTime::now()));
        assert!(
            !rendered.contains("csrf") && !rendered.contains("pkce"),
            "FlowState Debug leaked a secret: {rendered}"
        );
        assert!(
            rendered.contains("/somewhere"),
            "redaction must not blind the useful fields: {rendered}"
        );
    }

    #[tokio::test]
    async fn sweep_drops_sessions_older_than_max_age() {
        let s = MemoryStore::with_max_age(Duration::from_secs(60));
        s.put(
            "old".into(),
            session(SystemTime::now() - Duration::from_secs(120)),
        )
        .await;
        s.put("new".into(), session(SystemTime::now())).await;
        assert!(
            s.get("old").await.is_none(),
            "expired session must be swept"
        );
        assert!(s.get("new").await.is_some());
    }

    #[tokio::test]
    async fn an_entry_that_expires_between_put_and_get_is_gone_without_a_sweep() {
        let s = MemoryStore::with_max_age(Duration::from_millis(1));
        s.put("a".into(), session(SystemTime::now())).await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(s.get("a").await.is_none());
    }

    #[tokio::test]
    async fn concurrent_readers_do_not_serialise() {
        let s = std::sync::Arc::new(MemoryStore::default());
        s.put("a".into(), session(SystemTime::now())).await;
        let held = s.0.entries.read().unwrap();
        let reader = s.clone();
        let got = tokio::task::spawn_blocking(move || reader.0.get("a").is_some())
            .await
            .unwrap();
        drop(held);
        assert!(
            got,
            "a get must complete while another reader holds the lock"
        );
    }
}
