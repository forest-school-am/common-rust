//! `#[client(path = …)]` end to end: a handler whose path params are read
//! INSIDE a guard extractor, so its signature has no `Path<T>` at all. Before
//! this, such a handler could only be described by surfacing a second, redundant
//! `Path<T>` beside the guard — extracting the same segments twice.
//!
//! Unlike `client_macro.rs`, which hand-builds its descriptors, this drives the
//! macro-generated `export_client_*` functions directly with the export dir set,
//! so what reaches the generator is exactly what the macro emitted.

use std::collections::HashMap;

use axum::extract::{FromRequestParts, Path};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::Json;
use common_routing::{client, GenerateOptions, Router};
use ts_rs::TS;

#[derive(serde::Serialize, TS)]
#[ts(export)]
struct Group {
    name: String,
}

/// role-ui's shape: the guard reads the path itself to authorize, and hands the
/// handler the ALREADY-CHECKED value. The handler never sees a `Path<T>`.
struct GroupAccess(#[allow(dead_code)] String);

impl<S> FromRequestParts<S> for GroupAccess
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(params): Path<HashMap<String, String>> = Path::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        let name = params
            .get("group_name")
            .cloned()
            .ok_or(StatusCode::BAD_REQUEST)?;
        Ok(GroupAccess(name))
    }
}

#[client(path = "/api/groups/{group_name}")]
async fn group(_access: GroupAccess) -> Json<Group> {
    Json(Group {
        name: "x".to_owned(),
    })
}

#[client(path = "/api/groups/{group_name}/members/{position: number}")]
async fn member(_access: GroupAccess) -> Json<Group> {
    Json(Group {
        name: "y".to_owned(),
    })
}

fn routes() -> Router<()> {
    Router::new()
        .get("/api/groups/{group_name}", group)
        .get("/api/groups/{group_name}/members/{position}", member)
}

#[test]
fn a_guarded_handler_declares_its_params_and_needs_no_redundant_path() {
    let dir = std::env::temp_dir().join(format!("common-routing-declared-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // The generated export fns are inert unless this is set; setting it and
    // calling them directly is what makes this the macro's own output.
    std::env::set_var(common_routing::export::EXPORT_DIR, &dir);
    export_client_group();
    export_client_member();

    routes().write_manifest(&dir.join("routes.json")).unwrap();

    let n = GenerateOptions::default()
        .generate(
            &dir.join("routes.json"),
            &dir.join("handlers.json"),
            &dir.join("client.ts"),
        )
        .expect("generates");
    assert_eq!(n, 2);

    let ts = std::fs::read_to_string(dir.join("client.ts")).unwrap();
    assert!(
        ts.contains("export function group(group_name: string): Promise<Group>"),
        "{ts}"
    );
    // The declared type is honoured, and the params keep TEMPLATE order.
    assert!(
        ts.contains("export function member(group_name: string, position: number): Promise<Group>"),
        "{ts}"
    );
    // The guard is not a client argument.
    assert!(!ts.contains("GroupAccess"), "{ts}");

    let _ = std::fs::remove_dir_all(&dir);
}
