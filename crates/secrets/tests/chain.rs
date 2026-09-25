//! End-to-end proof of the login->jwt-login->read chain against a tiny local
//! HTTP mock: the caching (one auth for repeated reads) and the re-auth on a
//! 401 from the read. This never touches live authentik/OpenBao; the live
//! chain is the companion agent's proof.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use common_secrets::{AppPassword, Config, Error, SecretClient};

struct Req {
    method: String,
    path: String,
    body: String,
}

#[derive(Default)]
struct Counts {
    authentik: AtomicUsize,
    login: AtomicUsize,
    read: AtomicUsize,
}

async fn spawn_server(fail_first_read: bool) -> (SocketAddr, Arc<Counts>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let counts = Arc::new(Counts::default());
    let shared = counts.clone();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let counts = shared.clone();
            tokio::spawn(async move {
                let _ = handle(&mut stream, &counts, fail_first_read).await;
            });
        }
    });
    (addr, counts)
}

async fn handle(
    stream: &mut TcpStream,
    counts: &Counts,
    fail_first_read: bool,
) -> std::io::Result<()> {
    let req = read_request(stream).await?;
    let path = req.path.as_str();

    let (code, body) = if path.contains("/application/o/token/") {
        counts.authentik.fetch_add(1, Ordering::SeqCst);
        (
            200,
            r#"{"access_token":"jwt-xyz","expires_in":300}"#.to_string(),
        )
    } else if path == "/v1/auth/jwt/login" {
        counts.login.fetch_add(1, Ordering::SeqCst);
        (
            200,
            r#"{"auth":{"client_token":"vault-tok","lease_duration":900}}"#.to_string(),
        )
    } else if path.starts_with("/v1/kv/data/services/") {
        let n = counts.read.fetch_add(1, Ordering::SeqCst) + 1;
        if fail_first_read && n == 1 {
            (401, r#"{"errors":["permission denied"]}"#.to_string())
        } else {
            (
                200,
                r#"{"data":{"data":{"authentik_token":"s3cr3t"},"metadata":{"version":1}}}"#
                    .to_string(),
            )
        }
    } else {
        (404, r#"{"errors":[]}"#.to_string())
    };

    write_response(stream, code, &body).await
}

// Reads the request headers plus any Content-Length body and returns the
// method, path, and body. Responses carry Connection: close, so each request is
// its own connection and there is nothing to keep-alive across.
async fn read_request(stream: &mut TcpStream) -> std::io::Result<Req> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    let header_end = loop {
        if let Some(pos) = find(&buf, b"\r\n\r\n") {
            break pos;
        }
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Ok(Req {
                method: String::new(),
                path: String::new(),
                body: String::new(),
            });
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let content_length = headers.lines().find_map(|l| {
        let lower = l.to_ascii_lowercase();
        lower
            .strip_prefix("content-length:")
            .and_then(|v| v.trim().parse::<usize>().ok())
    });
    let body_start = header_end + 4;
    if let Some(len) = content_length {
        while buf.len() - body_start < len {
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
        }
    }
    let body = String::from_utf8_lossy(&buf[body_start..]).to_string();
    let request_line = headers.lines().next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    Ok(Req { method, path, body })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

async fn write_response(stream: &mut TcpStream, code: u16, body: &str) -> std::io::Result<()> {
    let resp = format!(
        "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(resp.as_bytes()).await?;
    stream.flush().await?;
    let _ = stream.shutdown().await;
    Ok(())
}

fn make_client(addr: SocketAddr) -> SecretClient {
    let config = Config::new(
        format!("http://{addr}/application/o/token/"),
        "cid",
        "svc-roleui",
        AppPassword::new("pw"),
        format!("http://{addr}"),
        "roleui",
    );
    SecretClient::new(config).unwrap()
}

#[tokio::test]
async fn full_chain_fetches_then_caches_the_vault_token() {
    let (addr, counts) = spawn_server(false).await;
    let client = make_client(addr);

    let first = client.fetch("roleui", "authentik_token").await.unwrap();
    assert_eq!(first.expose(), "s3cr3t");
    let second = client.fetch("roleui", "authentik_token").await.unwrap();
    assert_eq!(second.expose(), "s3cr3t");

    assert_eq!(counts.authentik.load(Ordering::SeqCst), 1);
    assert_eq!(counts.login.load(Ordering::SeqCst), 1);
    assert_eq!(counts.read.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn reauthenticates_when_the_read_returns_401() {
    let (addr, counts) = spawn_server(true).await;
    let client = make_client(addr);

    let value = client.fetch("roleui", "authentik_token").await.unwrap();
    assert_eq!(value.expose(), "s3cr3t");

    assert_eq!(counts.authentik.load(Ordering::SeqCst), 2);
    assert_eq!(counts.login.load(Ordering::SeqCst), 2);
    assert_eq!(counts.read.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn missing_key_is_not_found() {
    let (addr, _counts) = spawn_server(false).await;
    let client = make_client(addr);

    let err = client.fetch("roleui", "absent").await.unwrap_err();
    assert!(matches!(err, Error::NotFound));
}

// A doc-op mock: it records every kv-plane request so a test can assert the
// method, path, and body the client sent, and answers the paths the doc tests
// exercise. Auth legs mirror `spawn_server`.
async fn spawn_doc_server() -> (SocketAddr, Arc<Mutex<Vec<Req>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Arc<Mutex<Vec<Req>>> = Arc::new(Mutex::new(Vec::new()));
    let shared = seen.clone();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let seen = shared.clone();
            tokio::spawn(async move {
                let _ = doc_handle(&mut stream, &seen).await;
            });
        }
    });
    (addr, seen)
}

async fn doc_handle(stream: &mut TcpStream, seen: &Mutex<Vec<Req>>) -> std::io::Result<()> {
    let req = read_request(stream).await?;
    let (method, path) = (req.method.as_str(), req.path.as_str());

    let (code, body) = if path.contains("/application/o/token/") {
        (
            200,
            r#"{"access_token":"jwt-xyz","expires_in":300}"#.to_string(),
        )
    } else if path == "/v1/auth/jwt/login" {
        (
            200,
            r#"{"auth":{"client_token":"vault-tok","lease_duration":900}}"#.to_string(),
        )
    } else if method == "GET" && path == "/v1/kv/data/tasks/connector-principals" {
        (
            200,
            r#"{"data":{"data":{"alpha":"a-secret","beta":"b-secret"},"metadata":{"version":3}}}"#
                .to_string(),
        )
    } else if method == "POST" && path == "/v1/kv/data/tasks/connector-principals" {
        (200, r#"{"data":{"metadata":{"version":4}}}"#.to_string())
    } else if method == "DELETE" && path == "/v1/kv/metadata/tasks/connector-principals" {
        (204, String::new())
    } else {
        // Every other kv path, including the deleted `tasks/gone`, is absent.
        (404, r#"{"errors":[]}"#.to_string())
    };

    seen.lock().unwrap().push(req);
    write_response(stream, code, &body).await
}

fn kv_requests(seen: &Mutex<Vec<Req>>) -> Vec<(String, String, String)> {
    seen.lock()
        .unwrap()
        .iter()
        .filter(|r| {
            r.path.starts_with("/v1/kv/data/tasks") || r.path.starts_with("/v1/kv/metadata/tasks")
        })
        .map(|r| (r.method.clone(), r.path.clone(), r.body.clone()))
        .collect()
}

#[tokio::test]
async fn read_doc_parses_a_multi_field_document_into_a_map() {
    let (addr, _seen) = spawn_doc_server().await;
    let client = make_client(addr);

    let doc = client.read_doc("tasks/connector-principals").await.unwrap();
    assert_eq!(doc.len(), 2);
    assert_eq!(doc.get("alpha").unwrap().expose(), "a-secret");
    assert_eq!(doc.get("beta").unwrap().expose(), "b-secret");
}

#[tokio::test]
async fn read_doc_of_a_missing_path_is_not_found() {
    let (addr, _seen) = spawn_doc_server().await;
    let client = make_client(addr);

    let err = client.read_doc("tasks/absent").await.unwrap_err();
    assert!(matches!(err, Error::NotFound));
}

#[tokio::test]
async fn write_doc_posts_the_data_wrapper_to_the_data_plane() {
    let (addr, seen) = spawn_doc_server().await;
    let client = make_client(addr);

    let mut data = BTreeMap::new();
    data.insert("alpha".to_string(), "a-secret".to_string());
    data.insert("beta".to_string(), "b-secret".to_string());
    client
        .write_doc("tasks/connector-principals", &data)
        .await
        .unwrap();

    let kv = kv_requests(&seen);
    assert_eq!(kv.len(), 1);
    let (method, path, body) = &kv[0];
    assert_eq!(method, "POST");
    assert_eq!(path, "/v1/kv/data/tasks/connector-principals");
    let sent: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        sent,
        serde_json::json!({"data": {"alpha": "a-secret", "beta": "b-secret"}})
    );
}

#[tokio::test]
async fn delete_doc_targets_the_metadata_plane_and_404_is_ok() {
    let (addr, seen) = spawn_doc_server().await;
    let client = make_client(addr);

    client
        .delete_doc("tasks/connector-principals")
        .await
        .unwrap();
    client.delete_doc("tasks/gone").await.unwrap();

    let kv = kv_requests(&seen);
    assert_eq!(kv.len(), 2);
    for (method, path, _) in &kv {
        assert_eq!(method, "DELETE");
        assert!(path.starts_with("/v1/kv/metadata/tasks/"));
    }
}

#[tokio::test]
async fn leading_slash_in_a_doc_path_is_normalised() {
    let (addr, _seen) = spawn_doc_server().await;
    let client = make_client(addr);

    let doc = client
        .read_doc("/tasks/connector-principals")
        .await
        .unwrap();
    assert_eq!(doc.len(), 2);
}
