//! End-to-end proof of the login->jwt-login->read chain against a tiny local
//! HTTP mock: the caching (one auth for repeated reads) and the re-auth on a
//! 401 from the read. This never touches live authentik/OpenBao; the live
//! chain is the companion agent's proof.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use common_secrets::{AppPassword, Config, Error, SecretClient};

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
    let request_line = read_request(stream).await?;
    let path = request_line.split_whitespace().nth(1).unwrap_or("");

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

    let resp = format!(
        "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(resp.as_bytes()).await?;
    stream.flush().await?;
    let _ = stream.shutdown().await;
    Ok(())
}

// Reads the request headers (and drains any Content-Length body) and returns
// the request line. Responses carry Connection: close, so each request is its
// own connection and there is nothing to keep-alive across.
async fn read_request(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    let header_end = loop {
        if let Some(pos) = find(&buf, b"\r\n\r\n") {
            break pos;
        }
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Ok(String::new());
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
    if let Some(len) = content_length {
        let mut have = buf.len() - (header_end + 4);
        while have < len {
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                break;
            }
            have += n;
        }
    }
    Ok(headers.lines().next().unwrap_or("").to_string())
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
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
