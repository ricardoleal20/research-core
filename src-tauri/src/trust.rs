// Trust runtime (FR-5, Story 2.4): THE dispatcher every provider call goes
// through — the reservation protocol (AD-10), the autonomy dial (AD-15d), and
// the kill switch (AD-15e), all enforced here and nowhere else.
//
// The reservation protocol: before ANY provider dispatch the runtime appends
// `spend.reserved` (the estimate held against every applicable ceiling);
// after the call it settles — `spend.released` beside the call's
// `spend.recorded`, plus `target.spend_recorded` attributing the cost to the
// compute target. The ceiling check reads the spend fold PLUS in-flight
// reservations, and check+append run inside ONE critical section (the shared
// connection's mutex — one process, one writer, AD-14): two concurrent
// dispatches over the same headroom cannot both pass, because the second
// check sees the first's reservation in the log.
//
// The refusal is a RESULT, not an error to swallow: `spend.refused` lands
// with the scope, the ceiling, and the would-be cost, and the AD-12 evaluator
// turns it into the run's terminal state (`domain::nightshift`).
//
// The kill switch: while `runtime.killed` is the latest runtime-state event
// by seq, `reserve` refuses EVERY dispatch (typed `killed:`). A call already
// in flight at kill time is allowed to land its `spend.recorded` — no
// cancellation mid-wire (documented in-kill semantics).

use crate::adapters::providers::pricing;
use crate::adapters::providers::{ChatRequest, ChatResponse, Kind, ProviderError, ProviderLayer, Usage};
use crate::db::Db;
use crate::domain::missions::{Autonomy, MissionsProjection};
use crate::domain::trust::{
    check_ceilings, effective_autonomy, in_flight, spend_ledger, trust_config, Refusal,
    AutonomyConfiguredPayload, CeilingConfiguredPayload, SpendRefusedPayload,
    SpendReleasedPayload, SpendReservedPayload, TargetSpendRecordedPayload, COST_CEILING_RUN_REASON,
    RELEASE_PROVIDER_ERROR, RELEASE_RECORDED,
};
use crate::eventstore::{EventError, EventStore, NewEvent};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// The nominal envelope one agent step is reserved against: a generous
/// upper bound (100k in / 25k out tokens) priced like the call itself, so a
/// reservation always covers a realistic would-be cost (AD-10 — the estimate
/// can be wrong, but only in the safe direction).
pub const NOMINAL_USAGE: Usage = Usage { input_tokens: 100_000, output_tokens: 25_000 };

/// Everything that can go wrong at the trust boundary — typed, never a bare
/// string. Error strings lead with stable codes (`killed:`, `autonomy:`,
/// `cost_ceiling:`) — bilingual-safe by construction (EXPERIENCE.md).
#[derive(Debug, Error)]
pub enum TrustError {
    #[error("killed: the runtime is killed — every dispatch is refused until runtime.resumed (AD-15e)")]
    Killed,
    #[error("autonomy: the effective dial for this dispatch is `{}` — watch observes, it never dispatches (AD-15d)", .mode.as_str())]
    Autonomy { mode: Autonomy },
    #[error("cost_ceiling: the {scope} ceiling ({ceiling_cents}¢) would be crossed (would-be spend {would_be_cents}¢) — refused (AD-10)")]
    CostCeiling { scope: String, ceiling_cents: u64, would_be_cents: u64 },
    #[error(transparent)]
    Store(#[from] EventError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
}

impl TrustError {
    /// The run failure reason, in code form (AD-12 interplay: a refused step
    /// fails its run with `cost_ceiling_reached`, which the terminal
    /// evaluator recognizes as `mission.stopped`).
    pub fn run_reason(&self) -> String {
        match self {
            Self::Killed => "runtime_killed".into(),
            Self::Autonomy { .. } => "autonomy_watch".into(),
            Self::CostCeiling { .. } => COST_CEILING_RUN_REASON.into(),
            Self::Provider(_) => "provider_error".into(),
            Self::Store(_) => "store_error".into(),
        }
    }
}

/// One planned provider call, as the dispatcher reserves it.
#[derive(Debug, Clone)]
pub struct CallPlan {
    /// The reservation token: names the `spend.reserved` event and rides the
    /// call into its `spend.recorded` (per-run spend folds from it).
    pub run_id: String,
    /// The compute target (provider name or `cli`) the call runs on.
    pub target: String,
    /// The model the call runs (spend attribution).
    pub model: String,
    /// The mission the call advances, when it is mission-scoped.
    pub mission_id: Option<Uuid>,
    /// The pre-call estimate held against the ceilings (0 for simulated and
    /// CLI calls — they cost nothing measurable and reserve nothing).
    pub estimate_cents: u64,
    /// An autonomous (agent-initiated) call is gated by the dial (AD-15d);
    /// a user-initiated one (the Asistente chat, a review, first value) is
    /// not — the user pressed the button, the dial governs the agent.
    pub autonomous: bool,
}

/// The estimate to hold for one call (AD-10): nothing for simulated and CLI
/// calls (unmeasurable), the nominal-envelope price for real providers.
pub fn estimate_cents(layer: &ProviderLayer, model: &str) -> u64 {
    if layer.kind() != Kind::Remote {
        return 0;
    }
    pricing::cost_cents(layer.name(), model, &NOMINAL_USAGE)
}

/// A reservation held against the ceilings: `reserved` is false for
/// zero-cost calls (simulated / CLI) — the checks still ran (kill switch,
/// dial), but no ledger event was appended.
#[derive(Debug, Clone)]
pub struct Reservation {
    pub run_id: String,
    pub reserved: bool,
}

/// Reserve headroom for one provider call (AD-10). THE critical section: the
/// kill check, the dial check, and the ceiling check all read the same
/// folded log the `spend.reserved` append lands in, under the shared
/// connection's mutex — so a concurrent `reserve` cannot read a log that
/// misses this reservation (the TOCTOU guard). Refusals append
/// `spend.refused` before returning (a refusal is an event).
pub async fn reserve(db: &Db, plan: &CallPlan) -> Result<Reservation, TrustError> {
    let conn = db.0.lock().await;
    let store = EventStore::new(&conn);
    let events = store.events_all()?;
    let config = trust_config(&events);

    // The kill switch (AD-15e): while killed is latest, every dispatch is
    // refused — including the Night Shift tick.
    if config.runtime_killed {
        return Err(TrustError::Killed);
    }

    // The mission this call advances, when it is mission-scoped — one fold
    // serves both the dial check (its autonomy) and the ceiling check (its
    // creation ceiling).
    let missions = MissionsProjection::fold(&events)?;
    let mission = plan
        .mission_id
        .and_then(|id| missions.iter().find(|m| m.id == id))
        .map(|m| (m.id, m.autonomy, m.spend_ceiling_cents));

    // The autonomy dial (AD-15d): most restrictive of the applicable stops
    // gates AUTONOMOUS dispatches. No position ever enables an auto-merge —
    // every state mutation stays a proposal regardless (AD-3).
    if plan.autonomous {
        let dial = effective_autonomy(
            &config,
            mission.map(|(id, autonomy, _)| (id, autonomy)),
            Some(&plan.target),
        );
        if dial == Autonomy::Watch {
            return Err(TrustError::Autonomy { mode: dial });
        }
    }

    // Zero-cost calls (simulated / CLI) reserve nothing — but the kill and
    // dial checks above still ran.
    if plan.estimate_cents == 0 {
        return Ok(Reservation { run_id: plan.run_id.clone(), reserved: false });
    }

    // The ceiling check (FR-5.3): recorded spend PLUS in-flight reservations
    // against every applicable ceiling — most restrictive wins.
    let ledger = spend_ledger(&events);
    let flight = in_flight(&events);
    let mission_ceiling = mission.map(|(id, _, ceiling)| (id, ceiling));
    if let Some(Refusal { scope, ceiling_cents, would_be_cents }) =
        check_ceilings(&config, &ledger, &flight, mission_ceiling, &plan.target, plan.estimate_cents)
    {
        store.append(NewEvent::spend_refused(SpendRefusedPayload {
            run_id: plan.run_id.clone(),
            scope: scope.clone(),
            ceiling_cents,
            would_be_cost_cents: would_be_cents,
            mission_id: plan.mission_id,
            target: Some(plan.target.clone()),
        })?)?;
        return Err(TrustError::CostCeiling { scope, ceiling_cents, would_be_cents });
    }

    store.append(NewEvent::spend_reserved(SpendReservedPayload {
        run_id: plan.run_id.clone(),
        target: plan.target.clone(),
        amount_cents: plan.estimate_cents,
        mission_id: plan.mission_id,
    })?)?;
    Ok(Reservation { run_id: plan.run_id.clone(), reserved: true })
}

/// Settle a reservation after its call landed (AD-10): the cost attributed
/// to the compute target (`target.spend_recorded`), then the headroom
/// returned (`spend.released` — the call's `spend.recorded` took over the
/// accounting).
pub async fn settle_recorded(
    db: &Db,
    plan: &CallPlan,
    cost_cents: u64,
) -> Result<(), TrustError> {
    let conn = db.0.lock().await;
    let store = EventStore::new(&conn);
    store.append(NewEvent::target_spend_recorded(TargetSpendRecordedPayload {
        target: plan.target.clone(),
        cost_cents,
        run_id: Some(plan.run_id.clone()),
        mission_id: plan.mission_id,
    })?)?;
    store.append(NewEvent::spend_released(SpendReleasedPayload {
        run_id: plan.run_id.clone(),
        reason: RELEASE_RECORDED.into(),
    })?)?;
    Ok(())
}

/// Settle a reservation whose call never spent (AD-10): the headroom returns
/// unspent, with the reason receipts can show.
pub async fn settle_released(db: &Db, plan: &CallPlan, reason: &str) -> Result<(), TrustError> {
    let conn = db.0.lock().await;
    let store = EventStore::new(&conn);
    store.append(NewEvent::spend_released(SpendReleasedPayload {
        run_id: plan.run_id.clone(),
        reason: reason.to_string(),
    })?)?;
    Ok(())
}

/// THE dispatch (AD-10): reserve, call, settle. Every provider call in the
/// app goes through here — the adapter underneath refuses real calls that
/// arrive without the reservation this attaches (`no_reservation:`), so
/// nothing dispatches around the enforcement.
pub async fn reserve_and_chat(
    db: &Db,
    layer: &ProviderLayer,
    req: ChatRequest,
    plan: CallPlan,
) -> Result<ChatResponse, TrustError> {
    reserve(db, &plan).await?;
    let req = req.with_reservation(plan.run_id.clone());
    match layer.chat(req).await {
        Ok(resp) => {
            if plan.estimate_cents > 0 {
                let cost = pricing::cost_cents(&plan.target, &plan.model, &resp.usage);
                settle_recorded(db, &plan, cost).await?;
            }
            Ok(resp)
        }
        Err(e) => {
            if plan.estimate_cents > 0 {
                let _ = settle_released(db, &plan, RELEASE_PROVIDER_ERROR).await;
            }
            Err(e.into())
        }
    }
}

// ---------------------------------------------------------------------------
// The trust read model (the Settings trust center renders this)
// ---------------------------------------------------------------------------

/// The runtime's kill state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Running,
    Killed,
}

/// One scope's dial setting (global carries no scope id).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeDial {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    pub mode: Autonomy,
}

/// One scope's ceiling setting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeCeiling {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    pub ceiling_cents: u64,
}

/// One mission's spend meter: current spend vs the effective ceiling (the
/// creation ceiling, only ever tightened by a configured one), with the
/// DESIGN.md spend-meter state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionMeter {
    pub mission_id: Uuid,
    pub question: String,
    pub spend_cents: u64,
    pub ceiling_cents: u64,
    pub state: crate::domain::missions::SpendState,
}

/// One compute target's spend meter (no ceiling configured = unbounded).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetMeter {
    pub target: String,
    pub spend_cents: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ceiling_cents: Option<u64>,
}

/// The last run's spend line ("last run: 82¢ of 100¢"): the run the latest
/// `spend.recorded` landed in, against its ceiling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastRunSpend {
    pub run_id: String,
    pub spend_cents: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ceiling_cents: Option<u64>,
}

/// Everything the trust center renders (FR-5): the runtime state, the
/// effective dials and ceilings at every scope, current spend vs ceiling per
/// scope, and the last run's spend line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustStatus {
    pub runtime_state: RuntimeState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub killed_seq: Option<i64>,
    pub global_autonomy: Option<Autonomy>,
    pub mission_dials: Vec<ScopeDial>,
    pub target_dials: Vec<ScopeDial>,
    pub global_ceiling_cents: Option<u64>,
    pub mission_ceilings: Vec<ScopeCeiling>,
    pub target_ceilings: Vec<ScopeCeiling>,
    pub global_spend_cents: u64,
    pub missions: Vec<MissionMeter>,
    pub targets: Vec<TargetMeter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run: Option<LastRunSpend>,
}

/// Fold the trust status (the read model, AD-8 — pure over the log).
pub fn trust_status(
    events: &[crate::eventstore::StoredEvent],
) -> Result<TrustStatus, EventError> {
    let config = trust_config(events);
    let ledger = spend_ledger(events);
    let missions = MissionsProjection::fold(events)?;

    let mut killed_seq = None;
    for event in events {
        if event.kind == crate::domain::trust::RUNTIME_KILLED {
            killed_seq = Some(event.seq);
        } else if event.kind == crate::domain::trust::RUNTIME_RESUMED {
            killed_seq = None;
        }
    }

    // Per-mission meters: every mission, its effective ceiling, its spend.
    let mission_meters: Vec<MissionMeter> = missions
        .iter()
        .map(|m| {
            let ceiling = crate::domain::trust::effective_mission_ceiling(
                &config,
                m.id,
                m.spend_ceiling_cents,
            );
            MissionMeter {
                mission_id: m.id,
                question: m.question.clone(),
                spend_cents: m.spend_cents,
                ceiling_cents: ceiling,
                state: crate::domain::missions::spend_state(m.spend_cents, ceiling),
            }
        })
        .collect();

    // Per-target meters: every target with a configured dial or ceiling, or
    // any recorded target spend — union, sorted for stable rendering.
    let mut target_names: Vec<String> = config
        .target_autonomy
        .keys()
        .chain(config.target_ceiling_cents.keys())
        .chain(ledger.target_cents.keys())
        .cloned()
        .collect();
    target_names.sort();
    target_names.dedup();
    let targets: Vec<TargetMeter> = target_names
        .into_iter()
        .map(|target| TargetMeter {
            spend_cents: ledger.target_cents.get(&target).copied().unwrap_or(0),
            ceiling_cents: config.target_ceiling_cents.get(&target).copied(),
            target,
        })
        .collect();

    // The last run's spend line: the run the latest spend.recorded landed
    // in, against its ceiling (the mission's when mission-scoped, else the
    // global ceiling when configured).
    let last_run = ledger.last_run_id.as_ref().map(|run_id| LastRunSpend {
        run_id: run_id.clone(),
        spend_cents: ledger.run_cents.get(run_id).copied().unwrap_or(0),
        ceiling_cents: config.global_ceiling_cents,
    });

    let mut mission_dials: Vec<ScopeDial> = config
        .mission_autonomy
        .iter()
        .map(|(id, mode)| ScopeDial { scope_id: Some(id.to_string()), mode: *mode })
        .collect();
    mission_dials.sort_by(|a, b| a.scope_id.cmp(&b.scope_id));
    let mut target_dials: Vec<ScopeDial> = config
        .target_autonomy
        .iter()
        .map(|(target, mode)| ScopeDial { scope_id: Some(target.clone()), mode: *mode })
        .collect();
    target_dials.sort_by(|a, b| a.scope_id.cmp(&b.scope_id));
    let mut mission_ceilings: Vec<ScopeCeiling> = config
        .mission_ceiling_cents
        .iter()
        .map(|(id, cents)| ScopeCeiling { scope_id: Some(id.to_string()), ceiling_cents: *cents })
        .collect();
    mission_ceilings.sort_by(|a, b| a.scope_id.cmp(&b.scope_id));
    let mut target_ceilings: Vec<ScopeCeiling> = config
        .target_ceiling_cents
        .iter()
        .map(|(target, cents)| ScopeCeiling { scope_id: Some(target.clone()), ceiling_cents: *cents })
        .collect();
    target_ceilings.sort_by(|a, b| a.scope_id.cmp(&b.scope_id));

    Ok(TrustStatus {
        runtime_state: if config.runtime_killed {
            RuntimeState::Killed
        } else {
            RuntimeState::Running
        },
        killed_seq,
        global_autonomy: config.global_autonomy,
        mission_dials,
        target_dials,
        global_ceiling_cents: config.global_ceiling_cents,
        mission_ceilings,
        target_ceilings,
        global_spend_cents: ledger.global_cents,
        missions: mission_meters,
        targets,
        last_run,
    })
}

/// Append an `autonomy.configured` event (the `configure_autonomy` shell
/// command's core): validates the scope at the domain edge and returns the
/// fresh trust status — the read model, not the input.
pub async fn configure_autonomy(
    db: &Db,
    scope: &str,
    scope_id: Option<String>,
    mode: &str,
) -> Result<TrustStatus, String> {
    let scope = crate::domain::trust::Scope::parse(scope).ok_or_else(|| {
        format!("unknown scope `{scope}` — expected global | mission | target")
    })?;
    let mode = Autonomy::parse(mode).ok_or_else(|| {
        format!("unknown autonomy stop `{mode}` — expected watch | suggest | act_with_receipts")
    })?;
    let event = NewEvent::autonomy_configured(AutonomyConfiguredPayload {
        scope,
        scope_id,
        mode,
    })
    .map_err(|e| e.to_string())?;
    {
        let conn = db.0.lock().await;
        EventStore::new(&conn).append(event).map_err(|e| e.to_string())?;
    }
    status(db).await
}

/// Append a `ceiling.configured` event (the `configure_ceiling` shell
/// command's core).
pub async fn configure_ceiling(
    db: &Db,
    scope: &str,
    scope_id: Option<String>,
    ceiling_cents: u64,
) -> Result<TrustStatus, String> {
    let scope = crate::domain::trust::Scope::parse(scope).ok_or_else(|| {
        format!("unknown scope `{scope}` — expected global | mission | target")
    })?;
    let event = NewEvent::ceiling_configured(CeilingConfiguredPayload {
        scope,
        scope_id,
        ceiling_cents,
    })
    .map_err(|e| e.to_string())?;
    {
        let conn = db.0.lock().await;
        EventStore::new(&conn).append(event).map_err(|e| e.to_string())?;
    }
    status(db).await
}

/// Fire the kill switch (AD-15e): append `runtime.killed` — a runtime-owned
/// core command. While it is the latest runtime-state event by seq, every
/// dispatch is refused.
pub async fn kill_runtime(db: &Db) -> Result<TrustStatus, String> {
    {
        let conn = db.0.lock().await;
        let event = NewEvent::runtime_killed().map_err(|e| e.to_string())?;
        EventStore::new(&conn).append(event).map_err(|e| e.to_string())?;
    }
    status(db).await
}

/// Resume a killed runtime (AD-15e): append `runtime.resumed` — dispatch is
/// restored.
pub async fn resume_runtime(db: &Db) -> Result<TrustStatus, String> {
    {
        let conn = db.0.lock().await;
        let event = NewEvent::runtime_resumed().map_err(|e| e.to_string())?;
        EventStore::new(&conn).append(event).map_err(|e| e.to_string())?;
    }
    status(db).await
}

/// The trust status over the shared core (read side, AD-8).
pub async fn status(db: &Db) -> Result<TrustStatus, String> {
    let conn = db.0.lock().await;
    let events = EventStore::new(&conn).events_all().map_err(|e| e.to_string())?;
    trust_status(&events).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::missions::{MissionCreatedPayload, MissionStatus, SpendState};
    use crate::domain::trust::{SPEND_REFUSED, SPEND_RESERVED};
    use crate::eventstore::StoredEvent;
    use rusqlite::Connection;
    use serde_json::json;

    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::Db::migrate(&conn).unwrap();
        EventStore::init(&conn).unwrap();
        Db(std::sync::Arc::new(tokio::sync::Mutex::new(conn)))
    }

    async fn create_mission(db: &Db, autonomy: Autonomy, ceiling: u64) -> StoredEvent {
        let conn = db.0.lock().await;
        EventStore::new(&conn)
            .append(
                NewEvent::mission_created(MissionCreatedPayload {
                    question: "Does X hold?".into(),
                    stop_condition: "Stop after $5.".into(),
                    success_criterion: "A rater agrees.".into(),
                    autonomy,
                    spend_ceiling_cents: ceiling,
                    roles: vec![],
                    schedule: "off".into(),
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

    fn plan(run_id: &str, target: &str, mission_id: Option<Uuid>, estimate: u64) -> CallPlan {
        CallPlan {
            run_id: run_id.into(),
            target: target.into(),
            model: "gpt-4o".into(),
            mission_id,
            estimate_cents: estimate,
            autonomous: true,
        }
    }

    // ---- the kill switch ----

    #[tokio::test]
    async fn a_killed_runtime_refuses_every_dispatch_and_resume_restores() {
        let db = test_db();
        let mission = create_mission(&db, Autonomy::Suggest, 500).await;
        // kill
        let status = kill_runtime(&db).await.unwrap();
        assert_eq!(status.runtime_state, RuntimeState::Killed);
        assert!(status.killed_seq.is_some());
        // every dispatch is refused — typed killed:
        let err = reserve(&db, &plan("s1", "openai", Some(mission.id), 51)).await.unwrap_err();
        assert!(err.to_string().starts_with("killed:"), "unexpected: {err}");
        // zero-cost dispatches too (the kill switch is not about money)
        let err = reserve(&db, &plan("s2", "simulated", Some(mission.id), 0)).await.unwrap_err();
        assert!(err.to_string().starts_with("killed:"));
        // nothing was reserved or refused while killed
        assert!(events_of(&db, SPEND_RESERVED).await.is_empty());
        // resume restores
        let status = resume_runtime(&db).await.unwrap();
        assert_eq!(status.runtime_state, RuntimeState::Running);
        assert!(status.killed_seq.is_none());
        assert!(reserve(&db, &plan("s3", "simulated", Some(mission.id), 0)).await.is_ok());
    }

    // ---- the autonomy dial ----

    #[tokio::test]
    async fn a_watch_dial_refuses_autonomous_dispatches_but_not_user_initiated_ones() {
        let db = test_db();
        let mission = create_mission(&db, Autonomy::Suggest, 500).await;
        // mission dial to watch (latest wins within the scope)
        configure_autonomy(&db, "mission", Some(mission.id.to_string()), "watch")
            .await
            .unwrap();
        let err = reserve(&db, &plan("s1", "simulated", Some(mission.id), 0)).await.unwrap_err();
        assert!(err.to_string().starts_with("autonomy:"), "unexpected: {err}");
        // a user-initiated dispatch is not dial-gated (the user pressed the
        // button; the dial governs the agent)
        let mut user = plan("s2", "simulated", Some(mission.id), 0);
        user.autonomous = false;
        assert!(reserve(&db, &user).await.is_ok());
        // suggest and act_with_receipts dispatch (mutations still land as
        // proposals — AD-3 holds at every dial position)
        for mode in ["suggest", "act_with_receipts"] {
            configure_autonomy(&db, "mission", Some(mission.id.to_string()), mode)
                .await
                .unwrap();
            assert!(
                reserve(&db, &plan(&format!("s-{mode}"), "simulated", Some(mission.id), 0))
                    .await
                    .is_ok(),
                "{mode} must allow dispatch"
            );
        }
    }

    // ---- the reservation protocol + TOCTOU ----

    #[tokio::test]
    async fn two_concurrent_dispatches_over_a_dollar_headroom_cannot_both_pass() {
        let db = test_db();
        // A $1.00 mission ceiling with nothing spent: the whole ceiling is
        // headroom. Each dispatch's estimate is 51¢ (the nominal envelope on
        // gpt-4o rates), so two in flight would be 102¢ > 100¢.
        let mission = create_mission(&db, Autonomy::Suggest, 100).await;
        let a = reserve(&db, &plan("run-a", "openai", Some(mission.id), 51)).await.unwrap();
        assert!(a.reserved);
        // the SECOND concurrent dispatch — the first's reservation is still
        // in flight (nothing recorded, nothing released): 0 + 51 + 51 > 100
        let err = reserve(&db, &plan("run-b", "openai", Some(mission.id), 51)).await.unwrap_err();
        assert!(err.to_string().starts_with("cost_ceiling:"), "unexpected: {err}");
        // the refusal is an event: scope, ceiling, would-be cost
        let refusals = events_of(&db, SPEND_REFUSED).await;
        assert_eq!(refusals.len(), 1);
        assert_eq!(refusals[0].payload["scope"], json!("mission"));
        assert_eq!(refusals[0].payload["ceiling_cents"], json!(100));
        assert_eq!(refusals[0].payload["would_be_cost_cents"], json!(102));
        assert_eq!(refusals[0].payload["mission_id"], json!(mission.id.to_string()));
        // the first call lands: its spend.recorded (what the provider layer
        // appends) plus the settle — headroom returns, minus what it spent
        {
            let conn = db.0.lock().await;
            EventStore::new(&conn)
                .append(
                    NewEvent::spend_recorded(crate::domain::spend::SpendRecordedPayload {
                        provider: "openai".into(),
                        model: "gpt-4o".into(),
                        input_tokens: 400,
                        output_tokens: 100,
                        cost_cents: 40,
                        mission_id: Some(mission.id),
                        role: None,
                        run_id: Some("run-a".into()),
                    })
                    .unwrap(),
                )
                .unwrap();
        }
        settle_recorded(&db, &plan("run-a", "openai", Some(mission.id), 51), 40).await.unwrap();
        // 40 recorded + a fresh 61¢ estimate crosses — but a 50¢ call fits
        let err = reserve(&db, &plan("run-c", "openai", Some(mission.id), 61)).await.unwrap_err();
        assert!(err.to_string().starts_with("cost_ceiling:"));
        assert!(reserve(&db, &plan("run-d", "openai", Some(mission.id), 50)).await.is_ok());
    }

    // ---- the status read model ----

    #[tokio::test]
    async fn trust_status_renders_dials_ceilings_and_meters() {
        let db = test_db();
        let mission = create_mission(&db, Autonomy::Suggest, 100).await;
        configure_autonomy(&db, "global", None, "suggest").await.unwrap();
        configure_ceiling(&db, "global", None, 2000).await.unwrap();
        configure_autonomy(&db, "target", Some("openai".into()), "watch").await.unwrap();
        configure_ceiling(&db, "target", Some("openai".into()), 500).await.unwrap();
        // some recorded spend against the mission + target + a run
        {
            let conn = db.0.lock().await;
            let store = EventStore::new(&conn);
            store
                .append(
                    NewEvent::spend_recorded(crate::domain::spend::SpendRecordedPayload {
                        provider: "openai".into(),
                        model: "gpt-4o".into(),
                        input_tokens: 100,
                        output_tokens: 100,
                        cost_cents: 82,
                        mission_id: Some(mission.id),
                        role: Some("drafter".into()),
                        run_id: Some("run-82".into()),
                    })
                    .unwrap(),
                )
                .unwrap();
            store
                .append(
                    NewEvent::target_spend_recorded(TargetSpendRecordedPayload {
                        target: "openai".into(),
                        cost_cents: 82,
                        run_id: Some("run-82".into()),
                        mission_id: Some(mission.id),
                    })
                    .unwrap(),
                )
                .unwrap();
        }
        let status = status(&db).await.unwrap();
        assert_eq!(status.runtime_state, RuntimeState::Running);
        assert_eq!(status.global_autonomy, Some(Autonomy::Suggest));
        assert_eq!(status.global_ceiling_cents, Some(2000));
        assert_eq!(status.global_spend_cents, 82);
        assert_eq!(status.target_dials.len(), 1);
        assert_eq!(status.target_dials[0].mode, Autonomy::Watch);
        assert_eq!(status.targets.len(), 1);
        assert_eq!(status.targets[0].target, "openai");
        assert_eq!(status.targets[0].spend_cents, 82);
        assert_eq!(status.targets[0].ceiling_cents, Some(500));
        // the mission meter: 82¢ of the 100¢ ceiling, near (80%+)
        assert_eq!(status.missions.len(), 1);
        assert_eq!(status.missions[0].spend_cents, 82);
        assert_eq!(status.missions[0].ceiling_cents, 100);
        assert_eq!(status.missions[0].state, SpendState::Near);
        // the last-run line: 82¢, against the global ceiling (the run's
        // mission ceiling is tighter — the meter above shows it)
        assert_eq!(status.last_run.as_ref().unwrap().run_id, "run-82");
        assert_eq!(status.last_run.as_ref().unwrap().spend_cents, 82);
        let _ = MissionStatus::Active; // the mission folds active, unspent terminators
    }

    #[tokio::test]
    async fn configure_commands_validate_their_scope() {
        let db = test_db();
        assert!(configure_autonomy(&db, "galaxy", None, "watch").await.is_err());
        assert!(configure_autonomy(&db, "global", None, "auto").await.is_err());
        assert!(configure_ceiling(&db, "mission", Some("not-a-uuid".into()), 5).await.is_err());
        assert!(configure_ceiling(&db, "mission", None, 5).await.is_err());
    }
}
