// Night Shift scheduler (FR-4.1/4.2/4.3, Story 2.3): the overnight worker.
// On each tick it (1) probes the research connections and records
// connection.failed/restored transitions (FR-9.1 telemetry, Story 2.6), (2)
// reaps runs whose last heartbeat went past the dead-run threshold as
// `run.dead` alerts followed by honest `silently_dead` terminals (FR-9.1 —
// no run ends silently), (3) runs every ACTIVE mission whose daily schedule
// is due and has not run yet today — one literature-scan step through the
// provider layer (AD-9), its output emitted as PROPOSALS through the Story
// 2.2 quarantine seam (FR-4.2: nothing mutates the board unattended), and
// (4) evaluates every mission's terminators (AD-12) — so no mission rests
// without a terminal state once the evaluator can decide, including
// missions changed by the user since the last tick.
//
// A failed scan is a RESULT, not an error: the run.failed event lands with
// its reason and the digest still renders the honest row (FR-4.3). Only
// store failures propagate as errors.

use crate::db::Db;
use crate::domain::digest::{render_digest, MorningDigest};
use crate::domain::evidence::EvidenceProjection;
use crate::domain::hypotheses::{HypothesisStatus, HypothesesProjection};
use crate::domain::missions::{
    Mission, MissionStatus, MissionsProjection, Schedule, ROLE_DRAFTER,
};
use crate::domain::nightshift::{
    evaluate_terminals, RUN_FAILED, RUN_FINISHED, RUN_STARTED, SCAN_STEP,
};
use crate::domain::support::{SUPPORT_SWEEP_STEP, SupportStatus};
use crate::domain::telemetry::{
    record_probe, PROBED_CONNECTIONS, RUN_HEARTBEAT, SILENTLY_DEAD_REASON,
};
use crate::eventstore::{EventStore, NewEvent, StoredEvent};
use crate::runtime::AgentRuntime;
use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Utc};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

/// One connection probe's future — the health prober's shape (FR-9.1):
/// `Ok(())` = reachable, `Err(code)` = a code-form failure reason
/// (bilingual-safe by construction, EXPERIENCE.md).
pub type ProbeFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>;

/// The injectable health prober (same seam shape as the runtime's resolver):
/// the production prober exercises the real research connections; tests
/// inject deterministic outcomes.
pub type Prober = Arc<dyn Fn(&str) -> ProbeFuture + Send + Sync>;

/// The default prober: the three research connectors (Zotero, arXiv,
/// Semantic Scholar) over the mcp.rs REST/connector probes — any HTTP
/// answer means alive; a timeout or network error is a failure code.
fn default_prober() -> Prober {
    Arc::new(|name: &str| {
        let name = name.to_string();
        Box::pin(async move { crate::mcp::probe_connection(&name).await })
    })
}

/// Everything that can go wrong at the scheduler level — typed, never a bare
/// string. Run failures are NOT here: they are honest `run.failed` events.
#[derive(Debug, Error)]
pub enum NightShiftError {
    #[error(transparent)]
    Store(#[from] crate::eventstore::EventError),
    #[error("killed: the runtime is killed — the Night Shift refuses to dispatch until runtime.resumed (AD-15e)")]
    Killed,
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
    /// The adapter resolver the support sweep's judge dispatches through
    /// (Story 6.10) — the same seam shape as the runtime's, injectable so
    /// the sweep's tests never touch the network.
    resolver: crate::runtime::RoleResolver,
    /// The connection health prober (FR-9.1, Story 2.6) — injectable so the
    /// tick's telemetry tests never touch the network.
    prober: Prober,
}

impl NightShift {
    pub fn new(db: Db) -> Self {
        Self {
            db: db.clone(),
            runtime: AgentRuntime::new(db.clone()),
            resolver: crate::adapters::providers::ProviderLayer::for_role,
            prober: default_prober(),
        }
    }

    /// Test seam (test builds only — no dead code in release): a scheduler whose runtime resolves adapters through the
    /// injected resolver (same shape as `AgentRuntime`'s).
    #[cfg(test)]
    pub(crate) fn with_resolver(db: Db, resolver: crate::runtime::RoleResolver) -> Self {
        Self::with_resolver_and_prober(db, resolver, default_prober())
    }

    /// Test seam (test builds only): a scheduler with BOTH the runtime's
    /// resolver and the telemetry prober injected — deterministic scans and
    /// deterministic connection probes.
    #[cfg(test)]
    pub(crate) fn with_resolver_and_prober(
        db: Db,
        resolver: crate::runtime::RoleResolver,
        prober: Prober,
    ) -> Self {
        Self {
            db: db.clone(),
            runtime: AgentRuntime::with_resolver(db.clone(), resolver),
            resolver,
            prober,
        }
    }

    /// One scheduled tick at `now` (local time): probe the research
    /// connections, reap dead runs, run every due mission, then evaluate
    /// all terminators (the AD-12 catch-up — user-driven board changes get
    /// decided within a tick of landing). A killed runtime refuses the tick
    /// entirely (AD-15e): no probes, no reap, no scans, no evaluation —
    /// every dispatch is refused while `runtime.killed` is the latest
    /// runtime-state event by seq.
    pub async fn tick(&self, now: DateTime<Local>) -> Result<Vec<RunRecord>, NightShiftError> {
        if self.runtime_killed().await? {
            return Err(NightShiftError::Killed);
        }
        self.probe_connections().await?;
        self.reap_dead_runs(now.with_timezone(&Utc)).await?;
        // The compute-job monitor loop (Story 3.2, FR-11.4): every live job
        // gets one observation per tick — `job.running` on first sight,
        // reasoned terminals when jobs end (per command too, via
        // `poll_jobs`).
        crate::jobs_commands::poll_live_jobs(&self.db, None).await?;
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
        // The support sweep (Story 6.10, FR-23.3): after the scans, one
        // de-duped pass over every active mission's never-judged pins.
        self.sweep_support_checks().await?;
        self.evaluate().await?;
        Ok(records)
    }

    /// The manual trigger (`run_night_shift_now`, FR-4.1 UX seam): run every
    /// ACTIVE mission's scan once, ignoring due-ness — an explicit user
    /// action, so even a mission scheduled `off` runs when asked directly.
    /// Refused while the runtime is killed, like every dispatch (AD-15e).
    pub async fn run_all(&self) -> Result<Vec<RunRecord>, NightShiftError> {
        if self.runtime_killed().await? {
            return Err(NightShiftError::Killed);
        }
        let missions = self.fold_missions().await?;
        let mut records = Vec::new();
        for mission in &missions {
            if mission.status != MissionStatus::Active {
                continue;
            }
            records.push(self.run_scan(mission).await);
        }
        // The manual trigger re-verifies too (Story 6.10: re-verification
        // is explicit — manual or scheduled).
        self.sweep_support_checks().await?;
        self.evaluate().await?;
        Ok(records)
    }

    /// Is the runtime killed? (AD-15e — the latest runtime-state event by
    /// seq decides; `runtime.resumed` restores.)
    async fn runtime_killed(&self) -> Result<bool, NightShiftError> {
        let conn = self.db.0.lock().await;
        let events = EventStore::new(&conn).events_all()?;
        Ok(crate::domain::trust::trust_config(&events).runtime_killed)
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

    /// Probe the research connections (FR-9.1, Story 2.6) and record the
    /// state transitions — `connection.failed` when one dies,
    /// `connection.restored` when it heals. Transitions only: a 60s cadence
    /// never floods the log.
    async fn probe_connections(&self) -> Result<Vec<StoredEvent>, NightShiftError> {
        let mut appended = Vec::new();
        for name in PROBED_CONNECTIONS {
            let result = (self.prober)(name).await;
            let error_code = result.err().unwrap_or_default();
            let conn = self.db.0.lock().await;
            let store = EventStore::new(&conn);
            if let Some(event) = record_probe(&store, name, error_code.is_empty(), &error_code)? {
                appended.push(event);
            }
        }
        Ok(appended)
    }

    /// Reap runs whose last heartbeat went past the dead-run threshold
    /// (FR-9.1, Story 2.6 — the dead-man switch): a `run.started` with no
    /// terminal run event whose last `run.heartbeat` (falling back to its
    /// start, the pre-2.6 seam) is older than the threshold is silently
    /// dead — the runtime appends `run.dead` (the alert event, telemetry
    /// actor, last heartbeat + threshold) and THEN the terminal
    /// `run.failed(silently_dead)`, so every run reaches a terminal state
    /// with a timestamp and a reason. The threshold defaults to 30 minutes
    /// (2× the 15-minute scan cadence) and stays configurable here.
    async fn reap_dead_runs(&self, now: DateTime<Utc>) -> Result<Vec<StoredEvent>, NightShiftError> {
        let threshold = crate::domain::telemetry::DEAD_RUN_THRESHOLD_MINUTES;
        let stale = {
            let conn = self.db.0.lock().await;
            let events = EventStore::new(&conn).events_all()?;
            let cursor = crate::domain::checkpoints::FoldCursor::over(&events);
            let live: Vec<StoredEvent> = cursor.live_owned(&events);
            let mut terminal_runs: HashSet<String> = HashSet::new();
            let mut last_beat: HashMap<String, DateTime<Utc>> = HashMap::new();
            for event in &live {
                let Some(rid) = event.payload.get("run_id").and_then(|v| v.as_str()) else {
                    continue;
                };
                match event.kind.as_str() {
                    RUN_FINISHED | RUN_FAILED => {
                        terminal_runs.insert(rid.to_string());
                    }
                    RUN_HEARTBEAT => {
                        let beat = last_beat.entry(rid.to_string()).or_insert(event.ts);
                        if event.ts > *beat {
                            *beat = event.ts;
                        }
                    }
                    _ => {}
                }
            }
            let cutoff = now - Duration::minutes(threshold);
            live.iter()
                .filter(|e| {
                    e.kind == RUN_STARTED
                        && e.payload
                            .get("run_id")
                            .and_then(|v| v.as_str())
                            .is_some_and(|rid| {
                                !terminal_runs.contains(rid)
                                    && last_beat.get(rid).unwrap_or(&e.ts) < &cutoff
                            })
                })
                .cloned()
                .collect::<Vec<StoredEvent>>()
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
            // The heartbeat the run died at: its last run.heartbeat — or its
            // start, when it never managed one (the pre-2.6 seam).
            let heartbeat = {
                let conn = self.db.0.lock().await;
                let events = EventStore::new(&conn).events_all()?;
                crate::domain::telemetry::last_heartbeats(&events)
                    .get(&run_id)
                    .copied()
                    .unwrap_or(started.ts)
            };
            let conn = self.db.0.lock().await;
            let store = EventStore::new(&conn);
            // the alert event first (FR-4.3 amended)...
            if let Ok(ev) = NewEvent::run_dead(&run_id, mission_id, heartbeat, threshold) {
                reaped.push(store.append(ev)?);
            }
            // ...then the terminal: no run ends silently (FR-9.1)
            if let Ok(ev) = NewEvent::run_failed(&run_id, mission_id, SILENTLY_DEAD_REASON, heartbeat)
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
        let found: u32;
        {
            let conn = self.db.0.lock().await;
            let store = EventStore::new(&conn);
            let opened = NewEvent::run_started(
                &run_id,
                mission.id,
                &mission.schedule,
                SCAN_STEP,
            );
            let started = match opened.and_then(|ev| store.append(ev)) {
                Ok(started) => started,
                Err(e) => {
                    return RunRecord {
                        mission_id: mission.id,
                        run_id,
                        finished: false,
                        detail: format!("store_error: {e}"),
                        proposals: 0,
                    }
                }
            };
            // First heartbeat (FR-9.1, Story 2.6): the run is alive and
            // working. Best-effort — a heartbeat that cannot land does not
            // fail the run; the terminal event (or the reaper) still names
            // how it ended.
            if let Ok(beat) =
                NewEvent::run_heartbeat(&run_id, mission.id, started.seq, started.ts)
            {
                let _ = store.append(beat);
            }
            // FR-12.1 (Story 4.1): the scan's literature search runs
            // through the ONE search seam — `search::run_search` executes
            // the search, appends the `search.run` PRISMA record, and
            // returns the results, so the disclosure cannot be skipped. A
            // corpus that answers nothing is logged identically (null
            // results are first-class, never a hidden nothing).
            found = match crate::domain::search::run_search(
                &store,
                &crate::domain::search::SearchParams {
                    query: mission.question.clone(),
                    database: crate::domain::search::DATABASE_ARXIV.into(),
                    filters: Default::default(),
                    order: None,
                    first_page: true,
                    mission_id: Some(mission.id),
                    run_id: Some(run_id.clone()),
                },
            ) {
                Ok(outcome) => outcome.row.result_count,
                // the search record could not land — the run fails
                // honestly rather than search undisclosed
                Err(e) => {
                    return RunRecord {
                        mission_id: mission.id,
                        run_id,
                        finished: false,
                        detail: format!("store_error: {e}"),
                        proposals: 0,
                    }
                }
            };
        }
        let task = format!(
            "Escaneo nocturno de literatura sobre: «{}». El barrido inicial encontró {found} \
             fuente(s) candidata(s). Busca fuentes nuevas, resume los hallazgos en una línea y \
             propón el siguiente paso para el tablero.",
            mission.question
        );
        let record = match self.runtime.run_step(mission.id, ROLE_DRAFTER, &task).await {
            Ok(step) => {
                let verdict = one_line(&step.content);
                let proposals = self.emit_proposal(mission.id, &run_id, &verdict).await;
                {
                    let conn = self.db.0.lock().await;
                    let store = EventStore::new(&conn);
                    // Second heartbeat (FR-9.1): the step came back — the
                    // run is alive right up to its terminal event.
                    if let Ok(head) = store.head_seq() {
                        if let Ok(beat) =
                            NewEvent::run_heartbeat(&run_id, mission.id, head, Utc::now())
                        {
                            let _ = store.append(beat);
                        }
                    }
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

    /// The support sweep (Story 6.10, FR-23.3): for every ACTIVE mission
    /// with pins never judged for their current pin_seq, one `support-sweep`
    /// run — de-duped (a judged pin is never re-asked by a later sweep;
    /// re-verification is explicit), batched per mission, dispatched
    /// autonomously through the provider layer with the trust dispatch (the
    /// dial gates it; every call spend-evented, AD-10). A mission with
    /// nothing to judge costs no run and no spend. The run's one-line,
    /// code-form verdict lands in the morning digest with its receipts link
    /// (FR-4.4 discipline); a store failure is an honest `run.failed`.
    async fn sweep_support_checks(&self) -> Result<(), NightShiftError> {
        let missions = self.fold_missions().await?;
        for mission in &missions {
            if mission.status != MissionStatus::Active {
                continue; // terminal missions sleep (AD-12)
            }
            // De-dup gate: only missions with at least one UNCHECKED pin
            // (no support event for the current pin — None, or stale from a
            // re-pin) open a sweep run.
            let has_unchecked = {
                let conn = self.db.0.lock().await;
                let events = EventStore::new(&conn).events_all()?;
                let hyps = HypothesesProjection::fold(&events)?;
                let mission_hyps: HashSet<Uuid> =
                    hyps.iter().filter(|h| h.mission_id == mission.id).map(|h| h.id).collect();
                EvidenceProjection::fold(&events)?.iter().any(|c| {
                    mission_hyps.contains(&c.hypothesis_id)
                        && c.pin.as_ref().is_some_and(|pin| {
                            pin.support
                                .as_ref()
                                .is_none_or(|s| s.status == SupportStatus::Stale)
                        })
                })
            };
            if !has_unchecked {
                continue;
            }
            let run_id = format!("nightshift-support-{}", Uuid::new_v4().simple());
            {
                let conn = self.db.0.lock().await;
                let store = EventStore::new(&conn);
                let opened = NewEvent::run_started(
                    &run_id,
                    mission.id,
                    &mission.schedule,
                    SUPPORT_SWEEP_STEP,
                );
                if let Ok(started) = opened {
                    if let Ok(appended) = store.append(started) {
                        if let Ok(beat) = NewEvent::run_heartbeat(
                            &run_id,
                            mission.id,
                            appended.seq,
                            appended.ts,
                        ) {
                            let _ = store.append(beat);
                        }
                    }
                }
            }
            let swept = crate::support_commands::run_support_checks_inner(
                &self.db,
                &crate::support_commands::SupportScope::Mission(mission.id),
                true,   // de-dup: only pins never judged for their current pin_seq
                self.resolver,
                true,   // autonomous — the dial gates it (AD-15d)
            )
            .await;
            let conn = self.db.0.lock().await;
            let store = EventStore::new(&conn);
            match swept {
                Ok(summary) => {
                    let verdict = summary.verdict_line();
                    if let Ok(finished) =
                        NewEvent::run_finished(&run_id, mission.id, &verdict, 0)
                    {
                        let _ = store.append(finished);
                    }
                }
                Err(e) => {
                    // A store-level failure is an honest run.failed — the
                    // digest still renders the row (FR-4.3).
                    if let Ok(failed) =
                        NewEvent::run_failed(&run_id, mission.id, &format!("store_error: {e}"), Utc::now())
                    {
                        let _ = store.append(failed);
                    }
                }
            }
        }
        Ok(())
    }

    /// Emit the scan's output as a proposal through the Story 2.2 seam
    /// (FR-4.2: AD-3 — the board never mutates unattended). The first    /// (FR-4.2: AD-3 — the board never mutates unattended). The first
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
        // Story 2.4 (AD-12 interplay): a cost-ceiling refusal fails the run
        // with `cost_ceiling_reached` — the signal the terminal evaluator
        // recognizes as `mission.stopped`; a killed runtime fails it with
        // `runtime_killed`; a watch dial with `autonomy_watch`.
        RuntimeError::Trust(trust) => trust.run_reason(),
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
        NightShift::with_resolver_and_prober(db.clone(), fake_resolver, ok_prober())
    }

    /// A resolver whose every adapter call fails at the provider boundary —
    /// the failed-run path, end to end.
    fn failing_shift(db: &Db) -> NightShift {
        NightShift::with_resolver_and_prober(
            db.clone(),
            |_db, _conn, _role| {
                Err(ProviderError::Api {
                    name: "test".into(),
                    status: 503,
                    body: "connection refused by test".into(),
                })
            },
            ok_prober(),
        )
    }

    /// The telemetry test prober (Story 2.6): every connection answers Ok —
    /// the tick's probe pass records nothing. Connection-failure tests
    /// inject their own.
    fn ok_prober() -> Prober {
        Arc::new(|_name: &str| Box::pin(async { Ok(()) }))
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
    async fn the_scan_search_runs_through_the_one_seam_and_is_logged() {
        // FR-12.1 (Story 4.1): the scan's literature search cannot skip
        // the log — one `search.run` lands inside the run, attributed to
        // the run, caused by the mission, and the disclosure fold sees it.
        let db = test_db();
        let mission = create_mission(&db, "daily-00:00", 500).await;
        let shift = fake_shift(&db);
        let records = shift.tick(now_local()).await.unwrap();
        assert_eq!(records.len(), 1);
        let searches = events_of(&db, crate::domain::search::SEARCH_RUN).await;
        assert_eq!(searches.len(), 1, "the scan searched exactly once");
        let ev = &searches[0];
        // the PRISMA record: the mission's question, the database, the count
        assert_eq!(ev.payload["query"], mission.payload["question"]);
        assert_eq!(ev.payload["database"], json!("arxiv"));
        let count = ev.payload["result_count"].as_u64().unwrap();
        assert_eq!(ev.payload["null_result"], json!(count == 0));
        // attributed to the run (AD-2), caused by the mission
        let run_id = ev.payload["run_id"].as_str().unwrap().to_string();
        assert_eq!(run_id, records[0].run_id);
        assert_eq!(ev.actor, Actor::Agent { run_id });
        assert!(ev.causes.contains(&mission.id));
        // the disclosure fold discloses it for the mission
        let events = {
            let conn = db.0.lock().await;
            EventStore::new(&conn).events_all().unwrap()
        };
        let disclosure =
            crate::domain::search::search_disclosure(&events, Some(mission.id)).unwrap();
        assert_eq!(disclosure.total, 1);
        assert_eq!(disclosure.rows[0].database, "arxiv");
        assert_eq!(disclosure.rows[0].run_id.as_deref(), Some(records[0].run_id.as_str()));
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
        // A FIXED clock — 03:30, before the 23:30 schedule — so the test is
        // deterministic at any hour it runs at (a real-clock tick at 23:31+
        // would see daily-23:30 due: a time-of-day flake, now closed).
        let now = Local.with_ymd_and_hms(2026, 9, 19, 3, 30, 0).unwrap();
        let records = shift.tick(now).await.unwrap();
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
        let records = shift.tick(now).await.unwrap();
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
        // FR-9.1 (Story 2.6): the dead-man switch appends the run.dead ALERT
        // event (telemetry actor, last heartbeat + threshold) and then the
        // terminal run.failed(silently_dead) — the run ends with a timestamp
        // and a reason, never silently
        let dead = events_of(&db, crate::domain::telemetry::RUN_DEAD).await;
        assert_eq!(dead.len(), 1, "the dead run is detected");
        assert_eq!(dead[0].payload["run_id"], json!("nightshift-dead"));
        assert_eq!(
            dead[0].actor,
            crate::eventstore::Actor::System {
                component: crate::eventstore::SystemComponent::Telemetry
            }
        );
        assert_eq!(
            dead[0].payload["last_heartbeat_ts"],
            serde_json::to_value(started.ts).unwrap(),
            "no heartbeat landed — the run's start is the heartbeat it died at"
        );
        assert_eq!(dead[0].payload["threshold_minutes"], json!(30));
        let failures = events_of(&db, RUN_FAILED).await;
        assert_eq!(failures.len(), 1, "the dead run reaches its terminal");
        assert_eq!(failures[0].payload["reason"], json!("silently_dead"));
        assert_eq!(failures[0].payload["run_id"], json!("nightshift-dead"));
        // the digest renders the dead-man alert row (FR-9.1)
        let digest = morning_digest(&db).await.unwrap();
        assert_eq!(digest.alerts.len(), 1);
        assert_eq!(digest.alerts[0].run_id, "nightshift-dead");
        assert_eq!(digest.alerts[0].mission_id, mission.id);
        assert_eq!(digest.alerts[0].heartbeat_ts, started.ts);
        assert_eq!(digest.alerts[0].receipt_seq, started.seq);
    }

    #[tokio::test]
    async fn a_live_heartbeat_keeps_a_long_run_alive_and_unreaped() {
        let db = test_db();
        let mission = create_mission(&db, "daily-03:00", 500).await;
        // a run started 2 hours ago whose last heartbeat landed minutes ago
        // — alive (threshold: 30 min), the dead-man switch must NOT fire
        let started_at = Utc::now() - Duration::hours(2);
        let beat_at = Utc::now() - Duration::minutes(5);
        {
            let conn = db.0.lock().await;
            let store = EventStore::new(&conn);
            let mut event = NewEvent::run_started("nightshift-alive", mission.id, "daily-03:00", SCAN_STEP)
                .unwrap();
            event.ts = started_at;
            let started = store.append(event).unwrap();
            let mut beat =
                NewEvent::run_heartbeat("nightshift-alive", mission.id, started.seq, started.ts)
                    .unwrap();
            beat.ts = beat_at;
            store.append(beat).unwrap();
        }
        let shift = fake_shift(&db);
        shift.tick(now_local()).await.unwrap();
        assert!(
            events_of(&db, crate::domain::telemetry::RUN_DEAD).await.is_empty(),
            "a heartbeating run is alive — no dead-run alert"
        );
        let dead_failures: Vec<_> = events_of(&db, RUN_FAILED)
            .await
            .into_iter()
            .filter(|e| e.payload["reason"] == json!("silently_dead"))
            .collect();
        assert!(dead_failures.is_empty(), "no silently-dead terminal for a live run");
    }

    #[tokio::test]
    async fn the_tick_probes_connections_and_alerts_then_restores() {
        let db = test_db();
        create_mission(&db, "off", 500).await; // nothing runs — probes only
        // a prober with zotero down, everything else up
        let zotero_down: Prober = Arc::new(|name: &str| {
            let name = name.to_string();
            Box::pin(async move {
                if name == "zotero" {
                    Err("unreachable".to_string())
                } else {
                    Ok(())
                }
            })
        });
        let shift =
            NightShift::with_resolver_and_prober(db.clone(), fake_resolver, zotero_down);
        shift.tick(now_local()).await.unwrap();
        let failed = events_of(&db, crate::domain::telemetry::CONNECTION_FAILED).await;
        assert_eq!(failed.len(), 1, "the down connection alerts — once, not per probe");
        assert_eq!(failed[0].payload["connection"], json!("zotero"));
        assert_eq!(failed[0].payload["error_code"], json!("unreachable"));
        // a second tick with the same outage records NOTHING (transitions only)
        shift.tick(now_local()).await.unwrap();
        assert_eq!(
            events_of(&db, crate::domain::telemetry::CONNECTION_FAILED).await.len(),
            1
        );
        // the digest renders the connection alert row (FR-9.1, label + icon)
        let digest = morning_digest(&db).await.unwrap();
        assert_eq!(digest.connection_alerts.len(), 1);
        assert_eq!(digest.connection_alerts[0].connection, "zotero");
        assert_eq!(digest.connection_alerts[0].error_code, "unreachable");
        // the trust center's health line reflects it too (the trust status
        // fold carries the connections)
        let health = {
            let conn = db.0.lock().await;
            let events = EventStore::new(&conn).events_all().unwrap();
            crate::domain::telemetry::connection_health(&events)
        };
        assert_eq!(health.len(), 1);
        assert_eq!(health[0].connection, "zotero");
        assert!(!health[0].up);
        // recovery: the restore lands and the alert clears
        let healed = fake_shift(&db);
        healed.tick(now_local()).await.unwrap();
        let restored = events_of(&db, crate::domain::telemetry::CONNECTION_RESTORED).await;
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].payload["connection"], json!("zotero"));
        let digest = morning_digest(&db).await.unwrap();
        assert!(digest.connection_alerts.is_empty(), "a healed connection does not alert");
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
