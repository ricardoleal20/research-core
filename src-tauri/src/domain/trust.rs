// Trust domain (FR-5, Story 2.4): the cost-enforcement and control events —
// the reservation protocol (AD-10), the autonomy dial (AD-15d), the kill
// switch (AD-15e), and the per-target spend attribution. Everything here is
// HARD, not advisory: a dispatch the runtime did not reserve never reaches a
// provider, and a reservation that would cross ANY applicable ceiling is
// refused as an event.
//
// Event kinds owned here:
//
// - `spend.reserved` / `spend.released` — the reservation protocol (AD-10).
//   Before ANY provider dispatch the runtime appends `spend.reserved`; the
//   reservation settles as `spend.released` when the call's `spend.recorded`
//   landed (or the call failed/was skipped). The in-flight ledger (reserved
//   minus released, by run id) is what makes concurrent dispatches unable to
//   overshoot a ceiling: the ceiling check reads recorded spend PLUS
//   in-flight reservations, and check+append run in one critical section.
// - `spend.refused` — a ceiling refusal is an event (payload: run, scope,
//   ceiling, would-be cost); refusals count toward the run's terminal state
//   (AD-12 — see `domain::nightshift`).
// - `runtime.killed` / `runtime.resumed` — the kill switch (AD-15e).
//   Runtime-owned core commands (actor `system (runtime)`); while
//   `runtime.killed` is the latest runtime-state event by seq, every dispatch
//   is refused. In-kill semantics: a call already in flight at kill time is
//   allowed to land its `spend.recorded` — no cancellation mid-wire.
// - `autonomy.configured` / `ceiling.configured` — dial and ceiling settings
//   at global / mission / target scopes (FR-5.1/5.2/5.3), persisted as events
//   (AD-1); the fold renders the effective mode per scope with
//   most-restrictive-wins across scopes (AD-15d).
// - `target.spend_recorded` — per-target (compute target) spend attribution
//   (named for AD-10's `target.spend.recorded` concept; the AD-2 kind grammar
//   allows exactly one dot, so the verb carries the compound)
//   (AD-10): the runtime appends it beside the call's `spend.recorded` so
//   per-target ceilings fold from their own events.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::missions::Autonomy;
use crate::eventstore::{Actor, EventError, NewEvent, StoredEvent, SystemComponent};

pub const SPEND_RESERVED: &str = "spend.reserved";
pub const SPEND_RELEASED: &str = "spend.released";
pub const SPEND_REFUSED: &str = "spend.refused";
pub const RUNTIME_KILLED: &str = "runtime.killed";
pub const RUNTIME_RESUMED: &str = "runtime.resumed";
pub const AUTONOMY_CONFIGURED: &str = "autonomy.configured";
pub const CEILING_CONFIGURED: &str = "ceiling.configured";
pub const TARGET_SPEND_RECORDED: &str = "target.spend_recorded";

/// The reason a reservation settled without its spend landing — `recorded`
/// when the call's `spend.recorded` took over, `provider_error` / `skipped`
/// when nothing was spent.
pub const RELEASE_RECORDED: &str = "recorded";
pub const RELEASE_PROVIDER_ERROR: &str = "provider_error";

/// The run.failed reason the dispatcher writes when a run's step was refused
/// on a cost ceiling (AD-12 interplay: the evaluator turns it into
/// `mission.stopped` with the `cost_ceiling_reached` signal).
pub const COST_CEILING_RUN_REASON: &str = "cost_ceiling_reached";

// ---------------------------------------------------------------------------
// Scopes (FR-5.1/5.2/5.3)
// ---------------------------------------------------------------------------

/// The scope a dial or ceiling is configured at: `global`, `mission`, or
/// `target` (a compute target — a provider or the local CLI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Global,
    Mission,
    Target,
}

impl Scope {
    /// Parse the wire form used by shell commands.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "global" => Some(Self::Global),
            "mission" => Some(Self::Mission),
            "target" => Some(Self::Target),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Mission => "mission",
            Self::Target => "target",
        }
    }
}

// ---------------------------------------------------------------------------
// Payloads + typed constructors (AD-15)
// ---------------------------------------------------------------------------

/// The `spend.reserved` payload (AD-10): the reservation token is the run id;
/// `amount_cents` is the pre-call estimate the runtime holds against every
/// applicable ceiling until the call settles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpendReservedPayload {
    pub run_id: String,
    /// The compute target the call runs on (provider name or `cli`).
    pub target: String,
    pub amount_cents: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mission_id: Option<Uuid>,
}

/// The `spend.released` payload: the reservation settled — `recorded` when
/// the call's `spend.recorded` took over the accounting, else why the
/// reserved headroom was returned unspent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpendReleasedPayload {
    pub run_id: String,
    pub reason: String,
}

/// The `spend.refused` payload: which ceiling (scope + cents) the call would
/// have crossed, and the would-be spend including in-flight reservations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpendRefusedPayload {
    pub run_id: String,
    pub scope: String,
    pub ceiling_cents: u64,
    pub would_be_cost_cents: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mission_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// The `target.spend.recorded` payload (AD-10): one call's indicative cost
/// attributed to its compute target, so per-target ceilings fold from their
/// own events (never by re-counting `spend.recorded`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetSpendRecordedPayload {
    pub target: String,
    pub cost_cents: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mission_id: Option<Uuid>,
}

/// The `autonomy.configured` payload: one dial setting at one scope. The
/// latest event per (scope, scope_id) wins in the fold; across scopes,
/// most-restrictive-wins (AD-15d).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutonomyConfiguredPayload {
    pub scope: Scope,
    /// The mission id or target name; `None` for the global scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    pub mode: Autonomy,
}

/// The `ceiling.configured` payload: one spend ceiling at one scope (FR-5.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CeilingConfiguredPayload {
    pub scope: Scope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    pub ceiling_cents: u64,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `spend.reserved` event comes
    /// into being — the runtime holding headroom against the ceilings before
    /// a provider dispatch. Actor is `system (runtime)`: enforcement is
    /// solely the runtime's (AD-10).
    pub fn spend_reserved(payload: SpendReservedPayload) -> Result<Self, EventError> {
        if payload.run_id.trim().is_empty() {
            return Err(EventError::Invalid(
                "spend.reserved requires a run id — the reservation token (AD-10)".into(),
            ));
        }
        if payload.target.trim().is_empty() {
            return Err(EventError::Invalid(
                "spend.reserved requires a target — headroom is held per compute target (AD-10)"
                    .into(),
            ));
        }
        Self::new(
            SPEND_RESERVED,
            Actor::System { component: SystemComponent::Runtime },
            serde_json::to_value(&payload)?,
        )
    }

    /// Typed constructor (AD-15): the one way a reservation settles. Actor is
    /// `system (runtime)`.
    pub fn spend_released(payload: SpendReleasedPayload) -> Result<Self, EventError> {
        if payload.run_id.trim().is_empty() {
            return Err(EventError::Invalid(
                "spend.released requires a run id — a reservation settles by its token (AD-10)"
                    .into(),
            ));
        }
        if payload.reason.trim().is_empty() {
            return Err(EventError::Invalid(
                "spend.released requires a reason — receipts show how the reservation settled"
                    .into(),
            ));
        }
        Self::new(
            SPEND_RELEASED,
            Actor::System { component: SystemComponent::Runtime },
            serde_json::to_value(&payload)?,
        )
    }

    /// Typed constructor (AD-15): the one way a ceiling refusal lands in the
    /// log — an event, never a silent no. Actor is `system (runtime)`.
    pub fn spend_refused(payload: SpendRefusedPayload) -> Result<Self, EventError> {
        if payload.run_id.trim().is_empty() {
            return Err(EventError::Invalid(
                "spend.refused requires a run id — the refusal names the refused call (AD-10)"
                    .into(),
            ));
        }
        if payload.scope.trim().is_empty() {
            return Err(EventError::Invalid(
                "spend.refused requires a scope — global | mission | target (FR-5.3)".into(),
            ));
        }
        Self::new(
            SPEND_REFUSED,
            Actor::System { component: SystemComponent::Runtime },
            serde_json::to_value(&payload)?,
        )
    }

    /// Typed constructor (AD-15e): the one way the kill switch fires — a
    /// runtime-owned core command. Actor is `system (runtime)`; while this is
    /// the latest runtime-state event by seq, every dispatch is refused.
    pub fn runtime_killed() -> Result<Self, EventError> {
        Self::new(
            RUNTIME_KILLED,
            Actor::System { component: SystemComponent::Runtime },
            serde_json::json!({}),
        )
    }

    /// Typed constructor (AD-15e): the one way a killed runtime is restored.
    pub fn runtime_resumed() -> Result<Self, EventError> {
        Self::new(
            RUNTIME_RESUMED,
            Actor::System { component: SystemComponent::Runtime },
            serde_json::json!({}),
        )
    }

    /// Typed constructor (AD-15): the one way a dial setting changes — an
    /// event, actor=user (a shell command). Fails loudly on an unknown scope
    /// or a scope id missing where the scope needs one.
    pub fn autonomy_configured(payload: AutonomyConfiguredPayload) -> Result<Self, EventError> {
        validate_scope_id(payload.scope, payload.scope_id.as_deref())?;
        Self::new(
            AUTONOMY_CONFIGURED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )
    }

    /// Typed constructor (AD-15): the one way a ceiling setting changes — an
    /// event, actor=user (a shell command).
    pub fn ceiling_configured(payload: CeilingConfiguredPayload) -> Result<Self, EventError> {
        validate_scope_id(payload.scope, payload.scope_id.as_deref())?;
        Self::new(
            CEILING_CONFIGURED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )
    }

    /// Typed constructor (AD-15): the one way per-target spend lands — the
    /// runtime appends it beside the call's `spend.recorded`. Actor is
    /// `system (runtime)`.
    pub fn target_spend_recorded(
        payload: TargetSpendRecordedPayload,
    ) -> Result<Self, EventError> {
        if payload.target.trim().is_empty() {
            return Err(EventError::Invalid(
                "target.spend.recorded requires a target — spend must be attributable (AD-10)"
                    .into(),
            ));
        }
        Self::new(
            TARGET_SPEND_RECORDED,
            Actor::System { component: SystemComponent::Runtime },
            serde_json::to_value(&payload)?,
        )
    }
}

/// A scope id is required exactly where the scope is not global: a mission
/// uuid for `mission`, a target name for `target`.
fn validate_scope_id(scope: Scope, scope_id: Option<&str>) -> Result<(), EventError> {
    match scope {
        Scope::Global => Ok(()),
        Scope::Mission => {
            let id = scope_id.unwrap_or_default().trim();
            if id.is_empty() || Uuid::parse_str(id).is_err() {
                return Err(EventError::Invalid(
                    "scope_id: a mission-scoped setting requires the mission's uuid".into(),
                ));
            }
            Ok(())
        }
        Scope::Target => {
            if scope_id.unwrap_or_default().trim().is_empty() {
                return Err(EventError::Invalid(
                    "scope_id: a target-scoped setting requires the target's name".into(),
                ));
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// The trust fold (AD-1: current state is exclusively a projection)
// ---------------------------------------------------------------------------

/// The trust configuration folded from the log: dials and ceilings at every
/// scope (latest event per key wins), plus the runtime's kill state (the
/// latest runtime-state event by seq).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrustConfig {
    /// `true` while `runtime.killed` is the latest runtime-state event.
    pub runtime_killed: bool,
    pub global_autonomy: Option<Autonomy>,
    /// Latest `autonomy.configured` per mission id.
    pub mission_autonomy: HashMap<Uuid, Autonomy>,
    /// Latest `autonomy.configured` per target name.
    pub target_autonomy: HashMap<String, Autonomy>,
    pub global_ceiling_cents: Option<u64>,
    pub mission_ceiling_cents: HashMap<Uuid, u64>,
    pub target_ceiling_cents: HashMap<String, u64>,
}

impl TrustConfig {
    /// The configured mission dial, or the mission's creation autonomy when
    /// never reconfigured (latest wins within a scope — the same dial,
    /// edited, like schedules).
    pub fn mission_dial(&self, mission_id: Uuid, created: Autonomy) -> Autonomy {
        self.mission_autonomy.get(&mission_id).copied().unwrap_or(created)
    }
}

/// Pure fold of the log into the trust configuration. Events fold in `seq`
/// order; the latest `autonomy.configured` / `ceiling.configured` per
/// (scope, scope_id) wins; the runtime is killed while `runtime.killed` is
/// the latest runtime-state event by seq.
pub fn trust_config(events: &[StoredEvent]) -> TrustConfig {
    // The shared fold cursor (AD-1, Story 2.6): a rolled-back kill switch or
    // dial change never happened for the read model.
    let cursor = crate::domain::checkpoints::FoldCursor::over(events);
    let events = &cursor.live_owned(events);
    let mut config = TrustConfig::default();
    for event in events {
        match event.kind.as_str() {
            RUNTIME_KILLED => config.runtime_killed = true,
            RUNTIME_RESUMED => config.runtime_killed = false,
            AUTONOMY_CONFIGURED => {
                let Ok(payload) =
                    serde_json::from_value::<AutonomyConfiguredPayload>(event.payload.clone())
                else {
                    continue; // a corrupt settings event never breaks the fold
                };
                match payload.scope {
                    Scope::Global => config.global_autonomy = Some(payload.mode),
                    Scope::Mission => {
                        if let Some(id) = payload.scope_id.as_deref().and_then(|s| Uuid::parse_str(s).ok()) {
                            config.mission_autonomy.insert(id, payload.mode);
                        }
                    }
                    Scope::Target => {
                        if let Some(id) = payload.scope_id {
                            config.target_autonomy.insert(id, payload.mode);
                        }
                    }
                }
            }
            CEILING_CONFIGURED => {
                let Ok(payload) =
                    serde_json::from_value::<CeilingConfiguredPayload>(event.payload.clone())
                else {
                    continue;
                };
                match payload.scope {
                    Scope::Global => config.global_ceiling_cents = Some(payload.ceiling_cents),
                    Scope::Mission => {
                        if let Some(id) = payload.scope_id.as_deref().and_then(|s| Uuid::parse_str(s).ok()) {
                            config.mission_ceiling_cents.insert(id, payload.ceiling_cents);
                        }
                    }
                    Scope::Target => {
                        if let Some(id) = payload.scope_id {
                            config.target_ceiling_cents.insert(id, payload.ceiling_cents);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    config
}

// ---------------------------------------------------------------------------
// Most restrictive wins (AD-15d)
// ---------------------------------------------------------------------------

/// The autonomy ordering: `watch` (0) is the most restrictive stop,
/// `act_with_receipts` (2) the least. Most restrictive wins = the minimum.
pub fn restrictiveness(mode: Autonomy) -> u8 {
    match mode {
        Autonomy::Watch => 0,
        Autonomy::Suggest => 1,
        Autonomy::ActWithReceipts => 2,
    }
}

/// The most restrictive of two dial positions (AD-15d).
pub fn most_restrictive(a: Autonomy, b: Autonomy) -> Autonomy {
    if restrictiveness(a) <= restrictiveness(b) {
        a
    } else {
        b
    }
}

/// The effective autonomy for one dispatch (AD-15d): the most restrictive of
/// the global dial (when configured), the mission's dial (its creation
/// autonomy, or the latest mission-scoped `autonomy.configured`), and the
/// target's dial (when configured). No dial position ever enables an
/// auto-merge — every state mutation stays a proposal regardless; the dial
/// governs dispatch permissions only.
pub fn effective_autonomy(
    config: &TrustConfig,
    mission: Option<(Uuid, Autonomy)>,
    target: Option<&str>,
) -> Autonomy {
    // No restriction at a scope means the scope does not tighten the dial.
    let mut dial = Autonomy::ActWithReceipts;
    if let Some(global) = config.global_autonomy {
        dial = most_restrictive(dial, global);
    }
    if let Some((mission_id, created)) = mission {
        dial = most_restrictive(dial, config.mission_dial(mission_id, created));
    }
    if let Some(target) = target {
        if let Some(&target_dial) = config.target_autonomy.get(target) {
            dial = most_restrictive(dial, target_dial);
        }
    }
    dial
}

// ---------------------------------------------------------------------------
// The spend ledgers (AD-10)
// ---------------------------------------------------------------------------

/// Recorded spend, folded per scope: every `spend.recorded` cost globally and
/// per mission; every `target.spend.recorded` cost per compute target.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpendLedger {
    pub global_cents: u64,
    pub mission_cents: HashMap<Uuid, u64>,
    pub target_cents: HashMap<String, u64>,
    /// Spend grouped by run id (the reservation token rides the call): the
    /// latest run and its cost, for the "last run: X¢ of Y¢" meter.
    pub run_cents: HashMap<String, u64>,
    pub last_run_id: Option<String>,
}

/// In-flight reservations (AD-10): `spend.reserved` amounts whose runs have
/// not yet settled (`spend.released`), per scope. This is what a concurrent
/// dispatch's ceiling check reads on top of recorded spend — the TOCTOU guard.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InFlight {
    pub global_cents: u64,
    pub mission_cents: HashMap<Uuid, u64>,
    pub target_cents: HashMap<String, u64>,
}

/// Pure fold of recorded spend per scope (AD-10). Rollback-aware through
/// the shared fold cursor (Story 2.6): orphaned spend never counted.
pub fn spend_ledger(events: &[StoredEvent]) -> SpendLedger {
    let cursor = crate::domain::checkpoints::FoldCursor::over(events);
    let events = &cursor.live_owned(events);
    let mut ledger = SpendLedger::default();
    let mut last_run_seq: i64 = 0;
    for event in events {
        match event.kind.as_str() {
            crate::domain::spend::SPEND_RECORDED => {
                let cost = event
                    .payload
                    .get("cost_cents")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                ledger.global_cents += cost;
                if let Some(id) = event
                    .payload
                    .get("mission_id")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|s| Uuid::parse_str(s).ok())
                {
                    *ledger.mission_cents.entry(id).or_insert(0) += cost;
                }
                if let Some(run_id) = event.payload.get("run_id").and_then(serde_json::Value::as_str) {
                    *ledger.run_cents.entry(run_id.to_string()).or_insert(0) += cost;
                    if event.seq >= last_run_seq {
                        last_run_seq = event.seq;
                        ledger.last_run_id = Some(run_id.to_string());
                    }
                }
            }
            TARGET_SPEND_RECORDED => {
                let cost = event
                    .payload
                    .get("cost_cents")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                if let Some(target) = event.payload.get("target").and_then(serde_json::Value::as_str) {
                    *ledger.target_cents.entry(target.to_string()).or_insert(0) += cost;
                }
            }
            _ => {}
        }
    }
    ledger
}

/// Pure fold of in-flight reservations (AD-10): reserved minus settled, by
/// scope. A reservation settles when its run id sees a `spend.released`.
pub fn in_flight(events: &[StoredEvent]) -> InFlight {
    let mut reserved: HashMap<String, (u64, Option<Uuid>, Option<String>)> = HashMap::new();
    let mut settled: std::collections::HashSet<String> = std::collections::HashSet::new();
    for event in events {
        match event.kind.as_str() {
            SPEND_RESERVED => {
                if let (Some(run_id), Some(amount)) = (
                    event.payload.get("run_id").and_then(serde_json::Value::as_str),
                    event.payload.get("amount_cents").and_then(serde_json::Value::as_u64),
                ) {
                    let mission_id = event
                        .payload
                        .get("mission_id")
                        .and_then(serde_json::Value::as_str)
                        .and_then(|s| Uuid::parse_str(s).ok());
                    let target = event
                        .payload
                        .get("target")
                        .and_then(serde_json::Value::as_str)
                        .map(String::from);
                    reserved.insert(run_id.to_string(), (amount, mission_id, target));
                    settled.remove(run_id);
                }
            }
            SPEND_RELEASED => {
                if let Some(run_id) = event.payload.get("run_id").and_then(serde_json::Value::as_str) {
                    settled.insert(run_id.to_string());
                }
            }
            _ => {}
        }
    }
    let mut flight = InFlight::default();
    for (run_id, (amount, mission_id, target)) in reserved {
        if settled.contains(&run_id) {
            continue;
        }
        flight.global_cents += amount;
        if let Some(mission_id) = mission_id {
            *flight.mission_cents.entry(mission_id).or_insert(0) += amount;
        }
        if let Some(target) = target {
            *flight.target_cents.entry(target).or_insert(0) += amount;
        }
    }
    flight
}

// ---------------------------------------------------------------------------
// The ceiling check (FR-5.3 — most restrictive wins)
// ---------------------------------------------------------------------------

/// A crossed ceiling: the scope, the ceiling, and the would-be spend
/// (recorded + in-flight + this call's estimate).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Refusal {
    pub scope: String,
    pub ceiling_cents: u64,
    pub would_be_cents: u64,
}

/// The effective mission ceiling: the creation ceiling and any
/// mission-scoped `ceiling.configured`, most restrictive wins (a configured
/// ceiling can only tighten, never loosen, what the mission was created with).
pub fn effective_mission_ceiling(config: &TrustConfig, mission_id: Uuid, created: u64) -> u64 {
    match config.mission_ceiling_cents.get(&mission_id) {
        Some(configured) => (*configured).min(created),
        None => created,
    }
}

/// Pure ceiling check (FR-5.3, AD-10): would this call cross ANY applicable
/// ceiling? Applicable ceilings: the global ceiling (when configured), the
/// mission's effective ceiling (creation ∩ configured — only for
/// mission-scoped calls), and the target's ceiling (when configured). A call
/// is refused when ANY would be crossed — most restrictive wins. Zero-cost
/// calls (simulated / CLI, estimate 0) cross nothing unless spend is already
/// strictly over a ceiling.
pub fn check_ceilings(
    config: &TrustConfig,
    ledger: &SpendLedger,
    flight: &InFlight,
    mission: Option<(Uuid, u64)>,
    target: &str,
    amount_cents: u64,
) -> Option<Refusal> {
    // Global: all recorded spend plus all in-flight reservations.
    if let Some(ceiling) = config.global_ceiling_cents {
        let would_be = ledger.global_cents + flight.global_cents + amount_cents;
        if would_be > ceiling {
            return Some(Refusal {
                scope: Scope::Global.as_str().into(),
                ceiling_cents: ceiling,
                would_be_cents: would_be,
            });
        }
    }
    // Mission: the call's mission spend plus its in-flight reservations,
    // against the effective (most restrictive) mission ceiling.
    if let Some((mission_id, created_ceiling)) = mission {
        let ceiling = effective_mission_ceiling(config, mission_id, created_ceiling);
        let current = ledger.mission_cents.get(&mission_id).copied().unwrap_or(0)
            + flight.mission_cents.get(&mission_id).copied().unwrap_or(0);
        let would_be = current + amount_cents;
        if would_be > ceiling {
            return Some(Refusal {
                scope: Scope::Mission.as_str().into(),
                ceiling_cents: ceiling,
                would_be_cents: would_be,
            });
        }
    }
    // Target: the compute target's spend plus its in-flight reservations.
    if let Some(&ceiling) = config.target_ceiling_cents.get(target) {
        let current = ledger.target_cents.get(target).copied().unwrap_or(0)
            + flight.target_cents.get(target).copied().unwrap_or(0);
        let would_be = current + amount_cents;
        if would_be > ceiling {
            return Some(Refusal {
                scope: Scope::Target.as_str().into(),
                ceiling_cents: ceiling,
                would_be_cents: would_be,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::missions::MissionCreatedPayload;
    use crate::eventstore::{EventStore, NewEvent};
    use rusqlite::Connection;
    use serde_json::json;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    fn mission_payload(autonomy: Autonomy, ceiling: u64) -> MissionCreatedPayload {
        MissionCreatedPayload {
            question: "Does X hold?".into(),
            stop_condition: "Stop after $5.".into(),
            success_criterion: "A rater agrees.".into(),
            autonomy,
            spend_ceiling_cents: ceiling,
            roles: vec![],
            schedule: "daily-03:00".into(),
        }
    }

    // ---- typed constructors ----

    #[test]
    fn reservation_constructors_carry_the_runtime_actor() {
        let mission = Uuid::new_v4();
        let reserved =
            NewEvent::spend_reserved(SpendReservedPayload {
                run_id: "step-1".into(),
                target: "openai".into(),
                amount_cents: 51,
                mission_id: Some(mission),
            })
            .unwrap();
        assert_eq!(reserved.kind, SPEND_RESERVED);
        assert_eq!(
            reserved.actor,
            Actor::System { component: SystemComponent::Runtime }
        );
        assert_eq!(reserved.payload["amount_cents"], json!(51));
        assert_eq!(reserved.payload["mission_id"], json!(mission.to_string()));

        let released = NewEvent::spend_released(SpendReleasedPayload {
            run_id: "step-1".into(),
            reason: RELEASE_RECORDED.into(),
        })
        .unwrap();
        assert_eq!(released.kind, SPEND_RELEASED);

        let refused = NewEvent::spend_refused(SpendRefusedPayload {
            run_id: "step-2".into(),
            scope: "mission".into(),
            ceiling_cents: 100,
            would_be_cost_cents: 102,
            mission_id: Some(mission),
            target: Some("openai".into()),
        })
        .unwrap();
        assert_eq!(refused.kind, SPEND_REFUSED);
        assert_eq!(refused.payload["ceiling_cents"], json!(100));

        // empty run ids / targets / reasons never become events
        assert!(NewEvent::spend_reserved(SpendReservedPayload {
            run_id: "  ".into(),
            target: "openai".into(),
            amount_cents: 1,
            mission_id: None,
        })
        .is_err());
        assert!(NewEvent::spend_released(SpendReleasedPayload {
            run_id: "r".into(),
            reason: " ".into(),
        })
        .is_err());
        assert!(NewEvent::spend_refused(SpendRefusedPayload {
            run_id: "r".into(),
            scope: String::new(),
            ceiling_cents: 1,
            would_be_cost_cents: 1,
            mission_id: None,
            target: None,
        })
        .is_err());
    }

    #[test]
    fn kill_and_resume_are_runtime_owned_events() {
        let killed = NewEvent::runtime_killed().unwrap();
        assert_eq!(killed.kind, RUNTIME_KILLED);
        assert_eq!(
            killed.actor,
            Actor::System { component: SystemComponent::Runtime }
        );
        let resumed = NewEvent::runtime_resumed().unwrap();
        assert_eq!(resumed.kind, RUNTIME_RESUMED);
        assert_eq!(
            resumed.actor,
            Actor::System { component: SystemComponent::Runtime }
        );
    }

    #[test]
    fn settings_constructors_validate_their_scope() {
        // global needs no scope id; mission needs a uuid; target needs a name
        assert!(NewEvent::autonomy_configured(AutonomyConfiguredPayload {
            scope: Scope::Global,
            scope_id: None,
            mode: Autonomy::Watch,
        })
        .is_ok());
        assert!(NewEvent::ceiling_configured(CeilingConfiguredPayload {
            scope: Scope::Global,
            scope_id: None,
            ceiling_cents: 2000,
        })
        .is_ok());
        assert!(NewEvent::autonomy_configured(AutonomyConfiguredPayload {
            scope: Scope::Mission,
            scope_id: Some("not-a-uuid".into()),
            mode: Autonomy::Watch,
        })
        .is_err());
        assert!(NewEvent::ceiling_configured(CeilingConfiguredPayload {
            scope: Scope::Target,
            scope_id: Some("  ".into()),
            ceiling_cents: 100,
        })
        .is_err());
        let mission = Uuid::new_v4();
        let ev = NewEvent::autonomy_configured(AutonomyConfiguredPayload {
            scope: Scope::Mission,
            scope_id: Some(mission.to_string()),
            mode: Autonomy::Watch,
        })
        .unwrap();
        assert_eq!(ev.actor, Actor::User);
        assert_eq!(ev.payload["scope"], json!("mission"));
        assert_eq!(ev.payload["mode"], json!("watch"));
    }

    // ---- the trust fold ----

    #[test]
    fn trust_config_folds_latest_per_scope_and_kills_by_seq() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let mission = Uuid::new_v4();
        store
            .append(NewEvent::autonomy_configured(AutonomyConfiguredPayload {
                scope: Scope::Global,
                scope_id: None,
                mode: Autonomy::Suggest,
            })
            .unwrap())
            .unwrap();
        // latest wins per key
        store
            .append(NewEvent::autonomy_configured(AutonomyConfiguredPayload {
                scope: Scope::Global,
                scope_id: None,
                mode: Autonomy::Watch,
            })
            .unwrap())
            .unwrap();
        store
            .append(NewEvent::autonomy_configured(AutonomyConfiguredPayload {
                scope: Scope::Mission,
                scope_id: Some(mission.to_string()),
                mode: Autonomy::ActWithReceipts,
            })
            .unwrap())
            .unwrap();
        store
            .append(NewEvent::ceiling_configured(CeilingConfiguredPayload {
                scope: Scope::Target,
                scope_id: Some("openai".into()),
                ceiling_cents: 500,
            })
            .unwrap())
            .unwrap();
        let config = trust_config(&store.events_all().unwrap());
        assert_eq!(config.global_autonomy, Some(Autonomy::Watch));
        assert_eq!(
            config.mission_autonomy.get(&mission),
            Some(&Autonomy::ActWithReceipts)
        );
        assert_eq!(config.target_ceiling_cents.get("openai"), Some(&500));
        assert!(!config.runtime_killed);

        // kill → killed; resume → running (latest by seq wins)
        store.append(NewEvent::runtime_killed().unwrap()).unwrap();
        assert!(trust_config(&store.events_all().unwrap()).runtime_killed);
        store.append(NewEvent::runtime_resumed().unwrap()).unwrap();
        assert!(!trust_config(&store.events_all().unwrap()).runtime_killed);
    }

    // ---- most restrictive wins (AD-15d) ----

    #[test]
    fn effective_autonomy_is_the_most_restrictive_across_scopes() {
        let mission = Uuid::new_v4();
        let mut config = TrustConfig::default();
        // no dial configured anywhere: the mission's creation autonomy governs
        assert_eq!(
            effective_autonomy(&config, Some((mission, Autonomy::Suggest)), Some("openai")),
            Autonomy::Suggest
        );
        // global watch beats a suggest mission
        config.global_autonomy = Some(Autonomy::Watch);
        assert_eq!(
            effective_autonomy(&config, Some((mission, Autonomy::Suggest)), Some("openai")),
            Autonomy::Watch
        );
        // a target dial beats a permissive mission and absent global
        config.global_autonomy = None;
        config.target_autonomy.insert("openai".into(), Autonomy::Watch);
        assert_eq!(
            effective_autonomy(&config, Some((mission, Autonomy::ActWithReceipts)), Some("openai")),
            Autonomy::Watch
        );
        // the same target dial does not tighten another target
        assert_eq!(
            effective_autonomy(&config, Some((mission, Autonomy::ActWithReceipts)), Some("anthropic")),
            Autonomy::ActWithReceipts
        );
        // a configured mission dial replaces the creation autonomy (latest
        // wins within a scope — the same dial, edited)
        config.mission_autonomy.insert(mission, Autonomy::Suggest);
        assert_eq!(
            effective_autonomy(&config, Some((mission, Autonomy::ActWithReceipts)), None),
            Autonomy::Suggest
        );
        // every combination matrix: min of the applicable stops
        for (g, m, t, want) in [
            (Some(Autonomy::Watch), Some(Autonomy::ActWithReceipts), Some(Autonomy::Suggest), Autonomy::Watch),
            (Some(Autonomy::Suggest), Some(Autonomy::ActWithReceipts), Some(Autonomy::Watch), Autonomy::Watch),
            (Some(Autonomy::Suggest), Some(Autonomy::ActWithReceipts), None, Autonomy::Suggest),
            (None, Some(Autonomy::Watch), None, Autonomy::Watch),
            (None, None, None, Autonomy::ActWithReceipts),
        ] {
            let mut c = TrustConfig::default();
            c.global_autonomy = g;
            if let Some(m) = m {
                c.mission_autonomy.insert(mission, m);
            }
            if let Some(t) = t {
                c.target_autonomy.insert("openai".into(), t);
            }
            let mission_dial = if m.is_some() {
                Some((mission, Autonomy::ActWithReceipts)) // configured dial replaces creation
            } else {
                None
            };
            assert_eq!(
                effective_autonomy(&c, mission_dial, Some("openai")),
                want,
                "matrix ({g:?}, {m:?}, {t:?})"
            );
        }
    }

    // ---- the ledgers ----

    #[test]
    fn spend_ledger_folds_recorded_spend_per_scope() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let mission = Uuid::new_v4();
        // recorded spend: global + mission + run attribution
        store
            .append(NewEvent::spend_recorded(crate::domain::spend::SpendRecordedPayload {
                provider: "openai".into(),
                model: "gpt-4o".into(),
                input_tokens: 100,
                output_tokens: 100,
                cost_cents: 82,
                mission_id: Some(mission),
                role: Some("drafter".into()),
                run_id: Some("run-a".into()),
            })
            .unwrap())
            .unwrap();
        store
            .append(NewEvent::spend_recorded(crate::domain::spend::SpendRecordedPayload {
                provider: "openai".into(),
                model: "gpt-4o".into(),
                input_tokens: 100,
                output_tokens: 100,
                cost_cents: 18,
                mission_id: None,
                role: None,
                run_id: Some("run-a".into()),
            })
            .unwrap())
            .unwrap();
        // target spend: its own events, per target
        store
            .append(NewEvent::target_spend_recorded(TargetSpendRecordedPayload {
                target: "openai".into(),
                cost_cents: 82,
                run_id: Some("run-a".into()),
                mission_id: Some(mission),
            })
            .unwrap())
            .unwrap();
        let ledger = spend_ledger(&store.events_all().unwrap());
        assert_eq!(ledger.global_cents, 100); // 82 + 18
        assert_eq!(ledger.mission_cents.get(&mission), Some(&82));
        assert_eq!(ledger.target_cents.get("openai"), Some(&82));
        // run attribution: run-a spent 100¢, and it is the last run
        assert_eq!(ledger.run_cents.get("run-a"), Some(&100));
        assert_eq!(ledger.last_run_id.as_deref(), Some("run-a"));
    }

    #[test]
    fn in_flight_counts_reserved_minus_settled() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let mission = Uuid::new_v4();
        // two live reservations on different runs
        for (run, amount) in [("run-a", 51), ("run-b", 51)] {
            store
                .append(NewEvent::spend_reserved(SpendReservedPayload {
                    run_id: run.into(),
                    target: "openai".into(),
                    amount_cents: amount,
                    mission_id: Some(mission),
                })
                .unwrap())
                .unwrap();
        }
        let flight = in_flight(&store.events_all().unwrap());
        assert_eq!(flight.global_cents, 102);
        assert_eq!(flight.mission_cents.get(&mission), Some(&102));
        assert_eq!(flight.target_cents.get("openai"), Some(&102));
        // run-a settles: only run-b remains in flight
        store
            .append(NewEvent::spend_released(SpendReleasedPayload {
                run_id: "run-a".into(),
                reason: RELEASE_RECORDED.into(),
            })
            .unwrap())
            .unwrap();
        let flight = in_flight(&store.events_all().unwrap());
        assert_eq!(flight.global_cents, 51);
        assert_eq!(flight.mission_cents.get(&mission), Some(&51));
        // a re-reserved run id (a step retry) counts again
        store
            .append(NewEvent::spend_reserved(SpendReservedPayload {
                run_id: "run-a".into(),
                target: "openai".into(),
                amount_cents: 10,
                mission_id: Some(mission),
            })
            .unwrap())
            .unwrap();
        assert_eq!(in_flight(&store.events_all().unwrap()).global_cents, 61);
    }

    // ---- the ceiling check (FR-5.3) ----

    #[test]
    fn check_ceilings_refuses_when_any_scope_would_cross() {
        let mission = Uuid::new_v4();
        let mut config = TrustConfig::default();
        config.global_ceiling_cents = Some(600);
        config.target_ceiling_cents.insert("openai".into(), 150);
        let mut ledger = SpendLedger::default();
        ledger.global_cents = 400;
        ledger.mission_cents.insert(mission, 60);
        ledger.target_cents.insert("openai".into(), 100);
        let mut flight = InFlight::default();
        flight.global_cents = 50;
        flight.mission_cents.insert(mission, 10);
        flight.target_cents.insert("openai".into(), 0);

        // a $1.00-headroom mission: recorded 60 + in-flight 10 + 51 > 100
        let refusal = check_ceilings(
            &config,
            &ledger,
            &flight,
            Some((mission, 100)),
            "openai",
            51,
        )
        .expect("the mission ceiling would be crossed");
        assert_eq!(refusal.scope, "mission");
        assert_eq!(refusal.ceiling_cents, 100);
        assert_eq!(refusal.would_be_cents, 121);

        // under the mission ceiling but over the target ceiling
        let refusal = check_ceilings(
            &config,
            &ledger,
            &InFlight::default(),
            Some((mission, 1000)),
            "openai",
            51,
        )
        .expect("the target ceiling would be crossed");
        assert_eq!(refusal.scope, "target");
        assert_eq!(refusal.ceiling_cents, 150);
        assert_eq!(refusal.would_be_cents, 151);

        // over the global ceiling even with room elsewhere
        let mut tight_global = config.clone();
        tight_global.global_ceiling_cents = Some(500);
        let refusal = check_ceilings(
            &tight_global,
            &ledger,
            &flight,
            Some((mission, 10_000)),
            "simulated-free",
            60,
        )
        .expect("the global ceiling would be crossed");
        assert_eq!(refusal.scope, "global");
        assert_eq!(refusal.would_be_cents, 510);

        // nothing crosses: no refusal
        assert!(check_ceilings(
            &config,
            &SpendLedger::default(),
            &InFlight::default(),
            Some((mission, 100)),
            "openai",
            51,
        )
        .is_none());
        // zero-cost calls cross nothing
        assert!(check_ceilings(
            &config,
            &ledger,
            &flight,
            Some((mission, 100)),
            "openai",
            0,
        )
        .is_none());
    }

    #[test]
    fn a_configured_ceiling_can_only_tighten_a_mission() {
        let mission = Uuid::new_v4();
        let mut config = TrustConfig::default();
        assert_eq!(effective_mission_ceiling(&config, mission, 500), 500);
        config.mission_ceiling_cents.insert(mission, 200);
        assert_eq!(effective_mission_ceiling(&config, mission, 500), 200);
        // a looser configured ceiling never loosens the creation ceiling
        config.mission_ceiling_cents.insert(mission, 900);
        assert_eq!(effective_mission_ceiling(&config, mission, 500), 500);
    }

    #[test]
    fn a_zero_ceiling_mission_refuses_any_spend() {
        let mission = Uuid::new_v4();
        let refusal = check_ceilings(
            &TrustConfig::default(),
            &SpendLedger::default(),
            &InFlight::default(),
            Some((mission, 0)),
            "openai",
            1,
        )
        .expect("a zero ceiling refuses any spend at all (AD-10)");
        assert_eq!(refusal.scope, "mission");
        assert_eq!(refusal.ceiling_cents, 0);
    }

    /// The fold stays honest over a real mission creation + reservations —
    /// the exact shape the runtime's dispatcher writes.
    #[test]
    fn the_runtime_s_event_shapes_fold_end_to_end() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let created = store
            .append(NewEvent::mission_created(mission_payload(Autonomy::Suggest, 100)).unwrap())
            .unwrap();
        store
            .append(NewEvent::spend_reserved(SpendReservedPayload {
                run_id: "step-1".into(),
                target: "openai".into(),
                amount_cents: 51,
                mission_id: Some(created.id),
            })
            .unwrap())
            .unwrap();
        let events = store.events_all().unwrap();
        let config = trust_config(&events);
        assert!(!config.runtime_killed);
        let flight = in_flight(&events);
        assert_eq!(flight.mission_cents.get(&created.id), Some(&51));
        // the effective dial: no configured dials, mission created at suggest
        assert_eq!(
            effective_autonomy(&config, Some((created.id, Autonomy::Suggest)), Some("openai")),
            Autonomy::Suggest
        );
        // a second concurrent 51¢ reservation over the $1.00 mission ceiling
        // is refused: 0 recorded + 51 in flight + 51 > 100
        let refusal = check_ceilings(
            &config,
            &spend_ledger(&events),
            &flight,
            Some((created.id, 100)),
            "openai",
            51,
        )
        .expect("two concurrent dispatches cannot both hold the headroom");
        assert_eq!(refusal.would_be_cents, 102);
    }
}
