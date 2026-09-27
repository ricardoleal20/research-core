// The user-testing harness seed (demo workspace): ONE command that appends a
// RICH, representative demo workspace as REAL events — every screen gets data
// to browse, all of it through the typed constructors (AD-15), never raw JSON
// inserts, never a second writer. The seed exists so the owner can click
// through the whole UI before deciding on fixes; it is honestly labeled as
// demo data and carries zero secrets.
//
// Idempotence (the migration's marker pattern, AD-16): a `setup.demo_seeded`
// marker event lands LAST, after a complete generation. Re-running without
// `force` refuses with `already_seeded:`; with `force` it appends a NEW
// generation (a fresh set of events with fresh ids and seqs) — history is
// append-only, never mutated (AD-1).

use std::path::Path;

use chrono::Utc;
use rusqlite::Connection;
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::db::Db;
use crate::domain::digest;
use crate::domain::evidence::{self, EvidenceProjection};
use crate::domain::hypotheses::{
    HypothesesProjection, HypothesisRelatedPayload, HypothesisStatus, RelationKind,
};
use crate::domain::journals::{self, FitCandidate};
use crate::domain::library::{self, LibraryProjection, RefAddedPayload};
use crate::domain::manuscript::{self, ManuscriptsProjection};
use crate::domain::missions::{
    self, Autonomy, MissionCreatedPayload, MissionStatus, MissionsProjection, RoleConfig,
};
use crate::domain::nightshift::{self, TerminalKind};
use crate::domain::proposals::{self, ProposalStatus, ProposalsProjection};
use crate::domain::readiness::{self, ReadinessItemKind, ReadinessVerdict, TierTwoStatus};
use crate::domain::search::SearchRunPayload;
use crate::domain::submissions::{SubmissionCreatedPayload, SubmissionProjection};
use crate::domain::support::{self, SupportVerdict};
use crate::domain::telemetry;
use crate::domain::trust::{self, SpendRefusedPayload};
use crate::domain::verifier::VerificationOutcome;
use crate::eventstore::{ Actor, EventError, EventStore, NewEvent, StoredEvent};
use crate::AppPaths;

/// The idempotence marker (the migration's pattern): appended once after a
/// complete demo generation; a later run detects it and refuses unless forced.
pub const DEMO_SEEDED: &str = "setup.demo_seeded";

/// The assessing model the demo pins attribute their confidence to (a label,
/// never a secret — the same attribution the runtime stamps on real pins).
const ASSESSING_MODEL: &str = "GLM-5.3";
/// The support sweep's judging model: always a DIFFERENT model than the
/// assessor (NFR-3 — never one algorithm grading its own homework).
const JUDGING_MODEL: &str = "simulated";

/// The typed error of the seed command — coded, bilingual-safe prefixes.
#[derive(Debug, thiserror::Error)]
pub enum DemoSeedError {
    #[error("already_seeded: the demo workspace is present (generation {generation}) — pass force to append a fresh generation")]
    AlreadySeeded { generation: u32 },
    #[error(transparent)]
    Event(#[from] EventError),
    #[error(transparent)]
    Proposal(#[from] crate::domain::proposals::ProposalError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// What one seed run appended — the confirmation the UI renders.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DemoSeedOutcome {
    /// 1 for the first generation, +1 per forced re-seed.
    pub generation: u32,
    /// Total events this generation appended (the marker included).
    pub events_appended: usize,
    /// The marker event's seq (the generation's cut point).
    pub marker_seq: i64,
    pub missions: usize,
    pub hypotheses: usize,
    pub claims: usize,
    pub refs: usize,
    pub proposals: usize,
    pub searches: usize,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `setup.demo_seeded` marker
    /// comes into being — actor=user (the owner commanded the seed), payload
    /// carrying the generation number. Appended LAST, after a complete
    /// generation, so a partial seed never masquerades as a complete one.
    pub fn setup_demo_seeded(generation: u32) -> Result<Self, EventError> {
        if generation == 0 {
            return Err(EventError::Invalid(
                "setup.generation must be >= 1 — the first generation is 1".into(),
            ));
        }
        Self::new(
            DEMO_SEEDED,
            Actor::User,
            serde_json::json!({ "generation": generation }),
        )
    }
}

/// How many complete demo generations the log already carries.
fn demo_generation(events: &[StoredEvent]) -> u32 {
    events
        .iter()
        .filter(|e| e.kind == DEMO_SEEDED)
        .count() as u32
}

// ---------------------------------------------------------------------------
// The seed itself
// ---------------------------------------------------------------------------

/// Append one rich demo generation as REAL events. Everything goes through
/// the typed constructors; the marker lands last. Never mutates history.
/// Refuses `already_seeded:` when a generation is present, unless `force`
/// appends a new one.
pub fn seed_demo(
    conn: &Connection,
    manuscript_root: &Path,
    force: bool,
) -> Result<DemoSeedOutcome, DemoSeedError> {
    let store = EventStore::new(conn);
    let present = demo_generation(&store.events_all()?);
    if present > 0 && !force {
        return Err(DemoSeedError::AlreadySeeded { generation: present });
    }
    let generation = present + 1;
    let gen_tag = generation.to_string();
    let mut appended = 0usize;
    let mut missions_n = 0usize;
    let mut hypotheses_n = 0usize;
    let mut claims_n = 0usize;
    let mut refs_n = 0usize;
    let mut proposals_n = 0usize;
    let mut searches_n = 0usize;
    // Every demo run id carries the generation, so a forced re-seed can never
    // collide with a prior generation's runs.
    let run = |name: &str| format!("demo-{gen_tag}-{name}");

    // -- M-A: the ACTIVE flagship — an open board mid-research -------------
    let m_a = store.append(NewEvent::mission_created(MissionCreatedPayload {
        question: "Does retrieval-augmented grounding reduce hallucination in long-form answers?"
            .into(),
        stop_condition: "Stop after 14 nightly scans or 40 sources reviewed.".into(),
        success_criterion: "Hallucination rate drops on at least 4 of 5 benchmark sets.".into(),
        autonomy: Autonomy::Suggest,
        spend_ceiling_cents: 500,
        roles: vec![
            RoleConfig::drafter("simulated", "simulated"),
            RoleConfig::critic("simulated", "simulated"),
        ],
        schedule: "daily-03:00".into(),
    })?)?;
    missions_n += 1;
    appended += 1;

    // Hypotheses across the lifecycle: proposed, testing, supported, revised.
    let h_a1 = store.append(NewEvent::hypothesis_created(
        "Retrieval grounding cuts the hallucination rate on long-form QA.",
        m_a.id,
    )?)?;
    let h_a2 = store.append(NewEvent::hypothesis_created(
        "Grounding only helps when citations are verified first.",
        m_a.id,
    )?)?;
    let h_a3 = store.append(NewEvent::hypothesis_created(
        "Grounding helps less as the context window grows.",
        m_a.id,
    )?)?;
    let h_a4 = store.append(NewEvent::hypothesis_created(
        "Verified citations are the binding constraint on grounding gains.",
        m_a.id,
    )?)?;
    let h_a5 = store.append(NewEvent::hypothesis_created(
        "The fluency cost of grounding is negligible.",
        m_a.id,
    )?)?;
    hypotheses_n += 5;
    appended += 5;

    // X-A1: proposed -> testing (a trial is running).
    store.append(
        NewEvent::hypothesis_status_changed(
            HypothesisStatus::Proposed,
            HypothesisStatus::Testing,
            "Trial 1 is running on the seeded benchmark.",
        )?
        .with_causes(vec![h_a1.id]),
    )?;
    appended += 1;
    // X-A5: proposed -> testing -> supported -> revised (mid-rework).
    for (from, to, basis) in [
        (
            HypothesisStatus::Proposed,
            HypothesisStatus::Testing,
            "Trial 1 measured the fluency delta.",
        ),
        (
            HypothesisStatus::Testing,
            HypothesisStatus::Supported,
            "The delta stayed under 2% perplexity on 4 seeds.",
        ),
        (
            HypothesisStatus::Supported,
            HypothesisStatus::Revised,
            "The critic asked for a revised statement after the new seeds.",
        ),
    ] {
        store.append(
            NewEvent::hypothesis_status_changed(from, to, basis)?
                .with_causes(vec![h_a5.id]),
        )?;
        appended += 1;
    }
    // X-A4: proposed -> testing by hand, then supported through the MERGED
    // quarantine proposal below.
    store.append(
        NewEvent::hypothesis_status_changed(
            HypothesisStatus::Proposed,
            HypothesisStatus::Testing,
            "Trial 1 isolated the verification variable.",
        )?
        .with_causes(vec![h_a4.id]),
    )?;
    appended += 1;

    // Typed relations between the candidates (FR-2.3).
    store.append(NewEvent::hypothesis_related(HypothesisRelatedPayload {
        from_hypothesis_id: h_a2.id,
        to_hypothesis_id: h_a3.id,
        relation_kind: RelationKind::Contradicts,
    })?)?;
    store.append(NewEvent::hypothesis_related(HypothesisRelatedPayload {
        from_hypothesis_id: h_a4.id,
        to_hypothesis_id: h_a1.id,
        relation_kind: RelationKind::Extends,
    })?)?;
    appended += 2;

    // The demo refs (evented library, FR-15) — the archived one included.
    let ref_arxiv = format!("ref-demo-{gen_tag}-arxiv");
    let ref_doi = format!("ref-demo-{gen_tag}-doi");
    let ref_manual = format!("ref-demo-{gen_tag}-manual");
    let ref_s2 = format!("ref-demo-{gen_tag}-s2");
    let ref_archived = format!("ref-demo-{gen_tag}-archived");
    for (ref_id, title, authors, year, venue, doi, url, source) in [
        (
            ref_arxiv.as_str(),
            "Sparse Attention Memory Costs at Long Context",
            "Beltagy et al.",
            2020,
            "arxiv",
            "",
            "https://arxiv.org/abs/2004.05150",
            "arxiv",
        ),
        (
            ref_doi.as_str(),
            "Sparse Attention at Thirty-Two Thousand Tokens",
            "Romero and Watanabe",
            2025,
            "Journal of Honest Benchmarks",
            "10.1000/xyz123",
            "https://example.org/paper/sparse-32k",
            "doi",
        ),
        (
            ref_manual.as_str(),
            "Grounded Generation: A Survey",
            "Ito et al.",
            2024,
            "arXiv",
            "",
            "",
            "manual",
        ),
        (
            ref_s2.as_str(),
            "Lost in the Middle: Context Confuses Language Models",
            "Liu et al.",
            2023,
            "arxiv",
            "",
            "https://arxiv.org/abs/2307.03172",
            "s2",
        ),
        (
            ref_archived.as_str(),
            "A superseded preprint on grounding",
            "Old Author",
            2019,
            "arXiv",
            "",
            "",
            "manual",
        ),
    ] {
        store.append(NewEvent::ref_added(RefAddedPayload {
            ref_id: ref_id.to_string(),
            project_id: "proj-tesis-cap2".into(),
            title: title.to_string(),
            authors: authors.to_string(),
            year: Some(year),
            venue: venue.to_string(),
            doi: doi.to_string(),
            url: url.to_string(),
            tags: "demo".into(),
            source: source.to_string(),
            zotero_item_key: None,
            arxiv_id: None,
            abstract_text: None,
        })?)?;
        refs_n += 1;
        appended += 1;
    }
    // One archived ref: removed (masked) — restorable from the library view.
    let archived_added = store
        .events_all()?
        .into_iter()
        .rev()
        .find(|e| e.kind == library::REF_ADDED && e.payload["ref_id"] == ref_archived.as_str())
        .expect("the archived ref's add event");
    store.append(NewEvent::ref_removed(&ref_archived, Some(archived_added.id))?)?;
    appended += 1;

    // Claims + pins: citation and numerical, digest + confidence + model
    // (AD-5), one unpinned (the readiness blocker), verification results
    // (verified, and one failed-then-recovered), support verdicts.
    let c_a1 = store.append(NewEvent::claim_registered(
        "Grounded answers cite their sources 82% of the time on the long-form set.",
        h_a1.id,
        None,
    )?)?;
    let c_a2 = store.append(NewEvent::claim_registered(
        "The hallucination rate drops from 31% to 9% with grounding enabled.",
        h_a1.id,
        None,
    )?)?;
    let c_a3 = store.append(NewEvent::claim_registered(
        "Verification must precede generation for the gain to hold.",
        h_a2.id,
        None,
    )?)?; // stays UNPINNED — the tier-1 blocker
    let c_a4 = store.append(NewEvent::claim_registered(
        "Unverified citations account for 74% of remaining hallucinations.",
        h_a4.id,
        None,
    )?)?;
    claims_n += 4;
    appended += 4;

    // Citation pin: excerpt + sha-256 digest + confidence + model.
    let pin_a1 = store.append(NewEvent::evidence_pinned_citation(
        c_a1.id,
        h_a1.id,
        &ref_arxiv,
        "Grounded answers cite their sources 82% of the time on the long-form set.",
        0.82,
        ASSESSING_MODEL,
    )?)?;
    // Numerical pin: the artifact's digest binds to its content.
    let pin_a2 = store.append(NewEvent::evidence_pinned_numerical(
        c_a2.id,
        h_a1.id,
        "results/table1.csv",
        "31% -> 9% hallucination over four seeds",
        0.9,
        ASSESSING_MODEL,
    )?)?;
    let pin_a4 = store.append(NewEvent::evidence_pinned_citation(
        c_a4.id,
        h_a4.id,
        &ref_s2,
        "Unverified citations account for 74% of remaining hallucinations.",
        0.71,
        ASSESSING_MODEL,
    )?)?;
    appended += 3;

    // Verification: verified, and one FAILED-then-verified (the source
    // recovered — the latest result wins).
    store.append(NewEvent::evidence_verified(
        c_a1.id,
        h_a1.id,
        pin_a1.seq,
        VerificationOutcome::Verified,
        "excerpt_matched",
        "https://arxiv.org/abs/2004.05150".to_string(),
    )?)?;
    store.append(NewEvent::evidence_verified(
        c_a4.id,
        h_a4.id,
        pin_a4.seq,
        VerificationOutcome::Failed,
        "fetch_error",
        "https://arxiv.org/abs/2307.03172".to_string(),
    )?)?;
    store.append(NewEvent::evidence_verified(
        c_a4.id,
        h_a4.id,
        pin_a4.seq,
        VerificationOutcome::Verified,
        "excerpt_matched",
        "https://arxiv.org/abs/2307.03172".to_string(),
    )?)?;
    appended += 3;

    // Support verdicts (the third signal, a different model judges):
    // supported, partially, unsupported.
    store.append(NewEvent::pin_support_checked(
        c_a1.id,
        h_a1.id,
        pin_a1.seq,
        SupportVerdict::Supported,
        0.9,
        JUDGING_MODEL,
        ASSESSING_MODEL,
    )?)?;
    store.append(NewEvent::pin_support_checked(
        c_a2.id,
        h_a1.id,
        pin_a2.seq,
        SupportVerdict::Partially,
        0.64,
        JUDGING_MODEL,
        ASSESSING_MODEL,
    )?)?;
    store.append(NewEvent::pin_support_checked(
        c_a4.id,
        h_a4.id,
        pin_a4.seq,
        SupportVerdict::Unsupported,
        0.58,
        JUDGING_MODEL,
        ASSESSING_MODEL,
    )?)?;
    appended += 3;

    // The quarantine: one pending, one rejected, one merged (X-A4's
    // testing -> supported rides the approved proposal).
    proposals::propose_transition(
        &store,
        &run("propose-pending"),
        h_a1.id,
        HypothesisStatus::Supported,
        "the drafter's scan suggests advancing after trial 1",
    )?;
    proposals_n += 1;
    appended += 1;
    let to_reject = proposals::propose_transition(
        &store,
        &run("propose-reject"),
        h_a2.id,
        HypothesisStatus::Testing,
        "the critic wants trial 1 to start from the verified set",
    )?;
    proposals::reject(&store, to_reject.id, None)?;
    proposals_n += 1;
    appended += 2;
    let to_merge = proposals::propose_transition(
        &store,
        &run("propose-merge"),
        h_a4.id,
        HypothesisStatus::Supported,
        "trial 1 isolated the verification variable and held",
    )?;
    proposals::approve(&store, to_merge.id, false, None)?;
    proposals_n += 1;
    appended += 2;

    // M-A's night: two finished scans with heartbeats (digest rows).
    let ns_a1 = store.append(NewEvent::run_started(
        &run("ns-a1"),
        m_a.id,
        "daily-03:00",
        nightshift::SCAN_STEP,
    )?)?;
    store.append(NewEvent::run_heartbeat(
        &run("ns-a1"),
        m_a.id,
        ns_a1.seq,
        ns_a1.ts,
    )?)?;
    store.append(NewEvent::run_finished(
        &run("ns-a1"),
        m_a.id,
        "scan found 3 candidate sources; one pin verified; one proposal pending",
        1,
    )?)?;
    let ns_a2 = store.append(NewEvent::run_started(
        &run("ns-a2"),
        m_a.id,
        "daily-03:00",
        nightshift::SCAN_STEP,
    )?)?;
    store.append(NewEvent::run_heartbeat(
        &run("ns-a2"),
        m_a.id,
        ns_a2.seq,
        ns_a2.ts,
    )?)?;
    store.append(NewEvent::run_finished(
        &run("ns-a2"),
        m_a.id,
        "scan re-checked the failed pin; the source recovered; verified",
        0,
    )?)?;
    appended += 6;

    // M-A's spend (metered calls, under the ceiling — state ok).
    for (input, output, cents, role) in [
        (1_842u64, 913u64, 82u64, Some("drafter")),
        (1_204u64, 640u64, 65u64, Some("critic")),
    ] {
        store.append(NewEvent::spend_recorded(crate::domain::spend::SpendRecordedPayload {
            provider: "openrouter".into(),
            model: "anthropic/claude-sonnet-4.5".into(),
            input_tokens: input,
            output_tokens: output,
            cost_cents: cents,
            mission_id: Some(m_a.id),
            role: role.map(str::to_string),
            run_id: Some(run("ns-a1")),
            note: None,
        })?)?;
        appended += 1;
    }

    // M-A's searches: one with results, one null (logged identically).
    store.append(NewEvent::search_run(SearchRunPayload {
        query: "retrieval-augmented generation hallucination long-form".into(),
        database: "arxiv".into(),
        filters: [
            ("from_year".to_string(), serde_json::json!(2019)),
            ("to_year".to_string(), serde_json::json!(2025)),
        ]
        .into_iter()
        .collect(),
        order: Some("relevance".into()),
        first_page: true,
        result_count: 3,
        null_result: false, // derived by the constructor
        mission_id: Some(m_a.id),
        run_id: Some(run("ns-a1")),
    })?)?;
    searches_n += 1;
    appended += 1;
    store.append(NewEvent::search_run(SearchRunPayload {
        query: "perplexity-aware dense retrieval in low-resource Tigris".into(),
        database: "arxiv".into(),
        filters: Default::default(),
        order: None,
        first_page: true,
        result_count: 0, // the null result derives — never hidden (FR-12.1)
        null_result: false,
        mission_id: Some(m_a.id),
        run_id: Some(run("ns-a1")),
    })?)?;
    searches_n += 1;
    appended += 1;

    // -- M-B: AWAITING REVIEW — a settled board the owner holds ------------
    let m_b = store.append(NewEvent::mission_created(MissionCreatedPayload {
        question: "Do scaling laws predict small-model loss plateaus?".into(),
        stop_condition: "Stop after 10 nightly scans or 25 sources reviewed.".into(),
        success_criterion: "The plateau prediction holds within 5% on 3 of 4 model families."
            .into(),
        autonomy: Autonomy::Suggest,
        spend_ceiling_cents: 400,
        roles: vec![
            RoleConfig::drafter("simulated", "simulated"),
            RoleConfig::critic("simulated", "simulated"),
        ],
        schedule: "daily-03:00".into(),
    })?)?;
    missions_n += 1;
    appended += 1;
    let h_b1 = store.append(NewEvent::hypothesis_created(
        "The plateau is predicted by compute-effective data scaling.",
        m_b.id,
    )?)?;
    hypotheses_n += 1;
    appended += 1;
    for (from, to, basis) in [
        (
            HypothesisStatus::Proposed,
            HypothesisStatus::Testing,
            "Trial 1 fit the scaling curve on four families.",
        ),
        (
            HypothesisStatus::Testing,
            HypothesisStatus::Supported,
            "The prediction held within 5% on three families.",
        ),
    ] {
        store.append(
            NewEvent::hypothesis_status_changed(from, to, basis)?
                .with_causes(vec![h_b1.id]),
        )?;
        appended += 1;
    }
    // The human-held review state (the evaluator never ends it).
    store.append(
        NewEvent::new(missions::MISSION_AWAITING_REVIEW, Actor::User, serde_json::json!({}))?
            .with_causes(vec![m_b.id]),
    )?;
    appended += 1;

    // -- M-C: COMPLETED — the board settled with support -------------------
    let m_c = store.append(NewEvent::mission_created(MissionCreatedPayload {
        question: "Does kNN augmentation improve factual recall without hurting fluency?".into(),
        stop_condition: "Stop after 6 nightly scans or 15 sources reviewed.".into(),
        success_criterion: "Recall improves on 4 of 5 probes with fluency within 2%.".into(),
        autonomy: Autonomy::Suggest,
        spend_ceiling_cents: 300,
        roles: vec![
            RoleConfig::drafter("simulated", "simulated"),
            RoleConfig::critic("simulated", "simulated"),
        ],
        schedule: "daily-03:00".into(),
    })?)?;
    missions_n += 1;
    appended += 1;
    let h_c1 = store.append(NewEvent::hypothesis_created(
        "kNN augmentation improves factual recall on closed-book QA.",
        m_c.id,
    )?)?;
    let h_c2 = store.append(NewEvent::hypothesis_created(
        "The recall gain disappears when the datastore is stale.",
        m_c.id,
    )?)?;
    hypotheses_n += 2;
    appended += 2;
    for (hyp, steps) in [
        (
            h_c1.id,
            vec![
                (HypothesisStatus::Proposed, HypothesisStatus::Testing),
                (HypothesisStatus::Testing, HypothesisStatus::Supported),
            ],
        ),
        (
            h_c2.id,
            vec![
                (HypothesisStatus::Proposed, HypothesisStatus::Testing),
                (HypothesisStatus::Testing, HypothesisStatus::Refuted),
            ],
        ),
    ] {
        for (from, to) in steps {
            store.append(
                NewEvent::hypothesis_status_changed(
                    from,
                    to,
                    "the measures held on the probe set.",
                )?
                .with_causes(vec![hyp]),
            )?;
            appended += 1;
        }
    }
    store.append(NewEvent::mission_terminal(
        TerminalKind::Completed,
        m_c.id,
        "board_settled_supported",
        serde_json::json!({}),
    )?)?;
    appended += 1;

    // -- M-D: STOPPED — the cost ceiling was reached ------------------------
    let m_d = store.append(NewEvent::mission_created(MissionCreatedPayload {
        question: "Is citation grounding verifiable with non-LLM fetchers at scale?".into(),
        stop_condition: "Stop when the $3.00 spend ceiling is hit.".into(),
        success_criterion: "95% of pins verify within one pass over 100 sources.".into(),
        autonomy: Autonomy::ActWithReceipts,
        spend_ceiling_cents: 300,
        roles: vec![
            RoleConfig::drafter("simulated", "simulated"),
            RoleConfig::critic("simulated", "simulated"),
        ],
        schedule: "daily-03:00".into(),
    })?)?;
    missions_n += 1;
    appended += 1;
    for cents in [150u64, 150u64] {
        store.append(NewEvent::spend_recorded(crate::domain::spend::SpendRecordedPayload {
            provider: "openrouter".into(),
            model: "anthropic/claude-sonnet-4.5".into(),
            input_tokens: 2_100,
            output_tokens: 1_050,
            cost_cents: cents,
            mission_id: Some(m_d.id),
            role: Some("drafter".into()),
            run_id: Some(run("ns-d1")),
            note: None,
        })?)?;
        appended += 1;
    }
    // The ceiling refusal — an event, never a silent no (AD-10).
    store.append(NewEvent::spend_refused(SpendRefusedPayload {
        run_id: run("ns-d1"),
        scope: "mission".into(),
        ceiling_cents: 300,
        would_be_cost_cents: 450,
        mission_id: Some(m_d.id),
        target: None,
    })?)?;
    appended += 1;
    store.append(NewEvent::mission_terminal(
        TerminalKind::Stopped,
        m_d.id,
        "cost_ceiling_reached",
        serde_json::json!({ "spend_cents": 300, "ceiling_cents": 300 }),
    )?)?;
    appended += 1;

    // -- M-E: FAILED — every run failed honestly ----------------------------
    let m_e = store.append(NewEvent::mission_created(MissionCreatedPayload {
        question: "Can lossless compression bounds bound LLM reasoning depth?".into(),
        stop_condition: "Stop after 4 nightly scans or 10 sources reviewed.".into(),
        success_criterion: "A tight bound holds on 2 of 3 reasoning suites.".into(),
        autonomy: Autonomy::Watch,
        spend_ceiling_cents: 200,
        roles: vec![
            RoleConfig::drafter("simulated", "simulated"),
            RoleConfig::critic("simulated", "simulated"),
        ],
        schedule: "daily-03:00".into(),
    })?)?;
    missions_n += 1;
    appended += 1;
    let ns_e1 = store.append(NewEvent::run_started(
        &run("ns-e1"),
        m_e.id,
        "daily-03:00",
        nightshift::SCAN_STEP,
    )?)?;
    store.append(NewEvent::run_heartbeat(
        &run("ns-e1"),
        m_e.id,
        ns_e1.seq,
        ns_e1.ts,
    )?)?;
    store.append(NewEvent::run_failed(
        &run("ns-e1"),
        m_e.id,
        "provider_error",
        Utc::now(),
    )?)?;
    appended += 3;
    store.append(NewEvent::mission_terminal(
        TerminalKind::Failed,
        m_e.id,
        "all_runs_failed",
        serde_json::json!({}),
    )?)?;
    appended += 1;

    // -- M-F: ACTIVE with a dead run — the dead-man switch's honest alert --
    let m_f = store.append(NewEvent::mission_created(MissionCreatedPayload {
        question: "Does context rotation mitigate lost-in-the-middle effects?".into(),
        stop_condition: "Stop after 8 nightly scans or 20 sources reviewed.".into(),
        success_criterion: "Mid-context accuracy improves on 3 of 4 retrieval suites.".into(),
        autonomy: Autonomy::Suggest,
        spend_ceiling_cents: 350,
        roles: vec![
            RoleConfig::drafter("simulated", "simulated"),
            RoleConfig::critic("simulated", "simulated"),
        ],
        schedule: "daily-03:00".into(),
    })?)?;
    missions_n += 1;
    appended += 1;
    // Run 1 dies silently: started, heartbeats, reaped (run.dead), terminal
    // run.failed(silently_dead) — the digest's dead-run alert row.
    let ns_f1 = store.append(NewEvent::run_started(
        &run("ns-f1"),
        m_f.id,
        "daily-03:00",
        nightshift::SCAN_STEP,
    )?)?;
    let hb_f1 = store.append(NewEvent::run_heartbeat(
        &run("ns-f1"),
        m_f.id,
        ns_f1.seq,
        ns_f1.ts,
    )?)?;
    store.append(NewEvent::run_dead(
        &run("ns-f1"),
        m_f.id,
        hb_f1.ts,
        telemetry::DEAD_RUN_THRESHOLD_MINUTES,
    )?)?;
    store.append(NewEvent::run_failed(
        &run("ns-f1"),
        m_f.id,
        telemetry::SILENTLY_DEAD_REASON,
        hb_f1.ts,
    )?)?;
    appended += 4;
    // Run 2 succeeds the next night — the mission lives on.
    let ns_f2 = store.append(NewEvent::run_started(
        &run("ns-f2"),
        m_f.id,
        "daily-03:00",
        nightshift::SCAN_STEP,
    )?)?;
    store.append(NewEvent::run_heartbeat(
        &run("ns-f2"),
        m_f.id,
        ns_f2.seq,
        ns_f2.ts,
    )?)?;
    store.append(NewEvent::run_finished(
        &run("ns-f2"),
        m_f.id,
        "scan found 2 candidate sources after the dead run was reaped",
        0,
    )?)?;
    appended += 3;

    // The connection alerts: zotero down (the alert row), arxiv recovered.
    store.append(NewEvent::connection_failed("zotero", "conn_refused")?)?;
    store.append(NewEvent::connection_failed("arxiv", "timeout")?)?;
    store.append(NewEvent::connection_restored("arxiv")?)?;
    appended += 3;

    // -- M-G: the CLEAN mission — preprint-ready (tier-1 ready) -------------
    let m_g = store.append(NewEvent::mission_created(MissionCreatedPayload {
        question: "Does sparse attention match full attention at 32k context?".into(),
        stop_condition: "Stop after 3 runs or 20 sources reviewed.".into(),
        success_criterion: "The gain holds on 4 of 5 seeds.".into(),
        autonomy: Autonomy::Suggest,
        spend_ceiling_cents: 500,
        roles: vec![
            RoleConfig::drafter("simulated", "simulated"),
            RoleConfig::critic("simulated", "simulated"),
        ],
        schedule: "daily-03:00".into(),
    })?)?;
    missions_n += 1;
    appended += 1;
    let h_g1 = store.append(NewEvent::hypothesis_created(
        "Sparse attention matches full attention within 0.3 BLEU at 32k.",
        m_g.id,
    )?)?;
    let h_g2 = store.append(NewEvent::hypothesis_created(
        "The gain disappears beyond 64k context.",
        m_g.id,
    )?)?;
    hypotheses_n += 2;
    appended += 2;
    let c_g1 = store.append(NewEvent::claim_registered(
        "Sparse attention runs within 0.3 BLEU of full attention.",
        h_g1.id,
        None,
    )?)?;
    let c_g2 = store.append(NewEvent::claim_registered(
        "The gain holds across four seeds at 32k context.",
        h_g1.id,
        None,
    )?)?;
    claims_n += 2;
    appended += 2;
    let pin_g1 = store.append(NewEvent::evidence_pinned_citation(
        c_g1.id,
        h_g1.id,
        &ref_doi,
        "Sparse attention runs within 0.3 BLEU of full attention.",
        0.82,
        ASSESSING_MODEL,
    )?)?;
    let pin_g2 = store.append(NewEvent::evidence_pinned_citation(
        c_g2.id,
        h_g1.id,
        &ref_arxiv,
        "The gain holds across four seeds at 32k context.",
        0.75,
        ASSESSING_MODEL,
    )?)?;
    appended += 2;
    store.append(NewEvent::evidence_verified(
        c_g1.id,
        h_g1.id,
        pin_g1.seq,
        VerificationOutcome::Verified,
        "excerpt_matched",
        "https://example.org/paper/sparse-32k".to_string(),
    )?)?;
    store.append(NewEvent::evidence_verified(
        c_g2.id,
        h_g1.id,
        pin_g2.seq,
        VerificationOutcome::Verified,
        "excerpt_matched",
        "https://arxiv.org/abs/2004.05150".to_string(),
    )?)?;
    appended += 2;
    store.append(NewEvent::pin_support_checked(
        c_g1.id,
        h_g1.id,
        pin_g1.seq,
        SupportVerdict::Supported,
        0.91,
        JUDGING_MODEL,
        ASSESSING_MODEL,
    )?)?;
    store.append(NewEvent::pin_support_checked(
        c_g2.id,
        h_g1.id,
        pin_g2.seq,
        SupportVerdict::Unverifiable,
        0.5,
        JUDGING_MODEL,
        ASSESSING_MODEL,
    )?)?;
    appended += 2;
    for (hyp, steps) in [
        (
            h_g1.id,
            vec![
                (HypothesisStatus::Proposed, HypothesisStatus::Testing),
                (HypothesisStatus::Testing, HypothesisStatus::Supported),
            ],
        ),
        (
            h_g2.id,
            vec![
                (HypothesisStatus::Proposed, HypothesisStatus::Testing),
                (HypothesisStatus::Testing, HypothesisStatus::Refuted),
            ],
        ),
    ] {
        for (from, to) in steps {
            store.append(
                NewEvent::hypothesis_status_changed(from, to, "the measures held.")?
                    .with_causes(vec![hyp]),
            )?;
            appended += 1;
        }
    }

    // The demo .tex project (a real dir the manuscript screens scan).
    let tex_dir = manuscript_root.join(format!("gen-{generation}"));
    std::fs::create_dir_all(&tex_dir)?;
    let tex = format!(
        "\\documentclass{{article}}\n\
         \\usepackage{{natbib}}\n\
         \\begin{{abstract}}\n\
         We show sparse attention matches full attention at 32k context.\n\
         \\end{{abstract}}\n\
         \\section*{{Data availability}}\n\
         All data is available.\n\
         \\section{{Results}}\n\
         The gain holds \\hyp{{H-{h_g1_seq}}} and the claim \\claim{{CLAIMS-{c_g1_seq}}}, \
         as \\\\cite{{smith2020}} shows.\n\
         \\begin{{figure}}\\includegraphics{{fig1}}\\end{{figure}}\n\
         \\bibliographystyle{{siam}}\n",
        h_g1_seq = h_g1.seq,
        c_g1_seq = c_g1.seq,
    );
    std::fs::write(tex_dir.join("main.tex"), tex)?;
    std::fs::write(
        tex_dir.join("refs.bib"),
        "@article{smith2020,\n  doi = {10.1000/xyz123},\n}\n",
    )?;
    store.append(NewEvent::manuscript_registered(
        m_g.id,
        &tex_dir.display().to_string(),
        "main.tex",
    )?)?;
    store.append(NewEvent::manuscript_compiled(
        m_g.id,
        manuscript::CompileOutcome::Ok,
        Some("latexmk"),
        "Output written on main.pdf (12 pages, 250000 bytes).",
    )?)?;
    appended += 2;

    // -- The venue-tier-2 flight: fit finder -> submission checklist --------
    // The simulated fit ranking (siam-jsc tops the list).
    store.append(NewEvent::journal_fit_completed(
        &run("fit"),
        Some(m_g.id),
        "simulated",
        "simulated",
        &[
            FitCandidate {
                venue_id: "siam-jsc".into(),
                score: 92,
                rationale: "numerical analysis and HPC fit the scope".into(),
                refs: vec![format!("H-{}", h_g1.seq)],
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
        ],
    )?)?;
    appended += 1;
    // The checklist mission (a mission like any other, FR-19.4).
    let venue = journals::venue("siam-jsc").expect("siam-jsc is a venue");
    let sub = store.append(NewEvent::submission_created(SubmissionCreatedPayload {
        question: format!("Submit to {}", venue.name),
        stop_condition: "submission-ready".into(),
        success_criterion: "every checklist item is checked".into(),
        venue_id: venue.id.clone(),
        source_mission_id: Some(m_g.id),
    })?)?;
    missions_n += 1;
    appended += 1;
    // The machine items that PASS pre-check (through the owner's own check
    // command path — the demo is a user-commanded seed, actor=user); the
    // human-only items stay flagged and unchecked.
    let events = store.events_all()?;
    let (scans, stats, refs) = {
        let registered = ManuscriptsProjection::for_mission(&events, m_g.id)?
            .expect("the demo manuscript is registered");
        (
            vec![manuscript::build_scan(&registered)],
            vec![journals::build_venue_stats(&registered)],
            LibraryProjection::fold(conn, &events)?,
        )
    };
    let tier_two =
        readiness::tier_two_report(&events, Some(m_g.id), venue, &scans, &stats, &refs, &[])?;
    let passing: Vec<String> = tier_two
        .items
        .iter()
        .filter(|i| i.status == TierTwoStatus::Pass)
        .map(|i| i.criterion_id.clone())
        .collect();
    for item_id in &passing {
        store.append(NewEvent::submission_item_checked(
            sub.id,
            &venue.id,
            item_id,
            Some("machine check passed"),
        )?)?;
        appended += 1;
    }

    // -- The captured draft: a pending card from the mobile surface ---------
    store.append(NewEvent::mission_quick_capture(
        "Does the erratum change the verdict?",
        "mobile",
    )?)?;
    missions_n += 1;
    appended += 1;

    // -- The marker, LAST: this generation is complete ----------------------
    let marker = store.append(NewEvent::setup_demo_seeded(generation)?)?;
    appended += 1;

    Ok(DemoSeedOutcome {
        generation,
        events_appended: appended,
        marker_seq: marker.seq,
        missions: missions_n,
        hypotheses: hypotheses_n,
        claims: claims_n,
        refs: refs_n,
        proposals: proposals_n,
        searches: searches_n,
    })
}

// ---------------------------------------------------------------------------
// The shell command
// ---------------------------------------------------------------------------

/// Populate this workspace with sample data (the user-testing harness):
/// append one rich demo generation as REAL events — missions in every
/// lifecycle status, hypotheses across the board, pins with verification and
/// support, the quarantine, night-shift runs (a failed row, a dead-run alert,
/// a connection alert), spend (one ceiling refusal), readiness tier-1 both
/// ways, a tier-2 submission in flight, refs (one archived), and a null-result
/// search. Idempotent: refuses `already_seeded:` unless `force` appends a new
/// generation. History is never mutated.
#[tauri::command]
pub async fn seed_demo_workspace(
    db: State<'_, Db>,
    paths: State<'_, AppPaths>,
    force: Option<bool>,
) -> Result<DemoSeedOutcome, String> {
    let c = db.0.lock().await;
    seed_demo(
        &c,
        &paths.data_dir.join("demo-manuscript"),
        force.unwrap_or(false),
    )
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tests: the inventory, the idempotence, the honesty
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spend::SPEND_RECORDED;
    use crate::domain::trust::SPEND_REFUSED;

    fn tmp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rc-demo-seed-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn seeded_conn() -> (rusqlite::Connection, std::path::PathBuf) {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        let dir = tmp_dir();
        (conn, dir)
    }

    #[test]
    fn the_demo_generation_covers_every_screen() {
        let (conn, dir) = seeded_conn();
        let outcome = seed_demo(&conn, &dir, false).unwrap();
        assert_eq!(outcome.generation, 1);
        assert!(outcome.events_appended > 80, "a rich generation: {}", outcome.events_appended);

        let events = EventStore::new(&conn).events_all().unwrap();
        let missions = MissionsProjection::fold(&events).unwrap();
        // Every lifecycle status is represented (draft included).
        for status in [
            MissionStatus::Active,
            MissionStatus::AwaitingReview,
            MissionStatus::Completed,
            MissionStatus::Stopped,
            MissionStatus::Failed,
            MissionStatus::Draft,
        ] {
            assert!(
                missions.iter().any(|m| m.status == status),
                "a demo mission is {status:?}"
            );
        }
        // The stopped mission is at its ceiling (blocked) with the refusal.
        let stopped = missions.iter().find(|m| m.status == MissionStatus::Stopped).unwrap();
        assert_eq!(stopped.spend_cents, 300);
        assert_eq!(stopped.spend_state, missions::SpendState::Blocked);
        assert!(events
            .iter()
            .any(|e| e.kind == SPEND_REFUSED && e.payload["mission_id"] == stopped.id.to_string()));
        // The spend rows exist and are attributable.
        assert!(events.iter().any(|e| e.kind == SPEND_RECORDED));

        // Hypotheses across the lifecycle, with relations.
        let hyps = HypothesesProjection::fold(&events).unwrap();
        for status in [
            HypothesisStatus::Proposed,
            HypothesisStatus::Testing,
            HypothesisStatus::Supported,
            HypothesisStatus::Refuted,
            HypothesisStatus::Revised,
        ] {
            assert!(hyps.iter().any(|h| h.status == status), "a demo hypothesis is {status:?}");
        }
        assert!(events.iter().any(|e| e.kind == "hypothesis.related"));

        // Claims: pinned (citation + numerical) and one unpinned; verification
        // verified + failed-then-verified; support supported/partially/
        // unsupported/unverifiable.
        let claims = EvidenceProjection::fold(&events).unwrap();
        assert!(claims.iter().any(|c| c.pinned && c.pin.as_ref().unwrap().kind == evidence::PinKind::Citation));
        assert!(claims.iter().any(|c| c.pinned && c.pin.as_ref().unwrap().kind == evidence::PinKind::Numerical));
        let unpinned: Vec<_> = claims.iter().filter(|c| !c.pinned).collect();
        assert_eq!(unpinned.len(), 1, "exactly one unpinned claim blocks readiness");
        let recovered = claims
            .iter()
            .map(|c| c.pin.as_ref().and_then(|p| p.verification.as_ref()))
            .flatten()
            .filter(|v| v.status == evidence::VerificationStatus::Verified)
            .count();
        assert!(recovered >= 3, "verified pins exist: {recovered}");
        for verdict in [
            SupportVerdict::Supported,
            SupportVerdict::Partially,
            SupportVerdict::Unsupported,
            SupportVerdict::Unverifiable,
        ] {
            assert!(
                claims
                    .iter()
                    .filter_map(|c| c.pin.as_ref())
                    .filter_map(|p| p.support.as_ref())
                    .any(|s| s.status == support::SupportStatus::from_verdict(verdict)),
                "a demo support verdict is {verdict:?}"
            );
        }

        // The quarantine: one pending, one rejected, one merged.
        let proposals = ProposalsProjection::fold(&events).unwrap();
        assert!(proposals.iter().any(|p| p.status == ProposalStatus::Pending));
        assert!(proposals.iter().any(|p| p.status == ProposalStatus::Rejected));
        assert!(proposals.iter().any(|p| p.status == ProposalStatus::Merged));

        // The digest: rows including a FAILED row, a dead-run alert, a
        // connection alert.
        let digest = digest::render_digest(&events, Utc::now()).unwrap();
        assert!(!digest.rows.is_empty());
        assert_eq!(digest.outcome, digest::DigestOutcome::PartialSuccess);
        assert!(
            digest.rows.iter().any(|r| r.failed > 0 && r.failure_reason.as_deref() == Some("provider_error")),
            "a failed digest row renders honestly"
        );
        assert_eq!(digest.alerts.len(), 1, "the dead-run alert row");
        assert!(
            digest.connection_alerts.iter().any(|a| a.connection == "zotero"),
            "the zotero connection alert"
        );

        // Readiness tier-1: one not-ready (unpinned + load-bearing blockers)
        // and one preprint-ready.
        let not_ready_mission = missions
            .iter()
            .find(|m| m.question.starts_with("Does retrieval-augmented grounding"))
            .unwrap();
        let report = readiness::readiness_report(&events, Some(not_ready_mission.id)).unwrap();
        assert_eq!(report.verdict, ReadinessVerdict::NotReady);
        assert!(report
            .blockers
            .iter()
            .any(|b| b.kind == ReadinessItemKind::UnpinnedClaim));
        assert!(report
            .blockers
            .iter()
            .any(|b| b.kind == ReadinessItemKind::LoadBearingUnresolved));
        let ready_mission = missions
            .iter()
            .find(|m| m.question.starts_with("Does sparse attention match"))
            .unwrap();
        let ready = readiness::readiness_report(&events, Some(ready_mission.id)).unwrap();
        assert_eq!(ready.verdict, ReadinessVerdict::Ready, "the clean mission is preprint-ready");

        // The tier-2 submission in flight: machine items pre-checked, human
        // items flagged unchecked.
        let subs = SubmissionProjection::fold(&events).unwrap();
        assert_eq!(subs.len(), 1);
        let sub = &subs[0];
        assert_eq!(sub.venue_id, "siam-jsc");
        let prechecked = sub.items.iter().filter(|i| i.checked.is_some()).count();
        let human = sub.items.iter().filter(|i| i.human).count();
        assert!(prechecked > 0, "machine items are pre-checked");
        assert!(human > 0, "human items exist");
        assert!(sub
            .items
            .iter()
            .filter(|i| i.human)
            .all(|i| i.checked.is_none()),
            "human items are flagged, never pre-checked");
        assert!(!sub.all_checked(), "the flight is in progress, not done");

        // The library: evented refs, one archived (removed).
        let refs = LibraryProjection::fold(&conn, &events).unwrap();
        assert!(refs.iter().filter(|r| r.tags == "demo").count() >= 5);
        assert!(refs.iter().any(|r| r.removed), "one archived ref");

        // The searches: one with results, one null (logged identically).
        let searches: Vec<&StoredEvent> =
            events.iter().filter(|e| e.kind == "search.run").collect();
        assert!(searches.iter().any(|e| e.payload["null_result"] == serde_json::json!(true)));
        assert!(searches
            .iter()
            .any(|e| e.payload["result_count"].as_u64().unwrap_or(0) > 0));

        // The heartbeats and connection telemetry exist.
        assert!(events.iter().any(|e| e.kind == "run.heartbeat"));
        let health = telemetry::connection_health(&events);
        assert!(health.iter().any(|h| h.connection == "zotero" && !h.up));
        assert!(health.iter().any(|h| h.connection == "arxiv" && h.up));

        // The marker exists exactly once, last.
        let markers: Vec<&StoredEvent> =
            events.iter().filter(|e| e.kind == DEMO_SEEDED).collect();
        assert_eq!(markers.len(), 1);
        assert_eq!(events.last().unwrap().kind, DEMO_SEEDED);
        assert_eq!(markers[0].payload["generation"], serde_json::json!(1));
        assert_eq!(markers[0].seq, outcome.marker_seq);

        // Deterministic replay: folding twice yields identical state.
        assert_eq!(
            MissionsProjection::fold(&events).unwrap(),
            MissionsProjection::fold(&events).unwrap()
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_seed_is_idempotent_and_force_appends_a_new_generation() {
        let (conn, dir) = seeded_conn();
        let first = seed_demo(&conn, &dir, false).unwrap();
        let head = EventStore::new(&conn).head_seq().unwrap();
        let total = EventStore::new(&conn).events_all().unwrap().len();

        // Without force: the typed refusal, nothing appended.
        let refusal = seed_demo(&conn, &dir, false).unwrap_err();
        assert!(refusal.to_string().starts_with("already_seeded:"), "{refusal}");
        assert_eq!(EventStore::new(&conn).head_seq().unwrap(), head);
        assert_eq!(EventStore::new(&conn).events_all().unwrap().len(), total);

        // With force: a NEW generation appends; history is never mutated.
        let second = seed_demo(&conn, &dir, true).unwrap();
        assert_eq!(second.generation, 2);
        assert!(second.events_appended > 0);
        let events = EventStore::new(&conn).events_all().unwrap();
        assert!(events.len() > total, "the new generation appended");
        // The first generation's events are intact (prefix preserved).
        for (i, event) in events[..total].iter().enumerate() {
            assert_eq!(event.seq, (i + 1) as i64);
        }
        // Two markers, one per generation, the last one carrying generation 2.
        let markers: Vec<&StoredEvent> =
            events.iter().filter(|e| e.kind == DEMO_SEEDED).collect();
        assert_eq!(markers.len(), 2);
        assert_eq!(events.last().unwrap().payload["generation"], serde_json::json!(2));
        // The read models see both generations' missions.
        let missions = MissionsProjection::fold(&events).unwrap();
        assert!(missions.len() >= 18, "two generations of missions: {}", missions.len());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn no_secret_ever_enters_the_demo_log() {
        let (conn, dir) = seeded_conn();
        seed_demo(&conn, &dir, false).unwrap();
        for event in EventStore::new(&conn).events_all().unwrap() {
            let whole = serde_json::to_string(&event).unwrap();
            for marker in ["sk-", "api_key", "password", "secret"] {
                assert!(
                    !whole.contains(marker),
                    "event {} ({}) carries `{marker}`",
                    event.seq,
                    event.kind
                );
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
