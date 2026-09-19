// Night Shift scheduler (FR-4.1/4.2/4.3, Story 2.3): the overnight worker.
// On each tick it (1) reaps runs whose heartbeat went stale as honest
// `stale_heartbeat` failures (the FR-9.1 dead-man seam — Story 2.6 builds
// full detection), (2) runs every ACTIVE mission whose daily schedule is due
// and has not run yet today — one literature-scan step through the provider
// layer (AD-9), its output emitted as PROPOSALS through the Story 2.2
// quarantine seam (FR-4.2: nothing mutates the board unattended), and (3)
// evaluates every mission's terminators (AD-12) — so no mission rests
// without a terminal state once the evaluator can decide, including
// missions changed by the user since the last tick.
//
// A failed scan is a RESULT, not an error: the run.failed event lands with
// its reason and the digest still renders the honest row (FR-4.3). Only
// store failures propagate as errors.

use crate::db::Db;
use crate::domain::digest::{render_digest, MorningDigest};
use crate::domain::hypotheses::{HypothesisStatus, HypothesesProjection};
use crate::domain::missions::{
    Mission, MissionStatus, MissionsProjection, Schedule, ROLE_DRAFTER,
};
use crate::domain::nightshift::{
    evaluate_terminals, DEAD_RUN_AFTER_MINUTES, DEAD_RUN_REASON, RUN_FAILED, RUN_FINISHED,
    RUN_STARTED, SCAN_STEP,
};
use crate::eventstore::{EventStore, NewEvent, StoredEvent};
use crate::runtime::AgentRuntime;
use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Utc};
use serde::Serialize;
use std::collections::HashSet;
use thiserror::Error;
use uuid::Uuid;

/// Everything that can go wrong at the scheduler level — typed, never a bare
/// string. Run failures are NOT here: they are honest `run.failed` events.
#[derive(Debug, Error)]
pub enum NightShiftError {
    #[error(transparent)]
    Store(#[from] crate::eventstore::EventError),
}

/// One Night Shift run's outcome, as the tick reports it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub mission_id: Uuid,
    pub run_id: String,
    pub finished: bool,
    /// The one-line verdict (finished) or the failure reason (failed).
    pub detail: String,
    /// Proposals the run left in quarantine (FR-4.2).
    pub proposals: u32,
}

/// The Night Shift scheduler: owns no state — every decision re-folds the
/// log (AD-1), every mutation is an append. Shares the core `Db` handle with
/// the server shell and the Tauri commands (one process, one writer, AD-14).
pub struct NightShift {
    db: Db,
    runtime: AgentRuntime,
}

impl NightShift {
    pub fn new(db: Db) -> Self {
        Self { db: db.clone(), runtime: AgentRuntime::new(db) }
    }

    /// Test seam (test builds only — no dead code in release): a scheduler whose runtime resolves adapters through the
    /// injected resolver (same shape as `AgentRuntime`'s).
    #[cfg(test)]
    pub(crate) fn with_resolver(db: Db, resolver: crate::runtime::RoleResolver) -> Self {
        Self { db: db.clone(), runtime: AgentRuntime::with_resolver(db, resolver) }
    }

    /// One scheduled tick at `now` (local time): reap dead runs, run every
    /// due mission, then evaluate all terminators (the AD-12 catch-up —
    /// user-driven board changes get decided within a tick of landing).
    pub async fn tick(&self, now: DateTime<Local>) -> Result<Vec<RunRecord>, NightShiftError> {
        self.reap_dead_runs(now.with_timezone(&Utc)).await?;
        let missions = self.fold_missions().await?;
        let mut records = Vec::new();
        for mission in &missions {
            if mission.status != MissionStatus::Active {
                continue; // terminal missions sleep (AD-12)
            }
            if !self.is_due(mission, now).await? {
                continue;
            }
            records.push(self.run_scan(mission).await);
        }
        self.evaluate().await?;
        Ok(records)
    }

    /// The manual trigger (`run_night_shift_now`, FR-4.1 UX seam): run every
    /// ACTIVE mission's scan once, ignoring due-ness — an explicit user
    /// action, so even a mission scheduled `off` runs when asked directly.
    pub async fn run_all(&self) -> Result<Vec<RunRecord>, NightShiftError> {
        let missions = self.fold_missions().await?;
        let mut records = Vec::new();
        for mission in &missions {
            if mission.status != MissionStatus::Active {
                continue;
            }
            records.push(self.run_scan(mission).await);
        }
        self.evaluate().await?;
        Ok(records)
    }

    /// Is the mission's schedule due at `now`? Thin wrapper: the mission's
    /// latest `run.started` folds out of the log, the decision is pure.
    async fn is_due(&self, mission: &Mission, now: DateTime<Local>) -> Result<bool, NightShiftError> {
        let last_run = {
            let conn = self.db.0.lock().await;
            let events = EventStore::new(&conn).events_all()?;
            events
                .iter()
                .filter(|e| e.kind == RUN_STARTED && references_mission(e, mission.id))
                .map(|e| e.ts)
                .max()
        };
        Ok(schedule_due(&mission.schedule, last_run, now))
    }

    /// Reap runs whose heartbeat went stale: a `run.started` with no terminal
    /// run event older than the dead-run window becomes an honest
    /// `stale_heartbeat` failure (FR-9.1 seam — the digest's alert row
    /// renders from these; Story 2.6 builds the full switch).
    async fn reap_dead_runs(&self, now: DateTime<Utc>) -> Result<Vec<StoredEvent>, NightShiftError> {
        let stale = {
            let conn = self.db.0.lock().await;
            let events = EventStore::new(&conn).events_all()?;
            let mut terminal_runs: HashSet<String> = HashSet::new();
            for event in &events {
                if matches!(event.kind.as_str(), RUN_FINISHED | RUN_FAILED) {
                    if let Some(rid) = event.payload.get("run_id").and_then(|v| v.as_str()) {
                        terminal_runs.insert(rid.to_string());
                    }
                }
            }
            let cutoff = now - Duration::minutes(DEAD_RUN_AFTER_MINUTES);
            let stale: Vec<StoredEvent> = events
                .iter()
                .filter(|e| {
                    e.kind == RUN_STARTED
                        && e.ts < cutoff
                        && e.payload
                            .get("run_id")
                            .and_then(|v| v.as_str())
                            .is_some_and(|rid| !terminal_runs.contains(rid))
                })
                .cloned()
                .collect();
            stale
        };
        let mut reaped = Vec::new();
        for started in stale {
            let run_id = started
                .payload
                .get("run_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let Some(mission_id) = started
                .payload
                .get("mission_id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
                .or_else(|| started.causes.first().copied())
            else {
                continue;
            };
            // The heartbeat the run died at: its start — v1 runs report no
            // intermediate heartbeats (the telemetry seam lands with 2.6).
            let heartbeat = started.ts;
            let conn = self.db.0.lock().await;
            let store = EventStore::new(&conn);
            if let Some(ev) = NewEvent::run_failed(&run_id, mission_id, DEAD_RUN_REASON, heartbeat)
                .ok()
            {
                reaped.push(store.append(ev)?);
            }
        }
        Ok(reaped)
    }

    /// Run one mission's literature scan (FR-4.1/4.2): open the run, run the
    /// drafter's scan step through the provider layer, emit the output as a
    /// PROPOSAL through the quarantine seam, close the run — and evaluate
    /// the mission's terminators after the run's events (AD-12). A failed
    /// scan is an honest `run.failed` record, never a tick error (FR-4.3).
    async fn run_scan(&self, mission: &Mission) -> RunRecord {
        let run_id = format!("nightshift-{}", Uuid::new_v4().simple());
        {
            let conn = self.db.0.lock().await;
            let store = EventStore::new(&conn);
            let opened = NewEvent::run_started(
                &run_id,
                mission.id,
                &mission.schedule,
                SCAN_STEP,
            );
            if let Err(e) = opened.and_then(|ev| store.append(ev)) {
                return RunRecord {
                    mission_id: mission.id,
                    run_id,
                    finished: false,
                    detail: format!("store_error: {e}"),
                    proposals: 0,
                };
            }
        }
        let task = format!(
            "Escaneo nocturno de literatura sobre: «{}». Busca fuentes nuevas, resume los \
             hallazgos en una línea y propón el siguiente paso para el tablero.",
            mission.question
        );
        let record = match self.runtime.run_step(mission.id, ROLE_DRAFTER, &task).await {
            Ok(step) => {
                let verdict = one_line(&step.content);
                let proposals = self.emit_proposal(mission.id, &run_id, &verdict).await;
                {
                    let conn = self.db.0.lock().await;
                    let store = EventStore::new(&conn);
                    let closed = NewEvent::run_finished(&run_id, mission.id, &verdict, proposals);
                    // the run finished; if its receipt cannot land, surface
                    // that honestly in the record
                    if let Err(e) = closed.and_then(|ev| store.append(ev)) {
                        return RunRecord {
                            mission_id: mission.id,
                            run_id,
                            finished: true,
                            detail: one_line(&format!("store_error: {e}")),
                            proposals,
                        };
                    }
                }
                RunRecord { mission_id: mission.id, run_id, finished: true, detail: verdict, proposals }
            }
            Err(e) => {
                let reason = reason_code(&e);
                let died_at = Utc::now();
                {
                    let conn = self.db.0.lock().await;
                    let store = EventStore::new(&conn);
                    if let Ok(ev) = NewEvent::run_failed(&run_id, mission.id, &reason, died_at) {
                        let _ = store.append(ev);
                    }
                }
                RunRecord { mission_id: mission.id, run_id, finished: false, detail: reason, proposals: 0 }
            }
        };
        // AD-12: the run's events (and its spend) may have decided the
        // mission — no mission rests without a terminal once decidable.
        let _ = self.evaluate().await;
        record
    }

    /// Emit the scan's output as a proposal through the Story 2.2 seam
    /// (FR-4.2: AD-3 — the board never mutates unattended). The first
    /// hypothesis with a legal next transition gets one proposal; nothing to
    /// transition means zero proposals (an honest empty night). Best-effort:
    /// a refused proposal (e.g. quarantine rejected it) is not a run failure.
    async fn emit_proposal(&self, mission_id: Uuid, run_id: &str, verdict: &str) -> u32 {
        let hyps = {
            let conn = self.db.0.lock().await;
            let events = match EventStore::new(&conn).events_all() {
                Ok(events) => events,
                Err(_) => return 0,
            };
            match HypothesesProjection::fold_for(&events, mission_id) {
                Ok(hyps) => hyps,
                Err(_) => return 0,
            }
        };
        let basis = format!("escaneo nocturno {run_id}: {verdict}");
        for hyp in &hyps {
            let Some(to) = next_transition(hyp.status) else {
                continue;
            };
            if self
                .runtime
                .propose_transition(mission_id, hyp.id, to.as_str(), &basis, run_id)
                .await
                .is_ok()
            {
                return 1;
            }
        }
        0
    }

    async fn fold_missions(&self) -> Result<Vec<Mission>, NightShiftError> {
        let conn = self.db.0.lock().await;
        let events = EventStore::new(&conn).events_all()?;
        Ok(MissionsProjection::fold(&events)?)
    }

    /// The AD-12 evaluation pass (also the catch-up for user-driven changes).
    async fn evaluate(&self) -> Result<(), NightShiftError> {
        let conn = self.db.0.lock().await;
        let store = EventStore::new(&conn);
        evaluate_terminals(&store)?;
        Ok(())
    }
}

/// Pure due-ness (FR-4.1): is a daily schedule due at `now`, given the
/// mission's latest run? Due means the schedule's local time passed today
/// AND the mission has not run yet today (one scheduled run per local day —
/// a run at 02:00 from a manual trigger satisfies the 03:00 scan). `off`
/// (or an unparseable schedule) is never due; an ambiguous/nonexistent
/// local time (DST) never fires.
fn schedule_due(schedule: &str, last_run: Option<DateTime<Utc>>, now: DateTime<Local>) -> bool {
    let Some(Schedule::Daily { hour, minute }) = Schedule::parse(schedule) else {
        return false;
    };
    let Some(due_at) = now
        .timezone()
        .with_ymd_and_hms(now.year(), now.month(), now.day(), hour, minute, 0)
        .single()
    else {
        return false;
    };
    if now < due_at {
        return false; // not yet today
    }
    match last_run {
        Some(ts) => ts.with_timezone(&now.timezone()).date_naive() != now.date_naive(),
        None => true,
    }
}

/// Does this event reference the given mission (payload id, then causes)?
fn references_mission(event: &StoredEvent, mission_id: Uuid) -> bool {
    event.causes.contains(&mission_id)
        || event
            .payload
            .get("mission_id")
            .and_then(|v| v.as_str())
            .map(|s| s == mission_id.to_string())
            .unwrap_or(false)
}

/// The first legal next transition of a hypothesis status (FR-2.2 table) —
/// the conservative step a night scan proposes.
fn next_transition(from: HypothesisStatus) -> Option<HypothesisStatus> {
    use HypothesisStatus::*;
    let to = match from {
        Proposed => Testing,
        Testing => Supported,
        Supported => Revised,
        Refuted => Revised,
        Revised => Testing,
    };
    crate::domain::hypotheses::transition_allowed(from, to).then_some(to)
}

/// The one-line verdict of a scan's content: its first non-empty line,
/// truncated at a word boundary so a digest row never wraps past its budget.
fn one_line(content: &str) -> String {
    const MAX: usize = 140;
    let line = content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("sin hallazgos");
    if line.chars().count() <= MAX {
        return line.to_string();
    }
    let truncated: String = line.chars().take(MAX).collect();
    match truncated.rsplit_once(' ') {
        Some((head, _)) => format!("{head}…"),
        None => format!("{truncated}…"),
    }
}

/// A run failure's reason, in code form — bilingual-safe by construction
/// (EXPERIENCE.md: error strings stay in code form).
fn reason_code(e: &crate::runtime::RuntimeError) -> String {
    use crate::runtime::RuntimeError;
    match e {
        RuntimeError::Provider(_) => "provider_error".into(),
        RuntimeError::NotFound(_) => "mission_not_found".into(),
        RuntimeError::UnknownRole { .. } => "unknown_role".into(),
        RuntimeError::HypothesisNotOnMission { .. } => "hypothesis_not_on_mission".into(),
        RuntimeError::EmptyTask | RuntimeError::EmptyBasis | RuntimeError::EmptyRunId => {
            "empty_input".into()
        }
        _ => "runtime_error".into(),
    }
}

/// The morning digest over the shared core (read side, AD-8): the same
/// projection the `/api/digest` route and the `get_morning_digest` command
/// return — rendered for "now".
pub async fn morning_digest(db: &Db) -> Result<MorningDigest, crate::eventstore::EventError> {
    let conn = db.0.lock().await;
    let events = EventStore::new(&conn).events_all()?;
    render_digest(&events, Utc::now())
}

/// Spawn the scheduler on the async runtime the Tauri app already runs on
/// (same pattern as the server shell — plain `tokio::spawn` panics here).
/// One tick per minute: due-ness is idempotent (a mission that already ran
/// today is skipped), so the cadence only bounds how soon after 03:00 a
/// scan starts, never how many run. A failed tick is logged and retried on
/// the next beat — the scheduler never takes the app down with it.
pub fn spawn(db: Db) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let shift = NightShift::new(db.clone());
            if let Err(e) = shift.tick(Local::now()).await {
                eprintln!("[nightshift] tick failed: {e}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::providers::{fake_remote_layer, ProviderError, ProviderLayer, Usage};
    use crate::domain::hypotheses::HypothesesProjection;
    use crate::domain::missions::{
        Autonomy, MissionCreatedPayload, MISSION_COMPLETED, MISSION_FAILED, MISSION_STOPPED,
    };
    use crate::domain::proposals::{ProposalStatus, ProposalsProjection};
    use crate::eventstore::Actor;
    use rusqlite::Connection;
    use serde_json::json;

    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::Db::migrate(&conn).unwrap();
        EventStore::init(&conn).unwrap();
        Db(std::sync::Arc::new(tokio::sync::Mutex::new(conn)))
    }

    async fn create_mission(
        db: &Db,
        schedule: &str,
        ceiling: u64,
    ) -> crate::eventstore::StoredEvent {
        let conn = db.0.lock().await;
        EventStore::new(&conn)
            .append(
                NewEvent::mission_created(MissionCreatedPayload {
                    question: "Does retrieval grounding reduce hallucinated citations?".into(),
                    stop_condition: "Stop after 3 rounds or $5.00 spent.".into(),
                    success_criterion: "A blind rater finds zero fabricated citations.".into(),
                    autonomy: Autonomy::Suggest,
                    spend_ceiling_cents: ceiling,
                    roles: vec![
                        crate::domain::missions::RoleConfig::drafter("simulated", "simulated"),
                        crate::domain::missions::RoleConfig::critic("simulated", "simulated"),
                    ],
                    schedule: schedule.into(),
                })
                .unwrap(),
            )
            .unwrap()
    }

    async fn events_of(db: &Db, kind: &str) -> Vec<StoredEvent> {
        let conn = db.0.lock().await;
        EventStore::new(&conn)
            .events_all()
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == kind)
            .collect()
    }

    /// The night-shift fake: a no-network remote layer answering with a
    /// one-line finding, so the finished path (spend, receipts, proposals)
    /// runs end to end without network.
    fn fake_resolver(
        db: &Db,
        _conn: &Connection,
        role: &crate::domain::missions::RoleConfig,
    ) -> Result<ProviderLayer, ProviderError> {
        Ok(fake_remote_layer(
            db,
            &role.provider,
            &role.model,
            "Hallazgo: tres fuentes nuevas anclan la hipótesis central; el barrido sigue abierto.",
            Usage { input_tokens: 300, output_tokens: 120 },
        ))
    }

    fn fake_shift(db: &Db) -> NightShift {
        NightShift::with_resolver(db.clone(), fake_resolver)
    }

    /// A resolver whose every adapter call fails at the provider boundary —
    /// the failed-run path, end to end.
    fn failing_shift(db: &Db) -> NightShift {
        NightShift::with_resolver(db.clone(), |_db, _conn, _role| {
            Err(ProviderError::Api {
                name: "test".into(),
                status: 503,
                body: "connection refused by test".into(),
            })
        })
    }

    /// A schedule whose local time has already passed today — the tick test
    /// runs against the real clock, so daily-00:00 is always due and the
    /// run's real timestamp always counts as "today" (no wall-clock flakes).
    fn now_local() -> DateTime<Local> {
        Local::now()
    }

    #[test]
    fn schedule_due_is_pure_and_decides_the_daily_tick() {
        let now = Local.with_ymd_and_hms(2026, 9, 19, 3, 30, 0).unwrap();
        // not yet due today
        assert!(!schedule_due("daily-03:00", None, now - Duration::hours(1)));
        // due, never ran
        assert!(schedule_due("daily-03:00", None, now));
        // ran today (any time today) — one scheduled run per local day
        assert!(!schedule_due("daily-03:00", Some(now.with_timezone(&Utc) - Duration::minutes(25)), now));
        assert!(!schedule_due("daily-00:00", Some(now.with_timezone(&Utc) - Duration::hours(3)), now));
        // ran yesterday — due again today
        let yesterday = (now - Duration::days(1)).with_timezone(&Utc);
        assert!(schedule_due("daily-03:00", Some(yesterday), now));
        // off is never due; unparseable schedules never fire
        assert!(!schedule_due("off", None, now));
        assert!(!schedule_due("whenever", None, now));
    }

    #[tokio::test]
    async fn a_due_mission_runs_its_scan_and_lands_a_quarantined_proposal() {
        let db = test_db();
        let mission = create_mission(&db, "daily-00:00", 500).await;
        // the board grows a hypothesis for the scan to propose on
        {
            let conn = db.0.lock().await;
            EventStore::new(&conn)
                .append(NewEvent::hypothesis_created("El método reduce citas alucinadas.", mission.id).unwrap())
                .unwrap();
        }
        let shift = fake_shift(&db);
        let records = shift.tick(now_local()).await.unwrap();
        assert_eq!(records.len(), 1, "one due mission, one run");
        assert!(records[0].finished, "the scan finished: {:?}", records[0]);

        // the run lifecycle drove the digest (FR-4.1 → FR-4.4)
        assert_eq!(events_of(&db, RUN_STARTED).await.len(), 1);
        assert_eq!(events_of(&db, RUN_FINISHED).await.len(), 1);
        assert!(events_of(&db, RUN_FAILED).await.is_empty());
        let digest = morning_digest(&db).await.unwrap();
        assert_eq!(digest.rows.len(), 1);
        assert_eq!(digest.rows[0].mission_seq, mission.seq);
        assert_eq!(digest.rows[0].runs, 1);
        assert_eq!(digest.rows[0].finished, 1);

        // FR-4.2 / AD-3: the scan's output is a PROPOSAL in quarantine —
        // the board did not move
        let proposals = {
            let conn = db.0.lock().await;
            let events = EventStore::new(&conn).events_all().unwrap();
            ProposalsProjection::fold_for(&events, mission.id).unwrap()
        };
        assert_eq!(proposals.len(), 1, "one quarantined proposal");
        assert_eq!(proposals[0].status, ProposalStatus::Pending);
        assert_eq!(proposals[0].run_id, records[0].run_id);
        let board = {
            let conn = db.0.lock().await;
            let events = EventStore::new(&conn).events_all().unwrap();
            HypothesesProjection::fold_for(&events, mission.id).unwrap()
        };
        assert_eq!(board[0].status, HypothesisStatus::Proposed, "nothing mutates the board unattended");

        // a second tick the same day does not re-run the mission
        let records = shift.tick(now_local()).await.unwrap();
        assert!(records.is_empty(), "already ran today — no double run");
    }

    #[tokio::test]
    async fn a_failed_scan_records_an_honest_run_failed_and_the_digest_still_renders() {
        let db = test_db();
        let mission = create_mission(&db, "daily-00:00", 500).await;
        let shift = failing_shift(&db);
        let records = shift.tick(now_local()).await.unwrap();
        // the failed run is a record, not a tick error (FR-4.3)
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].mission_id, mission.id);
        assert!(!records[0].finished);
        assert_eq!(records[0].detail, "provider_error");
        assert_eq!(events_of(&db, RUN_STARTED).await.len(), 1);
        let failures = events_of(&db, RUN_FAILED).await;
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].payload["reason"], json!("provider_error"));
        assert!(events_of(&db, RUN_FINISHED).await.is_empty());
        // the digest is delivered anyway, with the honest row
        let digest = morning_digest(&db).await.unwrap();
        assert_eq!(digest.rows.len(), 1);
        assert_eq!(digest.rows[0].failure_reason.as_deref(), Some("provider_error"));
        assert_eq!(digest.outcome, crate::domain::digest::DigestOutcome::AllFailed);
    }

    #[tokio::test]
    async fn an_off_schedule_or_before_time_mission_never_runs_and_terminal_missions_sleep() {
        let db = test_db();
        let off = create_mission(&db, "off", 500).await;
        let later = create_mission(&db, "daily-23:30", 500).await;
        let shift = fake_shift(&db);
        let records = shift.tick(now_local()).await.unwrap();
        assert!(records.is_empty(), "off missions never run");
        assert!(events_of(&db, RUN_STARTED).await.is_empty());
        let _ = (off.id, later.id);

        // a terminal mission sleeps even when its schedule is due
        let terminal = create_mission(&db, "daily-00:00", 500).await;
        {
            let conn = db.0.lock().await;
            EventStore::new(&conn)
                .append(
                    NewEvent::new(MISSION_STOPPED, Actor::User, json!({ "reason": "user stopped" }))
                        .unwrap()
                        .with_causes(vec![terminal.id]),
                )
                .unwrap();
        }
        let records = shift.tick(now_local()).await.unwrap();
        assert!(records.is_empty(), "terminal missions sleep (AD-12)");
    }

    #[tokio::test]
    async fn a_stale_run_is_reaped_as_a_dead_run_and_the_digest_alerts() {
        let db = test_db();
        let mission = create_mission(&db, "daily-03:00", 500).await;
        // a run started 2 hours ago with no terminal — dead (window: 30
        // min); both the run's ts and the tick run on the real clock, so
        // the test never depends on the time of day it runs at
        let started_at = Utc::now() - Duration::hours(2);
        let started = {
            let conn = db.0.lock().await;
            let mut event = NewEvent::run_started("nightshift-dead", mission.id, "daily-03:00", SCAN_STEP)
                .unwrap();
            event.ts = started_at;
            EventStore::new(&conn).append(event).unwrap()
        };
        let shift = fake_shift(&db);
        // the tick reaps; whether today's scheduled scan also fires does
        // not affect the assertions (the dead run already counts as today's)
        shift.tick(now_local()).await.unwrap();
        let failures = events_of(&db, RUN_FAILED).await;
        assert_eq!(failures.len(), 1, "the dead run is reaped");
        assert_eq!(failures[0].payload["reason"], json!(DEAD_RUN_REASON));
        assert_eq!(failures[0].payload["run_id"], json!("nightshift-dead"));
        // the digest renders the dead-man alert row (FR-9.1 hook)
        let digest = morning_digest(&db).await.unwrap();
        assert_eq!(digest.alerts.len(), 1);
        assert_eq!(digest.alerts[0].run_id, "nightshift-dead");
        assert_eq!(digest.alerts[0].mission_id, mission.id);
        assert_eq!(digest.alerts[0].receipt_seq, started.seq);
    }

    #[tokio::test]
    async fn the_tick_evaluates_terminals_a_mission_cannot_rest_without() {
        let db = test_db();
        // a settled board (supported) — the mission must complete on the tick
        let mission = create_mission(&db, "off", 500).await;
        {
            let conn = db.0.lock().await;
            let store = EventStore::new(&conn);
            let hyp = store
                .append(NewEvent::hypothesis_created("X holds.", mission.id).unwrap())
                .unwrap();
            for (from, to) in [
                (HypothesisStatus::Proposed, HypothesisStatus::Testing),
                (HypothesisStatus::Testing, HypothesisStatus::Supported),
            ] {
                store
                    .append(
                        NewEvent::hypothesis_status_changed(from, to, "night work")
                            .unwrap()
                            .with_causes(vec![hyp.id]),
                    )
                    .unwrap();
            }
        }
        let shift = fake_shift(&db);
        shift.tick(now_local()).await.unwrap();
        assert_eq!(events_of(&db, MISSION_COMPLETED).await.len(), 1, "AD-12: no resting without a terminal");
        // and never twice
        shift.tick(now_local()).await.unwrap();
        assert_eq!(events_of(&db, MISSION_COMPLETED).await.len(), 1);

        // the all-failed path: every run failed → mission.failed
        let db2 = test_db();
        create_mission(&db2, "daily-00:00", 500).await;
        let shift2 = failing_shift(&db2);
        shift2.tick(now_local()).await.unwrap();
        assert_eq!(events_of(&db2, MISSION_FAILED).await.len(), 1, "all runs failed → failed");
    }

    #[tokio::test]
    async fn run_all_runs_every_active_mission_regardless_of_schedule() {
        let db = test_db();
        let a = create_mission(&db, "off", 500).await;
        let b = create_mission(&db, "daily-23:30", 500).await;
        let shift = fake_shift(&db);
        let records = shift.run_all().await.unwrap();
        assert_eq!(records.len(), 2, "the manual trigger ignores due-ness");
        assert!(records.iter().all(|r| r.finished));
        assert_eq!(events_of(&db, RUN_STARTED).await.len(), 2);
        let _ = (a.id, b.id);
    }

    #[test]
    fn one_line_truncates_at_a_word_boundary() {
        assert_eq!(one_line("\n\n  first line  \nsecond"), "first line");
        assert_eq!(one_line("   "), "sin hallazgos");
        let long = "word ".repeat(60);
        let line = one_line(&long);
        assert!(line.chars().count() <= 141, "truncated: {}", line.chars().count());
        assert!(line.ends_with('…'));
        assert!(!line.contains('\n'));
    }
}
