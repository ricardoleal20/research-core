// Spend ledger events (AD-10): every real provider call appends one
// `spend.recorded` event — tokens and cost, attributable to a provider+model
// and, when the call is mission-scoped, linked to the mission. The payload
// schema is owned here and exposed as a typed constructor (AD-15); the
// provider layer appends only through it. Simulated and local-CLI calls cost
// nothing measurable and append nothing.
//
// Actor is `system (telemetry)` — spend is part of AD-15's closed
// `actor=system` enumeration and never mutates a domain entity.

use uuid::Uuid;

use serde::{Deserialize, Serialize};

use crate::eventstore::{Actor, EventError, NewEvent, SystemComponent};

pub const SPEND_RECORDED: &str = "spend.recorded";

/// The `spend.recorded` payload (AD-10). `cost_cents` is the ledger's
/// indicative cost of the call in cents, computed from token usage; the
/// mission link is present only when the call was mission-scoped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpendRecordedPayload {
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_cents: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mission_id: Option<Uuid>,
    /// The agent role the call ran for (Story 2.1): `drafter` | `critic` —
    /// present only on role-scoped runtime calls, so receipts can show
    /// per-role spend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `spend.recorded` event comes
    /// into being. Fails loudly when provider or model is empty — spend that
    /// cannot be attributed is not spend, it is a leak (AD-10).
    pub fn spend_recorded(payload: SpendRecordedPayload) -> Result<Self, EventError> {
        if payload.provider.trim().is_empty() || payload.model.trim().is_empty() {
            return Err(EventError::Invalid(
                "spend.recorded requires a provider and a model — spend must be attributable (AD-10)"
                    .into(),
            ));
        }
        Self::new(
            SPEND_RECORDED,
            Actor::System {
                component: SystemComponent::Telemetry,
            },
            serde_json::to_value(&payload)?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventstore::EventStore;
    use rusqlite::Connection;

    fn payload() -> SpendRecordedPayload {
        SpendRecordedPayload {
            provider: "openrouter".into(),
            model: "anthropic/claude-3.5-sonnet".into(),
            input_tokens: 1200,
            output_tokens: 800,
            cost_cents: 1,
            mission_id: None,
            role: None,
        }
    }

    #[test]
    fn constructor_builds_a_telemetry_actor_event() {
        let ev = NewEvent::spend_recorded(payload()).unwrap();
        assert_eq!(ev.kind, "spend.recorded");
        assert_eq!(
            ev.actor,
            Actor::System {
                component: SystemComponent::Telemetry
            }
        );
        assert_eq!(
            ev.payload,
            serde_json::to_value(&payload()).unwrap()
        );
        // no mission link => no mission_id field at all
        assert!(ev.payload.get("mission_id").is_none());
    }

    #[test]
    fn mission_link_round_trips_through_the_store() {
        let mission = Uuid::new_v4();
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        let store = EventStore::new(&conn);
        let mut p = payload();
        p.mission_id = Some(mission);
        p.role = Some("critic".into());
        let stored = store.append(NewEvent::spend_recorded(p).unwrap()).unwrap();
        assert_eq!(stored.payload["mission_id"].as_str(), Some(mission.to_string().as_str()));
        // the role tag rides along (Story 2.1 per-role receipts)
        assert_eq!(stored.payload["role"].as_str(), Some("critic"));
    }

    #[test]
    fn constructor_rejects_unattributable_spend() {
        for (provider, model) in [("", "m"), ("openai", "  "), (" ", "")] {
            let mut p = payload();
            p.provider = provider.into();
            p.model = model.into();
            let err = NewEvent::spend_recorded(p)
                .expect_err("spend without provider+model must fail construction");
            assert!(err.to_string().contains("attributable"), "unexpected: {err}");
        }
    }
}
