//! Server-side state the browser holds only an opaque id for: established
//! sessions, and logins still in flight. Traits plus the in-memory backends.
//! What a session MEANS belongs in principal.rs; who may see it, web.rs.

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

/// A login in flight: what the callback needs to finish a flow it did not
/// start. Every field here is something the browser MUST NOT choose — `state`
/// is a CSRF token whose only security property is that the server remembers
/// it, `verifier` is the PKCE secret, `next` is a redirect target, and
/// `interactive_tried` is the loop breaker. The browser gets an opaque id and
/// none of this.
#[derive(Clone)]
pub struct FlowState {
    pub state: String,
    pub verifier: String,
    pub next: String,
    pub interactive_tried: bool,
    pub created: SystemTime,
}

/// Hand-written so `?flow` in a log line cannot print the PKCE verifier or the
/// CSRF token. Deriving `Debug` here would put both in any event that captures
/// the struct.
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

/// Flows are short-lived by nature — a login either completes or is abandoned
/// within minutes — so the default expiry is far shorter than a session's, and
/// abandoned ones are swept rather than accumulating.
pub struct MemoryFlowStore {
    flows: Mutex<HashMap<String, FlowState>>,
    max_age: Duration,
}

impl Default for MemoryFlowStore {
    fn default() -> Self {
        Self {
            flows: Mutex::new(HashMap::new()),
            max_age: Duration::from_secs(10 * 60),
        }
    }
}

impl MemoryFlowStore {
    pub fn with_max_age(max_age: Duration) -> Self {
        Self {
            flows: Mutex::new(HashMap::new()),
            max_age,
        }
    }

    fn sweep(&self, flows: &mut HashMap<String, FlowState>) {
        let now = SystemTime::now();
        flows.retain(|_, f| {
            now.duration_since(f.created)
                .map(|age| age < self.max_age)
                .unwrap_or(true)
        });
    }
}

impl FlowStore for MemoryFlowStore {
    fn get(&self, id: &str) -> BoxFuture<'_, Option<FlowState>> {
        let id = id.to_owned();
        Box::pin(async move {
            let mut flows = self.flows.lock().await;
            self.sweep(&mut flows);
            flows.get(&id).cloned()
        })
    }

    fn put(&self, id: String, flow: FlowState) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let mut flows = self.flows.lock().await;
            self.sweep(&mut flows);
            flows.insert(id, flow);
        })
    }

    fn remove(&self, id: &str) -> BoxFuture<'_, ()> {
        let id = id.to_owned();
        Box::pin(async move {
            self.flows.lock().await.remove(&id);
        })
    }
}

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
        Self {
            sessions: Mutex::new(HashMap::new()),
            max_age: Duration::from_secs(12 * 3600),
        }
    }
}

impl MemoryStore {
    pub fn with_max_age(max_age: Duration) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            max_age,
        }
    }

    fn sweep(&self, sessions: &mut HashMap<String, Session>) {
        let now = SystemTime::now();
        sessions.retain(|_, s| {
            now.duration_since(s.created)
                .map(|age| age < self.max_age)
                .unwrap_or(true)
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

    /// The struct holds the PKCE verifier and the CSRF token. `?flow` in any
    /// event must not print either — the spellings are written out here rather
    /// than read off the struct so the test still fails if the field values
    /// start reaching the formatter.
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
}
