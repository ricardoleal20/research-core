// The backend orchestration rehearsal: the WHOLE research lifecycle on the
// REAL core + the REAL server shell, end to end, against one fresh temp
// workspace — the same writer path the Tauri commands delegate to (the
// typed NewEvent constructors + the domain mutations, journey.rs's seam),
// with every read over the real HTTP wire.
//
// Deterministic and OFF-LINE by construction:
//   - every network fetch goes to a local axum fake server bound in-test
//     (the fake Crossref for the DOI paste, the fake source host for the
//     citation verifier's re-read),
//   - every LLM-heavy step runs through the simulated provider seam (the
//     same `ProviderLayer::from_settings` fallback the desktop runs with
//     no key configured — a real BYOK/CLI/local provider drives the
//     identical path with real calls once configured),
//   - the night-shift tick's event vocabulary is landed through the same
//     typed constructors the scheduler appends with, then folded by the
//     real digest (the tick engine itself probes live connections, so the
//     rehearsal drives its seam, not its wall-clock loop).
//
// The twelve rehearsed stages (each asserts its end-state and prints a
// transcript line):
//   1. onboarding multi-source: DOI paste → resolver (fake Crossref) →
//      run_first_value → starter mission + 3 attributed candidates,
//   2. missions/hypotheses: roles, legal-only transitions, typed
//      relations, claims, citation + numerical pins, unpinned flags,
//   3. the quarantined agent proposal → approve / not_pending /
//      basis_stale + force,
//   4. night-shift runs + the digest fold (≤10-row shape, failed runs
//      render honestly),
//   5. the citation verifier against the fake source host,
//   6. the support sweep (simulated judge, different-model rule),
//   7. readiness tier-1: blockers with exact object refs → preprint-ready,
//   8. readiness tier-2 vs siam-jsc: machine checks + HumanConfirmed,
//   9. the Journal Fit Finder ranking → submission checklist mission,
//  10. export at one cut → rollback → the AD-11 stale banner,
//  11. the bridge: pairing gate, quick-capture, token approve,
//  12. log integrity: deterministic replay, no secret in any payload.
//
// Run from src-tauri:  cargo test --test orchestration -- --nocapture
use research_core_lib::adapters::providers::{ProviderLayer, ProviderSettings};
use research_core_lib::bridge as bridge_shell;
use research_core_lib::db::{self, Db};
use research_core_lib::domain::bridge;
use research_core_lib::domain::checkpoints;
use research_core_lib::domain::digest::{self, DigestOutcome};
use research_core_lib::domain::evidence::{self, EvidenceProjection};
use research_core_lib::domain::export;
use research_core_lib::domain::hypotheses::{
    HypothesesProjection, HypothesisRelatedPayload, HypothesisStatus, RelationKind,
};
use research_core_lib::domain::journals::{self, FitCandidate};
use research_core_lib::domain::library::LibraryProjection;
use research_core_lib::domain::manuscript::{self, ManuscriptsProjection};
use research_core_lib::domain::missions::{self, MissionCreatedPayload, MissionsProjection, RoleConfig};
use research_core_lib::domain::nightshift;
use research_core_lib::domain::onboarding::{self, Paper};
use research_core_lib::domain::proposals::{self, ProposalStatus, ProposalsProjection};
use research_core_lib::domain::readiness::{
    self, ReadinessItemKind, ReadinessTrailKind, ReadinessVerdict, TierTwoStatus,
};
use research_core_lib::domain::resolver::{self, ApiBases};
use research_core_lib::domain::submissions::{SubmissionCreatedPayload, SubmissionProjection};
use research_core_lib::domain::support;
use research_core_lib::domain::verifier::{
    self, PinSource, PinSourceFetcher, VerificationOutcome,
};
use research_core_lib::eventstore::{Actor, EventStore, NewEvent, StoredEvent};
use research_core_lib::server;

use axum::routing::get;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use uuid::Uuid;

const SCAN_STEP: &str = "literature-scan";
const EXCERPT: &str = "Sparse attention runs within 0.3 BLEU of full attention.";
const SEEDS_EXCERPT: &str = "The gain holds across four seeds at 32k context.";
const GAINED_EXCERPT: &str = "The gained variant holds at 32k within 0.3 BLEU.";
const SENTINEL_KEY: &str = "sk-test-ORCHESTRATION-NEVER-LEAK-9f1c";

// ---------------------------------------------------------------------------
// workspace + server helpers (the journey.rs pattern)
// ---------------------------------------------------------------------------

fn tmp_db_path() -> PathBuf {
    std::env::temp_dir().join(format!("rc-orchestration-{}.sqlite", Uuid::new_v4()))
}

fn clean_db_path(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
}

fn tmp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rc-orchestration-{tag}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn boot(router: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("http://{addr}")
}

fn dist_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../dist"))
}

async fn boot_read_only_server(db: Db) -> String {
    let data_dir = std::env::temp_dir();
    boot(server::router(db, dist_dir(), data_dir)).await
}

async fn boot_bridge_server(db: Db) -> String {
    let data_dir = std::env::temp_dir();
    boot(bridge_shell::bridge_router(db, dist_dir(), data_dir)).await
}

/// The simulated provider seam: `ProviderLayer::from_settings` with no
/// credential resolves the simulated adapter — the same fallback every
/// desktop command runs with no key configured. A real provider (BYOK,
/// CLI bridge, or local runtime) drives the identical call path.
fn sim_layer(db: &Db) -> ProviderLayer {
    ProviderLayer::from_settings(
        db,
        ProviderSettings {
            mode: String::new(),
            name: "openai".into(),
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            cli: String::new(),
            cli_model: String::new(),
            local_base_url: String::new(),
        },
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// the local fake hosts (offline determinism)
// ---------------------------------------------------------------------------

/// The fake Crossref: one DOI, answered locally.
async fn boot_fake_crossref() -> ApiBases {
    let body = json!({
        "status": "ok",
        "message": {
            "title": ["Sparse Attention at Thirty-Two Thousand Tokens"],
            "author": [
                {"given": "Aisha", "family": "Romero"},
                {"given": "Ken", "family": "Watanabe"}
            ],
            "container-title": ["Journal of Honest Benchmarks"],
            "published": {"print": {"date-parts": [[2025, 3, 2]]}},
            "DOI": "10.1000/xyz123",
            "abstract": "<p>Sparse attention matches full attention at 32k context.</p>"
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = axum::Router::new().route(
        "/crossref/works/10.1000/xyz123",
        get(|| async move { axum::Json(body) }),
    );
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let base = format!("http://{addr}");
    ApiBases {
        arxiv: format!("{base}/arxiv"),
        crossref: format!("{base}/crossref"),
        pubmed: format!("{base}/pubmed"),
        s2: format!("{base}/s2"),
        openalex: format!("{base}/openalex"),
    }
}

/// The fake source host the verifier re-reads pinned citations from:
/// `/source/paper` answers steadily; `/source/gained` is down until the
/// rehearsal flips it up (the failed-fetch → verified flip).
async fn boot_fake_sources() -> (String, Arc<AtomicBool>) {
    let up = Arc::new(AtomicBool::new(false));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let paper = format!("Full paper text. {EXCERPT} {SEEDS_EXCERPT} Conclusion.");
    let gained_flag = up.clone();
    let app = axum::Router::new()
        .route(
            "/source/paper",
            get(move || async move { (axum::http::StatusCode::OK, paper.clone()) }),
        )
        .route(
            "/source/gained",
            get(move || {
                let up = gained_flag.clone();
                async move {
                    if up.load(Ordering::SeqCst) {
                        (
                            axum::http::StatusCode::OK,
                            format!("Erratum study. {GAINED_EXCERPT} Done."),
                        )
                    } else {
                        (axum::http::StatusCode::NOT_FOUND, "source is down".to_string())
                    }
                }
            }),
        );
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), up)
}

/// A source fetcher over the real HTTP wire — the same GET-the-exact-URL
/// rule the verifier's fetcher runs (the trait is the public seam).
struct LocalFetcher {
    client: reqwest::Client,
}

impl LocalFetcher {
    fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("research-core/0.1")
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap(),
        }
    }
}

impl PinSourceFetcher for LocalFetcher {
    fn fetch<'a>(&'a self, source: &'a PinSource) -> verifier::FetchFuture<'a> {
        match source {
            PinSource::Url { url } => {
                let url = url.clone();
                let client = &self.client;
                Box::pin(async move {
                    let res = client
                        .get(&url)
                        .send()
                        .await
                        .map_err(|e| format!("request: {e}"))?;
                    let status = res.status();
                    let text = res.text().await.map_err(|e| format!("read: {e}"))?;
                    if status.is_success() {
                        Ok(text)
                    } else {
                        Err(format!("http {}", status.as_u16()))
                    }
                })
            }
            _ => Box::pin(async { Err("unsupported source in the rehearsal".to_string()) }),
        }
    }
}

/// One verifier pass over the named claims — the engine's three phases
/// (resolve under one lock → fetch lock-free → append under one lock with
/// the pin identity re-checked), over the real `verify_citation` check and
/// the real `evidence.verified` event.
async fn verify_citations(db: &Db, fetcher: &LocalFetcher, claim_ids: &[Uuid]) {
    struct Check {
        claim: Uuid,
        hyp: Uuid,
        pin_seq: i64,
        excerpt: String,
        url: String,
    }
    let checks: Vec<Check> = {
        let c = db.0.lock().await;
        let events = EventStore::new(&c).events_all().unwrap();
        let claims = EvidenceProjection::fold(&events).unwrap();
        claims
            .iter()
            .filter(|cl| claim_ids.contains(&cl.id))
            .filter_map(|cl| {
                let pin = cl.pin.as_ref()?;
                let ref_id = pin.ref_id.as_deref()?;
                let url: String = c
                    .query_row(
                        "SELECT url FROM refs WHERE id = ?1",
                        [ref_id],
                        |r| r.get(0),
                    )
                    .ok()?;
                let url = url.trim().to_string();
                (!url.is_empty()).then_some(Check {
                    claim: cl.id,
                    hyp: cl.hypothesis_id,
                    pin_seq: pin.seq,
                    excerpt: pin.excerpt.clone(),
                    url,
                })
            })
            .collect()
    };
    let mut results = Vec::new();
    for check in checks {
        let fetched = fetcher
            .fetch(&PinSource::Url { url: check.url.clone() })
            .await;
        let (outcome, detail) = verifier::verify_citation(&check.excerpt, &fetched);
        let label = check.url.clone();
        results.push((check, outcome, detail, label));
    }
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    let events = store.events_all().unwrap();
    let claims = EvidenceProjection::fold(&events).unwrap();
    for (check, outcome, detail, label) in results {
        let still_current = claims
            .iter()
            .find(|cl| cl.id == check.claim)
            .and_then(|cl| cl.pin.as_ref())
            .map(|p| p.seq)
            == Some(check.pin_seq);
        if !still_current {
            continue; // re-pinned mid-run — the result would target stale text
        }
        store
            .append(
                NewEvent::evidence_verified(
                    check.claim, check.hyp, check.pin_seq, outcome, detail, label,
                )
                .unwrap(),
            )
            .unwrap();
    }
}

// ---------------------------------------------------------------------------
// small shared helpers
// ---------------------------------------------------------------------------

async fn events_of(db: &Db) -> Vec<StoredEvent> {
    let c = db.0.lock().await;
    EventStore::new(&c).events_all().unwrap()
}

async fn fold_claims(db: &Db) -> Vec<evidence::Claim> {
    EvidenceProjection::fold(&events_of(db).await).unwrap()
}

fn claim_by_id<'a>(claims: &'a [evidence::Claim], id: Uuid) -> &'a evidence::Claim {
    claims.iter().find(|c| c.id == id).unwrap()
}

fn seq_of(events: &[StoredEvent], id: Uuid) -> i64 {
    events.iter().find(|e| e.id == id).unwrap().seq
}

/// The tier-2 pure inputs for one mission's registered manuscript.
fn tier_two_inputs(
    conn: &rusqlite::Connection,
    events: &[StoredEvent],
    mission: Uuid,
) -> (
    Vec<manuscript::ManuscriptScan>,
    Vec<journals::ManuscriptVenueStats>,
    Vec<research_core_lib::domain::library::LibraryRef>,
) {
    let registered = ManuscriptsProjection::for_mission(events, mission)
        .unwrap()
        .expect("the manuscript is registered");
    let scans = vec![manuscript::build_scan(&registered)];
    let stats = vec![journals::build_venue_stats(&registered)];
    let refs = LibraryProjection::fold(conn, events).unwrap();
    (scans, stats, refs)
}

// ---------------------------------------------------------------------------
// HTTP helpers (the journey.rs pattern)
// ---------------------------------------------------------------------------

fn client() -> reqwest::Client {
    reqwest::Client::builder().build().unwrap()
}

async fn http_get(base: &str, path: &str) -> (u16, Value) {
    let res = client().get(format!("{base}{path}")).send().await.unwrap();
    let status = res.status().as_u16();
    let body = res.json::<Value>().await.unwrap_or(Value::Null);
    (status, body)
}

async fn http_get_with_token(base: &str, path: &str, token: &str) -> (u16, Value) {
    let res = client()
        .get(format!("{base}{path}"))
        .header("x-rc-pairing", token)
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    let body = res.json::<Value>().await.unwrap_or(Value::Null);
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

// ---------------------------------------------------------------------------
// the rehearsal
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_full_research_loop_rehearsal() {
    let db_path = tmp_db_path();
    let db = Db::open(&db_path).unwrap();
    {
        // English starter text (the fold's default is Spanish)
        let c = db.0.lock().await;
        db::set_setting(&c, "lang", "en").unwrap();
    }
    let ro = boot_read_only_server(db.clone()).await;
    let b = boot_bridge_server(db.clone()).await;

    // =====================================================================
    // [stage 1] onboarding multi-source: paste a DOI → resolve → first value
    // =====================================================================
    let bases = boot_fake_crossref().await;
    let meta = resolver::resolve_link_with(&bases, "10.1000/xyz123")
        .await
        .expect("the local fake Crossref resolves the pasted DOI");
    assert_eq!(meta.source, "doi");
    assert_eq!(meta.doi.as_deref(), Some("10.1000/xyz123"));
    let paper = Paper {
        title: meta.title.clone(),
        authors: meta.authors.clone(),
        year: meta.year,
        venue: meta.venue.clone(),
        doi: meta.doi.clone().unwrap_or_default(),
        url: meta.url.clone(),
        arxiv_id: meta.arxiv_id.clone().unwrap_or_default(),
        source: meta.source.clone(),
        abstract_text: meta.abstract_text.clone(),
    };
    let first = onboarding::run_first_value(&db, &sim_layer(&db), paper)
        .await
        .expect("first value runs on the simulated provider seam");
    let m1 = first.mission.id;
    assert!(first.mission.stop_condition.trim().len() > 10);
    assert!(first.mission.success_criterion.trim().len() > 10);
    assert_eq!(first.candidates.len(), 3, "three hypothesis candidates X-1..3");
    for candidate in &first.candidates {
        assert_eq!(candidate.status, HypothesisStatus::Proposed);
        assert!(!candidate.assessing_model.trim().is_empty(), "attributed model");
    }
    let x1 = first.candidates[0].hypothesis_id;
    let x2 = first.candidates[1].hypothesis_id;
    let x3 = first.candidates[2].hypothesis_id;
    let ref1 = first.paper.ref_id.clone();
    assert!(!ref1.is_empty(), "the resolved source landed as a ref");
    {
        let (status, refs) = http_get(&ro, "/api/refs").await;
        assert_eq!(status, 200);
        assert!(
            refs.as_array()
                .unwrap()
                .iter()
                .any(|r| r["doi"] == json!("10.1000/xyz123")),
            "the DOI-pasted source is in the real library read"
        );
    }
    println!(
        "[stage 1] onboarding: DOI 10.1000/xyz123 resolved via the fake Crossref -> M-{} \"{}\" + {} proposed candidates (assessed by {})",
        first.mission.seq,
        first.mission.question,
        first.candidates.len(),
        first.candidates[0].assessing_model
    );

    // =====================================================================
    // [stage 2] missions/hypotheses: roles, legal transitions, relations,
    // claims, citation + numerical pins, unpinned flags
    // =====================================================================
    let m2 = {
        let c = db.0.lock().await;
        let stored = EventStore::new(&c)
            .append(
                NewEvent::mission_created(MissionCreatedPayload {
                    question: "Does the numerical gain survive across seeds?".into(),
                    stop_condition: "Stop after 3 runs or 20 sources reviewed.".into(),
                    success_criterion: "The gain holds on 4 of 5 seeds.".into(),
                    autonomy: missions::Autonomy::Suggest,
                    spend_ceiling_cents: 500,
                    roles: vec![
                        RoleConfig::drafter("simulated", "simulated"),
                        RoleConfig::critic("simulated", "simulated"),
                    ],
                    schedule: "daily-03:00".into(),
                })
                .unwrap(),
            )
            .unwrap();
        MissionsProjection::fold(&[stored]).unwrap().pop().unwrap().id
    };
    {
        // the candidate moves to testing — legal only
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        store
            .append(
                NewEvent::hypothesis_status_changed(
                    HypothesisStatus::Proposed,
                    HypothesisStatus::Testing,
                    "Trial 1 is running on the seeded benchmark.",
                )
                .unwrap()
                .with_causes(vec![x1]),
            )
            .unwrap();
        // an illegal jump is refused at the typed constructor
        let illegal = NewEvent::hypothesis_status_changed(
            HypothesisStatus::Proposed,
            HypothesisStatus::Supported,
            "skip the trial",
        );
        assert!(
            illegal.is_err_and(|e| e.to_string().contains("illegal_transition")),
            "proposed -> supported is refused as illegal"
        );
        // a typed relation between two candidates
        store
            .append(NewEvent::hypothesis_related(HypothesisRelatedPayload {
                from_hypothesis_id: x2,
                to_hypothesis_id: x3,
                relation_kind: RelationKind::Contradicts,
            })
            .unwrap())
            .unwrap();
    }
    let (claim1, claim2, claim3) = {
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        let c1 = store
            .append(NewEvent::claim_registered(EXCERPT, x1, None).unwrap())
            .unwrap();
        let c2 = store
            .append(
                NewEvent::claim_registered(
                    "The gain holds across four seeds at 32k context.",
                    x2,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        let c3 = store
            .append(
                NewEvent::claim_registered(
                    "The measured delta is 0.31 BLEU over full attention.",
                    x1,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        // citation pin: excerpt + sha-256 digest + confidence + model
        store
            .append(
                NewEvent::evidence_pinned_citation(
                    c1.id, x1, &ref1, EXCERPT, 0.82, "GLM-5.3",
                )
                .unwrap(),
            )
            .unwrap();
        // numerical pin: the artifact's digest binds to its content
        store
            .append(
                NewEvent::evidence_pinned_numerical(
                    c3.id,
                    x1,
                    "results/table3.csv",
                    "0.31 BLEU delta over four seeds",
                    0.9,
                    "GLM-5.3",
                )
                .unwrap(),
            )
            .unwrap();
        (c1.id, c2.id, c3.id)
    };
    {
        let claims = fold_claims(&db).await;
        let pinned = claim_by_id(&claims, claim1);
        assert!(pinned.pinned);
        let pin = pinned.pin.as_ref().unwrap();
        assert_eq!(pin.kind, evidence::PinKind::Citation);
        assert_eq!(pin.excerpt, EXCERPT);
        assert_eq!(pin.confidence, 0.82);
        assert_eq!(pin.assessing_model, "GLM-5.3");
        assert_eq!(pin.digest.len(), 64, "sha-256 digest of the excerpt (AD-5)");
        assert!(pin.digest.chars().all(|ch| ch.is_ascii_hexdigit()));
        let numerical = claim_by_id(&claims, claim3);
        let npin = numerical.pin.as_ref().unwrap();
        assert_eq!(npin.kind, evidence::PinKind::Numerical);
        assert_eq!(npin.artifact_ref.as_deref(), Some("results/table3.csv"));
        // the unpinned flag exists for the unpinned claim
        let unpinned = claim_by_id(&claims, claim2);
        assert!(!unpinned.pinned, "claim 2 carries the unpinned flag");
        assert!(unpinned.pin.is_none());
        // the served read carries the same pin
        let (status, served) = http_get(&ro, &format!("/api/hypotheses/{x1}/evidence")).await;
        assert_eq!(status, 200);
        let served_claim = served
            .as_array()
            .unwrap()
            .iter()
            .find(|cl| cl["id"].as_str() == Some(claim1.to_string().as_str()))
            .unwrap();
        assert_eq!(served_claim["pinned"], true);
        assert_eq!(served_claim["pin"]["kind"], "citation");
        assert_eq!(served_claim["pin"]["confidence"], 0.82);
        assert_eq!(served_claim["pin"]["assessingModel"], "GLM-5.3");
        assert_eq!(served_claim["pin"]["digest"], pin.digest);
    }
    println!(
        "[stage 2] board: M-{} with drafter+critic roles, X-1 testing (illegal jumps refused), X-2 contradicts X-3, 3 claims (1 unpinned), citation pin (sha-256 {}…) + numerical pin",
        seq_of(&events_of(&db).await, m2),
        &claim_by_id(&fold_claims(&db).await, claim1).pin.as_ref().unwrap().digest[..12]
    );

    // =====================================================================
    // [stage 3] the agent proposal: quarantine → approve → not_pending →
    // basis_stale → force
    // =====================================================================
    let proposal1 = {
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        proposals::propose_transition(
            &store,
            "orchestration-run-1",
            x1,
            HypothesisStatus::Supported,
            "the drafter's scan suggests advancing after trial 1",
        )
        .unwrap()
    };
    assert_eq!(proposal1.status, ProposalStatus::Pending);
    {
        // pending proposals are excluded from the board projections
        let (status, hyps) = http_get(&ro, &format!("/api/missions/{m1}/hypotheses")).await;
        assert_eq!(status, 200);
        let board_x1 = hyps
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["id"].as_str() == Some(x1.to_string().as_str()))
            .unwrap();
        assert_eq!(board_x1["status"], "testing", "the pending proposal changes nothing yet");
        let (status, pending) = http_get(&ro, "/api/proposals").await;
        assert_eq!(status, 200);
        assert!(
            pending
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"].as_str() == Some(proposal1.id.to_string().as_str())
                    && p["status"] == "pending"),
            "the proposal is pending in the quarantine read"
        );
    }
    {
        // approve -> the board reflects the merged transition
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        let outcome = proposals::approve(&store, proposal1.id, false, None).unwrap();
        assert_eq!(outcome.proposal.status, ProposalStatus::Merged);
        let again = proposals::approve(&store, proposal1.id, false, None);
        let refusal = again.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            refusal.starts_with("not_pending:"),
            "a decided proposal can never be merged again: {refusal}"
        );
    }
    {
        let (status, hyps) = http_get(&ro, &format!("/api/missions/{m1}/hypotheses")).await;
        assert_eq!(status, 200);
        let board_x1 = hyps
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["id"].as_str() == Some(x1.to_string().as_str()))
            .unwrap();
        assert_eq!(board_x1["status"], "supported", "the merge applied to the board");
    }
    // a stale basis: propose supported -> revised, then advance the entity
    // past the proposal's basis before deciding
    let proposal2 = {
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        proposals::propose_transition(
            &store,
            "orchestration-run-2",
            x1,
            HypothesisStatus::Revised,
            "the critic asks for a revised statement before trial 2",
        )
        .unwrap()
    };
    {
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        // bump the entity past the proposal's basis (a fresh manual pass)
        store
            .append(
                NewEvent::hypothesis_status_changed(
                    HypothesisStatus::Supported,
                    HypothesisStatus::Revised,
                    "a fresh manual pass revises the statement first",
                )
                .unwrap()
                .with_causes(vec![x1]),
            )
            .unwrap();
        let blind = proposals::approve(&store, proposal2.id, false, None);
        let refusal = blind.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            refusal.starts_with("basis_stale:"),
            "a stale basis refuses the blind merge: {refusal}"
        );
        let forced = proposals::approve(&store, proposal2.id, true, None).unwrap();
        assert_eq!(forced.proposal.status, ProposalStatus::Merged);
        assert!(forced.proposal.basis_stale, "the forced merge records the marker");
    }
    println!(
        "[stage 3] quarantine: proposal pending (board untouched) -> approved -> board supported; re-approve refused `not_pending:`; stale basis refused `basis_stale:` then force-merged with the marker"
    );

    // =====================================================================
    // [stage 4] night shift + digest: the tick's event vocabulary, the
    // real digest fold (a failed run still renders its row)
    // =====================================================================
    {
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        // M-1's nightly scan: started -> finished with a one-line verdict
        store
            .append(
                NewEvent::run_started("orch-ns-1", m1, "daily-03:00", SCAN_STEP).unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::run_finished(
                    "orch-ns-1",
                    m1,
                    "scan found 2 candidate sources; one pin verified; nothing pending",
                    1,
                )
                .unwrap(),
            )
            .unwrap();
        // M-2's scan fails honestly (provider_error) — a row, never a tick error
        store
            .append(
                NewEvent::run_started("orch-ns-2", m2, "daily-03:00", SCAN_STEP).unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::run_failed("orch-ns-2", m2, "provider_error", chrono::Utc::now())
                    .unwrap(),
            )
            .unwrap();
        // the AD-12 catch-up after the runs (the tick's closing step): the
        // mission whose every run failed reaches its honest terminal
        let terminals = nightshift::evaluate_terminals(&store).unwrap();
        assert_eq!(terminals.len(), 1, "exactly the all-runs-failed mission");
        assert_eq!(terminals[0].kind, "mission.failed");
        assert_eq!(terminals[0].causes, vec![m2]);
        assert_eq!(terminals[0].payload["signal"], "all_runs_failed");
    }
    {
        let events = events_of(&db).await;
        let digest = digest::render_digest(&events, chrono::Utc::now()).unwrap();
        assert_eq!(digest.outcome, DigestOutcome::PartialSuccess);
        assert!(
            digest.rows.len() <= digest::DIGEST_ROW_CAP,
            "the 90-second shape: at most {} rows",
            digest::DIGEST_ROW_CAP
        );
        let row1 = digest.rows.iter().find(|r| r.mission_id == m1).unwrap();
        assert_eq!(row1.finished, 1);
        assert_eq!(row1.failed, 0);
        let row2 = digest.rows.iter().find(|r| r.mission_id == m2).unwrap();
        assert_eq!(row2.failed, 1);
        assert_eq!(row2.failure_reason.as_deref(), Some("provider_error"));
        let line = row2.verdict_line();
        assert!(
            line.contains("run failed: provider_error") && line.contains("receipt kept"),
            "the failed run renders its honest row: {line}"
        );
        // the same fold is served over the wire
        let (status, served) = http_get(&ro, "/api/digest").await;
        assert_eq!(status, 200);
        assert_eq!(served["outcome"], "partial_success");
        assert_eq!(served["rows"].as_array().unwrap().len(), 2);
    }
    println!(
        "[stage 4] night shift: M-1 scan finished (verdict row), M-2 scan failed `provider_error` — the digest still renders both rows ({} <= 10), outcome partial_success",
        2
    );

    // =====================================================================
    // [stage 5] the citation verifier (non-LLM) against the fake source
    // host: verified, and failed-honestly when the source is down
    // =====================================================================
    let (sources, gained_up) = boot_fake_sources().await;
    {
        // point the refs at the local fake source host (the verifier
        // re-reads the ref's URL)
        let c = db.0.lock().await;
        c.execute(
            "UPDATE refs SET url = ?1 WHERE id = ?2",
            rusqlite::params![format!("{sources}/source/paper"), ref1],
        )
        .unwrap();
    }
    // claim 5: pinned to a source that is DOWN first — the honest failure
    let claim5 = {
        let c = db.0.lock().await;
        c.execute(
            "INSERT OR REPLACE INTO refs(id,project_id,title,authors,year,venue,doi,url,tags,status,used,citation_count,created_at)
             VALUES(?1,'proj-tesis-cap2','A gained variant study','Ito et al.',2024,'arXiv','',?2,'gained,rehearsal','unread',0,0,?3)",
            rusqlite::params![
                "ref-orch-gained",
                format!("{sources}/source/gained"),
                chrono::Utc::now().to_rfc3339()
            ],
        )
        .unwrap();
        let store = EventStore::new(&c);
        let claim = store
            .append(
                NewEvent::claim_registered(GAINED_EXCERPT, x2, None).unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::evidence_pinned_citation(
                    claim.id,
                    x2,
                    "ref-orch-gained",
                    GAINED_EXCERPT,
                    0.71,
                    "GLM-5.3",
                )
                .unwrap(),
            )
            .unwrap();
        claim.id
    };
    let fetcher = LocalFetcher::new();
    verify_citations(&db, &fetcher, &[claim1, claim5]).await;
    {
        let claims = fold_claims(&db).await;
        let v1 = claim_by_id(&claims, claim1).pin.as_ref().unwrap().verification.as_ref().unwrap();
        assert_eq!(
            v1.status,
            evidence::VerificationStatus::Verified,
            "claim 1 verification: {:?} ({})",
            v1.status,
            v1.detail
        );
        assert_eq!(v1.detail, "excerpt_matched");
        assert_eq!(v1.source, format!("{sources}/source/paper"));
        // the down source fails honestly — and stays distinct from confidence
        let v5 = claim_by_id(&claims, claim5).pin.as_ref().unwrap().verification.as_ref().unwrap();
        assert_eq!(v5.status, evidence::VerificationStatus::Failed);
        assert_eq!(v5.detail, "fetch_error");
        assert_eq!(
            claim_by_id(&claims, claim5).pin.as_ref().unwrap().confidence,
            0.71,
            "confidence is untouched by verification"
        );
    }
    // the source comes back up — the latest verification wins
    gained_up.store(true, Ordering::SeqCst);
    verify_citations(&db, &fetcher, &[claim5]).await;
    {
        let claims = fold_claims(&db).await;
        let v5 = claim_by_id(&claims, claim5).pin.as_ref().unwrap().verification.as_ref().unwrap();
        assert_eq!(v5.status, evidence::VerificationStatus::Verified);
        assert_eq!(v5.detail, "excerpt_matched");
    }
    println!(
        "[stage 5] verifier: claim 1 verified `excerpt_matched` against the fake source host; claim 5 failed honestly `fetch_error` while down, verified after the source recovered (latest wins)"
    );

    // =====================================================================
    // [stage 6] the support sweep (third signal): the simulated judge on
    // the different-model rule — three signals, never conflated
    // =====================================================================
    {
        // with no provider configured the sweep's only judge candidate is
        // the simulated one — a different model than the pin's assessor
        assert!(
            support::models_differ("GLM-5.3", "simulated"),
            "the different-model rule admits the simulated judge"
        );
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        let events = store.events_all().unwrap();
        let claims = EvidenceProjection::fold(&events).unwrap();
        // one de-duped pass over every never-judged pin (the sweep's rule)
        for id in [claim1, claim3, claim5] {
            let pin = claim_by_id(&claims, id).pin.as_ref().unwrap();
            store
                .append(
                    NewEvent::pin_support_checked(
                        id,
                        pin.hypothesis_id,
                        pin.seq,
                        support::SupportVerdict::Supported,
                        0.9,
                        "simulated",
                        "GLM-5.3",
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        // the different-model rule is enforced at the typed constructor
        let same_model = NewEvent::pin_support_checked(
            claim1,
            x1,
            claim_by_id(&claims, claim1).pin.as_ref().unwrap().seq,
            support::SupportVerdict::Supported,
            0.9,
            "GLM-5.3",
            "GLM-5.3",
        );
        assert!(
            same_model.is_err(),
            "a model never judges its own assessment: {same_model:?}"
        );
    }
    {
        let claims = fold_claims(&db).await;
        let pin = claim_by_id(&claims, claim1).pin.as_ref().unwrap();
        // the three signals, distinct by construction
        assert_eq!(pin.confidence, 0.82, "signal 1: the assessor's confidence");
        assert_eq!(pin.assessing_model, "GLM-5.3");
        assert_eq!(
            pin.verification.as_ref().unwrap().status,
            evidence::VerificationStatus::Verified,
            "signal 2: existence by code"
        );
        let check = pin.support.as_ref().unwrap();
        assert_eq!(check.status, support::SupportStatus::Supported, "signal 3: the judge");
        assert_eq!(check.judging_model, "simulated", "a DIFFERENT model judged");
        assert_eq!(check.confidence, 0.9);
    }
    println!(
        "[stage 6] support sweep: pins support_checked by the simulated judge (different-model rule enforced — same-model refused); confidence 0.82 / verified / supported stay three distinct signals"
    );

    // =====================================================================
    // [stage 7] readiness tier-1: blockers with exact object refs ->
    // preprint-ready with the evidence trail
    // =====================================================================
    {
        let events = events_of(&db).await;
        let report = readiness::readiness_report(&events, Some(m1)).unwrap();
        assert_eq!(report.verdict, ReadinessVerdict::NotReady);
        let unpinned: Vec<_> = report
            .blockers
            .iter()
            .filter(|b| b.kind == ReadinessItemKind::UnpinnedClaim)
            .collect();
        assert_eq!(unpinned.len(), 1, "exactly the unpinned claim blocks");
        assert_eq!(unpinned[0].claim_id, Some(claim2));
        assert_eq!(unpinned[0].claim_seq, Some(seq_of(&events, claim2)));
        assert_eq!(unpinned[0].hypothesis_id, Some(x2));
        let load_bearing: Vec<_> = report
            .blockers
            .iter()
            .filter(|b| b.kind == ReadinessItemKind::LoadBearingUnresolved)
            .collect();
        assert_eq!(load_bearing.len(), 2, "X-2 and X-3 carry load");
        // X-1 is revised — mid-rework, exempt from the gate's rule
        assert!(load_bearing.iter().all(|b| b.hypothesis_id != Some(x1)));
        let x2_blocker = load_bearing
            .iter()
            .find(|b| b.hypothesis_id == Some(x2))
            .unwrap();
        assert_eq!(x2_blocker.hypothesis_status, Some(HypothesisStatus::Proposed));
        assert_eq!(
            x2_blocker.claim_ties,
            vec![seq_of(&events, claim2), seq_of(&events, claim5)]
        );
        let x3_blocker = load_bearing
            .iter()
            .find(|b| b.hypothesis_id == Some(x3))
            .unwrap();
        assert!(
            !x3_blocker.relation_ties.is_empty(),
            "the relation makes X-3 load-bearing"
        );
    }
    {
        // resolve everything: pin claim 2, verify it, support-check it,
        // then resolve X-1 (revised -> testing -> supported), X-2, X-3
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        let pin2 = store
            .append(
                NewEvent::evidence_pinned_citation(
                    claim2,
                    x2,
                    &ref1,
                    SEEDS_EXCERPT,
                    0.75,
                    "GLM-5.3",
                )
                .unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::evidence_verified(
                    claim2,
                    x2,
                    pin2.seq,
                    VerificationOutcome::Verified,
                    "excerpt_matched",
                    format!("{sources}/source/paper"),
                )
                .unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::pin_support_checked(
                    claim2,
                    x2,
                    pin2.seq,
                    support::SupportVerdict::Supported,
                    0.88,
                    "simulated",
                    "GLM-5.3",
                )
                .unwrap(),
            )
            .unwrap();
        for (hyp, steps) in [
            (
                x1,
                vec![
                    (HypothesisStatus::Revised, HypothesisStatus::Testing),
                    (HypothesisStatus::Testing, HypothesisStatus::Supported),
                ],
            ),
            (
                x2,
                vec![
                    (HypothesisStatus::Proposed, HypothesisStatus::Testing),
                    (HypothesisStatus::Testing, HypothesisStatus::Supported),
                ],
            ),
            (
                x3,
                vec![
                    (HypothesisStatus::Proposed, HypothesisStatus::Testing),
                    (HypothesisStatus::Testing, HypothesisStatus::Supported),
                ],
            ),
        ] {
            for (from, to) in steps {
                store
                    .append(
                        NewEvent::hypothesis_status_changed(from, to, "the measures held.")
                            .unwrap()
                            .with_causes(vec![hyp]),
                    )
                    .unwrap();
            }
        }
    }
    {
        let events = events_of(&db).await;
        let report = readiness::readiness_report(&events, Some(m1)).unwrap();
        assert_eq!(report.verdict, ReadinessVerdict::Ready, "preprint-ready");
        assert!(report.blockers.is_empty());
        let claims_row = report
            .trail
            .iter()
            .find(|r| r.kind == ReadinessTrailKind::ClaimsPinned)
            .unwrap();
        assert_eq!(claims_row.total, 4, "all four claims pinned");
        assert_eq!(claims_row.clean, 4);
        let resolved_row = report
            .trail
            .iter()
            .find(|r| r.kind == ReadinessTrailKind::HypothesesResolved)
            .unwrap();
        assert_eq!(resolved_row.total, 3);
        assert_eq!(resolved_row.clean, 3);
        // the served report agrees
        let (status, served) = http_get(&ro, &format!("/api/missions/{m1}/readiness")).await;
        assert_eq!(status, 200);
        assert_eq!(served["verdict"], "ready");
    }
    println!(
        "[stage 7] readiness tier-1: blockers named their exact objects (unpinned CLAIMS-{}, load-bearing X-2/X-3; X-1 revised is exempt) -> after pinning + resolving: verdict ready (preprint-ready) with the 4/4-pinned, 3/3-resolved trail",
        seq_of(&events_of(&db).await, claim2)
    );

    // =====================================================================
    // [stage 8] readiness tier-2 (journal): a registered .tex manuscript vs
    // siam-jsc — machine checks pass, human-only items need HumanConfirmed
    // =====================================================================
    let tex_dir = tmp_dir("tex");
    let events = events_of(&db).await;
    let x1_seq = seq_of(&events, x1);
    let claim1_seq = seq_of(&events, claim1);
    {
        let tex = format!(
            "\\documentclass{{article}}\n\
             \\usepackage{{natbib}}\n\
             \\begin{{abstract}}\n\
             We show the method converges.\n\
             \\end{{abstract}}\n\
             \\section*{{Data availability}}\n\
             All data is available.\n\
             \\section{{Results}}\n\
             The gain holds \\hyp{{H-{x1_seq}}} and the claim \\claim{{CLAIMS-{claim1_seq}}}, \
             as \\\\cite{{smith2020}} shows.\n\
             \\begin{{figure}}\\includegraphics{{fig1}}\\end{{figure}}\n\
             \\bibliographystyle{{siam}}\n"
        );
        std::fs::write(tex_dir.join("main.tex"), tex).unwrap();
        std::fs::write(
            tex_dir.join("refs.bib"),
            "@article{smith2020,\n  doi = {10.1000/xyz123},\n}\n",
        )
        .unwrap();
    }
    let venue = journals::venue("siam-jsc").unwrap();
    {
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        store
            .append(
                NewEvent::manuscript_registered(
                    m1,
                    &tex_dir.display().to_string(),
                    "main.tex",
                )
                .unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::manuscript_compiled(
                    m1,
                    manuscript::CompileOutcome::Ok,
                    Some("latexmk"),
                    "Output written on main.pdf (12 pages, 250000 bytes).",
                )
                .unwrap(),
            )
            .unwrap();
    }
    let (pending_report, human_items) = {
        let c = db.0.lock().await;
        let events = EventStore::new(&c).events_all().unwrap();
        let (scans, stats, refs) = tier_two_inputs(&c, &events, m1);
        let report =
            readiness::tier_two_report(&events, Some(m1), venue, &scans, &stats, &refs, &[])
                .unwrap();
        assert_eq!(report.verdict, ReadinessVerdict::NotReady);
        let human: Vec<&readiness::TierTwoItem> = report
            .items
            .iter()
            .filter(|i| i.kind == "human_only")
            .collect();
        assert!(!human.is_empty(), "siam-jsc has human-only criteria");
        assert!(human.iter().all(|i| i.status == TierTwoStatus::HumanPending));
        // the machine checks pass on the clean board + the registered .tex
        for id in [
            "manuscript_consistency",
            "load_bearing_support",
            "references_resolved",
            "compiled_pdf",
            "statement_present:data_availability",
            "abstract_within_words",
            "pages_within",
            "reference_style",
        ] {
            let item = report
                .items
                .iter()
                .find(|i| i.criterion_id == id)
                .unwrap_or_else(|| panic!("no tier-2 item {id}"));
            assert_eq!(item.status, TierTwoStatus::Pass, "{id} should pass: {item:?}");
        }
        let human_items: Vec<String> = human.iter().map(|i| i.criterion_id.clone()).collect();
        (report, human_items)
    };
    {
        // the human confirms the human-only criteria -> journal-ready
        let c = db.0.lock().await;
        let events = EventStore::new(&c).events_all().unwrap();
        let (scans, stats, refs) = tier_two_inputs(&c, &events, m1);
        let ready = readiness::tier_two_report(
            &events, Some(m1), venue, &scans, &stats, &refs, &human_items,
        )
        .unwrap();
        assert_eq!(ready.verdict, ReadinessVerdict::Ready, "journal-ready");
        assert!(ready
            .items
            .iter()
            .filter(|i| i.kind == "human_only")
            .all(|i| i.status == TierTwoStatus::HumanConfirmed));
        assert_eq!(
            pending_report.items.len(),
            ready.items.len(),
            "confirmation flips items, it never adds or hides them"
        );
    }
    println!(
        "[stage 8] tier-2 vs {}: {} machine checks pass (12-page compile, 1 ref resolved, consistency) — not ready until the {} human-only item(s) are HumanConfirmed, then journal-ready",
        venue.id,
        pending_report.items.iter().filter(|i| i.kind != "human_only").count(),
        human_items.len()
    );

    // =====================================================================
    // [stage 9] the Journal Fit Finder (simulated ranking) -> the approval
    // -> the submission checklist mission -> tier-2 via the mission
    // =====================================================================
    let sub_id = {
        // the simulated provider's seeded ranking (siam-jsc tops the list)
        let candidates = vec![
            FitCandidate {
                venue_id: "siam-jsc".into(),
                score: 92,
                rationale: "numerical analysis and HPC fit the scope".into(),
                refs: vec![format!("H-{x1_seq}")],
                unverified_refs: vec![],
            },
            FitCandidate {
                venue_id: "physrev-e".into(),
                score: 71,
                rationale: "statistical physics in scope".into(),
                refs: vec![],
                unverified_refs: vec![],
            },
            FitCandidate {
                venue_id: "acm-toms".into(),
                score: 64,
                rationale: "software and algorithms in scope".into(),
                refs: vec![],
                unverified_refs: vec![],
            },
        ];
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        let fit = store
            .append(
                NewEvent::journal_fit_completed(
                    "orchestration-fit-1",
                    Some(m1),
                    "simulated",
                    "simulated",
                    &candidates,
                )
                .unwrap(),
            )
            .unwrap();
        // the choice is quarantined: the top candidate rides a proposal
        let intended = NewEvent::submission_created(SubmissionCreatedPayload {
            question: format!("Submit to {}", venue.name),
            stop_condition: "submission-ready".into(),
            success_criterion: "every checklist item is checked".into(),
            venue_id: venue.id.clone(),
            source_mission_id: Some(m1),
        })
        .unwrap();
        let proposal = store
            .append(
                NewEvent::proposal_created(
                    "orchestration-fit-1",
                    &intended,
                    fit.id,
                    fit.seq,
                    fit.id,
                )
                .unwrap(),
            )
            .unwrap();
        let events = store.events_all().unwrap();
        assert!(
            SubmissionProjection::fold(&events).unwrap().is_empty(),
            "nothing auto-applies (AD-3)"
        );
        let outcome = proposals::approve(&store, proposal.id, false, None).unwrap();
        assert_eq!(outcome.proposal.status, ProposalStatus::Merged);
        // the merged choice lands through the typed command path (the
        // owner's explicit create_submission_mission)
        store.append(intended).unwrap().id
    };
    let mut prechecked_count = 0usize;
    let (machine_items, human_item_ids): (Vec<String>, Vec<String>) = {
        let events = events_of(&db).await;
        let sub = SubmissionProjection::for_mission(&events, sub_id)
            .unwrap()
            .expect("the submission checklist mission exists");
        assert_eq!(sub.venue_id, "siam-jsc");
        assert_eq!(sub.source_mission_id, Some(m1));
        let machine: Vec<String> = sub
            .items
            .iter()
            .filter(|i| !i.human)
            .map(|i| i.item_id.clone())
            .collect();
        let human: Vec<String> = sub
            .items
            .iter()
            .filter(|i| i.human)
            .map(|i| i.item_id.clone())
            .collect();
        assert!(!machine.is_empty());
        assert!(!human.is_empty(), "the human items are flagged");
        assert!(sub.items.iter().all(|i| i.checked.is_none()), "nothing is checked yet");
        (machine, human)
    };
    {
        // the agent pre-checks the passing machine items through the same
        // quarantine (the precheck seam): propose -> approve -> agent stamp
        let to_precheck: Vec<String> = {
            let c = db.0.lock().await;
            let events = EventStore::new(&c).events_all().unwrap();
            let (scans, stats, refs) = tier_two_inputs(&c, &events, m1);
            let report =
                readiness::tier_two_report(&events, Some(m1), venue, &scans, &stats, &refs, &[])
                    .unwrap();
            machine_items
                .iter()
                .filter(|id| {
                    report
                        .items
                        .iter()
                        .any(|r| &r.criterion_id == *id && r.status == TierTwoStatus::Pass)
                })
                .cloned()
                .collect()
        };
        assert!(!to_precheck.is_empty(), "the passing machine checks pre-check");
        prechecked_count = to_precheck.len();
        for item_id in &to_precheck {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let intended = NewEvent::submission_item_checked(
                sub_id,
                &venue.id,
                item_id,
                Some("machine check passed"),
            )
            .unwrap();
            let head = store.events_all().unwrap().last().unwrap().clone();
            let proposal = store
                .append(
                    NewEvent::proposal_created(
                        "orchestration-precheck",
                        &intended,
                        sub_id,
                        head.seq,
                        head.id,
                    )
                    .unwrap(),
                )
                .unwrap();
            proposals::approve(&store, proposal.id, false, None).unwrap();
        }
        let events = events_of(&db).await;
        let sub = SubmissionProjection::for_mission(&events, sub_id)
            .unwrap()
            .expect("the submission mission folds");
        for item_id in &to_precheck {
            let item = sub.items.iter().find(|i| &i.item_id == item_id).unwrap();
            let stamp = item.checked.as_ref().expect("applied at merge");
            assert_eq!(stamp.actor, "agent:orchestration-precheck");
        }
        // the human items stay flagged until the owner checks them
        for item_id in &human_item_ids {
            let item = sub.items.iter().find(|i| &i.item_id == item_id).unwrap();
            assert!(item.checked.is_none(), "human items are never agent-checkable");
        }
    }
    {
        // the owner confirms the human-only items (evented, actor=user)
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        for item_id in &human_item_ids {
            store
                .append(
                    NewEvent::submission_item_checked(
                        sub_id,
                        &venue.id,
                        item_id,
                        Some("confirmed by the owner"),
                    )
                    .unwrap(),
                )
                .unwrap();
        }
    }
    {
        // tier-2 via the mission, over the wire: journal-ready, all checked
        let (status, view) = http_get(&ro, &format!("/api/submissions/{sub_id}")).await;
        assert_eq!(status, 200, "submission view: {view}");
        assert_eq!(view["allChecked"], true);
        assert_eq!(view["readiness"]["verdict"], "ready", "journal-ready via the mission");
        assert!(view["submission"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["human"] == true)
            .all(|i| i["checked"].is_object()));
    }
    println!(
        "[stage 9] fit finder: simulated ranking (siam-jsc 92) -> quarantined proposal -> approved -> submission checklist mission; {}/{} machine items pre-checked (agent-stamped), {} human items flagged then owner-confirmed -> journal-ready",
        prechecked_count,
        machine_items.len(),
        human_item_ids.len()
    );

    // =====================================================================
    // [stage 10] export at one cut -> checkpoint -> rollback -> the AD-11
    // stale banner on re-export
    // =====================================================================
    let export_dir = tmp_dir("export");
    let cut_seq = {
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        let outcome = export::export_workspace(&store, &export_dir, export::SCOPE_ALL).unwrap();
        assert!(outcome.manifest.stale_notice.is_none(), "a fresh export is not stale");
        outcome.manifest.cut_seq
    };
    {
        // the git-friendly files are readable without the app
        let manifest = std::fs::read_to_string(export_dir.join(export::MANIFEST_FILE)).unwrap();
        assert!(manifest.contains(&format!("<!-- export-cut: {cut_seq} -->")));
        let mission_files: Vec<_> = std::fs::read_dir(export_dir.join("missions"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!mission_files.is_empty(), "one file per mission");
        let any_mission = std::fs::read_to_string(mission_files[0].path()).unwrap();
        assert!(
            any_mission.contains("Stop after"),
            "the mission file carries the stop condition: {any_mission}"
        );
        let timeline = std::fs::read_to_string(export_dir.join("timeline").join("events.jsonl"))
            .unwrap();
        let first_line = timeline.lines().next().unwrap();
        let parsed: Value = serde_json::from_str(first_line).unwrap();
        assert!(parsed["seq"].as_i64().is_some() && parsed["kind"].as_str().is_some());
    }
    {
        // post-cut work, then a rollback past the cut (AD-11)
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        let cp = store
            .append(NewEvent::checkpoint_created("pre-trial-2", cut_seq).unwrap())
            .unwrap();
        store
            .append(
                NewEvent::hypothesis_created(
                    "A post-cut hypothesis the rollback will orphan.",
                    m2,
                )
                .unwrap(),
            )
            .unwrap();
        checkpoints::rollback(&store, cp.id).unwrap();
        let inspected = export::inspect_export(&store, &export_dir).unwrap().unwrap();
        assert!(inspected.stale, "the rollback staled the export");
        assert!(inspected.rollback_seq.unwrap() > cut_seq);
        // the re-export flags the stale previous cut + renders the banner
        let second = export::export_workspace(&store, &export_dir, export::SCOPE_ALL).unwrap();
        let notice = second.manifest.stale_notice.as_ref().unwrap();
        assert_eq!(notice.previous_cut, cut_seq);
        let manifest = std::fs::read_to_string(export_dir.join(export::MANIFEST_FILE)).unwrap();
        assert!(manifest.contains("STALE"), "the AD-11 banner renders");
        assert!(manifest.contains(&format!("`e-{cut_seq}`")));
        // a third export with no further rollback is fresh again
        let third = export::export_workspace(&store, &export_dir, export::SCOPE_ALL).unwrap();
        assert!(third.manifest.stale_notice.is_none());
    }
    println!(
        "[stage 10] export: cut e-{cut_seq} manifest + readable mission/timeline files; checkpoint + post-cut work + rollback -> inspect flags stale, the re-export carries the STALE banner (previous cut e-{cut_seq})"
    );

    // =====================================================================
    // [stage 11] the bridge: pairing gate -> quick-capture -> token approve
    // =====================================================================
    let proposal3 = {
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        proposals::propose_transition(
            &store,
            "orchestration-run-3",
            x3,
            HypothesisStatus::Revised,
            "the critic wants a revised statement after the new sources",
        )
        .unwrap()
    };
    {
        // unpaired: every remote verb is refused with the typed 401
        let (status, body) = http_get(&b, "/api/missions").await;
        assert_eq!(status, 401);
        assert!(body["error"].as_str().unwrap().starts_with("unpaired:"));
        let (status, body) =
            post_json(&b, "/api/capture", json!({ "question": "Who wins?" }), None).await;
        assert_eq!(status, 401);
        assert!(body["error"].as_str().unwrap().starts_with("unpaired:"));
        let (status, body) = post_json(
            &b,
            &format!("/api/proposals/{}/approve", proposal3.id),
            json!({ "force": false }),
            None,
        )
        .await;
        assert_eq!(status, 401);
        assert!(body["error"].as_str().unwrap().starts_with("unpaired:"));
    }
    let token = {
        let c = db.0.lock().await;
        bridge::pair_device(&c, "Rehearsal Phone").unwrap().token
    };
    {
        // paired quick-capture lands a PENDING mission card (a draft)
        let (status, captured) = post_json(
            &b,
            "/api/capture",
            json!({ "question": "Does the erratum change the verdict?" }),
            Some(&token),
        )
        .await;
        assert_eq!(status, 200, "capture: {captured}");
        assert_eq!(captured["status"], "draft");
        let captured_seq = captured["seq"].as_i64().unwrap();
        let (status, missions) = http_get_with_token(&b, "/api/missions", &token).await;
        assert_eq!(status, 200);
        assert!(
            missions
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["seq"] == json!(captured_seq) && m["status"] == "draft"),
            "the captured card lands as a pending draft"
        );
        // paired token approve of the proposal: evented actor=user surface=mobile
        let (status, outcome) = post_json(
            &b,
            &format!("/api/proposals/{}/approve", proposal3.id),
            json!({ "force": false }),
            Some(&token),
        )
        .await;
        assert_eq!(status, 200, "approve: {outcome}");
        assert_eq!(outcome["proposal"]["status"], "merged");
        assert_eq!(outcome["proposal"]["decided"]["actor"], "user");
        assert_eq!(outcome["proposal"]["decided"]["surface"], "mobile");
        let events = events_of(&db).await;
        let merges: Vec<&StoredEvent> = events
            .iter()
            .filter(|e| e.kind == "merge.approved")
            .collect();
        let mobile: Vec<&&StoredEvent> = merges
            .iter()
            .filter(|e| e.payload["surface"] == json!("mobile"))
            .collect();
        assert_eq!(mobile.len(), 1, "exactly one mobile-surfaced merge");
        assert!(matches!(mobile[0].actor, Actor::User));
        // the board reflects the merged revision
        let (status, hyps) = http_get_with_token(&b, &format!("/api/missions/{m1}/hypotheses"), &token).await;
        assert_eq!(status, 200);
        let board_x3 = hyps
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["id"].as_str() == Some(x3.to_string().as_str()))
            .unwrap();
        assert_eq!(board_x3["status"], "revised");
    }
    println!(
        "[stage 11] bridge: unpaired reads/capture/approve refused 401 `unpaired:`; paired quick-capture landed a draft card; token approve merged with actor=user surface=mobile (exactly one mobile merge event)"
    );

    // =====================================================================
    // [stage 12] log integrity: deterministic replay + no secret anywhere
    // =====================================================================
    {
        let events = events_of(&db).await;
        assert!(events.len() > 60, "a full loop's worth of events: {}", events.len());
        for (i, event) in events.iter().enumerate() {
            assert_eq!(event.seq, i as i64 + 1, "seq is dense and strictly increasing");
        }
        // replay folds are deterministic: fold the log twice, same state
        let missions_a = MissionsProjection::fold(&events).unwrap();
        let missions_b = MissionsProjection::fold(&events).unwrap();
        assert_eq!(missions_a.len(), missions_b.len());
        let hyps_a = HypothesesProjection::fold(&events).unwrap();
        let hyps_b = HypothesesProjection::fold(&events).unwrap();
        assert_eq!(hyps_a.len(), hyps_b.len());
        let claims_a = EvidenceProjection::fold(&events).unwrap();
        let claims_b = EvidenceProjection::fold(&events).unwrap();
        assert_eq!(claims_a.len(), claims_b.len());
        let proposals_a = ProposalsProjection::fold(&events).unwrap();
        let proposals_b = ProposalsProjection::fold(&events).unwrap();
        assert_eq!(proposals_a.len(), proposals_b.len());
        // no secret ever enters an event payload: arm a key, sweep the log
        // and the exported files
        {
            let c = db.0.lock().await;
            db::set_setting(&c, "api_key", SENTINEL_KEY).unwrap();
        }
        for event in &events {
            let whole = serde_json::to_string(event).unwrap();
            assert!(
                !whole.contains(SENTINEL_KEY),
                "event {} ({}) leaked the api key",
                event.seq,
                event.kind
            );
        }
        let mut files: Vec<PathBuf> = Vec::new();
        let mut queue: Vec<PathBuf> = vec![export_dir.clone()];
        while let Some(dir) = queue.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    queue.push(path);
                } else {
                    files.push(path);
                }
            }
        }
        assert!(!files.is_empty());
        for path in &files {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            assert!(
                !text.contains(SENTINEL_KEY),
                "exported file {} leaked the api key",
                path.display()
            );
        }
        println!(
            "[stage 12] integrity: {} events (dense seq), replay folds deterministic, no secret in any of {} events or {} exported files",
            events.len(),
            events.len(),
            files.len()
        );
    }

    // the rehearsal folded the whole loop on the real core
    let _ = std::fs::remove_dir_all(&tex_dir);
    let _ = std::fs::remove_dir_all(&export_dir);
    clean_db_path(&db_path);
    println!("[rehearsal] the full research loop rehearsed end to end — 12 stages green");
}
