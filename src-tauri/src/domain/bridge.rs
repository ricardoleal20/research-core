// Bridge pairing domain (Story 6.14, FR-21.1, NFR-13): the pairing ledger
// behind the ONE bridge channel (AD-7). Pairing is a user action — a device
// name plus a one-time token the owner enters on the remote surface; the
// token's sha-256 lives in the `bridge_tokens` table (never the raw token —
// the log is exportable, AD-11), and the audit trail lives in events:
// `bridge.device_paired` / `bridge.device_unpaired` (actor=user, AD-2).
// Unpaired requests are refused by the bridge server with `unpaired:` — no
// open endpoints, no unauthenticated requests.

use crate::eventstore::{Actor, EventError, EventStore, NewEvent, StoredEvent};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const BRIDGE_DEVICE_PAIRED: &str = "bridge.device_paired";
pub const BRIDGE_DEVICE_UNPAIRED: &str = "bridge.device_unpaired";
/// A failed push is a visible event, never a silent miss (FR-9.1 spirit):
/// actor=system:bridge, payload names the adapter and the failure.
pub const BRIDGE_PUSH_FAILED: &str = "bridge.push_failed";

/// DDL for the pairing-token table: the device name and the sha-256 of its
/// one-time token. The raw token is returned once at pairing time and never
/// stored — a request proves itself by presenting the token, the server
/// compares hashes.
pub const BRIDGE_TOKENS_SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS bridge_tokens (
        device TEXT PRIMARY KEY,
        token_hash TEXT NOT NULL,
        created_at TEXT NOT NULL
    );
";

/// The `bridge.device_paired` payload: the device name and the token's
/// fingerprint (first 8 hex chars of its sha-256 — enough to recognize a
/// re-pair in the audit view, never enough to authenticate).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DevicePairedPayload {
    pub device: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceUnpairedPayload {
    pub device: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushFailedPayload {
    pub adapter: String,
    pub reason: String,
}

/// A paired device as read from the log — the visible paired-devices list
/// the bridge settings surface renders. Field names are camelCase on the
/// wire (Tauri 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    pub device: String,
    pub fingerprint: String,
    pub paired_seq: i64,
    pub paired_ts: DateTime<Utc>,
}

/// What pairing hands the owner once: the device name, the raw token (shown
/// once, never stored), and the fingerprint the receipts will carry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingReceipt {
    pub device: String,
    pub token: String,
    pub fingerprint: String,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `bridge.device_paired` event
    /// comes into being — actor is always the user (pairing is a user
    /// action, NFR-13).
    pub fn bridge_device_paired(device: &str, fingerprint: &str) -> Result<Self, EventError> {
        let payload = DevicePairedPayload {
            device: device.to_string(),
            fingerprint: fingerprint.to_string(),
        };
        Ok(Self::new(
            BRIDGE_DEVICE_PAIRED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?)
    }

    /// Typed constructor: `bridge.device_unpaired` — the auditable unpair.
    pub fn bridge_device_unpaired(device: &str) -> Result<Self, EventError> {
        let payload = DeviceUnpairedPayload {
            device: device.to_string(),
        };
        Ok(Self::new(
            BRIDGE_DEVICE_UNPAIRED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?)
    }

    /// Typed constructor: `bridge.push_failed` — a failed push is a visible
    /// event (actor=system:bridge), never a silent miss.
    pub fn bridge_push_failed(adapter: &str, reason: &str) -> Result<Self, EventError> {
        let payload = PushFailedPayload {
            adapter: adapter.to_string(),
            reason: reason.to_string(),
        };
        Ok(Self::new(
            BRIDGE_PUSH_FAILED,
            Actor::System {
                component: crate::eventstore::SystemComponent::Bridge,
            },
            serde_json::to_value(&payload)?,
        )?)
    }
}

/// The sha-256 of a pairing token, hex-encoded — what the table stores and
/// what a request is verified against.
pub fn token_hash(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Generate a fresh one-time pairing token: 32 random bytes, hex-encoded.
fn fresh_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Ensure the pairing-token table exists (idempotent — safe next to
/// `Db::migrate` and in test connections).
pub fn init_tables(conn: &Connection) -> Result<(), EventError> {
    conn.execute_batch(BRIDGE_TOKENS_SCHEMA)
        .map_err(EventError::Db)?;
    Ok(())
}

/// Pair a device (a user action): mint a one-time token, store its sha-256,
/// append the audit event. Re-pairing an existing device rotates its token —
/// the old token stops authenticating immediately.
pub fn pair_device(conn: &Connection, device: &str) -> Result<PairingReceipt, String> {
    let device = device.trim();
    if device.is_empty() {
        return Err(
            "invalid device name — a paired device is named (e.g. `Pixel 8`, `iPad sala`)"
                .into(),
        );
    }
    init_tables(conn).map_err(|e| e.to_string())?;
    let token = fresh_token();
    let hash = token_hash(&token);
    let fingerprint = hash[..8].to_string();
    conn.execute(
        "INSERT INTO bridge_tokens (device, token_hash, created_at) VALUES (?1, ?2, ?3) \
         ON CONFLICT(device) DO UPDATE SET token_hash=excluded.token_hash, created_at=excluded.created_at",
        params![device, hash, Utc::now().to_rfc3339()],
    )
    .map_err(|e| e.to_string())?;
    EventStore::new(conn)
        .append(
            NewEvent::bridge_device_paired(device, &fingerprint)
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    Ok(PairingReceipt {
        device: device.to_string(),
        token,
        fingerprint,
    })
}

/// Unpair a device: remove its token row and append the audit event. The
/// device's token stops authenticating immediately. Unpairing an unknown
/// device is refused honestly.
pub fn unpair_device(conn: &Connection, device: &str) -> Result<Vec<PairedDevice>, String> {
    let paired = list_devices(conn)?;
    if !paired.iter().any(|d| d.device == device) {
        return Err(format!(
            "unknown_device: no paired device named `{device}` — the paired list is the truth"
        ));
    }
    conn.execute("DELETE FROM bridge_tokens WHERE device = ?1", params![device])
        .map_err(|e| e.to_string())?;
    EventStore::new(conn)
        .append(NewEvent::bridge_device_unpaired(device).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    list_devices(conn)
}

/// The visible paired-devices list: the fold of the pairing ledger (a device
/// is paired when its latest pairing event is a pairing, not an unpairing —
/// rollback-aware through the shared fold cursor, AD-1).
pub fn list_devices(conn: &Connection) -> Result<Vec<PairedDevice>, String> {
    let events = EventStore::new(conn)
        .events_all()
        .map_err(|e| e.to_string())?;
    fold_devices(&events).map_err(|e| e.to_string())
}

/// Pure fold of the pairing ledger — testable without a store.
pub fn fold_devices(events: &[StoredEvent]) -> Result<Vec<PairedDevice>, EventError> {
    let cursor = crate::domain::checkpoints::FoldCursor::over(events);
    let events = &cursor.live_owned(events);
    let mut devices: Vec<PairedDevice> = Vec::new();
    for event in events {
        match event.kind.as_str() {
            BRIDGE_DEVICE_PAIRED => {
                let payload: DevicePairedPayload = serde_json::from_value(event.payload.clone())
                    .map_err(|e| {
                        EventError::Invalid(format!(
                            "corrupt {BRIDGE_DEVICE_PAIRED} payload at seq {}: {e}",
                            event.seq
                        ))
                    })?;
                devices.retain(|d| d.device != payload.device);
                devices.push(PairedDevice {
                    device: payload.device,
                    fingerprint: payload.fingerprint,
                    paired_seq: event.seq,
                    paired_ts: event.ts,
                });
            }
            BRIDGE_DEVICE_UNPAIRED => {
                let payload: DeviceUnpairedPayload =
                    serde_json::from_value(event.payload.clone()).map_err(|e| {
                        EventError::Invalid(format!(
                            "corrupt {BRIDGE_DEVICE_UNPAIRED} payload at seq {}: {e}",
                            event.seq
                        ))
                    })?;
                devices.retain(|d| d.device != payload.device);
            }
            _ => {}
        }
    }
    devices.sort_by_key(|d| d.paired_seq);
    Ok(devices)
}

/// Verify a pairing token: the device it authenticates, or `None` — an
/// unpaired or rotated token authenticates nothing. Constant-shape: a
/// missing header and a wrong token are the same refusal.
pub fn verify_pairing(conn: &Connection, token: &str) -> Result<Option<String>, String> {
    init_tables(conn).map_err(|e| e.to_string())?;
    let hash = token_hash(token);
    let mut stmt = conn
        .prepare("SELECT device FROM bridge_tokens WHERE token_hash = ?1")
        .map_err(|e| e.to_string())?;
    let device = stmt
        .query_row(params![hash], |r| r.get::<_, String>(0))
        .ok();
    Ok(device)
}

/// Record a failed push as a visible event (never a silent miss): append
/// `bridge.push_failed` with the adapter and the reason. Best-effort — a
/// failure to record a failure is logged to stderr, never crashes the
/// adapter loop.
pub fn record_push_failure(conn: &Connection, adapter: &str, reason: &str) {
    let store = EventStore::new(conn);
    match NewEvent::bridge_push_failed(adapter, reason)
        .map_err(|e| e.to_string())
        .and_then(|ev| store.append(ev).map_err(|e| e.to_string()))
    {
        Ok(_) => {}
        Err(e) => eprintln!("[bridge] failed to record push failure ({adapter}): {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    #[test]
    fn pairing_mints_a_once_token_and_a_user_actor_event() {
        let conn = conn();
        let receipt = pair_device(&conn, "Pixel 8").unwrap();
        assert_eq!(receipt.device, "Pixel 8");
        assert_eq!(receipt.token.len(), 64, "32 bytes hex-encoded");
        assert_eq!(receipt.fingerprint.len(), 8);
        // the raw token is never stored — only its sha-256
        let stored: String = conn
            .query_row(
                "SELECT token_hash FROM bridge_tokens WHERE device = 'Pixel 8'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, token_hash(&receipt.token));
        assert_ne!(stored, receipt.token);
        // the audit trail: one user-actor pairing event
        let events = EventStore::new(&conn).events_all().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, BRIDGE_DEVICE_PAIRED);
        assert_eq!(events[0].actor, Actor::User);
        let payload: DevicePairedPayload =
            serde_json::from_value(events[0].payload.clone()).unwrap();
        assert_eq!(payload.device, "Pixel 8");
        assert_eq!(payload.fingerprint, receipt.fingerprint);
    }

    #[test]
    fn a_blank_device_name_is_refused() {
        let conn = conn();
        assert!(pair_device(&conn, "   ").is_err());
        assert!(pair_device(&conn, "").is_err());
        assert!(EventStore::new(&conn).events_all().unwrap().is_empty());
    }

    #[test]
    fn verify_pairing_accepts_the_minted_token_and_refuses_everything_else() {
        let conn = conn();
        let receipt = pair_device(&conn, "iPhone").unwrap();
        assert_eq!(
            verify_pairing(&conn, &receipt.token).unwrap(),
            Some("iPhone".into())
        );
        assert_eq!(verify_pairing(&conn, "not-the-token").unwrap(), None);
        assert_eq!(verify_pairing(&conn, "").unwrap(), None);
    }

    #[test]
    fn re_pairing_rotates_the_token() {
        let conn = conn();
        let first = pair_device(&conn, "Pixel 8").unwrap();
        let second = pair_device(&conn, "Pixel 8").unwrap();
        assert_ne!(first.token, second.token, "a re-pair mints a fresh token");
        // the old token stops authenticating immediately
        assert_eq!(verify_pairing(&conn, &first.token).unwrap(), None);
        assert_eq!(
            verify_pairing(&conn, &second.token).unwrap(),
            Some("Pixel 8".into())
        );
        // the paired list holds ONE device — the latest pairing wins
        let devices = list_devices(&conn).unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].fingerprint, second.fingerprint);
    }

    #[test]
    fn unpair_removes_the_device_and_refuses_unknowns() {
        let conn = conn();
        pair_device(&conn, "Pixel 8").unwrap();
        let devices = unpair_device(&conn, "Pixel 8").unwrap();
        assert!(devices.is_empty());
        // the token row is gone — it no longer authenticates
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM bridge_tokens", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        // an unknown device is refused honestly
        let err = unpair_device(&conn, "Nadie").unwrap_err();
        assert!(err.starts_with("unknown_device:"), "unexpected: {err}");
    }

    #[test]
    fn the_paired_list_folds_pair_unpair_in_order() {
        let events = EventStore::new(&conn())
            .events_all()
            .unwrap();
        assert!(fold_devices(&events).unwrap().is_empty());
        let conn = conn();
        pair_device(&conn, "A").unwrap();
        pair_device(&conn, "B").unwrap();
        unpair_device(&conn, "A").unwrap();
        let devices = list_devices(&conn).unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device, "B");
        // an unpaired device's token is gone from the table
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM bridge_tokens", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn a_failed_push_is_a_visible_system_bridge_event() {
        let conn = conn();
        record_push_failure(&conn, "chopflow", "connection refused");
        let events = EventStore::new(&conn).events_all().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, BRIDGE_PUSH_FAILED);
        assert_eq!(
            events[0].actor,
            Actor::System {
                component: crate::eventstore::SystemComponent::Bridge
            }
        );
        let payload: PushFailedPayload =
            serde_json::from_value(events[0].payload.clone()).unwrap();
        assert_eq!(payload.adapter, "chopflow");
        assert_eq!(payload.reason, "connection refused");
    }
}
