// Telemetry domain (FR-9.1/FR-4.3, Story 2.6): the events that make death
// LOUD. A scheduled run reports `run.heartbeat` events while it lives; the
// reaper compares each running run's last heartbeat against the threshold
// and, past it, appends `run.dead` — the dead-run ALERT event — followed by
// the run's terminal `run.failed(silently_dead)`, so every run reaches a
// terminal state with a timestamp and a reason (FR-9.1: no run ends
// silently; the detection is the dead-man switch FR-4.3 amended). MCP
// connections (Zotero, arXiv, Semantic Scholar) report
// `connection.failed` / `connection.restored` on probe state transitions —
// never on every probe, so a 60s cadence cannot flood the log.
//
// All telemetry actors are `system (telemetry)` — AD-15's closed
// enumeration; telemetry never mutates a quarantine-covered domain entity.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::eventstore::{Actor, EventError, EventStore, NewEvent, StoredEvent, SystemComponent};

pub const RUN_HEARTBEAT: &str = "run.heartbeat";
pub const RUN_DEAD: &str = "run.dead";
pub const CONNECTION_FAILED: &str = "connection.failed";
pub const CONNECTION_RESTORED: &str = "connection.restored";

/// The `run.failed` reason that marks a run the dead-man switch reaped as
/// silently dead (FR-9.1/FR-4.3): the terminal event appended right after
/// `run.dead`, so the run ends with a timestamp and a reason — never
/// silently. (Story 2.3's seam used `stale_heartbeat`; 2.6's full switch
/// names the death honestly.)
pub const SILENTLY_DEAD_REASON: &str = "silently_dead";

/// The dead-run threshold (FR-4.3 amended): a running run whose last
/// heartbeat is older than this window is silently dead. Default 30 minutes
/// — 2× the 15-minute scan cadence; the reaping call takes the threshold as
/// a parameter, so the cadence stays configurable.
pub const DEAD_RUN_THRESHOLD_MINUTES: i64 = 30;

/// The research connections the health probe checks on the scheduler's
/// cadence (FR-9.1): the three connectors the app's research surface leans
/// on — Zotero (local connector), arXiv, and Semantic Scholar.
pub const PROBED_CONNECTIONS: &[&str] = &["arxiv", "semantic_scholar", "zotero"];

// ---------------------------------------------------------------------------
// Typed constructors (AD-15)
// ---------------------------------------------------------------------------

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `run.heartbeat` event comes
    /// into being — a living run reporting its last activity. Actor is
    /// `system (telemetry)`; the payload carries the seq and timestamp of
    /// the last meaningful event, so a receipt can name what the run was
    /// doing when it heartbeated.
    pub fn run_heartbeat(
        run_id: &str,
        mission_id: Uuid,
        last_activity_seq: i64,
        last_activity_ts: DateTime<Utc>,
    ) -> Result<Self, EventError> {
        let payload = RunHeartbeatPayload {
            run_id: run_id.to_string(),
            mission_id,
            last_activity_seq,
            last_activity_ts,
        };
        if payload.run_id.trim().is_empty() {
            return Err(EventError::Invalid(
                "run.heartbeat requires a run_id — a heartbeat names its run".into(),
            ));
        }
        if payload.last_activity_seq < 1 {
            return Err(EventError::Invalid(
                "run.heartbeat requires the seq of a real event — the activity it reports \
                 (AD-2)"
                    .into(),
            ));
        }
        Self::new(
            RUN_HEARTBEAT,
            Actor::System { component: SystemComponent::Telemetry },
            serde_json::to_value(&payload)?,
        )
        .map(|e| e.with_causes(vec![mission_id]))
    }

    /// Typed constructor (AD-15): the one way a `run.dead` event comes into
    /// being — the dead-man switch DETECTING a silent death (FR-4.3
    /// amended). Actor is `system (telemetry)`; the payload carries the
    /// heartbeat the run died at and the threshold that decided the death.
    /// This is the ALERT event; the terminal `run.failed(silently_dead)`
    /// lands right after it (FR-9.1 — every run reaches a terminal state).
    pub fn run_dead(
        run_id: &str,
        mission_id: Uuid,
        last_heartbeat_ts: DateTime<Utc>,
        threshold_minutes: i64,
    ) -> Result<Self, EventError> {
        let payload = RunDeadPayload {
            run_id: run_id.to_string(),
            mission_id,
            last_heartbeat_ts,
            threshold_minutes,
        };
        if payload.run_id.trim().is_empty() {
            return Err(EventError::Invalid(
                "run.dead requires a run_id — a dead-run alert names its run".into(),
            ));
        }
        if payload.threshold_minutes < 1 {
            return Err(EventError::Invalid(
                "run.dead requires a positive threshold — the window that decided the death"
                    .into(),
            ));
        }
        Self::new(
            RUN_DEAD,
            Actor::System { component: SystemComponent::Telemetry },
            serde_json::to_value(&payload)?,
        )
        .map(|e| e.with_causes(vec![mission_id]))
    }

    /// Typed constructor (AD-15): the one way a `connection.failed` event
    /// comes into being — a health probe catching a research connection
    /// (Zotero, arXiv, Semantic Scholar) down. The error stays in code form
    /// — bilingual-safe by construction (EXPERIENCE.md).
    pub fn connection_failed(connection: &str, error_code: &str) -> Result<Self, EventError> {
        let payload = ConnectionFailedPayload {
            connection: connection.trim().to_string(),
            error_code: error_code.trim().to_string(),
        };
        if payload.connection.is_empty() {
            return Err(EventError::Invalid(
                "connection.failed requires a connection name — an alert names what died"
                    .into(),
            ));
        }
        if payload.error_code.is_empty() {
            return Err(EventError::Invalid(
                "connection.failed requires an error code — an honest alert names why".into(),
            ));
        }
        Self::new(
            CONNECTION_FAILED,
            Actor::System { component: SystemComponent::Telemetry },
            serde_json::to_value(&payload)?,
        )
    }

    /// Typed constructor (AD-15): the one way a `connection.restored` event
    /// comes into being — a probe finding a previously-failed connection
    /// back up.
    pub fn connection_restored(connection: &str) -> Result<Self, EventError> {
        let payload = ConnectionRestoredPayload { connection: connection.trim().to_string() };
        if payload.connection.is_empty() {
            return Err(EventError::Invalid(
                "connection.restored requires a connection name — a recovery names what healed"
                    .into(),
            ));
        }
        Self::new(
            CONNECTION_RESTORED,
            Actor::System { component: SystemComponent::Telemetry },
            serde_json::to_value(&payload)?,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunHeartbeatPayload {
    pub run_id: String,
    pub mission_id: Uuid,
    pub last_activity_seq: i64,
    pub last_activity_ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunDeadPayload {
    pub run_id: String,
    pub mission_id: Uuid,
    pub last_heartbeat_ts: DateTime<Utc>,
    pub threshold_minutes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionFailedPayload {
    pub connection: String,
    pub error_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionRestoredPayload {
    pub connection: String,
}

// ---------------------------------------------------------------------------
// The connection health fold (the trust center's health line, FR-9.1)
// ---------------------------------------------------------------------------

/// One connection's health as read from the log (AD-8) — the read model the
/// trust center's health line and the digest's connection alerts render.
/// Field names are camelCase on the wire (Tauri 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionHealth {
    pub connection: String,
    /// `true` while the latest connection event is a restore (or the
    /// connection has only ever succeeded — it appears only once probed).
    pub up: bool,
    /// The latest failure's error code, in code form (bilingual-safe).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_ts: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_restored_ts: Option<DateTime<Utc>>,
}

/// Pure fold of the log into connection health (AD-1): the latest
/// `connection.failed` / `connection.restored` per connection name wins;
/// rollback-aware through the shared fold cursor (a rolled-back failure
/// never happened). Corrupt payloads are skipped — a malformed telemetry
/// event never breaks the health line (telemetry is observability, not
/// domain state).
pub fn connection_health(events: &[StoredEvent]) -> Vec<ConnectionHealth> {
    let cursor = crate::domain::checkpoints::FoldCursor::over(events);
    let blank = |name: &str| ConnectionHealth {
        connection: name.to_string(),
        up: true,
        last_error_code: None,
        last_error_ts: None,
        last_restored_ts: None,
    };
    let mut health: HashMap<String, ConnectionHealth> = HashMap::new();
    for event in cursor.live(events) {
        match event.kind.as_str() {
            CONNECTION_FAILED => {
                let Ok(payload) =
                    serde_json::from_value::<ConnectionFailedPayload>(event.payload.clone())
                else {
                    continue;
                };
                let entry = health.entry(payload.connection.clone()).or_insert_with(|| blank(&payload.connection));
                entry.up = false;
                entry.last_error_code = Some(payload.error_code);
                entry.last_error_ts = Some(event.ts);
            }
            CONNECTION_RESTORED => {
                let Ok(payload) =
                    serde_json::from_value::<ConnectionRestoredPayload>(event.payload.clone())
                else {
                    continue;
                };
                let entry = health.entry(payload.connection.clone()).or_insert_with(|| blank(&payload.connection));
                entry.up = true;
                entry.last_restored_ts = Some(event.ts);
            }
            _ => {}
        }
    }
    let mut out: Vec<ConnectionHealth> = health.into_values().collect();
    out.sort_by(|a, b| a.connection.cmp(&b.connection));
    out
}

/// Record one probe result (FR-9.1): append `connection.failed` /
/// `connection.restored` on STATE TRANSITIONS only — a connection already
/// known down does not re-fail on every 60s probe (the log is not a
/// heartbeat dump; AD-12 heartbeats are events, connection health is
/// transitions). Returns the appended event, if any.
pub fn record_probe(
    store: &EventStore<'_>,
    connection: &str,
    ok: bool,
    error_code: &str,
) -> Result<Option<StoredEvent>, EventError> {
    let events = store.events_all()?;
    let current = connection_health(&events)
        .into_iter()
        .find(|h| h.connection == connection);
    let currently_up = current.as_ref().map(|h| h.up).unwrap_or(true);
    if ok {
        if currently_up {
            return Ok(None); // healthy and known healthy — nothing to report
        }
        return store
            .append(NewEvent::connection_restored(connection)?)
            .map(Some);
    }
    if currently_up {
        return store
            .append(NewEvent::connection_failed(connection, error_code)?)
            .map(Some);
    }
    Ok(None) // already down and alerted — no flood
}

// ---------------------------------------------------------------------------
// The heartbeat fold (the dead-man switch's input)
// ---------------------------------------------------------------------------

/// The last heartbeat of each RUNNING run: the latest `run.heartbeat` event
/// ts per run id. Pure — the reaper folds this and compares against the
/// threshold (FR-4.3 amended). Runs without a heartbeat fall back to their
/// `run.started` ts (Story 2.3's seam).
pub fn last_heartbeats(events: &[StoredEvent]) -> HashMap<String, DateTime<Utc>> {
    let cursor = crate::domain::checkpoints::FoldCursor::over(events);
    let mut beats: HashMap<String, DateTime<Utc>> = HashMap::new();
    for event in cursor.live(events) {
        if event.kind != RUN_HEARTBEAT {
            continue;
        }
        let Some(run_id) = event.payload.get("run_id").and_then(|v| v.as_str()) else {
            continue;
        };
        let beat = beats.entry(run_id.to_string()).or_insert(event.ts);
        if event.ts > *beat {
            *beat = event.ts;
        }
    }
    beats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventstore::EventStore;
    use chrono::TimeZone;
    use rusqlite::Connection;
    use serde_json::json;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    fn mission(store: &EventStore<'_>) -> Uuid {
        store
            .append(
                NewEvent::mission_created(crate::domain::missions::MissionCreatedPayload {
                    question: "Does X hold?".into(),
                    stop_condition: "Stop after $5.".into(),
                    success_criterion: "A rater agrees.".into(),
                    autonomy: crate::domain::missions::Autonomy::Suggest,
                    spend_ceiling_cents: 500,
                    roles: vec![],
                    schedule: "off".into(),
                })
                .unwrap(),
            )
            .unwrap()
            .id
    }

    #[test]
    fn telemetry_constructors_carry_the_telemetry_actor() {
        let mission_id = Uuid::new_v4();
        let hb = NewEvent::run_heartbeat("ns-1", mission_id, 7, Utc::now()).unwrap();
        assert_eq!(hb.kind, RUN_HEARTBEAT);
        assert_eq!(
            hb.actor,
            Actor::System { component: SystemComponent::Telemetry }
        );
        assert_eq!(hb.causes, vec![mission_id]);
        assert_eq!(hb.payload["last_activity_seq"], json!(7));

        let died_at = Utc.with_ymd_and_hms(2026, 9, 19, 2, 31, 0).unwrap();
        let dead = NewEvent::run_dead("ns-17", mission_id, died_at, 30).unwrap();
        assert_eq!(dead.kind, RUN_DEAD);
        assert_eq!(dead.payload["last_heartbeat_ts"], serde_json::to_value(died_at).unwrap());
        assert_eq!(dead.payload["threshold_minutes"], json!(30));

        let failed = NewEvent::connection_failed("zotero", "unreachable").unwrap();
        assert_eq!(failed.kind, CONNECTION_FAILED);
        assert_eq!(failed.payload, json!({ "connection": "zotero", "error_code": "unreachable" }));
        let restored = NewEvent::connection_restored("zotero").unwrap();
        assert_eq!(restored.kind, CONNECTION_RESTORED);
        assert_eq!(restored.payload, json!({ "connection": "zotero" }));
    }

    #[test]
    fn telemetry_constructors_reject_empty_fields() {
        let mission_id = Uuid::new_v4();
        assert!(NewEvent::run_heartbeat(" ", mission_id, 1, Utc::now()).is_err());
        assert!(NewEvent::run_heartbeat("ns-1", mission_id, 0, Utc::now()).is_err());
        assert!(NewEvent::run_dead("", mission_id, Utc::now(), 30).is_err());
        assert!(NewEvent::run_dead("ns-1", mission_id, Utc::now(), 0).is_err());
        assert!(NewEvent::connection_failed("", "timeout").is_err());
        assert!(NewEvent::connection_failed("zotero", "  ").is_err());
        assert!(NewEvent::connection_restored(" ").is_err());
    }

    #[test]
    fn connection_health_folds_latest_state_per_connection() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        store.append(NewEvent::connection_failed("zotero", "unreachable").unwrap()).unwrap();
        store.append(NewEvent::connection_failed("arxiv", "timeout").unwrap()).unwrap();
        store.append(NewEvent::connection_restored("zotero").unwrap()).unwrap();

        let health = connection_health(&store.events_all().unwrap());
        assert_eq!(health.len(), 2, "one entry per connection, sorted");
        let zotero = &health[1];
        assert_eq!(zotero.connection, "zotero");
        assert!(zotero.up, "the restore is the latest event");
        assert_eq!(zotero.last_error_code.as_deref(), Some("unreachable"));
        assert!(zotero.last_restored_ts.is_some());
        let arxiv = &health[0];
        assert_eq!(arxiv.connection, "arxiv");
        assert!(!arxiv.up);
        assert_eq!(arxiv.last_error_code.as_deref(), Some("timeout"));
    }

    #[test]
    fn record_probe_appends_only_on_state_transitions() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        // healthy probe on an unknown connection: nothing to report
        assert!(record_probe(&store, "arxiv", true, "").unwrap().is_none());
        // first failure: the alert lands
        let failed = record_probe(&store, "arxiv", false, "timeout").unwrap().unwrap();
        assert_eq!(failed.kind, CONNECTION_FAILED);
        // still down: no flood
        assert!(record_probe(&store, "arxiv", false, "timeout").unwrap().is_none());
        assert!(record_probe(&store, "arxiv", false, "timeout").unwrap().is_none());
        // recovery: the restore lands
        let restored = record_probe(&store, "arxiv", true, "").unwrap().unwrap();
        assert_eq!(restored.kind, CONNECTION_RESTORED);
        // healthy again: nothing
        assert!(record_probe(&store, "arxiv", true, "").unwrap().is_none());
        // and down again: a fresh failure (a new outage is a new alert)
        assert!(record_probe(&store, "arxiv", false, "dns").unwrap().is_some());
        let events = store.events_all().unwrap();
        assert_eq!(
            events.iter().filter(|e| e.kind == CONNECTION_FAILED).count(),
            2,
            "two outages, two alerts — never one per probe"
        );
    }

    #[test]
    fn last_heartbeats_takes_the_latest_beat_per_run() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let mission_id = mission(&store);
        let t0 = Utc.with_ymd_and_hms(2026, 9, 19, 3, 0, 0).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 9, 19, 3, 10, 0).unwrap();
        let mut a = NewEvent::run_heartbeat("ns-a", mission_id, 2, t0).unwrap();
        a.ts = t0;
        store.append(a).unwrap();
        let mut b = NewEvent::run_heartbeat("ns-a", mission_id, 5, t1).unwrap();
        b.ts = t1;
        store.append(b).unwrap();
        let mut c = NewEvent::run_heartbeat("ns-b", mission_id, 6, t0).unwrap();
        c.ts = t0;
        store.append(c).unwrap();

        let beats = last_heartbeats(&store.events_all().unwrap());
        assert_eq!(beats.len(), 2);
        assert_eq!(beats["ns-a"], t1, "the latest beat wins");
        assert_eq!(beats["ns-b"], t0);
    }
}
