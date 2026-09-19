// In-process server shell (AD-7): an axum server embedded in the Tauri app
// process, serving the built Svelte UI at http://localhost:PORT and a small
// READ-ONLY API over the SAME core instance — single process, single writer
// (AD-14). Mutations stay on the Tauri command path in v1: this server
// exposes no mutation endpoints by design. Scope: the minimal shell that
// satisfies the AD-7 browser-identity AC — bridges, auth, and remote access
// are v0.2.0.

use crate::db::Db;
use crate::domain::evidence::Claim;
use crate::domain::hypotheses::{Hypothesis, HypothesesProjection};
use crate::domain::missions::{Mission, MissionRun, MissionsProjection};
use crate::evidence_commands::list_evidence_inner;
use crate::eventstore::EventStore;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use tower_http::services::ServeDir;
use uuid::Uuid;

/// Default listen port (override with `RC_PORT`). Bound to localhost only —
/// the same machine's browser is the v1 audience; LAN exposure arrives with
/// the v0.2.0 bridge work, never as a quiet default.
pub const DEFAULT_PORT: u16 = 4761;

#[derive(Clone)]
struct ServerState {
    /// The shared core handle — same instance, same connection, same writer.
    db: Db,
}

pub fn router(db: Db, dist_dir: std::path::PathBuf) -> Router {
    Router::new()
        .route("/api/missions", get(list_missions))
        .route("/api/missions/{mission_id}/runs", get(mission_runs))
        .route("/api/missions/{mission_id}/hypotheses", get(mission_hypotheses))
        .route(
            "/api/hypotheses/{hypothesis_id}/evidence",
            get(hypothesis_evidence),
        )
        .with_state(ServerState { db })
        // The same built Svelte UI the desktop webview loads (frontend dist).
        .fallback_service(ServeDir::new(dist_dir))
}

async fn list_missions(State(state): State<ServerState>) -> Result<Json<Vec<Mission>>, StatusCode> {
    let c = state.db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(|_| internal())?;
    MissionsProjection::fold(&events).map_err(|_| internal()).map(Json)
}

async fn mission_runs(
    State(state): State<ServerState>,
    Path(mission_id): Path<String>,
) -> Result<Json<Vec<MissionRun>>, StatusCode> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let c = state.db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(|_| internal())?;
    Ok(Json(MissionsProjection::runs_for(&events, mission_id)))
}

fn internal() -> StatusCode {
    StatusCode::INTERNAL_SERVER_ERROR
}

/// The hypothesis board of one mission (Story 1.5, read-only per AD-14).
async fn mission_hypotheses(
    State(state): State<ServerState>,
    Path(mission_id): Path<String>,
) -> Result<Json<Vec<Hypothesis>>, StatusCode> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let c = state.db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(|_| internal())?;
    HypothesesProjection::fold_for(&events, mission_id).map_err(|_| internal()).map(Json)
}

/// The evidence pins (claims + pin state) of one hypothesis (Story 1.7,
/// read-only per AD-14 — pinning stays on the Tauri command path).
async fn hypothesis_evidence(
    State(state): State<ServerState>,
    Path(hypothesis_id): Path<String>,
) -> Result<Json<Vec<Claim>>, StatusCode> {
    let hypothesis_id: Uuid = hypothesis_id
        .parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let c = state.db.0.lock().await;
    list_evidence_inner(&c, hypothesis_id)
        .map(Json)
        .map_err(|_| internal())
}

/// Resolve the frontend dist dir: `RC_DIST_DIR` override, else the compile-time
/// repo path (dev and local runs). In an installed bundle the UI ships as
/// Tauri assets and this dir does not exist — the server then serves only the
/// API, which is honest for this story's scope.
fn dist_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("RC_DIST_DIR") {
        return std::path::PathBuf::from(dir);
    }
    std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../dist"))
}

/// Spawn the server on the async runtime the Tauri app already runs on
/// (`tauri::async_runtime` — plain `tokio::spawn` panics here: the setup
/// closure runs before a reactor exists). Failure to bind is logged, never
/// fatal — the desktop app does not depend on the browser view to function.
pub fn spawn(db: Db) {
    tauri::async_runtime::spawn(async move {
        let port: u16 = std::env::var("RC_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(DEFAULT_PORT);
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let dir = dist_dir();
        let serving_ui = dir.join("index.html").is_file();
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[server] not started on {addr}: {e}");
                return;
            }
        };
        eprintln!(
            "[server] http://localhost:{port} — read-only API over the shared core{}",
            if serving_ui { " + UI" } else { " (UI build not found — API only)" }
        );
        if let Err(e) = axum::serve(listener, router(db, dir)).await {
            eprintln!("[server] stopped: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::missions::{Autonomy, MissionCreatedPayload, SPEND_RECORDED};
    use crate::eventstore::{Actor, NewEvent, SystemComponent};
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use rusqlite::Connection;
    use tower::ServiceExt;

    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        Db(std::sync::Arc::new(tokio::sync::Mutex::new(conn)))
    }

    fn app(db: Db) -> Router {
        router(db, std::path::PathBuf::from("/nonexistent-dist"))
    }

    async fn body_json<T: serde::de::DeserializeOwned>(body: Body) -> T {
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn get_api_missions_folds_the_shared_core() {
        let db = test_db();
        {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            store
                .append(
                    NewEvent::mission_created(MissionCreatedPayload {
                        question: "Does X hold up?".into(),
                        stop_condition: "Stop after $5.".into(),
                        success_criterion: "A blind rater agrees.".into(),
                        autonomy: Autonomy::Suggest,
                        spend_ceiling_cents: 500,
                    })
                    .unwrap(),
                )
                .unwrap();
        }
        let res = app(db)
            .oneshot(Request::get("/api/missions").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let missions: Vec<Mission> = body_json(res.into_body()).await;
        assert_eq!(missions.len(), 1);
        assert_eq!(missions[0].question, "Does X hold up?");
        assert_eq!(missions[0].status, crate::domain::missions::MissionStatus::Active);
    }

    #[tokio::test]
    async fn get_api_mission_runs_lists_referencing_events() {
        let db = test_db();
        let mission_id = {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let created = store
                .append(
                    NewEvent::mission_created(MissionCreatedPayload {
                        question: "Does X hold up?".into(),
                        stop_condition: "Stop after $5.".into(),
                        success_criterion: "A blind rater agrees.".into(),
                        autonomy: Autonomy::Watch,
                        spend_ceiling_cents: 500,
                    })
                    .unwrap(),
                )
                .unwrap();
            store
                .append(
                    NewEvent::new(
                        SPEND_RECORDED,
                        Actor::System { component: SystemComponent::Telemetry },
                        serde_json::json!({ "mission_id": created.id.to_string(), "cost_cents": 10 }),
                    )
                    .unwrap()
                    .with_causes(vec![created.id]),
                )
                .unwrap();
            created.id
        };
        let res = app(db)
            .oneshot(
                Request::get(format!("/api/missions/{mission_id}/runs"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let runs: Vec<MissionRun> = body_json(res.into_body()).await;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].kind, SPEND_RECORDED);
        assert_eq!(runs[0].actor, "system:telemetry");
    }

    #[tokio::test]
    async fn get_api_mission_runs_rejects_a_non_uuid_id() {
        let res = app(test_db())
            .oneshot(
                Request::get("/api/missions/not-a-uuid/runs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_api_mission_hypotheses_folds_the_board() {
        let db = test_db();
        let mission_id = {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let mission = store
                .append(NewEvent::mission_created(MissionCreatedPayload {
                    question: "Does X hold up?".into(),
                    stop_condition: "Stop after $5.".into(),
                    success_criterion: "A blind rater agrees.".into(),
                    autonomy: Autonomy::Watch,
                    spend_ceiling_cents: 500,
                })
                .unwrap())
                .unwrap();
            store
                .append(NewEvent::hypothesis_created("X holds under stiff systems.", mission.id).unwrap())
                .unwrap();
            mission.id
        };
        let res = app(db)
            .oneshot(
                Request::get(format!("/api/missions/{mission_id}/hypotheses"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let hyps: Vec<Hypothesis> = body_json(res.into_body()).await;
        assert_eq!(hyps.len(), 1);
        assert_eq!(hyps[0].statement, "X holds under stiff systems.");
        assert_eq!(hyps[0].status, crate::domain::hypotheses::HypothesisStatus::Proposed);
    }

    #[tokio::test]
    async fn get_api_hypothesis_evidence_lists_claims_with_pin_state() {
        let db = test_db();
        let hypothesis_id = {
            let c = db.0.lock().await;
            // A minimal refs table: the read path enriches pin labels from it.
            c.execute_batch(
                "CREATE TABLE refs (id TEXT PRIMARY KEY, title TEXT, authors TEXT, year INTEGER);",
            )
            .unwrap();
            c.execute(
                "INSERT INTO refs(id,title,authors,year) VALUES('ref-1','Attention Is All You Need','Vaswani et al.',2017)",
                [],
            )
            .unwrap();
            let store = EventStore::new(&c);
            let mission = store
                .append(NewEvent::mission_created(MissionCreatedPayload {
                    question: "Does X hold up?".into(),
                    stop_condition: "Stop after $5.".into(),
                    success_criterion: "A blind rater agrees.".into(),
                    autonomy: Autonomy::Watch,
                    spend_ceiling_cents: 500,
                })
                .unwrap())
                .unwrap();
            let hyp = store
                .append(NewEvent::hypothesis_created("X holds.", mission.id).unwrap())
                .unwrap();
            let claim = store
                .append(NewEvent::claim_registered("A claim about X.", hyp.id, None).unwrap())
                .unwrap();
            store
                .append(
                    NewEvent::evidence_pinned_citation(
                        claim.id,
                        hyp.id,
                        "ref-1",
                        "The quoted excerpt.",
                        0.82,
                        "GLM-5.3",
                    )
                    .unwrap(),
                )
                .unwrap();
            hyp.id
        };
        let res = app(db)
            .oneshot(
                Request::get(format!("/api/hypotheses/{hypothesis_id}/evidence"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let claims: Vec<Claim> = body_json(res.into_body()).await;
        assert_eq!(claims.len(), 1);
        assert!(claims[0].pinned);
        assert_eq!(claims[0].pin.as_ref().unwrap().assessing_model, "GLM-5.3");
    }

    #[tokio::test]
    async fn the_server_is_read_only_no_mutation_routes() {        // Single writer (AD-14): mutations exist only on the Tauri command
        // path. A POST to the read route must not be accepted.
        let res = app(test_db())
            .oneshot(Request::post("/api/missions").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}
