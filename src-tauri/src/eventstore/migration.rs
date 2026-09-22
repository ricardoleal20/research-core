// One-time import of the brownfield relational state into the event log
// (AD-16): one `import.<entity>` event per legacy row, actor
// `system (migration)` — part of AD-15's closed `actor=system` enumeration.
// Legacy tables are NOT dropped; after the migration they are read-only
// (no new write paths may target them).
//
// API keys never enter the event log: secret settings rows are excluded from
// the import and their values move to the OS keychain (`keyring`, service
// "ResearchCore"). A key that cannot be moved is left in `settings` and only
// counted — never logged.
use std::collections::BTreeMap;

use rusqlite::{Connection, Transaction};
use serde::Serialize;
use serde_json::{json, Value};

use super::{append_in_tx, Actor, EventError, EventStore, NewEvent, SystemComponent};

/// Keychain service under which migrated API keys are stored (account =
/// provider name, AD-16).
pub const KEYCHAIN_SERVICE: &str = "ResearchCore";

/// Marker event appended once after a complete import; re-running the
/// migration detects it and skips.
pub const MIGRATION_COMPLETED: &str = "migration.completed";

/// Legacy tables imported into the log, in import order (parents before
/// children): `(event entity name, table name)`.
const IMPORT_TABLES: &[(&str, &str)] = &[
    ("project", "projects"),
    ("collection", "collections"),
    ("ref", "refs"),
    ("ref_usage", "ref_usages"),
    ("review", "reviews"),
    ("finding", "findings"),
    ("action", "actions"),
    ("chat", "chats"),
    ("message", "messages"),
    ("agent", "agents"),
    ("mcp_server", "mcp_servers"),
    ("setting", "settings"),
];

/// Outcome of a migration run.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MigrationReport {
    /// entity name -> number of `import.<entity>` events appended.
    pub imported: BTreeMap<String, usize>,
    /// API keys successfully moved to the OS keychain.
    pub keys_moved_to_keychain: usize,
    /// API keys that could not be moved and were left in `settings`
    /// (the values themselves are never recorded).
    pub keys_left_in_settings: usize,
    /// True when the marker event was already present and nothing was done.
    pub skipped: bool,
}

impl NewEvent {
    /// Typed constructor (AD-15): one `import.<entity>` event carrying the
    /// legacy row's data as its payload.
    pub fn import_entity(entity: &str, row: Value) -> Result<Self, EventError> {
        Self::new(
            format!("import.{entity}"),
            Actor::System {
                component: SystemComponent::Migration,
            },
            row,
        )
    }

    /// Typed constructor (AD-15): the idempotency marker appended once after
    /// a complete one-time import.
    pub fn migration_completed(
        imported: &BTreeMap<String, usize>,
        keys_moved: usize,
        keys_left: usize,
    ) -> Result<Self, EventError> {
        Self::new(
            MIGRATION_COMPLETED,
            Actor::System {
                component: SystemComponent::Migration,
            },
            json!({
                "entities": imported,
                "keys_moved_to_keychain": keys_moved,
                "keys_left_in_settings": keys_left,
            }),
        )
    }
}

/// Run the one-time migration. Idempotent: if a `migration.completed` event
/// exists, nothing is appended. The whole import (every `import.<entity>`
/// event plus the marker) is one transaction, so a partial import can never
/// be mistaken for a completed one.
pub fn run(conn: &Connection) -> Result<MigrationReport, EventError> {
    EventStore::init(conn)?;
    if already_migrated(conn)? {
        return Ok(MigrationReport {
            skipped: true,
            ..Default::default()
        });
    }

    // Secrets move to the keychain before the import transaction: whatever
    // happens next, no key value can end up in an event.
    let (keys_moved, keys_left) = migrate_api_keys_to_keychain(conn)?;

    let tx = conn.unchecked_transaction()?;
    let mut imported = BTreeMap::new();
    for (entity, table) in IMPORT_TABLES {
        let count = import_table(&tx, entity, table)?;
        imported.insert((*entity).to_string(), count);
    }
    append_in_tx(
        &tx,
        NewEvent::migration_completed(&imported, keys_moved, keys_left)?,
    )?;
    tx.commit()?;

    Ok(MigrationReport {
        imported,
        keys_moved_to_keychain: keys_moved,
        keys_left_in_settings: keys_left,
        skipped: false,
    })
}

fn already_migrated(conn: &Connection) -> Result<bool, EventError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM events WHERE kind = ?1",
        rusqlite::params![MIGRATION_COMPLETED],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Append one `import.<entity>` event per row of `table` (ordered by rowid
/// for determinism). Secret settings rows are excluded (AD-16). Returns the
/// number of events appended.
fn import_table(tx: &Transaction, entity: &str, table: &str) -> Result<usize, EventError> {
    let sql = format!("SELECT * FROM {table} ORDER BY rowid");
    let mut stmt = tx.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut count = 0usize;
    while let Some(row) = rows.next()? {
        let value = crate::db::row_to_value(row)?;
        if entity == "setting" {
            let key = value.get("key").and_then(|k| k.as_str()).unwrap_or("");
            if is_secret_setting(key) {
                continue; // API-key fields are never imported as events.
            }
        }
        append_in_tx(tx, NewEvent::import_entity(entity, value)?)?;
        count += 1;
    }
    Ok(count)
}

/// A settings row is secret when its key names an API-key field.
pub fn is_secret_setting(key: &str) -> bool {
    key.contains("api_key")
}

/// The keychain account for a provider ("default" when unnamed).
pub fn keychain_account(provider: &str) -> String {
    let p = provider.trim();
    if p.is_empty() {
        "default".to_string()
    } else {
        p.to_string()
    }
}

/// Move every non-empty API-key setting to the OS keychain (account =
/// provider name) and clear the settings row on success. Keys that cannot be
/// moved are left in place and only counted — the value is never logged.
fn migrate_api_keys_to_keychain(conn: &Connection) -> Result<(usize, usize), EventError> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings WHERE key LIKE '%api_key%'")?;
    let mut rows = stmt.query([])?;
    let mut secrets: Vec<(String, String)> = Vec::new();
    while let Some(row) = rows.next()? {
        let key: String = row.get(0)?;
        let value: String = row.get(1)?;
        if !value.trim().is_empty() {
            secrets.push((key, value));
        }
    }
    drop(rows);
    drop(stmt);

    let mut moved = 0usize;
    let mut left = 0usize;
    for (key, value) in secrets {
        let account = keychain_account_for(conn, &key);
        match keyring::Entry::new(KEYCHAIN_SERVICE, &account)
            .and_then(|entry| entry.set_password(&value))
        {
            Ok(()) => {
                // Move semantics: the key never re-enters the DB.
                crate::db::set_setting(conn, &key, "")?;
                moved += 1;
            }
            // Keychain unavailable (e.g. no OS keychain backend): leave the
            // key where it is. Note the failure without logging the value.
            Err(_) => left += 1,
        }
    }
    Ok((moved, left))
}

/// Account for a secret settings key: the `api_key` row is stored under the
/// configured provider's name; `<provider>_api_key` rows under `<provider>`.
fn keychain_account_for(conn: &Connection, key: &str) -> String {
    if key == "api_key" {
        let provider = crate::db::get_setting(conn, "provider");
        keychain_account(&provider)
    } else {
        key.strip_suffix("_api_key").unwrap_or(key).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    /// Legacy schema + seed data on an in-memory connection.
    fn legacy_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::Db::migrate(&conn).unwrap();
        db::Db::seed(&conn).unwrap();
        conn
    }

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn import_events(conn: &Connection, entity: &str) -> Vec<Value> {
        EventStore::new(conn)
            .events_all()
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == format!("import.{entity}"))
            .map(|e| e.payload)
            .collect()
    }

    #[test]
    fn imports_one_event_per_legacy_row() {
        let conn = legacy_conn();
        let report = run(&conn).unwrap();
        assert!(!report.skipped);

        for (entity, table) in IMPORT_TABLES {
            let rows = count(&conn, &format!("SELECT COUNT(*) FROM {table}"));
            let expected = if *table == "settings" {
                // the api_key row is excluded from the import (AD-16)
                rows - 1
            } else {
                rows
            };
            let events = import_events(&conn, entity);
            assert_eq!(
                events.len() as i64,
                expected,
                "import.{entity} events must equal {table} rows"
            );
            assert_eq!(*report.imported.get(*entity).unwrap() as i64, expected);
        }

        // Marker appended exactly once, last.
        let all = EventStore::new(&conn).events_all().unwrap();
        let markers = all.iter().filter(|e| e.kind == MIGRATION_COMPLETED).count();
        assert_eq!(markers, 1);
        assert_eq!(all.last().unwrap().kind, MIGRATION_COMPLETED);
        assert_eq!(
            all.last().unwrap().actor,
            Actor::System {
                component: SystemComponent::Migration
            }
        );
    }

    #[test]
    fn legacy_rows_are_preserved_unchanged() {
        let conn = legacy_conn();
        let before: Vec<(String, i64)> = IMPORT_TABLES
            .iter()
            .map(|(_, table)| {
                (
                    (*table).to_string(),
                    count(&conn, &format!("SELECT COUNT(*) FROM {table}")),
                )
            })
            .collect();
        run(&conn).unwrap();
        for (table, n) in before {
            assert_eq!(count(&conn, &format!("SELECT COUNT(*) FROM {table}")), n);
        }
    }

    #[test]
    fn migration_is_idempotent() {
        let conn = legacy_conn();
        let first = run(&conn).unwrap();
        assert!(!first.skipped);
        let head = EventStore::new(&conn).head_seq().unwrap();
        let total = EventStore::new(&conn).events_all().unwrap().len();

        let second = run(&conn).unwrap();
        assert!(second.skipped, "second run must detect the marker and skip");
        assert_eq!(EventStore::new(&conn).head_seq().unwrap(), head);
        assert_eq!(EventStore::new(&conn).events_all().unwrap().len(), total);
    }

    #[test]
    fn api_key_never_enters_the_event_log() {
        const SENTINEL: &str = "sk-test-NEVER-LEAK-THIS-KEY-9f1c";
        const PROVIDER: &str = "rc-unit-test-provider";

        let conn = legacy_conn();
        db::set_setting(&conn, "provider", PROVIDER).unwrap();
        db::set_setting(&conn, "api_key", SENTINEL).unwrap();

        let report = run(&conn).unwrap();
        assert!(!report.skipped);
        assert_eq!(report.keys_moved_to_keychain + report.keys_left_in_settings, 1);

        // No event — kind, payload, id, causes, anything — contains the key.
        let all = EventStore::new(&conn).events_all().unwrap();
        assert!(!all.is_empty());
        for event in &all {
            let whole = serde_json::to_string(event).unwrap();
            assert!(
                !whole.contains(SENTINEL),
                "event {} ({}) leaked the api key",
                event.seq,
                event.kind
            );
        }
        // The api_key settings row is not imported as an event at all.
        for payload in import_events(&conn, "setting") {
            let key = payload.get("key").and_then(|k| k.as_str()).unwrap_or("");
            assert_ne!(key, "api_key");
        }

        // When the keychain accepted the key, it holds the value and the
        // settings row is cleared (move semantics). Clean up afterwards.
        match keyring::Entry::new(KEYCHAIN_SERVICE, PROVIDER) {
            Ok(entry) => {
                if let Ok(stored) = entry.get_password() {
                    assert_eq!(stored, SENTINEL);
                    assert_eq!(db::get_setting(&conn, "api_key"), "");
                    let _ = entry.delete_credential();
                } else {
                    // Keychain rejected the write: the key must remain in settings.
                    assert_eq!(db::get_setting(&conn, "api_key"), SENTINEL);
                }
            }
            Err(_) => {
                assert_eq!(db::get_setting(&conn, "api_key"), SENTINEL);
            }
        }

        // Re-running never re-imports or re-moves.
        let again = run(&conn).unwrap();
        assert!(again.skipped);
    }

    #[test]
    fn secret_setting_detection() {
        assert!(is_secret_setting("api_key"));
        assert!(is_secret_setting("openai_api_key"));
        assert!(!is_secret_setting("provider"));
        assert!(!is_secret_setting("model"));
    }

    #[test]
    fn import_events_use_the_migration_actor() {
        let conn = legacy_conn();
        run(&conn).unwrap();
        for event in EventStore::new(&conn).events_all().unwrap() {
            assert_eq!(
                event.actor,
                Actor::System {
                    component: SystemComponent::Migration
                }
            );
        }
    }
}
