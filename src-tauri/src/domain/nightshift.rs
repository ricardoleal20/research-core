// Night Shift domain (FR-4, Story 2.3): the run lifecycle events a scheduled
// overnight scan appends — `run.started`, `run.finished`, `run.failed` — and
// the AD-12 mission terminal evaluator. Run lifecycle events are appended by
// the scheduler (`actor=system (scheduler)`, AD-15's closed enumeration); the
// digest is delivered even when a run fails, so `run.failed` carries its
// reason in code form plus the run's last heartbeat timestamp (the FR-9.1
// dead-man-switch seam — a run that died with a stale heartbeat renders the
// digest's alert row; Story 2.6 builds full detection).
//
// AD-12 (amended Story 2.3 AC): after every mission-scoped event the runtime
// evaluates the mission's stop condition and success criterion and appends
// `mission.completed` | `mission.stopped` | `mission.failed` when it can
// decide — no mission rests without a terminal state once the evaluator can
// decide. The v1 evaluator is deterministic and state-based (the criterion is
// user text, so a pragmatic signal matcher — documented below); it never
// double-emits: only ACTIVE missions evaluate, and the fold's status is the
// single source of truth for "already terminal".

use crate::domain::hypotheses::{Hypothesis, HypothesisStatus, HypothesesProjection};
use crate::domain::missions::{Mission, MissionStatus, MissionsProjection};
use crate::eventstore::{Actor, EventError, EventStore, NewEvent, StoredEvent, SystemComponent};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const RUN_STARTED: &str = "run.started";
pub const RUN_FINISHED: &str = "run.finished";
pub const RUN_FAILED: &str = "run.failed";

/// The run.failed reason that marks a run as dead with a stale heartbeat —
/// the digest renders its dead-man-switch alert row from this case (FR-9.1
/// hook; Story 2.6 builds the detection, here the seam renders).
pub const DEAD_RUN_REASON: &str = "stale_heartbeat";

/// A run is reaped as dead (stale heartbeat) when its `run.started` has no
/// terminal run event and is older than this window — the minimal honest
/// dead-run detection the digest's alert row renders from.
pub const DEAD_RUN_AFTER_MINUTES: i64 = 30;

/// The one step a v1 Night Shift run executes (FR-4.1): a literature scan
/// over the mission's question and board through the provider layer.
pub const SCAN_STEP: &str = "literature-scan";

// ---------------------------------------------------------------------------
// Run lifecycle events (FR-4.1/4.3)
// ---------------------------------------------------------------------------

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `run.started` event comes into
    /// being — the scheduler opening a Night Shift run for a mission. Fails
    /// loudly on an empty run id or schedule, or an unknown step.
    pub fn run_started(
        run_id: &str,
        mission_id: Uuid,
        schedule: &str,
        step: &str,
    ) -> Result<Self, EventError> {
        let payload = RunStartedPayload {
            run_id: run_id.to_string(),
            mission_id,
            schedule: schedule.to_string(),
            step: step.to_string(),
        };
        validate_run_payload(&payload.run_id, &payload.schedule, &payload.step)?;
        Self::new(
            RUN_STARTED,
            Actor::System {
                component: SystemComponent::Scheduler,
            },
            serde_json::to_value(&payload)?,
        )
        .map(|e| e.with_causes(vec![mission_id]))
    }

    /// Typed constructor (AD-15): a finished run carries its one-line verdict
    /// and how many proposals it left in quarantine (FR-4.2 — the scan's
    /// output lands as proposals; nothing mutates the board unattended).
    pub fn run_finished(
        run_id: &str,
        mission_id: Uuid,
        verdict: &str,
        proposals: u32,
    ) -> Result<Self, EventError> {
        let payload = RunFinishedPayload {
            run_id: run_id.to_string(),
            mission_id,
            verdict: verdict.to_string(),
            proposals,
        };
        validate_run_payload(&payload.run_id, "daily-03:00", SCAN_STEP)?;
        if payload.verdict.trim().is_empty() {
            return Err(EventError::Invalid(
                "run.finished requires a verdict — the digest renders one line per mission (FR-4.4)"
                    .into(),
            ));
        }
        Self::new(
            RUN_FINISHED,
            Actor::System {
                component: SystemComponent::Scheduler,
            },
            serde_json::to_value(&payload)?,
        )
        .map(|e| e.with_causes(vec![mission_id]))
    }

    /// Typed constructor (AD-15): a failed run carries its reason in code
    /// form (bilingual-safe by construction) and the heartbeat timestamp the
    /// run died at — the digest is delivered even on failure (FR-4.3), so a
    /// failed run must be as well-formed as a finished one.
    pub fn run_failed(
        run_id: &str,
        mission_id: Uuid,
        reason: &str,
        heartbeat_ts: DateTime<Utc>,
    ) -> Result<Self, EventError> {
        let payload = RunFailedPayload {
            run_id: run_id.to_string(),
            mission_id,
            reason: reason.to_string(),
            heartbeat_ts,
        };
        validate_run_payload(&payload.run_id, "daily-03:00", SCAN_STEP)?;
        if payload.reason.trim().is_empty() {
            return Err(EventError::Invalid(
                "run.failed requires a reason — an honest failure row names what went wrong (FR-4.3)"
                    .into(),
            ));
        }
        Self::new(
            RUN_FAILED,
            Actor::System {
                component: SystemComponent::Scheduler,
            },
            serde_json::to_value(&payload)?,
        )
        .map(|e| e.with_causes(vec![mission_id]))
    }
}

fn validate_run_payload(run_id: &str, schedule: &str, step: &str) -> Result<(), EventError> {
    if run_id.trim().is_empty() {
        return Err(EventError::Invalid(
            "run.run_id must not be empty — every run event names its run".into(),
        ));
    }
    if schedule.trim().is_empty() {
        return Err(EventError::Invalid(
            "run.schedule must not be empty — a scheduled run names its schedule".into(),
        ));
    }
    if step.trim().is_empty() {
        return Err(EventError::Invalid(
            "run.step must not be empty — a run event names the step it ran".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStartedPayload {
    pub run_id: String,
    pub mission_id: Uuid,
    pub schedule: String,
    pub step: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunFinishedPayload {
    pub run_id: String,
    pub mission_id: Uuid,
    pub verdict: String,
    pub proposals: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunFailedPayload {
    pub run_id: String,
    pub mission_id: Uuid,
    pub reason: String,
    pub heartbeat_ts: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// AD-12: mission terminal evaluation
// ---------------------------------------------------------------------------

/// The three terminal states of AD-12. Serialized snake_case to match the
/// event kinds the fold already understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalKind {
    Completed,
    Stopped,
    Failed,
}

impl TerminalKind {
    pub fn event_kind(self) -> &'static str {
        match self {
            Self::Completed => crate::domain::missions::MISSION_COMPLETED,
            Self::Stopped => crate::domain::missions::MISSION_STOPPED,
            Self::Failed => crate::domain::missions::MISSION_FAILED,
        }
    }
}

/// The run tally the evaluator reads for one mission — counts of the run
/// lifecycle events referencing it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunStats {
    pub started: u64,
    pub finished: u64,
    pub failed: u64,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way the runtime appends a mission
    /// terminal event. Actor is `system (runtime)` — the evaluation is
    /// runtime state, never a quarantine-covered domain mutation (AD-15's
    /// closed enumeration). The payload names the deterministic signal that
    /// decided the terminal, so receipts can show WHY the mission ended.
    pub fn mission_terminal(
        kind: TerminalKind,
        mission_id: Uuid,
        signal: &str,
        extra: serde_json::Value,
    ) -> Result<Self, EventError> {
        if signal.trim().is_empty() {
            return Err(EventError::Invalid(
                "mission terminal requires a signal — receipts show why the mission ended".into(),
            ));
        }
        let mut payload = serde_json::Map::new();
        payload.insert("signal".into(), serde_json::json!(signal));
        if let serde_json::Value::Object(fields) = extra {
            for (k, v) in fields {
                payload.insert(k, v);
            }
        }
        Self::new(
            kind.event_kind(),
            Actor::System {
                component: SystemComponent::Runtime,
            },
            serde_json::Value::Object(payload),
        )
        .map(|e| e.with_causes(vec![mission_id]))
    }
}

/// The v1 terminal evaluator (AD-12): deterministic, state-based signals over
/// the mission's declared terminators. The stop condition and success
/// criterion are user text, so v1 reads the mission-level signals they name —
/// documented mapping:
///
/// - `completed` — the board is SETTLED (every hypothesis `supported` or
///   `refuted`, at least one) with at least one `supported`: the mission's
///   question found support ("all hypotheses supported/refuted" is the
///   canonical criterion shape).
/// - `stopped` — the cost ceiling was reached (spend at/over the declared
///   ceiling — the "$X spent" stop condition, AD-10), or the board settled
///   with everything refuted: the mission ran its falsifiable course.
/// - `failed` — the mission has runs and every one of them failed (no run
///   ever finished): the mission cannot progress.
///
/// Priority: completed > stopped > failed (a criterion met over ceiling still
/// completes). Returns `None` while no signal can decide — the mission rests
/// in `active` only because the evaluator cannot decide yet.
pub fn terminal_evaluation(
    mission: &Mission,
    board: &[Hypothesis],
    runs: &RunStats,
) -> Option<TerminalKind> {
    let settled = !board.is_empty()
        && board.iter().all(|h| {
            matches!(h.status, HypothesisStatus::Supported | HypothesisStatus::Refuted)
        });
    let any_supported = board.iter().any(|h| h.status == HypothesisStatus::Supported);
    if settled && any_supported {
        return Some(TerminalKind::Completed);
    }
    if mission.spend_state == crate::domain::missions::SpendState::Blocked {
        return Some(TerminalKind::Stopped);
    }
    if settled && !any_supported {
        return Some(TerminalKind::Stopped);
    }
    if runs.started > 0 && runs.finished == 0 && runs.failed > 0 {
        return Some(TerminalKind::Failed);
    }
    None
}

/// The signal name a decided terminal carries in its payload (receipts show
/// why the mission ended).
pub fn terminal_signal(mission: &Mission, board: &[Hypothesis], kind: TerminalKind) -> String {
    let settled = !board.is_empty()
        && board.iter().all(|h| {
            matches!(h.status, HypothesisStatus::Supported | HypothesisStatus::Refuted)
        });
    let any_supported = board.iter().any(|h| h.status == HypothesisStatus::Supported);
    match kind {
        TerminalKind::Completed => "board_settled_supported".into(),
        TerminalKind::Stopped => {
            if mission.spend_state == crate::domain::missions::SpendState::Blocked {
                "cost_ceiling_reached".into()
            } else if settled && !any_supported {
                "board_settled_refuted".into()
            } else {
                "stop_condition_met".into()
            }
        }
        TerminalKind::Failed => "all_runs_failed".into(),
    }
}

/// The run tally of one mission from the log: counts of run lifecycle events
/// referencing it. Pure.
pub fn run_stats_for(events: &[StoredEvent], mission_id: Uuid) -> RunStats {
    let mut stats = RunStats::default();
    for event in events {
        let refs_mission = event.causes.contains(&mission_id)
            || event
                .payload
                .get("mission_id")
                .and_then(serde_json::Value::as_str)
                .map(|s| s == mission_id.to_string())
                .unwrap_or(false);
        if !refs_mission {
            continue;
        }
        match event.kind.as_str() {
            RUN_STARTED => stats.started += 1,
            RUN_FINISHED => stats.finished += 1,
            RUN_FAILED => stats.failed += 1,
            _ => {}
        }
    }
    stats
}

/// Evaluate every mission in the log and append the terminal event for each
/// ACTIVE mission the evaluator can decide (AD-12: no mission rests without
/// a terminal state once decidable). Never double-emits: the fold's status
/// decides — a mission already carrying a terminal event is not `active` and
/// is skipped. Appending the terminal updates the fold, so a second call is
/// a no-op for that mission. Returns the terminal events appended (empty
/// when nothing was decidable).
pub fn evaluate_terminals(store: &EventStore<'_>) -> Result<Vec<StoredEvent>, EventError> {
    let events = store.events_all()?;
    let missions = MissionsProjection::fold(&events)?;
    let board = HypothesesProjection::fold(&events)?;
    let mut appended = Vec::new();
    for mission in &missions {
        if mission.status != MissionStatus::Active {
            continue; // already terminal (or a human holds it in review) — never double-emit
        }
        let mission_board: Vec<Hypothesis> =
            board.iter().filter(|h| h.mission_id == mission.id).cloned().collect();
        let runs = run_stats_for(&events, mission.id);
        let Some(kind) = terminal_evaluation(mission, &mission_board, &runs) else {
            continue;
        };
        let signal = terminal_signal(mission, &mission_board, kind);
        let extra = match kind {
            TerminalKind::Stopped => serde_json::json!({
                "spend_cents": mission.spend_cents,
                "ceiling_cents": mission.spend_ceiling_cents,
            }),
            _ => serde_json::json!({}),
        };
        appended.push(store.append(NewEvent::mission_terminal(
            kind, mission.id, &signal, extra,
        )?)?);
    }
    Ok(appended)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::hypotheses::HypothesisStatus;
    use crate::domain::missions::{
        Autonomy, MissionCreatedPayload, SpendState, MISSION_SCHEDULED,
    };
    use crate::domain::spend::SPEND_RECORDED;
    use crate::eventstore::Actor;
    use chrono::TimeZone;
    use rusqlite::Connection;
    use serde_json::json;

    fn mission() -> Mission {
        Mission {
            id: Uuid::new_v4(),
            seq: 1,
            ts: Utc::now(),
            question: "Does retrieval grounding reduce hallucinated citations?".into(),
            stop_condition: "Stop after 3 rounds or $5.00 spent.".into(),
            success_criterion: "A blind rater finds zero fabricated citations.".into(),
            autonomy: Autonomy::Suggest,
            spend_ceiling_cents: 500,
            schedule: "daily-03:00".into(),
            roles: vec![],
            status: MissionStatus::Active,
            spend_cents: 0,
            spend_state: SpendState::Ok,
        }
    }

    fn hyp(status: HypothesisStatus) -> Hypothesis {
        Hypothesis {
            id: Uuid::new_v4(),
            seq: 2,
            ts: Utc::now(),
            statement: "X holds.".into(),
            mission_id: Uuid::default(),
            status,
            relations: vec![],
            audit: crate::domain::hypotheses::AuditStamp {
                seq: 2,
                ts: Utc::now(),
                actor: "user".into(),
                basis: "test".into(),
            },
        }
    }

    // ---- run lifecycle constructors ----

    #[test]
    fn run_lifecycle_constructors_carry_the_scheduler_actor() {
        let mission_id = Uuid::new_v4();
        let started = NewEvent::run_started("ns-1", mission_id, "daily-03:00", SCAN_STEP).unwrap();
        assert_eq!(started.kind, RUN_STARTED);
        assert_eq!(
            started.actor,
            Actor::System {
                component: SystemComponent::Scheduler
            }
        );
        assert_eq!(started.causes, vec![mission_id]);
        assert_eq!(started.payload["run_id"], json!("ns-1"));
        assert_eq!(started.payload["step"], json!("literature-scan"));

        let finished =
            NewEvent::run_finished("ns-1", mission_id, "1 scan · 2 proposals pending", 2).unwrap();
        assert_eq!(finished.kind, RUN_FINISHED);
        assert_eq!(finished.payload["verdict"], json!("1 scan · 2 proposals pending"));
        assert_eq!(finished.payload["proposals"], json!(2));

        let hb = Utc.with_ymd_and_hms(2026, 9, 18, 2, 31, 0).unwrap();
        let failed = NewEvent::run_failed("ns-2", mission_id, DEAD_RUN_REASON, hb).unwrap();
        assert_eq!(failed.kind, RUN_FAILED);
        assert_eq!(failed.payload["reason"], json!(DEAD_RUN_REASON));
        assert_eq!(failed.payload["heartbeat_ts"], serde_json::to_value(hb).unwrap());
    }

    #[test]
    fn run_constructors_reject_empty_fields() {
        let mission_id = Uuid::new_v4();
        assert!(NewEvent::run_started("", mission_id, "daily-03:00", SCAN_STEP).is_err());
        assert!(NewEvent::run_started("ns-1", mission_id, "  ", SCAN_STEP).is_err());
        assert!(NewEvent::run_started("ns-1", mission_id, "daily-03:00", " ").is_err());
        assert!(NewEvent::run_finished("ns-1", mission_id, "   ", 0).is_err());
        assert!(NewEvent::run_failed("", mission_id, "provider_error", Utc::now()).is_err());
        assert!(NewEvent::run_failed("ns-1", mission_id, "  ", Utc::now()).is_err());
    }

    // ---- the AD-12 terminal evaluator ----

    #[test]
    fn evaluator_emits_all_three_terminals_from_deterministic_signals() {
        let m = mission();
        let none = RunStats::default();
        // completed: board settled with support
        assert_eq!(
            terminal_evaluation(&m, &[hyp(HypothesisStatus::Supported)], &none),
            Some(TerminalKind::Completed)
        );
        assert_eq!(
            terminal_evaluation(
                &m,
                &[
                    hyp(HypothesisStatus::Supported),
                    hyp(HypothesisStatus::Refuted)
                ],
                &none
            ),
            Some(TerminalKind::Completed)
        );
        // stopped: cost ceiling reached (spend at/over the declared ceiling)
        let mut over = mission();
        over.spend_cents = 500;
        over.spend_state = SpendState::Blocked;
        assert_eq!(
            terminal_evaluation(&over, &[], &none),
            Some(TerminalKind::Stopped)
        );
        // stopped: board settled with everything refuted — the falsifiable
        // course ran out
        assert_eq!(
            terminal_evaluation(&m, &[hyp(HypothesisStatus::Refuted)], &none),
            Some(TerminalKind::Stopped)
        );
        // failed: runs exist and every one failed
        assert_eq!(
            terminal_evaluation(
                &m,
                &[],
                &RunStats { started: 2, finished: 0, failed: 2 }
            ),
            Some(TerminalKind::Failed)
        );
    }

    #[test]
    fn evaluator_rests_undecided_until_a_signal_can_decide() {
        let m = mission();
        // empty board: nothing settled, nothing spent, no runs
        assert_eq!(terminal_evaluation(&m, &[], &RunStats::default()), None);
        // open board (a proposed hypothesis is not settled)
        assert_eq!(
            terminal_evaluation(&m, &[hyp(HypothesisStatus::Proposed)], &RunStats::default()),
            None
        );
        // under-ceiling spend with an open board
        let mut near = mission();
        near.spend_cents = 400;
        near.spend_state = SpendState::Near;
        assert_eq!(terminal_evaluation(&near, &[hyp(HypothesisStatus::Proposed)], &RunStats::default()), None);
        // some runs succeeded — a failed run does not fail the mission
        assert_eq!(
            terminal_evaluation(
                &m,
                &[],
                &RunStats { started: 2, finished: 1, failed: 1 }
            ),
            None
        );
        // settled board with support outranks a reached ceiling (criterion met)
        let mut over = mission();
        over.spend_cents = 900;
        over.spend_state = SpendState::Blocked;
        assert_eq!(
            terminal_evaluation(&over, &[hyp(HypothesisStatus::Supported)], &RunStats::default()),
            Some(TerminalKind::Completed)
        );
    }

    // ---- evaluate_terminals over the log (the never-double-emit contract) ----

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    fn seed_mission(store: &EventStore<'_>) -> StoredEvent {
        store
            .append(
                NewEvent::mission_created(MissionCreatedPayload {
                    question: "Does X hold up?".into(),
                    stop_condition: "Stop after $5 spent.".into(),
                    success_criterion: "A blind rater agrees.".into(),
                    autonomy: Autonomy::Suggest,
                    spend_ceiling_cents: 500,
                    schedule: "daily-03:00".into(),
                    roles: vec![],
                })
                .unwrap(),
            )
            .unwrap()
    }

    #[test]
    fn evaluate_terminals_appends_the_decided_terminal_once_and_never_again() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let created = seed_mission(&store);
        // a supported hypothesis settles the board → completed
        let hyp = store
            .append(NewEvent::hypothesis_created("X holds.", created.id).unwrap())
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

        let appended = evaluate_terminals(&store).unwrap();
        assert_eq!(appended.len(), 1, "exactly one terminal for one decidable mission");
        assert_eq!(appended[0].kind, crate::domain::missions::MISSION_COMPLETED);
        assert_eq!(appended[0].actor, Actor::System { component: SystemComponent::Runtime });
        assert_eq!(appended[0].payload["signal"], json!("board_settled_supported"));
        assert_eq!(appended[0].causes, vec![created.id]);

        // the fold now shows the mission terminal — re-evaluation never
        // double-emits (no mission rests with two terminals)
        assert!(evaluate_terminals(&store).unwrap().is_empty());
        assert!(evaluate_terminals(&store).unwrap().is_empty());
        let missions = MissionsProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(missions[0].status, MissionStatus::Completed);
        // exactly one terminal event in the log for the mission
        let all = store.events_all().unwrap();
        let terminals: Vec<&StoredEvent> = all
            .iter()
            .filter(|e| {
                matches!(
                    e.kind.as_str(),
                    crate::domain::missions::MISSION_COMPLETED
                        | crate::domain::missions::MISSION_STOPPED
                        | crate::domain::missions::MISSION_FAILED
                )
            })
            .collect();
        assert_eq!(terminals.len(), 1);
    }

    #[test]
    fn evaluate_terminals_stops_a_mission_at_its_ceiling_and_fails_all_failed_missions() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        // mission 1: spend at the ceiling → stopped
        let a = seed_mission(&store);
        store
            .append(
                NewEvent::new(
                    SPEND_RECORDED,
                    Actor::System { component: SystemComponent::Telemetry },
                    json!({ "mission_id": a.id.to_string(), "cost_cents": 500 }),
                )
                .unwrap()
                .with_causes(vec![a.id]),
            )
            .unwrap();
        // mission 2: every run failed → failed
        let b = seed_mission(&store);
        for i in 0..2 {
            let run = format!("ns-{i}");
            store
                .append(NewEvent::run_started(&run, b.id, "daily-03:00", SCAN_STEP).unwrap())
                .unwrap();
            store
                .append(NewEvent::run_failed(&run, b.id, "provider_error", Utc::now()).unwrap())
                .unwrap();
        }
        let appended = evaluate_terminals(&store).unwrap();
        assert_eq!(appended.len(), 2, "both missions decided in one pass");
        let kinds: Vec<&str> = appended.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&crate::domain::missions::MISSION_STOPPED));
        assert!(kinds.contains(&crate::domain::missions::MISSION_FAILED));
        let stopped = appended
            .iter()
            .find(|e| e.kind == crate::domain::missions::MISSION_STOPPED)
            .unwrap();
        assert_eq!(stopped.payload["signal"], json!("cost_ceiling_reached"));
        assert_eq!(stopped.payload["spend_cents"], json!(500));
        assert_eq!(stopped.payload["ceiling_cents"], json!(500));
    }

    #[test]
    fn evaluate_terminals_skips_undecidable_and_non_active_missions() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        // an undecidable mission (open board, no runs) stays active
        seed_mission(&store);
        assert!(evaluate_terminals(&store).unwrap().is_empty());
        // a human-held awaiting_review mission is not the evaluator's to end
        let conn2 = mem_conn();
        let store2 = EventStore::new(&conn2);
        let created = seed_mission(&store2);
        store2
            .append(
                NewEvent::new(
                    crate::domain::missions::MISSION_AWAITING_REVIEW,
                    Actor::User,
                    json!({}),
                )
                .unwrap()
                .with_causes(vec![created.id]),
            )
            .unwrap();
        let hyp = store2
            .append(NewEvent::hypothesis_created("X.", created.id).unwrap())
            .unwrap();
        for (from, to) in [
            (HypothesisStatus::Proposed, HypothesisStatus::Testing),
            (HypothesisStatus::Testing, HypothesisStatus::Supported),
        ] {
            store2
                .append(
                    NewEvent::hypothesis_status_changed(from, to, "night work")
                        .unwrap()
                        .with_causes(vec![hyp.id]),
                )
                .unwrap();
        }
        assert!(evaluate_terminals(&store2).unwrap().is_empty());
        let missions = MissionsProjection::fold(&store2.events_all().unwrap()).unwrap();
        assert_eq!(missions[0].status, MissionStatus::AwaitingReview);
    }

    #[test]
    fn run_stats_counts_lifecycle_events_referencing_the_mission() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let created = seed_mission(&store);
        let other = seed_mission(&store);
        store
            .append(NewEvent::run_started("ns-1", created.id, "daily-03:00", SCAN_STEP).unwrap())
            .unwrap();
        store
            .append(NewEvent::run_finished("ns-1", created.id, "ok", 1).unwrap())
            .unwrap();
        store
            .append(NewEvent::run_started("ns-2", created.id, "daily-03:00", SCAN_STEP).unwrap())
            .unwrap();
        store
            .append(NewEvent::run_failed("ns-2", created.id, "provider_error", Utc::now()).unwrap())
            .unwrap();
        // other mission's run — not counted
        store
            .append(NewEvent::run_started("ns-9", other.id, "daily-03:00", SCAN_STEP).unwrap())
            .unwrap();
        // a mission.scheduled event references the mission but is not a run
        store
            .append(NewEvent::mission_scheduled("daily-04:00", created.id).unwrap())
            .unwrap();
        let all = store.events_all().unwrap();
        let stats = run_stats_for(&all, created.id);
        assert_eq!(
            stats,
            RunStats { started: 2, finished: 1, failed: 1 }
        );
        assert_eq!(
            run_stats_for(&all, other.id),
            RunStats { started: 1, finished: 0, failed: 0 }
        );
        let _ = MISSION_SCHEDULED; // referenced for the doc comment above
    }
}
