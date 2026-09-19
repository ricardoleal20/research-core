// Missions domain (FR-1, AD-12): a mission is created by one `mission.created`
// event whose payload carries the fields that make the mission *terminable* —
// a stop condition and a falsifiable success criterion, both non-nullable and
// non-empty, validated at the domain edge (not just the UI). The payload
// schema is owned here and exposed as a typed constructor (AD-15); the shell
// appends only through it, never via raw event JSON.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::eventstore::{Actor, EventError, NewEvent, StoredEvent};

pub const MISSION_CREATED: &str = "mission.created";

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
}

fn require_non_empty(field: &str, value: &str) -> Result<(), EventError> {
    if value.trim().is_empty() {
        return Err(EventError::Invalid(format!(
            "mission.{field} must not be empty — a mission that cannot end cannot exist (AD-12)"
        )));
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
        Self::new(
            MISSION_CREATED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )
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
}

/// Pure fold of `mission.created` events into mission read models (AD-1:
/// current state is exclusively a projection over the log). Events are folded
/// in `seq` order; a corrupt payload fails loudly rather than being skipped.
pub struct MissionsProjection;

impl MissionsProjection {
    pub fn fold(events: &[StoredEvent]) -> Result<Vec<Mission>, EventError> {
        let mut missions = Vec::new();
        for event in events {
            if event.kind != MISSION_CREATED {
                continue;
            }
            missions.push(Self::mission_from_event(event)?);
        }
        Ok(missions)
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
            })
        );
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
