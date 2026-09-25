//! End-to-end tests of the recording [`Router`] against real axum. The
//! `#[client]` generation pipeline is tested in `client_macro.rs`.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{Request, StatusCode};
use axum::Json;
use common_routing::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

#[derive(Clone)]
struct App;

async fn list(State(_app): State<Arc<App>>) -> Json<Vec<String>> {
    Json(vec!["a".into()])
}

async fn one(Path((id, n)): Path<(String, i64)>) -> String {
    format!("{id}/{n}")
}

async fn create() -> StatusCode {
    StatusCode::CREATED
}

async fn root() -> &'static str {
    "root"
}

async fn other() -> &'static str {
    "other"
}

fn api() -> Router<Arc<App>> {
    Router::new()
        .get("/", root)
        .get("/tasks", list)
        .get("/tasks/{id}/runs/{n}", one)
        .post("/tasks", create)
}

#[test]
fn fqnames_methods_paths_and_params_are_recorded() {
    let r = api();
    let m = r.manifest();
    assert_eq!(m.len(), 4);
    assert_eq!(m[2].fqname, std::any::type_name_of_val(&one));
    assert!(m[2].fqname.ends_with("::one"), "{}", m[2].fqname);
    assert_eq!(m[2].method, "GET");
    assert_eq!(m[2].path, "/tasks/{id}/runs/{n}");
    assert_eq!(m[2].path_params, vec!["id", "n"]);
    assert_eq!(
        (m[3].method.as_str(), m[3].path.as_str()),
        ("POST", "/tasks")
    );
    assert!(m[3].path_params.is_empty());
}

#[test]
fn nesting_prefixes_paths_and_merging_keeps_them() {
    let r = Router::new()
        .nest("/api", api())
        .merge(Router::new().get("/healthz", other));
    let paths: Vec<(String, String)> = r
        .manifest()
        .iter()
        .map(|r| (r.method.clone(), r.path.clone()))
        .collect();
    assert_eq!(
        paths,
        vec![
            ("GET".into(), "/api".into()),
            ("GET".into(), "/api/tasks".into()),
            ("GET".into(), "/api/tasks/{id}/runs/{n}".into()),
            ("POST".into(), "/api/tasks".into()),
            ("GET".into(), "/healthz".into()),
        ]
    );
    assert_eq!(r.manifest()[2].path_params, vec!["id", "n"]);
}

#[test]
fn an_unrecorded_route_is_absent_from_the_manifest() {
    let r: Router<()> = Router::new()
        .route("/x", axum::routing::get(other))
        .get("/y", other);
    assert_eq!(r.manifest().len(), 1);
    assert_eq!(r.manifest()[0].path, "/y");
}

#[tokio::test]
async fn the_axum_router_serves_what_was_recorded() {
    let app = Router::new()
        .nest("/api", api())
        .layer(axum::extract::DefaultBodyLimit::max(1024))
        .with_state(Arc::new(App))
        .into_axum();
    let resp = app
        .clone()
        .oneshot(
            Request::get("/api/tasks/t1/runs/7")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"t1/7");
    let resp = app
        .oneshot(Request::post("/api/tasks").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[test]
fn the_manifest_file_is_sorted_and_round_trips() {
    let dir = std::env::temp_dir().join(format!("common-routing-manifest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("routes.json");
    Router::new()
        .merge(Router::new().get("/healthz", other))
        .nest("/api", api())
        .write_manifest(&path)
        .unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let regs: Vec<common_routing::Registration> = serde_json::from_str(&text).unwrap();
    let paths: Vec<&str> = regs.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "/api",
            "/api/tasks",
            "/api/tasks",
            "/api/tasks/{id}/runs/{n}",
            "/healthz"
        ]
    );
    assert_eq!(
        (regs[1].method.as_str(), regs[2].method.as_str()),
        ("GET", "POST")
    );
    assert!(text.ends_with("\n"));
    let _ = std::fs::remove_dir_all(&dir);
}
