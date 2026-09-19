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

use crate::eventstore::{Actor, EventError, NewEvent, StoredEvent};

pub const MISSION_CREATED: &str = "mission.created";

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
    /// question / stop_condition / success_criterion is empty or blank.
    pub fn mission_created(payload: MissionCreatedPayload) -> Result<Self, EventError> {
        require_non_empty("question", &payload.question)?;
        require_non_empty("stop_condition", &payload.stop_condition)?;
        require_non_empty("success_criterion", &payload.success_criterion)?;
        validate_roles(&payload.roles)?;
        Self::new(
            MISSION_CREATED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )
    }
}

/// A mission's lifecycle status (Story 1.4). Derived by the fold: a mission
/// is `active` from creation until an event referencing it transitions it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    Active,
    AwaitingReview,
    Completed,
    Stopped,
    Failed,
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
        let mut missions: Vec<Mission> = Vec::new();
        let mut index: HashMap<Uuid, usize> = HashMap::new();
        for event in events {
            if event.kind == MISSION_CREATED {
                let mission = Self::mission_from_event(event)?;
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
                MISSION_AWAITING_REVIEW => mission.status = MissionStatus::AwaitingReview,
                MISSION_COMPLETED => mission.status = MissionStatus::Completed,
                MISSION_STOPPED => mission.status = MissionStatus::Stopped,
                MISSION_FAILED => mission.status = MissionStatus::Failed,
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
    pub fn runs_for(events: &[StoredEvent], mission_id: Uuid) -> Vec<MissionRun> {
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
            })
            .collect()
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
            status: MissionStatus::Active,
            spend_cents: 0,
            spend_state: SpendState::Ok,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventstore::EventStore;
    use rusqlite::Connection;
    use serde_json::json;

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
        }
    }

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
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
                },
                MissionRun {
                    seq: spend_event.seq,
                    id: spend_event.id,
                    ts: spend_event.ts,
                    kind: SPEND_RECORDED.into(),
                    actor: "system:telemetry".into(),
                    role: Some("critic".into()),
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
