//! `#[client]` end to end: annotate handlers, mount them, generate the client.
//! The macro-generated `export_client_*` tests are driven in-process here,
//! because a test binary cannot re-run its own tests with an env var set.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use common_routing::export::{self, Arg, Handler, Response};
use common_routing::{client, GenerateOptions, Router};
use serde::Deserialize;
use ts_rs::TS;

#[derive(Deserialize, TS)]
#[ts(export)]
#[allow(dead_code)]
struct Filter {
    q: Option<String>,
}

#[derive(Deserialize, TS)]
#[ts(export)]
#[allow(dead_code)]
struct NewThing {
    name: String,
}

#[derive(serde::Serialize, TS)]
#[ts(export)]
struct Thing {
    id: String,
}

#[client]
async fn things(State(_s): State<()>, Query(_f): Query<Filter>) -> Json<Vec<Thing>> {
    Json(vec![])
}

#[client]
async fn thing(Path((id, _n)): Path<(String, u32)>) -> Result<Json<Thing>, StatusCode> {
    Ok(Json(Thing { id }))
}

#[client]
async fn create(Json(_b): Json<NewThing>) -> (StatusCode, Json<Thing>) {
    (StatusCode::CREATED, Json(Thing { id: "x".into() }))
}

#[client]
async fn remove(Path(_id): Path<String>) -> StatusCode {
    StatusCode::NO_CONTENT
}

#[client(link)]
async fn download(Path(_id): Path<String>) -> axum::response::Response {
    StatusCode::OK.into_response()
}
use axum::response::IntoResponse;

fn routes() -> Router<()> {
    Router::new()
        .get("/api/things", things)
        .get("/api/things/{id}/{n}", thing)
        .post("/api/things", create)
        .post("/api/things/{id}/delete", remove)
        .get("/api/things/{id}/download", download)
}

#[test]
fn export_tests_are_generated_and_inert_without_the_variable() {
    assert!(
        export::dir().is_none(),
        "COMMON_ROUTING_EXPORT_DIR must be unset for this suite"
    );
    export_client_things();
    export_client_thing();
    export_client_create();
    export_client_remove();
    export_client_download();
}

#[test]
fn the_whole_pipeline_produces_a_typed_client() {
    let dir = std::env::temp_dir().join(format!("common-routing-pipeline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let fq = |n: &str| format!("{}::{n}", module_path!());
    for h in [
        Handler {
            fqname: fq("things"),
            name: "things".into(),
            args: vec![Arg::query::<Filter>("_f")],
            response: Response::json::<Vec<Thing>>(),
        },
        Handler {
            fqname: fq("thing"),
            name: "thing".into(),
            args: vec![Arg::path::<(String, u32)>("path")],
            response: Response::json::<Thing>(),
        },
        Handler {
            fqname: fq("create"),
            name: "create".into(),
            args: vec![Arg::body::<NewThing>("_b")],
            response: Response::json::<Thing>(),
        },
        Handler {
            fqname: fq("remove"),
            name: "remove".into(),
            args: vec![Arg::path::<String>("_id")],
            response: Response::NoContent,
        },
        Handler {
            fqname: fq("download"),
            name: "download".into(),
            args: vec![Arg::path::<String>("_id")],
            response: Response::Link,
        },
    ] {
        export::append(&dir, h).unwrap();
    }
    routes().write_manifest(&dir.join("routes.json")).unwrap();

    let manifest = routes();
    for r in manifest.manifest() {
        assert!(r.fqname.starts_with(module_path!()), "{}", r.fqname);
    }

    let n = GenerateOptions::default()
        .generate(
            &dir.join("routes.json"),
            &dir.join("handlers.json"),
            &dir.join("client.ts"),
        )
        .expect("generates");
    assert_eq!(n, 5);
    let ts = std::fs::read_to_string(dir.join("client.ts")).unwrap();
    assert!(
        ts.contains("export function things(query: Filter): Promise<Array<Thing>>"),
        "{ts}"
    );
    assert!(
        ts.contains("export function thing(id: string, n: number): Promise<Thing>"),
        "{ts}"
    );
    assert!(
        ts.contains("export function create(body: NewThing): Promise<Thing>"),
        "{ts}"
    );
    assert!(
        ts.contains("export function remove(id: string): Promise<void>"),
        "{ts}"
    );
    assert!(
        ts.contains("export function downloadUrl(id: string): string"),
        "{ts}"
    );
    assert!(
        ts.contains("  Filter,\n  NewThing,\n  Thing,\n} from \"./index\";"),
        "{ts}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
