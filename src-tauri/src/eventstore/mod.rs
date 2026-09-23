// Event-sourced core — the single append-only `events` table (AD-1) with the
// fixed event envelope (AD-2). Every domain mutation is an append; current
// state is exclusively a projection over this log. `seq` is assigned by the
// store at append time and is the sole ordering.
//
// Payload schemas are owned by the core and exposed as typed constructors
// (AD-15): other layers build events through `NewEvent` constructors, never
// through hand-built SQL inserts or raw event JSON.

pub mod migration;

use chrono::{DateTime, SecondsFormat, SubsecRound, Utc};
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// DDL for the append-only `events` table. `seq` is an AUTOINCREMENT primary
/// key assigned by the store at insert — monotonic, the sole ordering.
pub const EVENTS_SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS events (
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        id TEXT NOT NULL UNIQUE,
        ts TEXT NOT NULL,
        actor TEXT NOT NULL,
        kind TEXT NOT NULL,
        payload TEXT NOT NULL,
        causes TEXT NOT NULL DEFAULT '[]'
    );
    CREATE INDEX IF NOT EXISTS idx_events_kind ON events(kind);
";

#[derive(Debug, Error)]
pub enum EventError {
    #[error("invalid event kind `{kind}`: expected snake_case `<entity>.<verb>`")]
    InvalidKind { kind: String },
    #[error("invalid event: {0}")]
    Invalid(String),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// The audit stamp's `actor` (AD-2). `Actor::System` is a closed enumeration
/// (AD-15): heartbeats, telemetry, spend, runtime state, verification
/// results, migration imports — never a mutation of quarantine-covered
/// domain entities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Actor {
    User,
    Agent {
        run_id: String,
    },
    System {
        component: SystemComponent,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SystemComponent {
    Migration,
    Scheduler,
    Verifier,
    Telemetry,
    Runtime,
    /// The bridge channel (Story 6.14): failed-push events — a push that
    /// never reached its surface is visible, never a silent miss.
    Bridge,
}

/// An event before it is appended: `seq` is not yet assigned (the store
/// assigns it at insert). Built through typed constructors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewEvent {
    pub id: Uuid,
    pub ts: DateTime<Utc>,
    pub actor: Actor,
    pub kind: String,
    pub payload: serde_json::Value,
    pub causes: Vec<Uuid>,
}

/// An event as stored in the log, with the store-assigned `seq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredEvent {
    pub seq: i64,
    pub id: Uuid,
    pub ts: DateTime<Utc>,
    pub actor: Actor,
    pub kind: String,
    pub payload: serde_json::Value,
    pub causes: Vec<Uuid>,
}

impl NewEvent {
    /// Raw core-internal constructor: validates the kind, assigns a fresh
    /// uuid v4 id and UTC timestamp, and starts with an empty `causes`.
    /// Prefer the typed constructors (e.g. `migration::NewEvent::import_entity`)
    /// when appending from domain or shell code (AD-15).
    pub fn new(kind: impl Into<String>, actor: Actor, payload: serde_json::Value) -> Result<Self, EventError> {
        let kind = kind.into();
        validate_kind(&kind)?;
        Ok(Self {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            actor,
            kind,
            payload,
            causes: Vec::new(),
        })
    }

    /// Set the `causes` (event ids this event builds on — the audit stamp, AD-2).
    pub fn with_causes(mut self, causes: Vec<Uuid>) -> Self {
        self.causes = causes;
        self
    }
}

/// Validate the event `kind`: snake_case `<entity>.<verb>` — exactly one dot,
/// segments of lowercase ascii letters, digits and single interior underscores.
pub fn validate_kind(kind: &str) -> Result<(), EventError> {
    let invalid = || EventError::InvalidKind { kind: kind.to_string() };
    let (entity, verb) = kind.split_once('.').ok_or_else(invalid)?;
    if !valid_snake_segment(entity) || !valid_snake_segment(verb) {
        return Err(invalid());
    }
    Ok(())
}

fn valid_snake_segment(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('_')
        && !s.ends_with('_')
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The append-only event store over the workspace's SQLite connection
/// (AD-11: one workspace, one store). Reads and appends share the same
/// connection; `seq` is assigned by SQLite at insert, so appends from any
/// number of connections to one database are still totally ordered.
pub struct EventStore<'a> {
    conn: &'a Connection,
}

impl<'a> EventStore<'a> {
    /// Create the `events` table if absent. Idempotent.
    pub fn init(conn: &Connection) -> Result<(), EventError> {
        conn.execute_batch(EVENTS_SCHEMA)?;
        Ok(())
    }

    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Append one event in its own transaction. Returns the stored event with
    /// the store-assigned `seq`.
    pub fn append(&self, event: NewEvent) -> Result<StoredEvent, EventError> {
        let tx = self.conn.unchecked_transaction()?;
        let stored = insert_event(&tx, event)?;
        tx.commit()?;
        Ok(stored)
    }

    /// All events with `seq >= from`, in `seq` order.
    pub fn events_from(&self, from: i64) -> Result<Vec<StoredEvent>, EventError> {
        self.select_events(
            "SELECT seq, id, ts, actor, kind, payload, causes FROM events WHERE seq >= ?1 ORDER BY seq",
            rusqlite::params![from],
        )
    }

    /// The entire log, in `seq` order.
    pub fn events_all(&self) -> Result<Vec<StoredEvent>, EventError> {
        self.select_events("SELECT seq, id, ts, actor, kind, payload, causes FROM events ORDER BY seq", ())
    }

    /// The current head `seq` (0 for an empty log).
    pub fn head_seq(&self) -> Result<i64, EventError> {
        let seq: i64 = self
            .conn
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |r| r.get(0))?;
        Ok(seq)
    }

    fn select_events<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<Vec<StoredEvent>, EventError> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params, row_to_event)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

/// Append inside an existing transaction (used by the migration so the whole
/// one-time import + marker is atomic).
pub(crate) fn append_in_tx(tx: &Transaction, event: NewEvent) -> Result<StoredEvent, EventError> {
    insert_event(tx, event)
}

fn insert_event(conn: &Connection, event: NewEvent) -> Result<StoredEvent, EventError> {
    // Re-validate at the boundary: constructors validated on construction,
    // but the store is the last line of defense for the envelope contract.
    validate_kind(&event.kind)?;
    // The log stores millisecond precision; truncate so the returned event
    // is exactly what a read-back yields.
    let ts = event.ts.trunc_subsecs(3);
    let ts_str = ts.to_rfc3339_opts(SecondsFormat::Millis, true);
    let actor = serde_json::to_string(&event.actor)?;
    let payload = event.payload.to_string();
    let causes = serde_json::to_string(&event.causes)?;
    conn.execute(
        "INSERT INTO events (id, ts, actor, kind, payload, causes) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![event.id.to_string(), ts_str, actor, event.kind, payload, causes],
    )?;
    let seq = conn.last_insert_rowid();
    Ok(StoredEvent {
        seq,
        id: event.id,
        ts,
        actor: event.actor,
        kind: event.kind,
        payload: event.payload,
        causes: event.causes,
    })
}

fn row_to_event(r: &rusqlite::Row) -> rusqlite::Result<StoredEvent> {
    let id: String = r.get("id")?;
    let ts: String = r.get("ts")?;
    let actor: String = r.get("actor")?;
    let kind: String = r.get("kind")?;
    let payload: String = r.get("payload")?;
    let causes: String = r.get("causes")?;
    Ok(StoredEvent {
        seq: r.get("seq")?,
        id: Uuid::parse_str(&id)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e)))?,
        ts: DateTime::parse_from_rfc3339(&ts)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e)))?
            .with_timezone(&Utc),
        actor: serde_json::from_str(&actor)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e)))?,
        kind,
        payload: serde_json::from_str(&payload)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e)))?,
        causes: serde_json::from_str(&causes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e)))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    #[test]
    fn actor_json_shapes_match_the_envelope() {
        assert_eq!(
            serde_json::to_string(&Actor::User).unwrap(),
            r#"{"kind":"user"}"#
        );
        assert_eq!(
            serde_json::to_string(&Actor::Agent { run_id: "run-42".into() }).unwrap(),
            r#"{"kind":"agent","run_id":"run-42"}"#
        );
        assert_eq!(
            serde_json::to_string(&Actor::System { component: SystemComponent::Migration }).unwrap(),
            r#"{"kind":"system","component":"migration"}"#
        );
        for component in [
            SystemComponent::Migration,
            SystemComponent::Scheduler,
            SystemComponent::Verifier,
            SystemComponent::Telemetry,
            SystemComponent::Runtime,
            SystemComponent::Bridge,
        ] {
            let s = serde_json::to_string(&Actor::System { component }).unwrap();
            let back: Actor = serde_json::from_str(&s).unwrap();
            assert_eq!(back, Actor::System { component });
        }
    }

    #[test]
    fn kind_validation_accepts_snake_case_entity_verb_only() {
        for ok in ["mission.created", "hypothesis.status_changed", "import.ref_usage", "a.b", "x2.y3"] {
            assert!(validate_kind(ok).is_ok(), "should accept {ok}");
        }
        for bad in [
            "Mission.created",   // uppercase
            "mission",           // no dot
            "mission.created.now", // extra dot
            ".created",          // empty entity
            "mission.",          // empty verb
            "_mission.created",  // leading underscore
            "mission_.created",  // trailing underscore
            "mission.créated",   // non-ascii
            "",                  // empty
        ] {
            assert!(validate_kind(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn append_assigns_a_complete_envelope() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let cause = Uuid::new_v4();
        let ev = NewEvent::new("mission.created", Actor::User, json!({"question": "why?"}))
            .unwrap()
            .with_causes(vec![cause]);
        let stored = store.append(ev).unwrap();

        // seq assigned by the store, starting at 1
        assert_eq!(stored.seq, 1);
        assert_eq!(store.head_seq().unwrap(), 1);
        // uuid v4, unique per append
        assert_eq!(stored.id.get_version_num(), 4);
        // ts is ISO-8601 UTC (RFC-3339 with Z)
        assert!(stored.ts.to_rfc3339_opts(SecondsFormat::Millis, true).ends_with('Z'));
        // kind + causes survive the round-trip
        assert_eq!(stored.kind, "mission.created");
        assert_eq!(stored.causes, vec![cause]);

        let all = store.events_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0], stored);
    }

    #[test]
    fn append_rejects_invalid_kinds() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let ev = NewEvent::new("NotSnake.Case", Actor::User, json!({}));
        assert!(matches!(ev, Err(EventError::InvalidKind { .. })));
        assert_eq!(store.head_seq().unwrap(), 0);
    }

    #[test]
    fn seq_is_monotonic_across_many_appends() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let mut ids = std::collections::HashSet::new();
        for i in 0..100 {
            let ev = NewEvent::new("test.appended", Actor::User, json!({"i": i})).unwrap();
            let stored = store.append(ev).unwrap();
            assert_eq!(stored.seq, i + 1);
            assert!(ids.insert(stored.id), "event ids must be unique");
        }
        assert_eq!(store.head_seq().unwrap(), 100);
        let all = store.events_all().unwrap();
        let seqs: Vec<i64> = all.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, (1..=100).collect::<Vec<_>>());
        assert_eq!(ids.len(), 100);
    }

    #[test]
    fn events_from_is_inclusive_and_ordered() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        for i in 0..10 {
            store.append(NewEvent::new("test.appended", Actor::User, json!({"i": i})).unwrap()).unwrap();
        }
        let from = store.events_from(7).unwrap();
        assert_eq!(from.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![7, 8, 9, 10]);
        assert!(store.events_from(11).unwrap().is_empty());
    }
}
