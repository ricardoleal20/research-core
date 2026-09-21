// Dashboard shell commands (AD-15a, Story 5.10, FR-18): the home panel's
// ONE read — a pure composition over the EXISTING read models (FR-18.1,
// AD-1). No new domain model, no new writes: the fold reads the log once
// and feeds the same projections the individual reads serve (missions,
// boards, digest, trust, receipts, readiness), so the dashboard renders
// identically in the desktop app and the served browser view — and a
// dashboard render never appends an event (the widgets are derived views,
// FR-7/AD-1).
//
// Why one aggregated read instead of the frontend composing the individual
// reads: the composition needs listMissions + listHypotheses per mission +
// getMorningDigest + getTrustStatus + the run reads per mission +
// getReadinessReport — 4 + 2N round-trips (10+ with three missions). One
// read-only fold over one log read is cheaper and stays a pure query.

use chrono::Utc;
use rusqlite::Connection;
use tauri::State;

use crate::db::Db;
use crate::domain::checkpoints::FoldCursor;
use crate::domain::digest::{render_digest, MorningDigest};
use crate::domain::hypotheses::{Hypothesis, HypothesesProjection};
use crate::domain::missions::{Mission, MissionsProjection};
use crate::domain::nightshift::RUN_STARTED;
use crate::domain::readiness::{readiness_report, ReadinessReport};
use crate::domain::receipts::{render_receipt, RunReceipt};
use crate::eventstore::EventStore;
use crate::trust::{trust_status, TrustStatus};
use serde::Serialize;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// How many recent runs the receipts widget lists (FR-18.6): the last N
/// across missions, newest first — the drill-down opens the full ledger.
pub const RECENT_RUNS_LIMIT: usize = 5;

/// The dashboard's aggregated read (FR-18.1): every widget's data, folded
/// from ONE log read. A transport envelope, not a domain model — each field
/// is the exact read model the corresponding individual read serves.
#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSummary {
    /// Widget 1 (FR-18.2): every mission — status counts and the active
    /// shortlist with spend meters derive client-side.
    pub missions: Vec<Mission>,
    /// Widget 2 (FR-18.3): every board's hypotheses across missions, in
    /// creation `seq` order — lifecycle counts derive client-side.
    pub hypotheses: Vec<Hypothesis>,
    /// Widget 3 (FR-18.4): the morning digest, rendered for "now" — the
    /// same projection `/api/digest` serves.
    pub digest: MorningDigest,
    /// Widget 4 (FR-18.5): the trust read model — the meters the trust
    /// center renders (global + per-mission spend vs ceiling).
    pub trust: TrustStatus,
    /// Widget 5 (FR-18.6): the last `RECENT_RUNS_LIMIT` runs' receipts,
    /// newest first — one-line verdicts, mono ids, drill-down targets.
    pub recent_receipts: Vec<RunReceipt>,
    /// Widget 6 (FR-18.7): the workspace-scoped readiness report — the
    /// preprint-tier verdict + blockers, the same fold `/api/readiness`
    /// serves.
    pub readiness: ReadinessReport,
}

/// The dashboard read (Story 5.10, FR-18.1): one read-only fold over the
/// shared log at its current head — asking again re-folds; a render never
/// appends an event.
#[tauri::command]
pub async fn dashboard_summary(db: State<'_, Db>) -> Result<DashboardSummary, String> {
    let c = db.0.lock().await;
    dashboard_summary_inner(&c)
}

/// Plain inner (testable without Tauri state; the server route serves the
/// same read over the shared core): fold the summary off one log read.
pub(crate) fn dashboard_summary_inner(conn: &Connection) -> Result<DashboardSummary, String> {
    let events = EventStore::new(conn).events_all().map_err(err)?;
    dashboard_summary_fold(&events)
}

/// The pure fold (no IO): compose the six widgets over one event slice.
pub(crate) fn dashboard_summary_fold(events: &[crate::eventstore::StoredEvent]) -> Result<DashboardSummary, String> {
    let missions = MissionsProjection::fold(events).map_err(err)?;
    let hypotheses = HypothesesProjection::fold(events).map_err(err)?;
    let digest = render_digest(events, Utc::now()).map_err(err)?;
    let trust = trust_status(events).map_err(err)?;
    let readiness = readiness_report(events, None).map_err(err)?;

    // The recent-runs widget (FR-18.6): the last `RECENT_RUNS_LIMIT` live
    // `run.started` anchors, newest first — each rendered through the same
    // receipt fold the drill-down serves (AD-2: re-query replays).
    let cursor = FoldCursor::over(events);
    let live = cursor.live_owned(events);
    let mut started: Vec<(i64, String)> = live
        .iter()
        .filter(|e| e.kind == RUN_STARTED)
        .filter_map(|e| {
            e.payload
                .get("run_id")
                .and_then(serde_json::Value::as_str)
                .map(|run_id| (e.seq, run_id.to_string()))
        })
        .collect();
    started.sort_by(|a, b| b.0.cmp(&a.0));
    started.dedup_by(|a, b| a.1 == b.1);
    let mut recent_receipts = Vec::new();
    for (_, run_id) in started.into_iter().take(RECENT_RUNS_LIMIT) {
        if let Some(receipt) = render_receipt(events, &run_id).map_err(err)? {
            recent_receipts.push(receipt);
        }
    }

    Ok(DashboardSummary {
        missions,
        hypotheses,
        digest,
        trust,
        recent_receipts,
        readiness,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::missions::{Autonomy, MissionCreatedPayload, SPEND_RECORDED};
    use crate::domain::nightshift::SCAN_STEP;
    use crate::eventstore::{Actor, NewEvent, StoredEvent, SystemComponent};

    fn test_db() -> (Db, std::path::PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("rc-dashboard-cmd-test-{}.sqlite", uuid::Uuid::new_v4()));
        let db = Db::open(&path).unwrap();
        (db, path)
    }

    fn cleanup(path: &std::path::PathBuf) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    }

    /// Seed one mission with a hypothesis, one finished run, and recorded
    /// spend — enough surface for every widget. Returns the mission id.
    async fn seed(db: &Db, run_id: &str) -> uuid::Uuid {
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        let mission = store
            .append(NewEvent::mission_created(MissionCreatedPayload {
                question: "Does X hold up?".into(),
                stop_condition: "Stop after $5.".into(),
                success_criterion: "A blind rater agrees.".into(),
                autonomy: Autonomy::Suggest,
                spend_ceiling_cents: 500,
                schedule: "daily-03:00".into(),
                roles: vec![],
            }).unwrap())
            .unwrap();
        let mission_id = mission.id;
        let hypothesis = store
            .append(NewEvent::hypothesis_created("X holds.", mission_id).unwrap())
            .unwrap();
        // an unpinned claim — the readiness gate's blocker (the board is
        // not preprint-ready while a claim rides unpinned)
        store
            .append(NewEvent::claim_registered("X holds at 32k.", hypothesis.id, None).unwrap())
            .unwrap();
        store
            .append(
                NewEvent::run_started(run_id, mission_id, "daily-03:00", SCAN_STEP)
                    .unwrap()
                    .with_causes(vec![mission_id]),
            )
            .unwrap();
        store
            .append(
                NewEvent::run_finished(run_id, mission_id, "1 scan · X holds", 0)
                    .unwrap()
                    .with_causes(vec![mission_id]),
            )
            .unwrap();
        store
            .append(
                NewEvent::new(
                    SPEND_RECORDED,
                    Actor::System { component: SystemComponent::Telemetry },
                    serde_json::json!({ "mission_id": mission_id.to_string(), "cost_cents": 42 }),
                )
                .unwrap()
                .with_causes(vec![mission_id]),
            )
            .unwrap();
        mission_id
    }

    /// The fold composes all six widgets over one log read: the mission,
    /// its board's hypothesis, the digest row, the trust spend, the run's
    /// receipt, and the readiness blockers the unpinned board implies.
    #[tokio::test]
    async fn the_summary_folds_all_six_widgets_over_one_log_read() {
        let (db, path) = test_db();
        let mission_id = seed(&db, "dash-1").await;
        {
            let c = db.0.lock().await;
            let summary = dashboard_summary_inner(&c).unwrap();
            assert_eq!(summary.missions.len(), 1);
            assert_eq!(summary.missions[0].id, mission_id);
            assert_eq!(summary.hypotheses.len(), 1);
            assert_eq!(summary.hypotheses[0].mission_id, mission_id);
            assert!(summary.trust.global_spend_cents >= 42, "spend folds into the trust meter");
            assert_eq!(summary.recent_receipts.len(), 1);
            assert_eq!(summary.recent_receipts[0].run_id, "dash-1");
            assert_eq!(
                summary.recent_receipts[0].outcome,
                crate::domain::receipts::RunOutcome::Finished
            );
            assert_eq!(summary.digest.rows.len(), 1, "the finished run folds a digest row");
            // the board is not preprint-ready: the claim rides unpinned and
            // its hypothesis stays unresolved
            assert!(!summary.readiness.blockers.is_empty());
        }
        cleanup(&path);
    }

    /// A dashboard render NEVER appends an event (FR-18.1, NFR-9): the log
    /// head is byte-identical before and after the fold.
    #[tokio::test]
    async fn the_summary_is_read_only_no_events_appended() {
        let (db, path) = test_db();
        seed(&db, "dash-ro").await;
        {
            let c = db.0.lock().await;
            let before = EventStore::new(&c).events_all().unwrap();
            let _ = dashboard_summary_inner(&c).unwrap();
            let after = EventStore::new(&c).events_all().unwrap();
            assert_eq!(before.len(), after.len());
            assert_eq!(before, after);
        }
        cleanup(&path);
    }

    /// The receipts widget caps at `RECENT_RUNS_LIMIT`, newest first: six
    /// runs list the last five, latest run's receipt on top.
    #[tokio::test]
    async fn recent_receipts_cap_at_the_limit_newest_first() {
        let (db, path) = test_db();
        {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let mission = store
                .append(NewEvent::mission_created(MissionCreatedPayload {
                    question: "Six runs?".into(),
                    stop_condition: "Stop after $5.".into(),
                    success_criterion: "A blind rater agrees.".into(),
                    autonomy: Autonomy::Watch,
                    spend_ceiling_cents: 500,
                    schedule: "daily-03:00".into(),
                    roles: vec![],
                }).unwrap())
                .unwrap();
            for i in 1..=6 {
                let run_id = format!("run-{i}");
                store
                    .append(
                        NewEvent::run_started(&run_id, mission.id, "daily-03:00", SCAN_STEP)
                            .unwrap()
                            .with_causes(vec![mission.id]),
                    )
                    .unwrap();
                store
                    .append(
                        NewEvent::run_finished(&run_id, mission.id, "done", 0)
                            .unwrap()
                            .with_causes(vec![mission.id]),
                    )
                    .unwrap();
            }
        }
        {
            let c = db.0.lock().await;
            let summary = dashboard_summary_inner(&c).unwrap();
            assert_eq!(summary.recent_receipts.len(), RECENT_RUNS_LIMIT);
            assert_eq!(summary.recent_receipts[0].run_id, "run-6", "newest first");
            assert_eq!(
                summary.recent_receipts[RECENT_RUNS_LIMIT - 1].run_id,
                "run-2",
                "the oldest of the kept five"
            );
        }
        cleanup(&path);
    }

    /// The pure fold needs no database at all — an empty event slice folds
    /// an honest empty dashboard (every widget's empty state).
    #[test]
    fn an_empty_log_folds_an_empty_summary() {
        let events: Vec<StoredEvent> = Vec::new();
        let summary = dashboard_summary_fold(&events).unwrap();
        assert!(summary.missions.is_empty());
        assert!(summary.hypotheses.is_empty());
        assert!(summary.recent_receipts.is_empty());
        assert_eq!(
            summary.digest.outcome,
            crate::domain::digest::DigestOutcome::NoRuns
        );
    }
}
