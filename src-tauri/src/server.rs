// In-process server shell (AD-7): an axum server embedded in the Tauri app
// process, serving the built Svelte UI at http://localhost:PORT and a small
// READ-ONLY API over the SAME core instance — single process, single writer
// (AD-14). Mutations stay on the Tauri command path in v1: this server
// exposes no mutation endpoints by design. Scope: the minimal shell that
// satisfies the AD-7 browser-identity AC — bridges, auth, and remote access
// are v0.2.0.

use crate::chat_commands::list_skills_inner;
use crate::db::Db;
use crate::domain::checkpoints::{fold_checkpoints, CheckpointsView};
use crate::domain::evidence::Claim;
use crate::domain::hypotheses::{Hypothesis, HypothesesProjection};
use crate::domain::missions::{Mission, MissionRun, MissionsProjection};
use crate::domain::proposals::Proposal;
use crate::domain::digest::MorningDigest;
use crate::evidence_commands::list_evidence_inner;
use crate::eventstore::EventStore;
use crate::nightshift::morning_digest;
use crate::proposals_commands::list_proposals_inner;
use crate::domain::readiness::ReadinessReport;
use crate::domain::search::SearchDisclosure;
use crate::domain::skills::Skill;
use crate::readiness_commands::readiness_report_inner;
use crate::search_commands::search_disclosure_inner;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::Value;
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
        .route("/api/digest", get(digest))
        .route("/api/runs/{run_id}/receipt", get(run_receipt))
        .route("/api/trust", get(trust))
        .route("/api/checkpoints", get(checkpoints))
        .route("/api/proposals", get(all_proposals))
        .route(
            "/api/missions/{mission_id}/proposals",
            get(mission_proposals),
        )
        .route("/api/missions/{mission_id}/jobs", get(mission_jobs))
        .route(
            "/api/missions/{mission_id}/search-disclosure",
            get(mission_search_disclosure),
        )
        .route("/api/search-disclosure", get(search_disclosure_all))
        .route("/api/readiness", get(readiness_all))
        .route(
            "/api/missions/{mission_id}/readiness",
            get(mission_readiness),
        )
        .route("/api/jobs/{job_id}/result-proposals", get(job_result_proposals))
        .route("/api/targets", get(compute_targets))
        .route("/api/skills", get(list_skills))
        .route("/api/ai-config", get(ai_config))
        .route("/api/host-allowlist", get(host_allowlist))
        .route("/api/refs", get(library_refs))
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

/// The skills registry (Story 5.6, FR-16.8): the read-only list the served
/// browser view's chat header renders — the curated six plus user-added.
async fn list_skills(State(state): State<ServerState>) -> Result<Json<Vec<Skill>>, StatusCode> {
    let c = state.db.0.lock().await;
    list_skills_inner(&c).map_err(|_| internal()).map(Json)
}

/// The AI provider configuration read (Stories 5.7–5.9, read-only per
/// AD-14): what the served view's chat header and unconfigured state
/// render — never the key itself, only its presence.
async fn ai_config(State(state): State<ServerState>) -> Result<Json<Value>, StatusCode> {
    let c = state.db.0.lock().await;
    crate::commands::ai_config_inner(&c)
        .map(Json)
        .map_err(|_| internal())
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

/// The search disclosure of one mission (Story 4.1, FR-12.1 — read-only
/// per AD-14; running a search stays on the Tauri command path): every
/// search's PRISMA row, null results rendered as rows, never as "nothing
/// happened". An unknown mission is an honest empty disclosure.
async fn mission_search_disclosure(
    State(state): State<ServerState>,
    Path(mission_id): Path<String>,
) -> Result<Json<SearchDisclosure>, StatusCode> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let c = state.db.0.lock().await;
    search_disclosure_inner(&c, Some(mission_id))
        .map(Json)
        .map_err(|_| internal())
}

/// The workspace-wide search disclosure (Story 4.1, FR-12.1 — read-only):
/// every search every mission's runs and the user performed, at the log's
/// current head.
async fn search_disclosure_all(
    State(state): State<ServerState>,
) -> Result<Json<SearchDisclosure>, StatusCode> {
    let c = state.db.0.lock().await;
    search_disclosure_inner(&c, None)
        .map(Json)
        .map_err(|_| internal())
}

/// The workspace readiness report (Story 4.3, FR-13.1/13.2 — read-only per
/// AD-14): the preprint-tier gate, a pure derived view over the shared log
/// at its current head — no readiness state exists anywhere to serve stale.
async fn readiness_all(
    State(state): State<ServerState>,
) -> Result<Json<ReadinessReport>, StatusCode> {
    let c = state.db.0.lock().await;
    readiness_report_inner(&c, None)
        .map(Json)
        .map_err(|_| internal())
}

/// The readiness report of one mission (Story 4.3, FR-13.1/13.2 —
/// read-only): the same gate scoped to the mission's board. An unknown
/// mission is an honest empty scope (the same contract as the disclosure
/// read).
async fn mission_readiness(
    State(state): State<ServerState>,
    Path(mission_id): Path<String>,
) -> Result<Json<ReadinessReport>, StatusCode> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let c = state.db.0.lock().await;
    readiness_report_inner(&c, Some(mission_id))
        .map(Json)
        .map_err(|_| internal())
}

/// The morning digest (Story 2.3, FR-4.4 — read-only per AD-14; the manual
/// Night Shift trigger and schedule changes stay on the Tauri command path).
/// Delivered even when runs failed (FR-4.3) — the projection never depends
/// on run success.
async fn digest(State(state): State<ServerState>) -> Result<Json<MorningDigest>, StatusCode> {
    morning_digest(&state.db).await.map(Json).map_err(|_| internal())
}

/// One run's timeline receipt (Story 2.5, FR-6.1/6.2 — read-only per AD-14):
/// the ordered audit ledger the drill-down renders — a pure query over the
/// shared core, so the served browser view replays the identical ledger the
/// desktop webview does. An unknown run id (no `run.started` carries it) is
/// an honest 404, never an empty receipt.
async fn run_receipt(
    State(state): State<ServerState>,
    Path(run_id): Path<String>,
) -> Result<Json<crate::domain::receipts::RunReceipt>, StatusCode> {
    let run_id = run_id.trim().to_string();
    if run_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let c = state.db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(|_| internal())?;
    crate::domain::receipts::render_receipt(&events, &run_id)
        .map_err(|_| internal())?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// The trust status (Story 2.4, FR-5 — read-only per AD-14): runtime state,
/// effective dials and ceilings, spend vs ceiling meters, the last run's
/// spend line. Dial, ceiling, and kill-switch changes stay on the Tauri
/// command path.
async fn trust(
    State(state): State<ServerState>,
) -> Result<Json<crate::trust::TrustStatus>, StatusCode> {
    crate::trust::status(&state.db)
        .await
        .map(Json)
        .map_err(|_| internal())
}

/// The checkpoints read model (Story 2.6, FR-10.1 — read-only per AD-14):
/// the restore points and the rollback history. Creating checkpoints,
/// previewing, and rolling back stay on the Tauri command path.
async fn checkpoints(
    State(state): State<ServerState>,
) -> Result<Json<CheckpointsView>, StatusCode> {
    let c = state.db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(|_| internal())?;
    fold_checkpoints(&events).map(Json).map_err(|_| internal())
}

/// The quarantine read model, all missions (Story 2.2, read-only per AD-14
/// — merging and rejecting stay on the Tauri command path).
async fn all_proposals(
    State(state): State<ServerState>,
) -> Result<Json<Vec<Proposal>>, StatusCode> {
    let c = state.db.0.lock().await;
    list_proposals_inner(&c, None).map(Json).map_err(|_| internal())
}

/// The quarantine read model of one mission (Story 2.2, read-only).
async fn mission_proposals(
    State(state): State<ServerState>,
    Path(mission_id): Path<String>,
) -> Result<Json<Vec<Proposal>>, StatusCode> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let c = state.db.0.lock().await;
    list_proposals_inner(&c, Some(mission_id))
        .map(Json)
        .map_err(|_| internal())
}

/// The compute jobs of one mission (Story 3.2, FR-11.4 — read-only per
/// AD-14: submitting and polling stay on the Tauri command path). The
/// served browser replays the last observed lifecycle state.
async fn mission_jobs(
    State(state): State<ServerState>,
    Path(mission_id): Path<String>,
) -> Result<Json<Vec<crate::domain::jobs::Job>>, StatusCode> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let c = state.db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(|_| internal())?;
    crate::domain::jobs::JobsProjection::fold_for(&events, mission_id)
        .map(Json)
        .map_err(|_| internal())
}

/// The result proposals of one job (Story 3.4, FR-11.5 — read-only per
/// AD-14: fetching results into quarantine stays on the Tauri command
/// path). Decided proposals included — history is honest.
async fn job_result_proposals(
    State(state): State<ServerState>,
    Path(job_id): Path<String>,
) -> Result<Json<Vec<Proposal>>, StatusCode> {
    let job_id: Uuid = job_id
        .parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let c = state.db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(|_| internal())?;
    crate::jobs_commands::list_job_result_proposals_inner(&events, job_id)
        .map(Json)
        .map_err(|_| internal())
}

/// The compute target list (Story 3.2/3.3, FR-11.1 — read-only per
/// AD-14): declared targets plus the built-in `local`, with each ssh
/// target's host and allowlisted status.
async fn compute_targets(
    State(state): State<ServerState>,
) -> Result<Json<Vec<crate::jobs_commands::ComputeTargetView>>, StatusCode> {
    let c = state.db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(|_| internal())?;
    crate::jobs_commands::list_targets_inner(&events)
        .map(Json)
        .map_err(|_| internal())
}

/// The host allowlist (Story 3.3 — read-only per AD-14): the hosts SSH
/// targets may connect to. Editing happens in the desktop app (the
/// mutation commands are Tauri-only, AD-14).
async fn host_allowlist(
    State(state): State<ServerState>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let c = state.db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(|_| internal())?;
    Ok(Json(crate::domain::jobs::fold_host_allowlist(&events)))
}

/// The references library (FR-15, Epic 5 — read-only per AD-14): the
/// evented library fold (legacy baseline + ref.added/removed/restored
/// events), optionally scoped to one project via `?project_id=`. Adds,
/// removes, restores, and the Zotero import are mutations — they stay on
/// the Tauri command path.
async fn library_refs(
    State(state): State<ServerState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<crate::domain::library::LibraryRef>>, StatusCode> {
    let c = state.db.0.lock().await;
    crate::library_commands::list_refs_inner(
        &c,
        params.get("project_id").map(String::as_str),
        params.get("filter").map(String::as_str),
    )
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
                        schedule: "daily-03:00".into(),
                    roles: vec![],
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
                        schedule: "daily-03:00".into(),
                    roles: vec![],
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
                    schedule: "daily-03:00".into(),
                roles: vec![],
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
            // The real relational schema + one library ref: the evidence
            // read enriches pin labels + removed flags from the library
            // fold over it (the legacy baseline, FR-15).
            crate::db::Db::migrate(&c).unwrap();
            c.execute(
                "INSERT INTO projects(id,name,folder,kind,tags,color,chapter_index,chapter_count,created_at,updated_at,is_active) \
                 VALUES('p1','Research','~','paper','','#3B5BDB',1,1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO refs(id,project_id,title,authors,year,venue,doi,url,tags,status,used,citation_count,created_at) \
                 VALUES('ref-1','p1','Attention Is All You Need','Vaswani et al.',2017,'NeurIPS',\
                 '10.48550/arXiv.1706.03762','https://arxiv.org/abs/1706.03762','','read',1,2,'2026-01-01T00:00:00Z')",
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
                    schedule: "daily-03:00".into(),
                roles: vec![],
                })
                .unwrap())
                .unwrap();
            let hyp = store
                .append(NewEvent::hypothesis_created("X holds.", mission.id).unwrap())
                .unwrap();
            let claim = store
                .append(NewEvent::claim_registered("A claim about X.", hyp.id, None).unwrap())
                .unwrap();
            let pin = store
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
            // Story 4.2: a machine verification of the pin — the route
            // serves the same verification status the desktop webview
            // renders (AD-15, read-only per AD-14).
            store
                .append(
                    NewEvent::evidence_verified(
                        claim.id,
                        hyp.id,
                        pin.seq,
                        crate::domain::verifier::VerificationOutcome::Verified,
                        crate::domain::verifier::DETAIL_EXCERPT_MATCHED,
                        "arxiv:1706.03762",
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
        // The verification axis renders alongside the confidence axis —
        // two separate fields, never one driving the other.
        let verification = claims[0]
            .pin
            .as_ref()
            .unwrap()
            .verification
            .as_ref()
            .expect("the verification status is served");
        assert_eq!(
            verification.status,
            crate::domain::evidence::VerificationStatus::Verified
        );
        assert_eq!(verification.detail, "excerpt_matched");
    }

    #[tokio::test]
    async fn get_api_search_disclosure_serves_the_prisma_rows() {
        // Story 4.1 (FR-12.1): the served browser view replays the
        // identical disclosure the desktop webview does — null results
        // as rows, never as "nothing happened".
        let db = test_db();
        let mission_id = {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let mission = store
                .append(
                    NewEvent::mission_created(MissionCreatedPayload {
                        question: "Does sparse attention hold at 32k?".into(),
                        stop_condition: "Stop after $5.".into(),
                        success_criterion: "A blind rater agrees.".into(),
                        autonomy: Autonomy::Watch,
                        spend_ceiling_cents: 500,
                        schedule: "daily-03:00".into(),
                        roles: vec![],
                    })
                    .unwrap(),
                )
                .unwrap();
            // one search with results, one null — through the ONE seam
            crate::domain::search::run_search(
                &store,
                &crate::domain::search::SearchParams {
                    query: "sparse attention".into(),
                    database: "arxiv".into(),
                    filters: Default::default(),
                    order: None,
                    first_page: true,
                    mission_id: Some(mission.id),
                    run_id: Some("nightshift-32k".into()),
                },
            )
            .unwrap();
            crate::domain::search::run_search(
                &store,
                &crate::domain::search::SearchParams {
                    query: "qqqq zzzz".into(),
                    database: "web".into(),
                    filters: Default::default(),
                    order: None,
                    first_page: true,
                    mission_id: Some(mission.id),
                    run_id: Some("nightshift-32k".into()),
                },
            )
            .unwrap();
            mission.id
        };
        // mission-scoped: both searches, the null one visible and counted
        let res = app(db.clone())
            .oneshot(
                Request::get(format!("/api/missions/{mission_id}/search-disclosure"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let disclosure: SearchDisclosure = body_json(res.into_body()).await;
        assert_eq!(disclosure.total, 2);
        assert_eq!(disclosure.null_result_count, 1);
        assert_eq!(disclosure.rows[0].database, "arxiv");
        assert!(!disclosure.rows[0].null_result);
        assert!(disclosure.rows[1].null_result);
        assert_eq!(disclosure.rows[1].result_count, 0);
        // workspace-wide sees the same searches
        let res = app(db)
            .oneshot(
                Request::get("/api/search-disclosure")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let all: SearchDisclosure = body_json(res.into_body()).await;
        assert_eq!(all.total, 2);
        // a malformed mission id is a 400, never a 500
        let res = app(test_db())
            .oneshot(
                Request::get("/api/missions/not-a-uuid/search-disclosure")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_api_proposals_lists_the_quarantine_read_only() {
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
                    schedule: "daily-03:00".into(),
                    roles: vec![],
                })
                .unwrap())
                .unwrap();
            let hyp = store
                .append(NewEvent::hypothesis_created("X holds.", mission.id).unwrap())
                .unwrap();
            crate::domain::proposals::propose_transition(
                &store,
                "run-7",
                hyp.id,
                crate::domain::hypotheses::HypothesisStatus::Testing,
                "run 7 suggests testing",
            )
            .unwrap();
            mission.id
        };
        // all proposals
        let res = app(db.clone())
            .oneshot(Request::get("/api/proposals").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let proposals: Vec<Proposal> = body_json(res.into_body()).await;
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].status, crate::domain::proposals::ProposalStatus::Pending);
        assert_eq!(proposals[0].mission_id, Some(mission_id));
        assert_eq!(proposals[0].run_id, "run-7");
        // mission-scoped
        let res = app(db.clone())
            .oneshot(
                Request::get(format!("/api/missions/{mission_id}/proposals"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let scoped: Vec<Proposal> = body_json(res.into_body()).await;
        assert_eq!(scoped.len(), 1);
        // a non-uuid mission id is a 400
        let res = app(db.clone())
            .oneshot(
                Request::get("/api/missions/not-a-uuid/proposals")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        // single writer (AD-14): merging stays on the Tauri command path
        let res = app(db)
            .oneshot(Request::post("/api/proposals").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    /// The job result-proposals route (Story 3.4, FR-11.5): read-only over
    /// the shared core — the fetch itself stays on the Tauri command path
    /// (single writer, AD-14).
    #[tokio::test]
    async fn get_api_job_result_proposals_lists_the_jobs_quarantine_read_only() {
        let db = test_db();
        let (mission_id, job_id) = {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let mission = store
                .append(NewEvent::mission_created(MissionCreatedPayload {
                    question: "Does X hold up?".into(),
                    stop_condition: "Stop after $5.".into(),
                    success_criterion: "A blind rater agrees.".into(),
                    autonomy: Autonomy::Watch,
                    spend_ceiling_cents: 500,
                    schedule: "daily-03:00".into(),
                    roles: vec![],
                })
                .unwrap())
                .unwrap();
            let hyp = store
                .append(NewEvent::hypothesis_created("X holds.", mission.id).unwrap())
                .unwrap();
            let claim = store
                .append(NewEvent::claim_registered("Result artifact.", hyp.id, None).unwrap())
                .unwrap();
            let job = store
                .append(NewEvent::job_submitted(crate::domain::jobs::JobSubmittedPayload {
                    mission_id: mission.id,
                    target: "local".into(),
                    handle: "h-1".into(),
                    spec: crate::domain::jobs::JobSpec {
                        cmd: "echo".into(),
                        args: vec!["done".into()],
                        env: Default::default(),
                        resources: None,
                        workdir: None,
                    },
                })
                .unwrap())
                .unwrap();
            store
                .append(NewEvent::job_finished(crate::domain::jobs::JobLifecyclePayload {
                    mission_id: mission.id,
                    job_id: job.id,
                    target: "local".into(),
                    code: Some(0),
                    reason: None,
                })
                .unwrap())
                .unwrap();
            crate::domain::proposals::propose_evidence_pin(
                &store,
                "fetch-1",
                claim.id,
                hyp.id,
                "jobs/abc/stdout",
                "accuracy: 0.912, n=5",
                0.5,
                "GLM-5.3",
                job.id,
                vec![job.id, mission.id],
            )
            .unwrap();
            (mission.id, job.id)
        };
        // the job's result proposals, read-only
        let res = app(db.clone())
            .oneshot(
                Request::get(format!("/api/jobs/{job_id}/result-proposals"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let proposals: Vec<Proposal> = body_json(res.into_body()).await;
        assert_eq!(proposals.len(), 1);
        assert_eq!(
            proposals[0].proposed_kind,
            crate::domain::evidence::EVIDENCE_PINNED
        );
        assert_eq!(proposals[0].mission_id, Some(mission_id));
        // another job's route is empty (scoped), and a non-uuid is a 400
        let res = app(db.clone())
            .oneshot(
                Request::get(format!("/api/jobs/{}/result-proposals", Uuid::new_v4()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(body_json::<Vec<Proposal>>(res.into_body()).await.is_empty());
        let res = app(db)
            .oneshot(
                Request::get("/api/jobs/not-a-uuid/result-proposals")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    /// The run receipt route (Story 2.5, FR-6.1): read-only over the shared
    /// core — the drill-down's ledger, replayed identically from the events.
    #[tokio::test]
    async fn get_api_run_receipt_renders_the_ledger_read_only() {
        let db = test_db();
        {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let mission = store
                .append(
                    NewEvent::mission_created(
                        crate::domain::missions::MissionCreatedPayload {
                            question: "Does X hold up?".into(),
                            stop_condition: "Stop after $5.".into(),
                            success_criterion: "A blind rater agrees.".into(),
                            autonomy: Autonomy::Suggest,
                            spend_ceiling_cents: 100,
                            roles: vec![],
                            schedule: "daily-03:00".into(),
                        },
                    )
                    .unwrap(),
                )
                .unwrap();
            store
                .append(
                    NewEvent::run_started(
                        "ns-42",
                        mission.id,
                        "daily-03:00",
                        crate::domain::nightshift::SCAN_STEP,
                    )
                    .unwrap(),
                )
                .unwrap();
            store
                .append(
                    NewEvent::spend_recorded(crate::domain::spend::SpendRecordedPayload {
                        provider: "openrouter".into(),
                        model: "GLM-5.3".into(),
                        input_tokens: 3_812,
                        output_tokens: 964,
                        cost_cents: 9,
                        mission_id: Some(mission.id),
                        role: Some("drafter".into()),
                        run_id: Some("step-1".into()),
                        note: None,
                    })
                    .unwrap(),
                )
                .unwrap();
            store
                .append(NewEvent::run_finished("ns-42", mission.id, "1 scan done", 0).unwrap())
                .unwrap();
        }
        let res = app(db.clone())
            .oneshot(
                Request::get("/api/runs/ns-42/receipt").body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let receipt: crate::domain::receipts::RunReceipt = body_json(res.into_body()).await;
        assert_eq!(receipt.run_id, "ns-42");
        assert_eq!(
            receipt.outcome,
            crate::domain::receipts::RunOutcome::Finished
        );
        assert_eq!(receipt.spend_cents, 9);
        // the ledger: run start, the scan's search, the call, the finish —
        // in seq order, with e-seq refs
        let kinds: Vec<&str> = receipt
            .rows
            .iter()
            .map(|r| r.action.kind())
            .collect();
        assert_eq!(kinds, vec!["run_start", "search", "call", "run_end"]);
        assert!(receipt.rows.iter().all(|r| r.seq > 0));
        // an unknown run id is an honest 404 — no run.started, no receipt
        let res = app(db.clone())
            .oneshot(
                Request::get("/api/runs/never-heard-of/receipt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        // single writer (AD-14): the receipt is a read — POST is not routed
        let res = app(db)
            .oneshot(
                Request::post("/api/runs/ns-42/receipt").body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn get_api_digest_renders_the_night_read_only() {
        let db = test_db();
        {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let mission = store
                .append(
                    NewEvent::mission_created(MissionCreatedPayload {
                        question: "Does X hold up?".into(),
                        stop_condition: "Stop after $5.".into(),
                        success_criterion: "A blind rater agrees.".into(),
                        autonomy: Autonomy::Watch,
                        spend_ceiling_cents: 500,
                        roles: vec![],
                        schedule: "daily-03:00".into(),
                    })
                    .unwrap(),
                )
                .unwrap();
            // a finished night run — the digest renders its row
            store
                .append(
                    NewEvent::run_started("ns-1", mission.id, "daily-03:00", "literature-scan")
                        .unwrap(),
                )
                .unwrap();
            store
                .append(NewEvent::run_finished("ns-1", mission.id, "1 scan done", 0).unwrap())
                .unwrap();
        }
        let res = app(db)
            .oneshot(Request::get("/api/digest").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let digest: crate::domain::digest::MorningDigest = body_json(res.into_body()).await;
        assert_eq!(digest.outcome, crate::domain::digest::DigestOutcome::AllFinished);
        assert_eq!(digest.rows.len(), 1);
        assert_eq!(digest.rows[0].runs, 1);
        // an empty core renders an honest no-runs digest, not an error
        let res = app(test_db())
            .oneshot(Request::get("/api/digest").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let digest: crate::domain::digest::MorningDigest = body_json(res.into_body()).await;
        assert_eq!(digest.outcome, crate::domain::digest::DigestOutcome::NoRuns);
        // single writer (AD-14): triggering the night shift is a mutation —
        // it stays on the Tauri command path
        let res = app(test_db())
            .oneshot(Request::post("/api/digest").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    /// The trust status route (Story 2.4, FR-5): read-only over the shared
    /// core — dials, ceilings, meters, runtime state; mutations stay on the
    /// Tauri command path.
    #[tokio::test]
    async fn get_api_trust_renders_the_trust_center_read_only() {
        let db = test_db();
        {
            let conn = db.0.lock().await;
            let store = EventStore::new(&conn);
            store
                .append(
                    NewEvent::mission_created(crate::domain::missions::MissionCreatedPayload {
                        question: "Does X hold?".into(),
                        stop_condition: "Stop after $1.".into(),
                        success_criterion: "A rater agrees.".into(),
                        autonomy: Autonomy::Suggest,
                        spend_ceiling_cents: 100,
                        roles: vec![],
                        schedule: "off".into(),
                    })
                    .unwrap(),
                )
                .unwrap();
            store
                .append(
                    NewEvent::ceiling_configured(crate::domain::trust::CeilingConfiguredPayload {
                        scope: crate::domain::trust::Scope::Global,
                        scope_id: None,
                        ceiling_cents: 2000,
                    })
                    .unwrap(),
                )
                .unwrap();
        }
        let res = app(db)
            .oneshot(Request::get("/api/trust").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let status: crate::trust::TrustStatus = body_json(res.into_body()).await;
        assert_eq!(status.runtime_state, crate::trust::RuntimeState::Running);
        assert_eq!(status.global_ceiling_cents, Some(2000));
        assert_eq!(status.missions.len(), 1);
        assert_eq!(status.missions[0].ceiling_cents, 100);
        // single writer (AD-14): the kill switch is a mutation — POST is not
        // even routed
        let res = app(test_db())
            .oneshot(Request::post("/api/trust").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
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

    /// The checkpoints route (Story 2.6, FR-10.1): read-only over the shared
    /// core — the restore points and the rollback history. Creating
    /// checkpoints, previewing, and rolling back stay on the Tauri command
    /// path (single writer, AD-14).
    #[tokio::test]
    async fn get_api_checkpoints_lists_restore_points_read_only() {
        let db = test_db();
        {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            store
                .append(
                    NewEvent::mission_created(MissionCreatedPayload {
                        question: "Does X hold up?".into(),
                        stop_condition: "Stop after $5.".into(),
                        success_criterion: "A rater agrees.".into(),
                        autonomy: Autonomy::Watch,
                        spend_ceiling_cents: 500,
                        roles: vec![],
                        schedule: "off".into(),
                    })
                    .unwrap(),
                )
                .unwrap();
            let head = store.head_seq().unwrap();
            let cp = store
                .append(NewEvent::checkpoint_created("pre-trial", head).unwrap())
                .unwrap();
            store
                .append(NewEvent::checkpoint_rolled_back(cp.id, "pre-trial", head, 0).unwrap())
                .unwrap();
        }
        let res = app(db)
            .oneshot(Request::get("/api/checkpoints").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let view: CheckpointsView = body_json(res.into_body()).await;
        assert_eq!(view.checkpoints.len(), 1);
        assert_eq!(view.checkpoints[0].name, "pre-trial");
        assert_eq!(view.checkpoints[0].seq, 1, "the log head at creation");
        assert_eq!(view.head_seq, 3);
        assert_eq!(view.rollbacks.len(), 1);
        assert_eq!(view.rollbacks[0].name, "pre-trial");
        assert_eq!(view.rollbacks[0].orphaned_count, 0);
        // an empty core renders an honest empty view, not an error
        let res = app(test_db())
            .oneshot(Request::get("/api/checkpoints").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let view: CheckpointsView = body_json(res.into_body()).await;
        assert!(view.checkpoints.is_empty());
        assert!(view.rollbacks.is_empty());
        // single writer (AD-14): rollback is a mutation — POST is not routed
        let res = app(test_db())
            .oneshot(Request::post("/api/checkpoints").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    /// The compute-job + target reads (Story 3.2, read-only per AD-14):
    /// the mission's jobs fold from the log — the served browser replays
    /// the last observed lifecycle — and the target list carries the
    /// built-in `local`. Submitting/polling stay on the Tauri command
    /// path: POST is not routed.
    #[tokio::test]
    async fn get_api_jobs_and_targets_fold_the_shared_core() {
        let db = test_db();
        let mission_id;
        {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let mission = store
                .append(
                    NewEvent::mission_created(MissionCreatedPayload {
                        question: "Does X hold?".into(),
                        stop_condition: "3 rounds".into(),
                        success_criterion: "A rater agrees.".into(),
                        autonomy: Autonomy::Suggest,
                        spend_ceiling_cents: 500,
                        schedule: "off".into(),
                        roles: vec![],
                    })
                    .unwrap(),
                )
                .unwrap();
            mission_id = mission.id;
            let submitted = store
                .append(
                    NewEvent::job_submitted(crate::domain::jobs::JobSubmittedPayload {
                        mission_id,
                        target: "local".into(),
                        handle: "h-1".into(),
                        spec: crate::domain::jobs::JobSpec {
                            cmd: "python3".into(),
                            args: vec!["train.py".into()],
                            env: Default::default(),
                            resources: None,
                            workdir: None,
                        },
                    })
                    .unwrap(),
                )
                .unwrap();
            store
                .append(NewEvent::job_running(crate::domain::jobs::JobLifecyclePayload {
                    mission_id,
                    job_id: submitted.id,
                    target: "local".into(),
                    code: None,
                    reason: None,
                })
                .unwrap())
                .unwrap();
        }
        let res = app(db)
            .oneshot(
                Request::get(format!("/api/missions/{mission_id}/jobs"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let jobs: Vec<crate::domain::jobs::Job> = body_json(res.into_body()).await;
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].phase, crate::domain::jobs::JobPhase::Running);
        assert_eq!(jobs[0].target, "local");
        assert!(jobs[0].running_ts.is_some());

        // the target list: the built-in local, no declaration behind it
        let res = app(test_db())
            .oneshot(Request::get("/api/targets").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let targets: Vec<crate::jobs_commands::ComputeTargetView> =
            body_json(res.into_body()).await;
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].name, "local");
        assert!(targets[0].builtin);
        // single writer (AD-14): job submission is a mutation — POST is not routed
        let res = app(test_db())
            .oneshot(
                Request::post(format!("/api/missions/{mission_id}/jobs"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
        // a malformed mission id is a 400, not a 500
        let res = app(test_db())
            .oneshot(Request::get("/api/missions/not-a-uuid/jobs").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
    /// The readiness routes (Story 4.3, FR-13.1/13.2): the workspace report
    /// and the mission-scoped report serve the same pure fold the desktop
    /// webview renders — blockers reference their specific board objects; a
    /// malformed mission id is a 400; the routes are read-only (POST is not
    /// routed).
    #[tokio::test]
    async fn get_api_readiness_serves_the_derived_gate() {
        let db = test_db();
        let mission_id = {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let mission = store
                .append(
                    NewEvent::mission_created(MissionCreatedPayload {
                        question: "Does X hold up?".into(),
                        stop_condition: "Stop after $5.".into(),
                        success_criterion: "A blind rater agrees.".into(),
                        autonomy: Autonomy::Suggest,
                        spend_ceiling_cents: 500,
                        schedule: "daily-03:00".into(),
                        roles: vec![],
                    })
                    .unwrap(),
                )
                .unwrap();
            let h = store
                .append(NewEvent::hypothesis_created("X holds.", mission.id).unwrap())
                .unwrap();
            store
                .append(NewEvent::claim_registered("X holds at 32k.", h.id, None).unwrap())
                .unwrap();
            mission.id
        };
        // workspace-wide: not ready, the unpinned claim + the load-bearing
        // hypothesis both referenced by their specific ids
        let res = app(db.clone())
            .oneshot(Request::get("/api/readiness").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let report: ReadinessReport = body_json(res.into_body()).await;
        assert_eq!(report.verdict, crate::domain::readiness::ReadinessVerdict::NotReady);
        assert!(report.blockers.iter().any(|b| b.claim_id.is_some()));
        assert!(report.blockers.iter().any(|b| b.hypothesis_id.is_some()));
        // mission-scoped: the same derived gate over the mission's board
        let res = app(db.clone())
            .oneshot(
                Request::get(format!("/api/missions/{mission_id}/readiness"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let scoped: ReadinessReport = body_json(res.into_body()).await;
        assert_eq!(scoped.scope, Some(mission_id));
        assert_eq!(scoped.blockers.len(), report.blockers.len());
        // a malformed mission id is a 400, not a 500
        let res = app(db)
            .oneshot(
                Request::get("/api/missions/not-a-uuid/readiness")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}
