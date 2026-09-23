// Missions domain (FR-1, AD-12): a mission is created by one `mission.created`
// event whose payload carries the fields that make the mission *terminable* —
// a stop condition and a falsifiable success criterion, both non-nullable and
// non-empty, validated at the domain edge (not just the UI). The payload
// schema is owned here and exposed as a typed constructor (AD-15); the shell
// appends only through it, never via raw event JSON.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::eventstore::{Actor, EventError, EventStore, NewEvent, StoredEvent};

pub const MISSION_CREATED: &str = "mission.created";

/// A mission's Night Shift schedule changed (Story 2.3, FR-4.1): the one way
/// a schedule moves after creation — an event, cause-linked to the mission's
/// creation, actor=user (a shell command). The fold takes the latest.
pub const MISSION_SCHEDULED: &str = "mission.scheduled";

/// The default Night Shift schedule every mission launches with (FR-4.1):
/// a nightly literature scan at 03:00 local time.
pub const DEFAULT_SCHEDULE: &str = "daily-03:00";

/// A Night Shift schedule (FR-4.1): `off` (no scheduled runs) or a daily
/// `daily-HH:MM` local-time tick. Parsed from the wire form stored on the
/// mission; invalid schedules never become events (the typed constructors
/// reject them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Schedule {
    Off,
    Daily { hour: u32, minute: u32 },
}

impl Schedule {
    /// Parse the wire form: `off` | `daily-HH:MM` (00–23 / 00–59).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("off") {
            return Some(Self::Off);
        }
        let Some(rest) = s.strip_prefix("daily-") else {
            return None;
        };
        let (h, m) = rest.split_once(':')?;
        if h.len() != 2 || m.len() != 2 {
            return None; // strict HH:MM — the wire form stays stable
        }
        let hour: u32 = h.parse().ok()?;
        let minute: u32 = m.parse().ok()?;
        if hour > 23 || minute > 59 {
            return None;
        }
        Some(Self::Daily { hour, minute })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            _ => "daily",
        }
    }
}

/// The two agent role names a mission's runtime config knows (Story 2.1,
/// AD-9): drafters advance the mission; critics evaluate the drafters' work.
pub const ROLE_DRAFTER: &str = "drafter";
pub const ROLE_CRITIC: &str = "critic";

/// One agent role of a mission's runtime config (Story 2.1, NFR-3): a named
/// role bound to one (provider, model) pair resolved through the provider
/// layer (AD-9) — never one algorithm grading its own homework.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleConfig {
    /// `drafter` | `critic` — the role's name is its identity.
    pub name: String,
    /// The provider-layer provider name (or `simulated` / `cli`).
    pub provider: String,
    /// The model identifier the role runs on.
    pub model: String,
}

impl RoleConfig {
    pub fn drafter(provider: &str, model: &str) -> Self {
        Self { name: ROLE_DRAFTER.into(), provider: provider.into(), model: model.into() }
    }

    pub fn critic(provider: &str, model: &str) -> Self {
        Self { name: ROLE_CRITIC.into(), provider: provider.into(), model: model.into() }
    }

    /// The (provider, model) pair the different-model rule compares (NFR-3):
    /// trimmed and lowercased — `OpenAI`/`GPT-4o` and `openai`/`gpt-4o` are
    /// the same algorithm.
    pub fn pair(&self) -> (String, String) {
        (
            self.provider.trim().to_lowercase(),
            self.model.trim().to_lowercase(),
        )
    }
}

/// Mission status transitions (Story 1.4 read side): recognized kinds of the
/// events that move a mission's lifecycle. The fold derives status from the
/// log — last transition in `seq` order wins.
pub const MISSION_AWAITING_REVIEW: &str = "mission.awaiting_review";
pub const MISSION_COMPLETED: &str = "mission.completed";
pub const MISSION_STOPPED: &str = "mission.stopped";
pub const MISSION_FAILED: &str = "mission.failed";
/// Quick-capture (Story 6.15, FR-21.3, resolving FR-1.5): a question
/// captured on a remote surface lands on the home machine as a PENDING
/// MISSION CARD — a draft awaiting its stop condition and falsifiable
/// success criterion. Capture never launches a mission by itself (FR-1.2):
/// a draft is never Active and the Night Shift never runs it.
pub const MISSION_QUICK_CAPTURE: &str = "mission.quick_capture";
/// The owner completing a captured draft on the home machine: the
/// terminator fields arrive, the draft becomes Active — the mission launches
/// only through this explicit completion.
pub const MISSION_CAPTURE_COMPLETED: &str = "mission.capture_completed";

/// Spend is folded from `spend.recorded` events referencing the mission
/// (the provider layer appends them via `domain::spend`'s typed constructor;
/// the fold already understands them).
pub use super::spend::SPEND_RECORDED;

/// The autonomy dial's three named stops (FR-5.1). Three contracts, not a
/// percentage; governs dispatch permissions only (AD-15d).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Autonomy {
    Watch,
    Suggest,
    ActWithReceipts,
}

impl Autonomy {
    /// Parse the wire form used by shell commands (`watch | suggest |
    /// act_with_receipts`).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "watch" => Some(Self::Watch),
            "suggest" => Some(Self::Suggest),
            "act_with_receipts" => Some(Self::ActWithReceipts),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Watch => "watch",
            Self::Suggest => "suggest",
            Self::ActWithReceipts => "act_with_receipts",
        }
    }
}

/// The `mission.created` payload — non-nullable and validated at construction
/// (AD-12: a mission that cannot end cannot exist).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissionCreatedPayload {
    pub question: String,
    pub stop_condition: String,
    pub success_criterion: String,
    pub autonomy: Autonomy,
    pub spend_ceiling_cents: u64,
    /// The mission's agent-role config (Story 2.1): drafter + critic, each
    /// bound to a (provider, model) pair. Shells resolve layer defaults
    /// before construction, so a post-2.1 creation always carries a full
    /// config; the `serde` default keeps pre-2.1 events foldable (their
    /// roles resolve from the layer at step time).
    #[serde(default)]
    pub roles: Vec<RoleConfig>,
    /// The mission's Night Shift schedule (Story 2.3, FR-4.1): `off` or
    /// `daily-HH:MM`. Defaults to a nightly 03:00 literature scan; the
    /// `serde` default keeps pre-2.3 events foldable.
    #[serde(default = "default_schedule")]
    pub schedule: String,
}

fn default_schedule() -> String {
    DEFAULT_SCHEDULE.to_string()
}

/// The `mission.quick_capture` payload (Story 6.15, FR-21.3): the captured
/// question and the surface it came from (`mobile` — attribution visible in
/// receipts, NFR-13). Nothing else: a capture is a draft, not a mission —
/// the stop condition and falsifiable success criterion are the owner's to
/// give on the home machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuickCapturePayload {
    pub question: String,
    pub surface: String,
}

/// The `mission.capture_completed` payload: the terminator fields the owner
/// gave the draft on the home machine. Validated at construction like a
/// creation (AD-12) — a completed capture IS a launchable mission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureCompletedPayload {
    pub mission_id: Uuid,
    pub stop_condition: String,
    pub success_criterion: String,
    pub autonomy: Autonomy,
    pub spend_ceiling_cents: u64,
    #[serde(default = "default_schedule")]
    pub schedule: String,
}

fn require_non_empty(field: &str, value: &str) -> Result<(), EventError> {
    if value.trim().is_empty() {
        return Err(EventError::Invalid(format!(
            "mission.{field} must not be empty — a mission that cannot end cannot exist (AD-12)"
        )));
    }
    Ok(())
}

/// Validate a mission's role config (NFR-3, AD-9) — BEFORE any event exists.
/// Every role needs a known name and a complete (provider, model) pair, and
/// no critic may resolve to the same pair as any drafter: never one
/// algorithm grading its own homework. The simulated fallback is exempt —
/// it is the no-key mock, not an algorithm, and exempting it keeps the app
/// fully usable with no provider key configured (Story 2.1 AC).
fn validate_roles(roles: &[RoleConfig]) -> Result<(), EventError> {
    for role in roles {
        if role.name != ROLE_DRAFTER && role.name != ROLE_CRITIC {
            return Err(EventError::Invalid(format!(
                "mission.roles: unknown role name `{}` — expected drafter | critic",
                role.name
            )));
        }
        if role.provider.trim().is_empty() || role.model.trim().is_empty() {
            return Err(EventError::Invalid(format!(
                "mission.roles: role `{}` requires a provider and a model — every role runs through the provider layer (AD-9)",
                role.name
            )));
        }
    }
    let drafters: Vec<(String, String)> = roles
        .iter()
        .filter(|r| r.name == ROLE_DRAFTER)
        .map(RoleConfig::pair)
        .collect();
    for critic in roles.iter().filter(|r| r.name == ROLE_CRITIC) {
        let pair = critic.pair();
        if pair.0 == "simulated" {
            continue;
        }
        if drafters.contains(&pair) {
            return Err(EventError::Invalid(format!(
                "same_model_critic: the critic role resolves to {} + {} — the same (provider, model) pair as a drafter. Configure a different model for the critic (NFR-3: never one algorithm grading its own homework)",
                critic.provider.trim(),
                critic.model.trim()
            )));
        }
    }
    Ok(())
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `mission.created` event comes
    /// into being. Actor is always the user (shells append only
    /// human-attributed command events). Construction fails loudly when any of
    /// question / stop_condition / success_criterion is empty or blank. An
    /// empty schedule launches on the default nightly scan (daily-03:00); a
    /// non-empty schedule must parse (`off` | `daily-HH:MM`).
    pub fn mission_created(payload: MissionCreatedPayload) -> Result<Self, EventError> {
        require_non_empty("question", &payload.question)?;
        require_non_empty("stop_condition", &payload.stop_condition)?;
        require_non_empty("success_criterion", &payload.success_criterion)?;
        validate_roles(&payload.roles)?;
        validate_schedule(&payload.schedule)?;
        let mut payload = payload;
        if payload.schedule.trim().is_empty() {
            payload.schedule = DEFAULT_SCHEDULE.into(); // FR-4.1: nightly by default
        }
        Self::new(
            MISSION_CREATED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )
    }

    /// Typed constructor (AD-15, Story 2.3): the one way a mission's Night
    /// Shift schedule changes — a `mission.scheduled` event, actor=user,
    /// cause-linked to the mission's creation. Rejects schedules that do not
    /// parse (`off` | `daily-HH:MM`) at the domain edge.
    pub fn mission_scheduled(schedule: &str, mission_id: Uuid) -> Result<Self, EventError> {
        validate_schedule(schedule)?;
        Self::new(
            MISSION_SCHEDULED,
            Actor::User,
            serde_json::json!({ "schedule": schedule.trim() }),
        )
        .map(|e| e.with_causes(vec![mission_id]))
    }

    /// Typed constructor (AD-15, Story 6.15, FR-21.3): the one way a
    /// quick-capture comes into being — a `mission.quick_capture` event,
    /// actor=user, carrying the captured question and its surface. A blank
    /// question is refused at the edge: a capture says something.
    pub fn mission_quick_capture(
        question: &str,
        surface: &str,
    ) -> Result<Self, EventError> {
        require_non_empty("question", question)?;
        let surface = if surface.trim().is_empty() { "mobile" } else { surface };
        let payload = QuickCapturePayload {
            question: question.trim().to_string(),
            surface: surface.to_string(),
        };
        Self::new(
            MISSION_QUICK_CAPTURE,
            Actor::User,
            serde_json::to_value(&payload)?,
        )
    }

    /// Typed constructor (AD-15, Story 6.15): the one way a captured draft
    /// is completed — a `mission.capture_completed` event, actor=user,
    /// cause-linked to the capture. The terminator fields are validated like
    /// a creation (AD-12): a completed capture is a launchable mission, so
    /// it may not exist without a stop condition and a falsifiable success
    /// criterion. An empty schedule launches on the default nightly scan.
    pub fn mission_capture_completed(
        payload: CaptureCompletedPayload,
    ) -> Result<Self, EventError> {
        if payload.mission_id.is_nil() {
            return Err(EventError::Invalid(
                "mission.mission_id must not be nil — a completion names its captured draft"
                    .into(),
            ));
        }
        require_non_empty("stop_condition", &payload.stop_condition)?;
        require_non_empty("success_criterion", &payload.success_criterion)?;
        validate_schedule(&payload.schedule)?;
        let mut payload = payload;
        if payload.schedule.trim().is_empty() {
            payload.schedule = DEFAULT_SCHEDULE.into();
        }
        Self::new(
            MISSION_CAPTURE_COMPLETED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )
        .map(|e| e.with_causes(vec![payload.mission_id]))
    }
}

/// A schedule must parse (`off` | `daily-HH:MM`); an empty one is the default
/// nightly scan (a mission never fails creation over an unset schedule).
fn validate_schedule(schedule: &str) -> Result<(), EventError> {
    let s = schedule.trim();
    if s.is_empty() {
        return Ok(());
    }
    if Schedule::parse(s).is_none() {
        return Err(EventError::Invalid(format!(
            "mission.schedule: `{s}` — expected `off` or `daily-HH:MM` (e.g. daily-03:00)"
        )));
    }
    Ok(())
}

/// A mission's lifecycle status (Story 1.4). Derived by the fold: a mission
/// is `active` from creation until an event referencing it transitions it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    /// A quick-captured draft (Story 6.15, FR-21.3): the card exists, its
    /// terminator fields do not — it awaits the owner's stop condition and
    /// falsifiable success criterion. Never runs, never spends.
    Draft,
    Active,
    AwaitingReview,
    Completed,
    Stopped,
    Failed,
}

impl MissionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::AwaitingReview => "awaiting_review",
            Self::Completed => "completed",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}

/// The spend meter's three states (DESIGN.md components.spend-meter): `ok`
/// under 80% of the ceiling, `near` at 80%+, `blocked` at/over the hard
/// ceiling (AD-10 — dispatch is refused past it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpendState {
    Ok,
    Near,
    Blocked,
}

/// Pure spend classification: nothing spent is `ok` (a zero ceiling with zero
/// spend has refused nothing); any spend at or over the ceiling is `blocked`
/// (a zero ceiling refuses any spend at all); otherwise `near` from 80%.
pub fn spend_state(spend_cents: u64, ceiling_cents: u64) -> SpendState {
    if spend_cents == 0 {
        return SpendState::Ok;
    }
    if ceiling_cents == 0 || spend_cents >= ceiling_cents {
        return SpendState::Blocked;
    }
    if spend_cents * 5 >= ceiling_cents * 4 {
        SpendState::Near
    } else {
        SpendState::Ok
    }
}

/// A mission as read from the log — the read model the UI renders (AD-8).
/// Field names are camelCase on the wire (Tauri 2 convention for the shell).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mission {
    /// The `mission.created` event's id — the mission's identity.
    pub id: Uuid,
    /// The `seq` of the creation event (the sole ordering, AD-2).
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub question: String,
    pub stop_condition: String,
    pub success_criterion: String,
    pub autonomy: Autonomy,
    pub spend_ceiling_cents: u64,
    /// The mission's agent-role config, as appended with `mission.created`
    /// (Story 2.1); empty for pre-2.1 missions (roles resolve from the layer
    /// at step time).
    pub roles: Vec<RoleConfig>,
    /// The mission's Night Shift schedule (Story 2.3): `off` | `daily-HH:MM`.
    /// Default `daily-03:00`; the latest `mission.scheduled` event wins.
    pub schedule: String,
    /// Derived: lifecycle status from events referencing this mission.
    pub status: MissionStatus,
    /// Derived: total `spend.recorded` cost against the ceiling, in cents.
    pub spend_cents: u64,
    /// Derived: the spend meter state (`ok` / `near` / `blocked`).
    pub spend_state: SpendState,
}

/// One entry of a mission's run list (FR-1.3): any log event that references
/// the mission — cause-linked to the creation event or carrying the mission's
/// id in its payload — excluding the creation event itself. The full receipts
/// timeline is a later story; this is the basic read model the missions home
/// drills into.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionRun {
    pub seq: i64,
    pub id: Uuid,
    pub ts: DateTime<Utc>,
    /// The raw event kind, rendered as-is (mono, receipt voice).
    pub kind: String,
    /// Short actor label: `user` | `agent` | `system:<component>`.
    pub actor: String,
    /// The agent role a `spend.recorded` event is attributed to (Story 2.1
    /// per-role receipts); `None` for events without a role tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// The run the event belongs to (Story 2.5, FR-6.2): the payload `run_id`
    /// (run lifecycle, spend, reservation events) or the agent actor's run id
    /// (proposals); `None` for events without a run link. The runs list's
    /// receipt drill-down opens this run's receipt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// A one-line detail the run row renders (Stories 6.2–6.4, AD-12):
    /// the `reason` a `job.failed` event carries — so a job row shows its
    /// terminal reason right in the runs drill-down, never a "failed"
    /// with no explanation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

fn actor_label(actor: &Actor) -> String {
    match actor {
        Actor::User => "user".into(),
        Actor::Agent { .. } => "agent".into(),
        Actor::System { component } => format!("system:{}", format!("{component:?}").to_lowercase()),
    }
}

/// Does this event reference the given mission? A event references a mission
/// when the mission's id is among its causes or in its `mission_id` payload.
fn references(event: &StoredEvent, mission_id: Uuid) -> bool {
    event.causes.contains(&mission_id)
        || event
            .payload
            .get("mission_id")
            .and_then(serde_json::Value::as_str)
            .map(|s| s == mission_id.to_string())
            .unwrap_or(false)
}

/// Pure fold of the log into mission read models (AD-1: current state is
/// exclusively a projection over the log). Events are folded in `seq` order;
/// a corrupt payload fails loudly rather than being skipped. Status and spend
/// are derived from events referencing each mission — last transition wins.
pub struct MissionsProjection;

impl MissionsProjection {
    pub fn fold(events: &[StoredEvent]) -> Result<Vec<Mission>, EventError> {
        // The shared fold cursor (AD-1, Story 2.6): fold the live events —
        // rollbacks orphan their suffix; the read model returns to the
        // checkpoint state and continues after the rollback action.
        let cursor = crate::domain::checkpoints::FoldCursor::over(events);
        let events = &cursor.live_owned(events);
        let mut missions: Vec<Mission> = Vec::new();
        let mut index: HashMap<Uuid, usize> = HashMap::new();
        for event in events {
            if event.kind == MISSION_CREATED {
                let mission = Self::mission_from_event(event)?;
                index.insert(mission.id, missions.len());
                missions.push(mission);
                continue;
            }
            if event.kind == crate::domain::submissions::SUBMISSION_CREATED {
                // A submission mission (Story 6.13, FR-19.4): choosing a
                // venue spawns a mission — the checklist appears on the
                // missions home and dashboard like any mission. Autonomy
                // is act-with-receipts by construction (agent pre-checks
                // ride the quarantine), spend ceiling 0 (the pre-check is
                // deterministic and free), schedule off.
                let mission = Self::submission_from_event(event)?;
                index.insert(mission.id, missions.len());
                missions.push(mission);
                continue;
            }
            if event.kind == MISSION_QUICK_CAPTURE {
                // A captured draft (Story 6.15, FR-21.3): the card exists,
                // its terminators do not — status Draft, schedule off, no
                // budget. Never runs until the owner completes it.
                let mission = Self::draft_from_capture(event)?;
                index.insert(mission.id, missions.len());
                missions.push(mission);
                continue;
            }
            // Any other event referencing a mission updates its read model.
            let Some(i) = Self::referenced(&index, event) else {
                continue;
            };
            let mission = &mut missions[i];
            match event.kind.as_str() {
                MISSION_CAPTURE_COMPLETED => {
                    // The owner completed the draft (Story 6.15): the
                    // terminator fields arrive, the mission becomes Active —
                    // a completed capture is a launchable mission.
                    let payload: CaptureCompletedPayload =
                        serde_json::from_value(event.payload.clone()).map_err(|e| {
                            EventError::Invalid(format!(
                                "corrupt {MISSION_CAPTURE_COMPLETED} payload at seq {}: {e}",
                                event.seq
                            ))
                        })?;
                    mission.stop_condition = payload.stop_condition;
                    mission.success_criterion = payload.success_criterion;
                    mission.autonomy = payload.autonomy;
                    mission.spend_ceiling_cents = payload.spend_ceiling_cents;
                    mission.schedule = payload.schedule;
                    if mission.status == MissionStatus::Draft {
                        mission.status = MissionStatus::Active;
                    }
                }
                MISSION_AWAITING_REVIEW => mission.status = MissionStatus::AwaitingReview,
                MISSION_COMPLETED => mission.status = MissionStatus::Completed,
                MISSION_STOPPED => mission.status = MissionStatus::Stopped,
                MISSION_FAILED => mission.status = MissionStatus::Failed,
                MISSION_SCHEDULED => {
                    // The latest schedule event wins (schedules are edited by
                    // appending, like every mutation — AD-1).
                    if let Some(schedule) = event.payload.get("schedule").and_then(serde_json::Value::as_str) {
                        mission.schedule = schedule.to_string();
                    }
                }
                SPEND_RECORDED => {
                    mission.spend_cents += event
                        .payload
                        .get("cost_cents")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0);
                }
                _ => {}
            }
        }
        for mission in &mut missions {
            mission.spend_state = spend_state(mission.spend_cents, mission.spend_ceiling_cents);
        }
        Ok(missions)
    }

    /// The mission an event references, if any (payload `mission_id` first,
    /// then causes) — `None` for events that reference no known mission.
    fn referenced(index: &HashMap<Uuid, usize>, event: &StoredEvent) -> Option<usize> {
        if let Some(id) = event
            .payload
            .get("mission_id")
            .and_then(serde_json::Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
        {
            if let Some(&i) = index.get(&id) {
                return Some(i);
            }
        }
        event.causes.iter().find_map(|c| index.get(c).copied())
    }

    /// The run list of one mission (FR-1.3): every event referencing it, in
    /// `seq` order, excluding the creation event itself. Pure — no IO.
    /// Rollback-aware through the shared fold cursor (Story 2.6): orphaned
    /// events leave the run list — they are superseded history, listable
    /// through the checkpoint read model, never here.
    pub fn runs_for(events: &[StoredEvent], mission_id: Uuid) -> Vec<MissionRun> {
        let cursor = crate::domain::checkpoints::FoldCursor::over(events);
        let events = &cursor.live_owned(events);
        events
            .iter()
            .filter(|event| event.id != mission_id && references(event, mission_id))
            .map(|event| MissionRun {
                seq: event.seq,
                id: event.id,
                ts: event.ts,
                kind: event.kind.clone(),
                actor: actor_label(&event.actor),
                role: event
                    .payload
                    .get("role")
                    .and_then(serde_json::Value::as_str)
                    .map(String::from),
                run_id: event
                    .payload
                    .get("run_id")
                    .and_then(serde_json::Value::as_str)
                    .map(String::from)
                    .or_else(|| match &event.actor {
                        Actor::Agent { run_id } => Some(run_id.clone()),
                        _ => None,
                    }),
                // The terminal reason a failed job row renders (Stories
                // 6.2–6.4, AD-12): a failed job's reason is the row's
                // detail — never a bare "job.failed".
                detail: if event.kind == crate::domain::jobs::JOB_FAILED {
                    event
                        .payload
                        .get("reason")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                } else {
                    None
                },
            })
            .collect()
    }

    /// A quick-captured draft's read model (Story 6.15): the card carries
    /// the question and its capture surface's stamp; everything a mission
    /// needs to end is empty by design — the draft awaits the owner.
    fn draft_from_capture(event: &StoredEvent) -> Result<Mission, EventError> {
        let payload: QuickCapturePayload =
            serde_json::from_value(event.payload.clone()).map_err(|e| {
                EventError::Invalid(format!(
                    "corrupt {MISSION_QUICK_CAPTURE} payload at seq {}: {e}",
                    event.seq
                ))
            })?;
        Ok(Mission {
            id: event.id,
            seq: event.seq,
            ts: event.ts,
            question: payload.question,
            stop_condition: String::new(),
            success_criterion: String::new(),
            autonomy: Autonomy::Watch,
            spend_ceiling_cents: 0,
            roles: Vec::new(),
            schedule: "off".into(),
            status: MissionStatus::Draft,
            spend_cents: 0,
            spend_state: SpendState::Ok,
        })
    }

    /// A mission born from `submission.created` (Story 6.13): the same
    /// read model the missions home renders, with the submission's
    /// defaults — the checklist is a mission like any other.
    fn submission_from_event(event: &StoredEvent) -> Result<Mission, EventError> {
        let payload: crate::domain::submissions::SubmissionCreatedPayload =
            serde_json::from_value(event.payload.clone()).map_err(|e| {
                EventError::Invalid(format!(
                    "corrupt {} payload at seq {}: {e}",
                    crate::domain::submissions::SUBMISSION_CREATED,
                    event.seq
                ))
            })?;
        Ok(Mission {
            id: event.id,
            seq: event.seq,
            ts: event.ts,
            question: payload.question,
            stop_condition: payload.stop_condition,
            success_criterion: payload.success_criterion,
            autonomy: Autonomy::ActWithReceipts,
            spend_ceiling_cents: 0,
            roles: Vec::new(),
            schedule: "off".into(),
            status: MissionStatus::Active,
            spend_cents: 0,
            spend_state: SpendState::Ok,
        })
    }

    fn mission_from_event(event: &StoredEvent) -> Result<Mission, EventError> {
        let payload: MissionCreatedPayload =
            serde_json::from_value(event.payload.clone()).map_err(|e| {
                EventError::Invalid(format!(
                    "corrupt {MISSION_CREATED} payload at seq {}: {e}",
                    event.seq
                ))
            })?;
        Ok(Mission {
            id: event.id,
            seq: event.seq,
            ts: event.ts,
            question: payload.question,
            stop_condition: payload.stop_condition,
            success_criterion: payload.success_criterion,
            autonomy: payload.autonomy,
            spend_ceiling_cents: payload.spend_ceiling_cents,
            roles: payload.roles,
            schedule: payload.schedule,
            status: MissionStatus::Active,
            spend_cents: 0,
            spend_state: SpendState::Ok,
        })
    }
}

/// Quick-capture (Story 6.15, FR-21.3, resolving FR-1.5): a question
/// captured on a remote surface lands as a PENDING MISSION CARD — one
/// `mission.quick_capture` event (actor=user, surface-attributed) appended
/// by the home writer, whatever surface asked. The card is a draft
/// awaiting its stop condition and falsifiable success criterion; capture
/// never launches a mission by itself (FR-1.2).
pub fn quick_capture(
    conn: &rusqlite::Connection,
    question: &str,
    surface: &str,
) -> Result<Mission, String> {
    let store = EventStore::new(conn);
    let event =
        NewEvent::mission_quick_capture(question, surface).map_err(|e| e.to_string())?;
    let stored = store.append(event).map_err(|e| e.to_string())?;
    // Return exactly what the log now holds — the read model, not the input.
    let missions = MissionsProjection::fold(&[stored]).map_err(|e| e.to_string())?;
    Ok(missions
        .into_iter()
        .next()
        .expect("the fold of one capture event yields one draft mission"))
}

/// Complete a captured draft (Story 6.15): the owner gives the draft its
/// stop condition and falsifiable success criterion on the home machine —
/// one `mission.capture_completed` event, and only then does the mission
/// become Active. A non-draft (or unknown) mission is refused with
/// `not_draft:`; blank terminators are refused at the edge (AD-12).
pub fn complete_quick_capture(
    conn: &rusqlite::Connection,
    mission_id: Uuid,
    stop_condition: &str,
    success_criterion: &str,
    autonomy: Autonomy,
    spend_ceiling_cents: u64,
) -> Result<Mission, String> {
    let store = EventStore::new(conn);
    let events = store.events_all().map_err(|e| e.to_string())?;
    let missions = MissionsProjection::fold(&events).map_err(|e| e.to_string())?;
    let draft = missions
        .iter()
        .find(|m| m.id == mission_id)
        .ok_or_else(|| format!("not_found: no mission with id `{mission_id}`"))?;
    if draft.status != MissionStatus::Draft {
        return Err(format!(
            "not_draft: mission `{mission_id}` is `{}`, not a captured draft — completion is the draft's path (Story 6.15)",
            draft.status.as_str()
        ));
    }
    let event = NewEvent::mission_capture_completed(CaptureCompletedPayload {
        mission_id,
        stop_condition: stop_condition.to_string(),
        success_criterion: success_criterion.to_string(),
        autonomy,
        spend_ceiling_cents,
        schedule: String::new(), // the default nightly scan, like a creation
    })
    .map_err(|e| e.to_string())?;
    store.append(event).map_err(|e| e.to_string())?;
    let missions =
        MissionsProjection::fold(&store.events_all().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    missions
        .into_iter()
        .find(|m| m.id == mission_id)
        .ok_or_else(|| "not_found: the completed draft vanished from the fold".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventstore::EventStore;
    use rusqlite::Connection;
    use serde_json::json;

    #[test]
    fn a_failed_job_run_carries_its_reason_as_the_detail() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let created = store.append(NewEvent::mission_created(payload()).unwrap()).unwrap();
        let submitted = store
            .append(NewEvent::job_submitted(crate::domain::jobs::JobSubmittedPayload {
                mission_id: created.id,
                target: "cluster-1".into(),
                handle: "h-1".into(),
                spec: crate::domain::jobs::JobSpec {
                    cmd: "python3".into(),
                    args: vec![],
                    env: Default::default(),
                    resources: None,
                    workdir: None,
                },
            })
            .unwrap())
            .unwrap();
        store
            .append(
                NewEvent::job_failed(crate::domain::jobs::JobLifecyclePayload {
                    mission_id: created.id,
                    job_id: submitted.id,
                    target: "cluster-1".into(),
                    code: Some(3),
                    reason: Some("exit_code_3".into()),
                })
                .unwrap(),
            )
            .unwrap();
        let runs = MissionsProjection::runs_for(&store.events_all().unwrap(), created.id);
        let failed =
            runs.iter().find(|r| r.kind == "job.failed").expect("the job.failed run exists");
        assert_eq!(failed.detail.as_deref(), Some("exit_code_3"));
        // other run rows carry no detail (the runs-for test's own rows)
        assert!(runs.iter().filter(|r| r.kind != "job.failed").all(|r| r.detail.is_none()));
    }

    fn payload() -> MissionCreatedPayload {
        MissionCreatedPayload {
            question: "Does retrieval-augmented drafting reduce hallucinated citations?".into(),
            stop_condition: "Stop after 3 rounds or $5.00 spent, whichever comes first.".into(),
            success_criterion: "A blind rater finds zero fabricated citations in 20 sampled claims.".into(),
            autonomy: Autonomy::Suggest,
            spend_ceiling_cents: 500,
            roles: vec![
                RoleConfig::drafter("openai", "gpt-4o"),
                RoleConfig::critic("anthropic", "claude-sonnet-4-5"),
            ],
            schedule: "daily-03:00".into(),
        }
    }

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    /// Quick-capture (Story 6.15, FR-21.3, resolving FR-1.5): a captured
    /// question lands as a PENDING MISSION CARD — a draft awaiting its stop
    /// condition and falsifiable success criterion, attributed to its
    /// surface. Capture never launches a mission by itself (FR-1.2): the
    /// draft is never Active, never scheduled.
    #[test]
    fn quick_capture_lands_a_pending_draft_card() {
        let conn = mem_conn();
        let mission = quick_capture(&conn, "  Does attention sparsity hold at 32k?  ", "mobile")
            .unwrap();
        assert_eq!(mission.status, MissionStatus::Draft);
        assert_eq!(mission.question, "Does attention sparsity hold at 32k?");
        assert_eq!(mission.stop_condition, "", "the terminators are the owner's to give");
        assert_eq!(mission.success_criterion, "");
        assert_eq!(mission.schedule, "off", "a draft never runs (FR-1.2)");
        assert_eq!(mission.spend_ceiling_cents, 0, "a draft has no budget");
        // the audit trail: one user-actor capture event, surface-attributed
        let events = EventStore::new(&conn).events_all().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, MISSION_QUICK_CAPTURE);
        assert_eq!(events[0].actor, Actor::User);
        let payload: QuickCapturePayload =
            serde_json::from_value(events[0].payload.clone()).unwrap();
        assert_eq!(payload.surface, "mobile");
        // a blank question is refused at the edge
        assert!(quick_capture(&conn, "   ", "mobile").is_err());
    }

    /// Completing the draft (Story 6.15): the owner gives the terminators on
    /// the home machine — the mission becomes Active with the default
    /// nightly schedule, exactly like a creation. A non-draft is refused
    /// with `not_draft:`; blank terminators are refused at the edge (AD-12).
    #[test]
    fn completing_a_draft_fills_the_terminators_and_launches() {
        let conn = mem_conn();
        let draft = quick_capture(&conn, "Does X hold up?", "mobile").unwrap();
        let mission = complete_quick_capture(
            &conn,
            draft.id,
            "Stop after $5.",
            "A blind rater agrees.",
            Autonomy::Suggest,
            500,
        )
        .unwrap();
        assert_eq!(mission.status, MissionStatus::Active);
        assert_eq!(mission.stop_condition, "Stop after $5.");
        assert_eq!(mission.success_criterion, "A blind rater agrees.");
        assert_eq!(mission.autonomy, Autonomy::Suggest);
        assert_eq!(mission.spend_ceiling_cents, 500);
        assert_eq!(mission.schedule, DEFAULT_SCHEDULE, "nightly by default, like a creation");
        // the completion is a second event — the capture stays in the log
        let events = EventStore::new(&conn).events_all().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].kind, MISSION_CAPTURE_COMPLETED);
        assert_eq!(events[1].actor, Actor::User);
        // a completed mission is not a draft — a second completion refuses
        let err = complete_quick_capture(&conn, draft.id, "s", "c", Autonomy::Watch, 100)
            .unwrap_err();
        assert!(err.starts_with("not_draft:"), "unexpected: {err}");
        // an unknown mission is refused honestly
        let err = complete_quick_capture(&conn, Uuid::new_v4(), "s", "c", Autonomy::Watch, 100)
            .unwrap_err();
        assert!(err.starts_with("not_found:"), "unexpected: {err}");
        // blank terminators never even construct the event (AD-12)
        let conn = mem_conn();
        let draft = quick_capture(&conn, "Q?", "mobile").unwrap();
        assert!(
            NewEvent::mission_capture_completed(CaptureCompletedPayload {
                mission_id: draft.id,
                stop_condition: "  ".into(),
                success_criterion: "c".into(),
                autonomy: Autonomy::Watch,
                spend_ceiling_cents: 100,
                schedule: String::new(),
            })
            .is_err()
        );
    }

    /// A creation event is not a draft: completing it is refused — the
    /// draft path belongs to captured cards only.
    #[test]
    fn a_created_mission_is_not_completable_as_a_draft() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let created = store.append(NewEvent::mission_created(payload()).unwrap()).unwrap();
        let err =
            complete_quick_capture(&conn, created.id, "s", "c", Autonomy::Watch, 100)
                .unwrap_err();
        assert!(err.starts_with("not_draft:"), "unexpected: {err}");
    }

    #[test]
    fn constructor_rejects_empty_terminator_fields() {
        for field in ["question", "stop_condition", "success_criterion"] {
            let mut p = payload();
            match field {
                "question" => p.question = "   ".into(),
                "stop_condition" => p.stop_condition = String::new(),
                "success_criterion" => p.success_criterion = "\t\n".into(),
                _ => unreachable!(),
            }
            let err = NewEvent::mission_created(p)
                .expect_err("blank terminator fields must fail construction");
            assert!(
                err.to_string().contains(field),
                "error should name the field `{field}`: {err}"
            );
        }
    }

    #[test]
    fn constructor_carries_the_user_actor_and_snake_case_kind() {
        let ev = NewEvent::mission_created(payload()).unwrap();
        assert_eq!(ev.kind, "mission.created");
        assert_eq!(ev.actor, Actor::User);
        assert_eq!(
            ev.payload,
            json!({
                "question": "Does retrieval-augmented drafting reduce hallucinated citations?",
                "stop_condition": "Stop after 3 rounds or $5.00 spent, whichever comes first.",
                "success_criterion": "A blind rater finds zero fabricated citations in 20 sampled claims.",
                "autonomy": "suggest",
                "spend_ceiling_cents": 500,
                "roles": [
                    { "name": "drafter", "provider": "openai", "model": "gpt-4o" },
                    { "name": "critic", "provider": "anthropic", "model": "claude-sonnet-4-5" },
                ],
                "schedule": "daily-03:00",
            })
        );
    }

    #[test]
    fn constructor_rejects_a_critic_on_a_drafter_s_pair() {
        // The matrix (NFR-3): critic == drafter on the same (provider, model)
        // pair is rejected — before any event exists.
        let mut p = payload();
        p.roles = vec![
            RoleConfig::drafter("openai", "gpt-4o"),
            RoleConfig::critic("openai", "gpt-4o"),
        ];
        let err = NewEvent::mission_created(p)
            .expect_err("a critic on the drafter's exact pair must be rejected");
        assert!(
            err.to_string().contains("same_model_critic:"),
            "error must carry the typed prefix: {err}"
        );
        // same provider, different model => accepted (different algorithm)
        let mut p = payload();
        p.roles = vec![
            RoleConfig::drafter("openai", "gpt-4o"),
            RoleConfig::critic("openai", "gpt-4o-mini"),
        ];
        assert!(NewEvent::mission_created(p).is_ok());
        // different provider, same model name => accepted (different algorithm)
        let mut p = payload();
        p.roles = vec![
            RoleConfig::drafter("openai", "m"),
            RoleConfig::critic("anthropic", "m"),
        ];
        assert!(NewEvent::mission_created(p).is_ok());
        // the pair comparison is normalized: case and whitespace do not
        // create a "different" model
        let mut p = payload();
        p.roles = vec![
            RoleConfig::drafter("OpenAI", " GPT-4o "),
            RoleConfig::critic("openai", "gpt-4o"),
        ];
        let err = NewEvent::mission_created(p)
            .expect_err("normalized pairs must still collide");
        assert!(err.to_string().contains("same_model_critic:"), "unexpected: {err}");
    }

    /// Story 6.1 (FR-24.2): the critic ≠ drafter rule (NFR-3) is enforced
    /// across MIXED local/remote configurations exactly as remote/remote —
    /// a local critic against a remote drafter is a different algorithm
    /// (accepted); the same local pair grading itself is rejected.
    #[test]
    fn the_different_model_critic_rule_holds_across_mixed_local_remote_configs() {
        // remote drafter + local critic => different pair, accepted
        let mut p = payload();
        p.roles = vec![
            RoleConfig::drafter("openai", "gpt-4o"),
            RoleConfig::critic("local", "llama3.1:8b"),
        ];
        assert!(
            NewEvent::mission_created(p).is_ok(),
            "a local critic against a remote drafter is a different algorithm"
        );
        // local drafter + remote critic => accepted
        let mut p = payload();
        p.roles = vec![
            RoleConfig::drafter("local", "llama3.1:8b"),
            RoleConfig::critic("anthropic", "claude-sonnet-4-5"),
        ];
        assert!(NewEvent::mission_created(p).is_ok());
        // both local, different models => accepted
        let mut p = payload();
        p.roles = vec![
            RoleConfig::drafter("local", "llama3.1:8b"),
            RoleConfig::critic("local", "qwen2.5:14b"),
        ];
        assert!(NewEvent::mission_created(p).is_ok());
        // both local, the SAME pair => rejected exactly as remote/remote
        let mut p = payload();
        p.roles = vec![
            RoleConfig::drafter("local", "llama3.1:8b"),
            RoleConfig::critic("local", "llama3.1:8b"),
        ];
        let err = NewEvent::mission_created(p)
            .expect_err("a local critic on the local drafter's pair must be rejected");
        assert!(
            err.to_string().contains("same_model_critic:"),
            "error must carry the typed prefix: {err}"
        );
    }

    #[test]
    fn constructor_rejects_multi_role_configs_where_any_critic_collides() {
        // Two drafters on different pairs; the critic collides with the
        // second drafter => rejected.
        let mut p = payload();
        p.roles = vec![
            RoleConfig::drafter("openai", "gpt-4o"),
            RoleConfig::drafter("google", "gemini-2.0-flash"),
            RoleConfig::critic("google", "gemini-2.0-flash"),
        ];
        let err = NewEvent::mission_created(p)
            .expect_err("a critic colliding with ANY drafter must be rejected");
        assert!(err.to_string().contains("same_model_critic:"), "unexpected: {err}");
        // a second critic on its own pair does not condemn the first
        let mut p = payload();
        p.roles = vec![
            RoleConfig::drafter("openai", "gpt-4o"),
            RoleConfig::critic("anthropic", "claude-sonnet-4-5"),
            RoleConfig::critic("google", "gemini-2.0-flash"),
        ];
        assert!(NewEvent::mission_created(p).is_ok());
    }

    #[test]
    fn simulated_roles_are_exempt_from_the_different_model_rule() {
        // With no key configured both roles run the simulated fallback — the
        // app stays fully usable (Story 2.1 AC); the mock is not an algorithm
        // grading its own homework.
        let mut p = payload();
        p.roles = vec![
            RoleConfig::drafter("simulated", "simulated"),
            RoleConfig::critic("simulated", "simulated"),
        ];
        assert!(NewEvent::mission_created(p).is_ok());
    }

    #[test]
    fn constructor_rejects_unknown_or_incomplete_roles() {
        // unknown role name
        let mut p = payload();
        p.roles = vec![RoleConfig { name: "judge".into(), provider: "openai".into(), model: "m".into() }];
        let err = NewEvent::mission_created(p).expect_err("unknown role names must fail");
        assert!(err.to_string().contains("judge"), "unexpected: {err}");
        // missing model — a role without a pair cannot run through the layer
        let mut p = payload();
        p.roles = vec![RoleConfig::drafter("openai", "  "), RoleConfig::critic("anthropic", "m")];
        let err = NewEvent::mission_created(p).expect_err("incomplete roles must fail");
        assert!(err.to_string().contains("provider and a model"), "unexpected: {err}");
    }

    #[test]
    fn autonomy_parses_only_the_three_named_stops() {
        assert_eq!(Autonomy::parse("watch"), Some(Autonomy::Watch));
        assert_eq!(Autonomy::parse("suggest"), Some(Autonomy::Suggest));
        assert_eq!(Autonomy::parse("act_with_receipts"), Some(Autonomy::ActWithReceipts));
        for bad in ["", "act", "ACT_WITH_RECEIPTS", "auto"] {
            assert_eq!(Autonomy::parse(bad), None, "should reject {bad:?}");
        }
        // wire form round-trips through serde
        for a in [Autonomy::Watch, Autonomy::Suggest, Autonomy::ActWithReceipts] {
            let back: Autonomy = serde_json::from_value(json!(a.as_str())).unwrap();
            assert_eq!(back, a);
        }
    }

    #[test]
    fn schedule_parses_off_and_daily_times_only() {
        assert_eq!(Schedule::parse("off"), Some(Schedule::Off));
        assert_eq!(Schedule::parse("OFF"), Some(Schedule::Off));
        assert_eq!(
            Schedule::parse("daily-03:00"),
            Some(Schedule::Daily { hour: 3, minute: 0 })
        );
        assert_eq!(
            Schedule::parse("daily-23:59"),
            Some(Schedule::Daily { hour: 23, minute: 59 })
        );
        for bad in ["", "daily", "daily-3:00", "daily-24:00", "daily-03:60", "weekly-3", "daily-03", "03:00"] {
            assert!(Schedule::parse(bad).is_none(), "should reject {bad:?}");
        }
    }

    #[test]
    fn creation_defaults_to_the_nightly_scan_and_rejects_bad_schedules() {
        // an empty schedule launches on the default nightly scan (FR-4.1)
        let mut p = payload();
        p.schedule = "  ".into();
        let ev = NewEvent::mission_created(p).unwrap();
        assert_eq!(ev.payload["schedule"], json!("daily-03:00"));
        // a valid explicit schedule passes through
        let mut p = payload();
        p.schedule = "daily-05:30".into();
        let ev = NewEvent::mission_created(p).unwrap();
        assert_eq!(ev.payload["schedule"], json!("daily-05:30"));
        // `off` disables the night shift
        let mut p = payload();
        p.schedule = "off".into();
        assert!(NewEvent::mission_created(p).is_ok());
        // an unparseable schedule fails construction before any event exists
        let mut p = payload();
        p.schedule = "nightly-ish".into();
        let err = NewEvent::mission_created(p).expect_err("bad schedules must fail");
        assert!(err.to_string().contains("daily-HH:MM"), "unexpected: {err}");
    }

    #[test]
    fn mission_scheduled_updates_the_fold_latest_wins() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let created = store.append(NewEvent::mission_created(payload()).unwrap()).unwrap();
        // the creation carries the default nightly scan
        let missions = MissionsProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(missions[0].schedule, "daily-03:00");
        // a schedule change is an event — cause-linked, actor=user
        let scheduled = store
            .append(NewEvent::mission_scheduled("daily-05:30", created.id).unwrap())
            .unwrap();
        assert_eq!(scheduled.kind, MISSION_SCHEDULED);
        assert_eq!(scheduled.actor, Actor::User);
        assert_eq!(scheduled.causes, vec![created.id]);
        let missions = MissionsProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(missions[0].schedule, "daily-05:30");
        // the latest schedule event wins
        store
            .append(NewEvent::mission_scheduled("off", created.id).unwrap())
            .unwrap();
        let missions = MissionsProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(missions[0].schedule, "off");
        // an invalid schedule never becomes an event
        assert!(NewEvent::mission_scheduled("whenever", created.id).is_err());
        // a pre-2.3 creation event (no schedule field) folds on the default
        let legacy = store
            .append(
                NewEvent::new(
                    MISSION_CREATED,
                    Actor::User,
                    json!({
                        "question": "Legacy?",
                        "stop_condition": "Stop after $5.",
                        "success_criterion": "A rater agrees.",
                        "autonomy": "watch",
                        "spend_ceiling_cents": 500
                    }),
                )
                .unwrap(),
            )
            .unwrap();
        let missions = MissionsProjection::fold(&store.events_all().unwrap()).unwrap();
        let legacy_m = missions.iter().find(|m| m.id == legacy.id).unwrap();
        assert_eq!(legacy_m.schedule, "daily-03:00");
    }

    #[test]
    fn fold_reads_missions_from_the_log_in_seq_order() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        // an unrelated event is skipped by the fold
        store
            .append(NewEvent::new("import.setting", crate::eventstore::Actor::System {
                component: crate::eventstore::SystemComponent::Migration,
            }, json!({"key": "provider"})).unwrap())
            .unwrap();
        let mut expected = Vec::new();
        for i in 0..3 {
            let mut p = payload();
            p.question = format!("Question {i}?");
            let stored = store.append(NewEvent::mission_created(p).unwrap()).unwrap();
            expected.push(Mission {
                id: stored.id,
                seq: stored.seq,
                ts: stored.ts,
                question: format!("Question {i}?"),
                stop_condition: stored.payload["stop_condition"].as_str().unwrap().into(),
                success_criterion: stored.payload["success_criterion"].as_str().unwrap().into(),
                autonomy: Autonomy::Suggest,
                spend_ceiling_cents: 500,
                roles: payload().roles.clone(),
                schedule: "daily-03:00".into(),
                status: MissionStatus::Active,
                spend_cents: 0,
                spend_state: SpendState::Ok,
            });
        }

        let missions = MissionsProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(missions.len(), 3);
        assert_eq!(missions, expected);
        // seq order, store-assigned
        assert_eq!(
            missions.iter().map(|m| m.seq).collect::<Vec<_>>(),
            vec![2, 3, 4]
        );
    }

    #[test]
    fn fold_of_an_empty_log_is_empty() {
        let conn = mem_conn();
        assert!(MissionsProjection::fold(&EventStore::new(&conn).events_all().unwrap())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn spend_state_classifies_ok_near_blocked() {
        use SpendState::{Blocked, Near, Ok};
        // nothing spent is always ok — even against a zero ceiling
        assert_eq!(spend_state(0, 0), Ok);
        assert_eq!(spend_state(0, 500), Ok);
        // under 80% of ceiling: ok
        assert_eq!(spend_state(399, 500), Ok);
        // at exactly 80%: near (DESIGN.md: near at 80%+)
        assert_eq!(spend_state(400, 500), Near);
        assert_eq!(spend_state(499, 500), Near);
        // at/over the hard ceiling: blocked
        assert_eq!(spend_state(500, 500), Blocked);
        assert_eq!(spend_state(650, 500), Blocked);
        // a zero ceiling refuses any spend at all (AD-10)
        assert_eq!(spend_state(1, 0), Blocked);
    }

    #[test]
    fn fold_derives_status_and_spend_from_referencing_events() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let created = store.append(NewEvent::mission_created(payload()).unwrap()).unwrap();
        // unrelated mission — stays active with zero spend
        let other = store.append(NewEvent::mission_created(payload()).unwrap()).unwrap();
        // spend recorded against the first mission (cause-linked)
        store
            .append(
                NewEvent::new(
                    SPEND_RECORDED,
                    Actor::System { component: crate::eventstore::SystemComponent::Telemetry },
                    json!({ "mission_id": created.id.to_string(), "cost_cents": 430 }),
                )
                .unwrap()
                .with_causes(vec![created.id]),
            )
            .unwrap();
        // then the mission stops — last transition in seq order wins
        store
            .append(
                NewEvent::new(MISSION_STOPPED, Actor::User, json!({ "reason": "stop condition met" }))
                    .unwrap()
                    .with_causes(vec![created.id]),
            )
            .unwrap();
        // an event referencing the *other* mission only
        store
            .append(
                NewEvent::new(MISSION_COMPLETED, Actor::User, json!({}))
                    .unwrap()
                    .with_causes(vec![other.id]),
            )
            .unwrap();

        let missions = MissionsProjection::fold(&store.events_all().unwrap()).unwrap();
        let first = missions.iter().find(|m| m.id == created.id).unwrap();
        assert_eq!(first.status, MissionStatus::Stopped);
        assert_eq!(first.spend_cents, 430);
        assert_eq!(first.spend_state, SpendState::Near); // 430/500 = 86%
        let second = missions.iter().find(|m| m.id == other.id).unwrap();
        assert_eq!(second.status, MissionStatus::Completed);
        assert_eq!(second.spend_cents, 0);
        assert_eq!(second.spend_state, SpendState::Ok);
    }

    #[test]
    fn fold_marks_a_mission_blocked_at_its_ceiling() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let created = store.append(NewEvent::mission_created(payload()).unwrap()).unwrap();
        store
            .append(
                NewEvent::new(
                    SPEND_RECORDED,
                    Actor::System { component: crate::eventstore::SystemComponent::Telemetry },
                    json!({ "cost_cents": 500 }),
                )
                .unwrap()
                .with_causes(vec![created.id]),
            )
            .unwrap();
        let [mission] = MissionsProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one mission");
        assert_eq!(mission.spend_state, SpendState::Blocked);
    }

    #[test]
    fn runs_for_lists_referencing_events_in_seq_order() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let created = store.append(NewEvent::mission_created(payload()).unwrap()).unwrap();
        // an unrelated event references nothing
        store
            .append(
                NewEvent::new("import.setting", Actor::System {
                    component: crate::eventstore::SystemComponent::Migration,
                }, json!({"key": "provider"})).unwrap(),
            )
            .unwrap();
        let run_event = store
            .append(
                NewEvent::new(MISSION_AWAITING_REVIEW, Actor::Agent { run_id: "r1".into() }, json!({}))
                    .unwrap()
                    .with_causes(vec![created.id]),
            )
            .unwrap();
        // a payload-linked event also references the mission — role-tagged
        // spend (Story 2.1) carries its role into the run receipt
        let spend_event = store
            .append(
                NewEvent::new(
                    SPEND_RECORDED,
                    Actor::System { component: crate::eventstore::SystemComponent::Telemetry },
                    json!({ "mission_id": created.id.to_string(), "cost_cents": 10, "role": "critic" }),
                )
                .unwrap(),
            )
            .unwrap();

        let runs = MissionsProjection::runs_for(&store.events_all().unwrap(), created.id);
        assert_eq!(
            runs,
            vec![
                MissionRun {
                    seq: run_event.seq,
                    id: run_event.id,
                    ts: run_event.ts,
                    kind: MISSION_AWAITING_REVIEW.into(),
                    actor: "agent".into(),
                    role: None,
                    // the agent actor's run id (Story 2.5): the receipt
                    // drill-down's target
                    run_id: Some("r1".into()),
                    detail: None,
                },
                MissionRun {
                    seq: spend_event.seq,
                    id: spend_event.id,
                    ts: spend_event.ts,
                    kind: SPEND_RECORDED.into(),
                    actor: "system:telemetry".into(),
                    role: Some("critic".into()),
                    run_id: None,
                    detail: None,
                },
            ]
        );
        // an unknown mission id yields an empty run list
        assert!(MissionsProjection::runs_for(&store.events_all().unwrap(), Uuid::new_v4()).is_empty());
    }

    #[test]
    fn fold_fails_loudly_on_a_corrupt_payload() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        store
            .append(
                NewEvent::new(MISSION_CREATED, Actor::User, json!({"question": "orphan"}))
                    .unwrap(),
            )
            .unwrap();
        let err = MissionsProjection::fold(&store.events_all().unwrap())
            .expect_err("a payload missing terminator fields must fail the fold");
        assert!(err.to_string().contains("corrupt"), "unexpected error: {err}");
    }

    #[test]
    fn appended_event_envelope_is_complete() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let stored = store.append(NewEvent::mission_created(payload()).unwrap()).unwrap();
        // store-assigned seq + user actor + complete payload survive the round-trip
        assert_eq!(stored.seq, 1);
        assert_eq!(stored.actor, Actor::User);
        let missions = MissionsProjection::fold(&store.events_all().unwrap()).unwrap();
        let [m] = missions.as_slice() else {
            panic!("expected exactly one mission");
        };
        assert_eq!(m.id, stored.id);
        assert_eq!(m.seq, stored.seq);
        assert_eq!(m.stop_condition, payload().stop_condition);
        assert_eq!(m.success_criterion, payload().success_criterion);
    }
}
