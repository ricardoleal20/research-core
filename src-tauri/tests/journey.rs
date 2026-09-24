// The core journey E2E (docs/e2e.md): the REAL core — a temp workspace
// (Db::open → migrate + seed + eventstore), the REAL in-process server
// shell (server::router / bridge::bridge_router over axum), and the REAL
// writer path the Tauri commands delegate to (the typed NewEvent
// constructors + the domain mutations — the same code every desktop
// command runs after unwrapping its State). No mock anywhere: every
// assertion is over the real projected state (folded from the event log)
// and the real HTTP wire.
//
// Coverage:
//   - the served UI (GET / → the app markers),
//   - the read-only API over the shared core (missions → hypotheses →
//     evidence → readiness → digest) after the writer lands the happy path,
//   - the closed remote vocabulary over the bridge (unpaired refusal →
//     paired capture → token-approved/rejected proposals, evented
//     actor=user / surface=mobile, basis-stale force rule),
//   - the preprint-ready end state with its evidence trail.
//
// The server boots on an OS-assigned ephemeral port (the fixed RC_PORT
// spawner is process-global and would collide across parallel tests; the
// router is the identical axum app the spawner serves).
//
// Run from src-tauri:  cargo test --test journey
use research_core_lib::bridge;
use research_core_lib::db::Db;
use research_core_lib::domain;
use research_core_lib::eventstore::{EventStore, NewEvent};
use research_core_lib::server;

use serde_json::{json, Value};
use std::path::PathBuf;
use uuid::Uuid;

const SCAN_STEP: &str = "literature-scan";

// ---------------------------------------------------------------------------
// workspace + server helpers
// ---------------------------------------------------------------------------

fn tmp_db_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("rc-journey-{tag}-{}.sqlite", Uuid::new_v4()))
}

fn clean_db_path(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
}

fn dist_dir() -> PathBuf {
    // the worktree's built UI — `npm run build` produces dist/index.html;
    // the server shell serves it as the same-origin app (AD-7/AD-14)
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../dist"))
}

/// Boot the router on an ephemeral port, returning its base URL.
async fn boot(router: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

/// The in-process read-only server shell (the app's `server::router`).
async fn boot_read_only_server(db: Db, data_dir: PathBuf) -> String {
    boot(server::router(db, dist_dir(), data_dir)).await
}

/// The bridge router (the same read-only slice + the closed remote
/// vocabulary under the pairing gate).
async fn boot_bridge(db: Db, data_dir: PathBuf) -> String {
    boot(bridge::bridge_router(db, dist_dir(), data_dir)).await
}

// ---------------------------------------------------------------------------
// the writer path (the domain seam the Tauri commands delegate to)
// ---------------------------------------------------------------------------

async fn writer_create_mission(db: &Db, question: &str) -> Uuid {
    let c = db.0.lock().await;
    let stored = EventStore::new(&c)
        .append(
            NewEvent::mission_created(domain::missions::MissionCreatedPayload {
                question: question.into(),
                stop_condition: "Stop after $5 or 20 sources reviewed.".into(),
                success_criterion: "A blind rater agrees with the surviving claims.".into(),
                autonomy: domain::missions::Autonomy::Suggest,
                spend_ceiling_cents: 500,
                roles: vec![],
                schedule: "daily-03:00".into(),
            })
            .unwrap(),
        )
        .unwrap();
    domain::missions::MissionsProjection::fold(&[stored.clone()])
        .unwrap()
        .pop()
        .map(|m| m.id)
        .unwrap()
}

async fn writer_add_hypothesis(db: &Db, mission_id: Uuid, statement: &str) -> Uuid {
    let c = db.0.lock().await;
    let stored = EventStore::new(&c)
        .append(NewEvent::hypothesis_created(statement, mission_id).unwrap())
        .unwrap();
    domain::hypotheses::HypothesesProjection::fold(&[stored])
        .unwrap()
        .pop()
        .map(|h| h.id)
        .unwrap()
}

async fn writer_add_claim(db: &Db, hyp_id: Uuid, text: &str) -> Uuid {
    let c = db.0.lock().await;
    let stored = EventStore::new(&c)
        .append(NewEvent::claim_registered(text, hyp_id, None).unwrap())
        .unwrap();
    domain::evidence::EvidenceProjection::fold(&[stored])
        .unwrap()
        .pop()
        .map(|cl| cl.id)
        .unwrap()
}

async fn writer_pin_citation(db: &Db, claim_id: Uuid, hyp_id: Uuid, ref_id: &str) {
    let c = db.0.lock().await;
    EventStore::new(&c)
        .append(
            NewEvent::evidence_pinned_citation(
                claim_id,
                hyp_id,
                ref_id,
                "Sparse attention runs within 0.3 BLEU of full attention.",
                0.82,
                "GLM-5.3",
            )
            .unwrap(),
        )
        .unwrap();
}

async fn writer_resolve_hypothesis(db: &Db, hyp_id: Uuid) {
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    store
        .append(
            NewEvent::hypothesis_status_changed(
                domain::hypotheses::HypothesisStatus::Proposed,
                domain::hypotheses::HypothesisStatus::Testing,
                "Trial 1 ran — the measures held.",
            )
            .unwrap()
            .with_causes(vec![hyp_id]),
        )
        .unwrap();
    store
        .append(
            NewEvent::hypothesis_status_changed(
                domain::hypotheses::HypothesisStatus::Testing,
                domain::hypotheses::HypothesisStatus::Supported,
                "The pinned evidence held across both trials.",
            )
            .unwrap()
            .with_causes(vec![hyp_id]),
        )
        .unwrap();
}

async fn writer_add_night_run(db: &Db, mission_id: Uuid) {
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    store
        .append(NewEvent::run_started("journey-ns-1", mission_id, "daily-03:00", SCAN_STEP).unwrap())
        .unwrap();
    store
        .append(NewEvent::run_finished("journey-ns-1", mission_id, "1 scan done; nothing pending", 0).unwrap())
        .unwrap();
}

/// The agent seam: a quarantined transition proposal (AD-3).
async fn writer_propose_transition(
    db: &Db,
    hyp_id: Uuid,
    to: domain::hypotheses::HypothesisStatus,
) -> Uuid {
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    domain::proposals::propose_transition(&store, "journey-run-9", hyp_id, to, "the run suggests advancing")
        .unwrap()
        .id
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

fn client() -> reqwest::Client {
    reqwest::Client::builder().build().unwrap()
}

async fn get(base: &str, path: &str) -> (u16, Value) {
    get_with_token(base, path, None).await
}

async fn get_with_token(base: &str, path: &str, token: Option<&str>) -> (u16, Value) {
    let mut req = client().get(format!("{base}{path}"));
    if let Some(t) = token {
        req = req.header("x-rc-pairing", t);
    }
    let res = req.send().await.unwrap();
    let status = res.status().as_u16();
    let body = res.json::<Value>().await.unwrap_or(Value::Null);
    (status, body)
}

async fn get_ui(base: &str, path: &str) -> (u16, String) {
    let res = client().get(format!("{base}{path}")).send().await.unwrap();
    let status = res.status().as_u16();
    let body = res.text().await.unwrap();
    (status, body)
}

async fn post_json(base: &str, path: &str, body: Value, token: Option<&str>) -> (u16, Value) {
    let mut req = client()
        .post(format!("{base}{path}"))
        .json(&body)
        .header("content-type", "application/json");
    if let Some(t) = token {
        req = req.header("x-rc-pairing", t);
    }
    let res = req.send().await.unwrap();
    let status = res.status().as_u16();
    let resp_body = res.json::<Value>().await.unwrap_or(Value::Null);
    (status, resp_body)
}

async fn pair(db: &Db, device: &str) -> String {
    let c = db.0.lock().await;
    domain::bridge::pair_device(&c, device).unwrap().token
}

// ---------------------------------------------------------------------------
// 1. the served UI + the read-only API
// ---------------------------------------------------------------------------

#[tokio::test]
async fn served_ui_and_read_only_api() {
    let db_path = tmp_db_path("ui");
    let db = Db::open(&db_path).unwrap();
    let base = boot_read_only_server(db.clone(), std::env::temp_dir()).await;

    // (a) the UI is served — the app markers are in the document
    let (status, body) = get_ui(&base, "/").await;
    assert_eq!(status, 200, "GET / must serve the app");
    assert!(body.contains("ResearchCore"), "the served HTML carries the app name");
    assert!(body.contains(r#"<div id="app">"#), "the served HTML mounts #app");

    // a fresh workspace is honestly empty
    let (status, missions) = get(&base, "/api/missions").await;
    assert_eq!(status, 200);
    assert_eq!(missions.as_array().unwrap().len(), 0);

    // (b) the localhost server is structurally read-only (AD-14): the
    // mutation verbs do not exist — 405, never a partial effect
    let (status, _body) = post_json(&base, "/api/missions", json!({ "question": "x" }), None).await;
    assert_eq!(status, 405, "POST /api/missions must be refused structurally");

    clean_db_path(&db_path);
}

// ---------------------------------------------------------------------------
// 2. the happy-path journey: writer → projected reads → preprint-ready
// ---------------------------------------------------------------------------

#[tokio::test]
async fn writer_journey_reaches_preprint_ready() {
    let db_path = tmp_db_path("journey");
    let db = Db::open(&db_path).unwrap();
    let base = boot_read_only_server(db.clone(), std::env::temp_dir()).await;

    // the writer path (the Tauri commands' seam): mission → hypothesis →
    // claim → citation pin → resolved hypothesis → one night run
    let mission_id = writer_create_mission(&db, "Does sparse attention hold at 32k context?").await;
    let hyp_id = writer_add_hypothesis(&db, mission_id, "Sparse attention matches full attention at 32k.").await;
    let claim_id = writer_add_claim(&db, hyp_id, "Sparse attention runs within 0.3 BLEU of full attention.").await;

    // the pin's ref comes from the real library (the seeded legacy refs)
    let (_status, refs) = get(&base, "/api/refs?project_id=proj-tesis-cap2").await;
    let refs = refs.as_array().unwrap();
    assert!(!refs.is_empty(), "the real library seed has refs to pin to");
    let ref_id = refs[0]["id"].as_str().unwrap();

    writer_pin_citation(&db, claim_id, hyp_id, ref_id).await;
    writer_resolve_hypothesis(&db, hyp_id).await;
    writer_add_night_run(&db, mission_id).await;

    // missions → hypotheses → evidence: the real projections
    let (status, missions) = get(&base, "/api/missions").await;
    assert_eq!(status, 200);
    let mission = &missions.as_array().unwrap()[0];
    assert_eq!(mission["question"], "Does sparse attention hold at 32k context?");
    assert_eq!(mission["status"], "active");
    assert_eq!(mission["autonomy"], "suggest");
    assert_eq!(mission["spendCeilingCents"], 500);

    let (status, hyps) = get(&base, &format!("/api/missions/{mission_id}/hypotheses")).await;
    assert_eq!(status, 200);
    let hyp = &hyps.as_array().unwrap()[0];
    assert_eq!(hyp["status"], "supported");
    assert_eq!(hyp["statement"], "Sparse attention matches full attention at 32k.");

    let (status, claims) = get(&base, &format!("/api/hypotheses/{hyp_id}/evidence")).await;
    assert_eq!(status, 200);
    let claim = &claims.as_array().unwrap()[0];
    assert_eq!(claim["pinned"], true);
    assert_eq!(claim["pin"]["kind"], "citation");
    assert_eq!(claim["pin"]["confidence"], 0.82);
    assert_eq!(claim["pin"]["assessingModel"], "GLM-5.3");
    assert_eq!(claim["pin"]["refId"], ref_id);

    // (d) the end state: preprint-ready with the evidence trail
    let (status, rd) = get(&base, &format!("/api/missions/{mission_id}/readiness")).await;
    assert_eq!(status, 200);
    assert_eq!(rd["verdict"], "ready", "a pinned + resolved board is preprint-ready");
    assert_eq!(rd["blockers"].as_array().unwrap().len(), 0);
    let trail = rd["trail"].as_array().unwrap();
    assert!(
        trail.iter().any(|row| row["kind"] == "claims_pinned" && row["clean"] == 1 && row["total"] == 1),
        "the evidence trail names 1/1 pinned claims"
    );
    assert!(
        trail.iter().any(|row| row["kind"] == "hypotheses_resolved" && row["clean"] == 1 && row["total"] == 1),
        "the evidence trail names 1/1 resolved hypotheses"
    );

    // the digest folds the real night run
    let (status, digest) = get(&base, "/api/digest").await;
    assert_eq!(status, 200);
    assert_eq!(digest["outcome"], "all_finished");
    let rows = digest["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0]["missionSeq"].as_i64().unwrap() >= 1);
    assert_eq!(rows[0]["question"], "Does sparse attention hold at 32k context?");
    assert_eq!(rows[0]["failed"], 0);

    clean_db_path(&db_path);
}

// ---------------------------------------------------------------------------
// 3. the closed remote vocabulary over the bridge
// ---------------------------------------------------------------------------

#[tokio::test]
async fn closed_remote_vocabulary_enforces_pairing() {
    let db_path = tmp_db_path("bridge");
    let db = Db::open(&db_path).unwrap();
    let base = boot_bridge(db.clone(), std::env::temp_dir()).await;

    // (c) unpaired: every remote verb is refused with the typed 401
    let (status, body) = get(&base, "/api/missions").await;
    assert_eq!(status, 401);
    assert!(
        body["error"].as_str().unwrap().starts_with("unpaired:"),
        "unpaired reads carry the typed `unpaired:` refusal"
    );

    // one mission + hypothesis so there is quarantine content to decide
    let mission_id = writer_create_mission(&db, "Does the gain hold across seeds?").await;
    let hyp_id = writer_add_hypothesis(&db, mission_id, "The gain holds across seeds and datasets.").await;

    let token = pair(&db, "E2E Test Phone").await;

    // unpaired mutation still refused even though a token exists in the DB
    let (status, body) = post_json(
        &base,
        "/api/capture",
        json!({ "question": "Fake question" }),
        None,
    )
    .await;
    assert_eq!(status, 401);
    assert!(body["error"].as_str().unwrap().starts_with("unpaired:"));

    // paired quick-capture lands a PENDING mission card (FR-21.3): a draft,
    // never a launch (FR-1.2)
    let (status, captured) = post_json(&base, "/api/capture", json!({ "question": "Who wins the trial?" }), Some(&token)).await;
    assert_eq!(status, 200, "capture (token first 12: {}): {:?}", token.chars().take(12).collect::<String>(), captured);
    assert_eq!(captured["status"], "draft");
    assert_eq!(captured["stopCondition"], "");
    let captured_seq = captured["seq"].as_i64().expect("the draft carries its mission seq");

    let (status, missions) = get_with_token(&base, "/api/missions", Some(&token)).await;
    assert_eq!(status, 200);
    let seqs: Vec<i64> = missions.as_array().unwrap().iter().map(|m| m["seq"].as_i64().unwrap()).collect();
    assert!(seqs.contains(&captured_seq), "the draft lands as a pending card beside the launched mission");

    // the agent's quarantined transition proposal is decided via the token,
    // evented actor=user / surface=mobile
    let proposal_id = writer_propose_transition(&db, hyp_id, domain::hypotheses::HypothesisStatus::Testing).await;

    let (status, outcome) = post_json(
        &base,
        &format!("/api/proposals/{proposal_id}/approve"),
        json!({ "force": false }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "approve: {}", outcome);
    assert_eq!(outcome["proposal"]["status"], "merged");
    assert_eq!(outcome["proposal"]["decided"]["actor"], "user");
    assert_eq!(outcome["proposal"]["decided"]["surface"], "mobile");

    // the merge is evented exactly once, with the mobile surface attributed
    {
        let c = db.0.lock().await;
        let events = EventStore::new(&c).events_all().unwrap();
        let merges: Vec<&research_core_lib::eventstore::StoredEvent> = events
            .iter()
            .filter(|e| e.kind == "merge.approved")
            .collect();
        assert_eq!(merges.len(), 1);
        assert_eq!(merges[0].payload["surface"], "mobile");
    }

    // the board now reflects the merged transition (proposed → testing)
    let (status, hyps) = get_with_token(&base, &format!("/api/missions/{mission_id}/hypotheses"), Some(&token)).await;
    assert_eq!(status, 200);
    assert_eq!(hyps.as_array().unwrap()[0]["status"], "testing");

    // rejection through the same token: a second proposal, refused cleanly
    let hyp_id_2 = writer_add_hypothesis(&db, mission_id, "A second, weaker hypothesis stays open.").await;
    let proposal_2 = writer_propose_transition(&db, hyp_id_2, domain::hypotheses::HypothesisStatus::Testing).await;
    let (status, rejected) = post_json(
        &base,
        &format!("/api/proposals/{proposal_2}/reject"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "reject: {}", rejected);
    assert_eq!(rejected["status"], "rejected");
    assert_eq!(rejected["decided"]["surface"], "mobile");

    let (status, hyps) = get_with_token(&base, &format!("/api/missions/{mission_id}/hypotheses"), Some(&token)).await;
    assert_eq!(status, 200);
    let second = hyps.as_array().unwrap().iter().find(|h| h["id"].as_str() == Some(hyp_id_2.to_string().as_str())).unwrap();
    assert_eq!(second["status"], "proposed", "a rejected proposal changes nothing");

    // the basis-stale rule: advancing the entity past the proposal's basis
    // refuses the blind merge (409) and records the marker on force
    // after the first approve the merged mission sits at testing; the next
    // proposal targets supported (testing → supported is legal)
    assert_eq!(hyps.as_array().unwrap()[0]["status"], "testing");
    let proposal_3 = writer_propose_transition(&db, hyp_id, domain::hypotheses::HypothesisStatus::Supported).await;
    // bump the entity past the proposal's basis (testing → supported,
    // a DIFFERENT occurrence than the proposal predates)
    {
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        store
            .append(
                NewEvent::hypothesis_status_changed(
                    domain::hypotheses::HypothesisStatus::Testing,
                    domain::hypotheses::HypothesisStatus::Supported,
                    "a fresh manual run resolves the hypothesis first",
                )
                .unwrap()
                .with_causes(vec![hyp_id]),
            )
            .unwrap();
    }
    let (status, body) = post_json(
        &base,
        &format!("/api/proposals/{proposal_3}/approve"),
        json!({ "force": false }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 409, "a stale basis refuses the blind merge");
    assert!(body["error"].as_str().unwrap().starts_with("basis_stale:"));

    let (status, outcome) = post_json(
        &base,
        &format!("/api/proposals/{proposal_3}/approve"),
        json!({ "force": true }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(outcome["proposal"]["basisStale"], true, "the force merge records the marker");
    assert_eq!(outcome["proposal"]["status"], "merged");

    clean_db_path(&db_path);
}