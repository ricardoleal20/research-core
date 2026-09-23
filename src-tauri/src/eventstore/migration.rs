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
    /// MCP-server env secrets successfully moved to the OS keychain
    /// (review R-02) — the documented home of MCP credentials.
    pub mcp_secrets_moved_to_keychain: usize,
    /// MCP-server env secrets that could not be moved and stay in the
    /// legacy column (redacted from the import event either way — the
    /// values themselves are never recorded).
    pub mcp_secrets_left_in_settings: usize,
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
    // MCP-server env secrets too (review R-02) — the documented home of MCP
    // credentials is `mcp_servers.env`, and it must never reach the log.
    let (mcp_moved, mcp_left) = migrate_mcp_env_to_keychain(conn)?;

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
        mcp_secrets_moved_to_keychain: mcp_moved,
        mcp_secrets_left_in_settings: mcp_left,
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
/// for determinism). Secret settings rows are excluded (AD-16), and
/// `mcp_server` rows are scrubbed of their credential-bearing fields before
/// the event is built (review R-02) — `env` values, `args` tokens, and `url`
/// query strings never enter the log. Returns the number of events appended.
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
                continue; // secret-named fields are never imported as events.
            }
        }
        // The value that enters the log: mcp_server rows are scrubbed.
        let value = if entity == "mcp_server" {
            redact_mcp_server_row(&value)
        } else {
            value
        };
        append_in_tx(tx, NewEvent::import_entity(entity, value)?)?;
        count += 1;
    }
    Ok(count)
}

/// A settings row is secret when its key names a credential field. The
/// match is deliberately broad (review R-13): anything token/secret/
/// password/credential/key-shaped never enters the event log — a false
/// positive only skips an import, a false negative leaks a secret forever.
pub fn is_secret_setting(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    ["api_key", "token", "secret", "password", "credential", "passphrase"]
        .iter()
        .any(|needle| k.contains(needle))
        || k.ends_with("_key")
        || k == "key"
}

/// True when an MCP env/args/url name is credential-shaped (shared by the
/// migration keychain move, the import scrub, and the export scrub —
/// one matcher, three boundaries).
fn is_secret_name(name: &str) -> bool {
    is_secret_setting(name)
}

/// The keychain account for one MCP server env secret: `mcp:<server id>:<KEY>`
/// (the key pattern `keychain_account` established for providers — service
/// `ResearchCore`, one account per secret).
pub fn mcp_keychain_account(server_id: &str, key: &str) -> String {
    format!("mcp:{server_id}:{key}")
}

/// Scrub one `import.mcp_server` payload of its credential-bearing fields
/// (review R-02): `env` values of secret-named keys, `args` tokens that
/// carry secret-named assignments or follow secret-named flags, and `url`
/// query-string values of secret-named parameters. Non-secret data (server
/// names, commands, plain env like `NODE_ENV=production`) survives — the
/// event stays useful as a record, it just never carries a credential.
/// Shared by the import boundary AND the export boundary (logs written
/// before this scrub existed are scrubbed again on their way out).
pub fn redact_mcp_server_row(row: &Value) -> Value {
    let mut out = row.clone();
    if !out.is_object() {
        return out;
    }
    // env: `KEY=value` pairs (comma-separated) — secret-named values go.
    if let Some(env) = out.get("env").and_then(|v| v.as_str()) {
        let redacted = env
            .split(',')
            .filter(|p| !p.trim().is_empty())
            .map(|pair| match pair.split_once('=') {
                Some((k, _)) if !is_secret_name(k.trim()) => pair.to_string(),
                Some((k, _)) => format!("{}=[redacted]", k.trim()),
                None => pair.to_string(),
            })
            .collect::<Vec<_>>()
            .join(",");
        out["env"] = Value::String(redacted);
    }
    // args: a shell-ish string — redact `name=secret` tokens and the value
    // after --token/--secret/--password/--api-key/--key flags.
    if let Some(args) = out.get("args").and_then(|v| v.as_str()) {
        let mut tokens: Vec<String> = args.split_whitespace().map(String::from).collect();
        let mut redact_next = false;
        for token in tokens.iter_mut() {
            if redact_next {
                *token = "[redacted]".into();
                redact_next = false;
                continue;
            }
            if let Some((name, _)) = token.split_once('=') {
                if is_secret_name(name.trim_start_matches('-')) {
                    *token = format!("{name}[redacted]");
                    continue;
                }
            }
            let flag = token.trim_start_matches('-');
            if is_secret_name(flag) || flag == "api-key" {
                redact_next = true;
            }
        }
        out["args"] = Value::String(tokens.join(" "));
    }
    // url: keep scheme://host/path, redact secret-named query values.
    if let Some(url) = out.get("url").and_then(|v| v.as_str()) {
        if let Some((base, query)) = url.split_once('?') {
            let redacted = query
                .split('&')
                .filter(|p| !p.is_empty())
                .map(|param| match param.split_once('=') {
                    Some((name, _)) if is_secret_name(name) => format!("{name}=[redacted]"),
                    _ => param.to_string(),
                })
                .collect::<Vec<_>>()
                .join("&");
            out["url"] = Value::String(format!("{base}?{redacted}"));
        }
    }
    out
}

/// Move MCP-server env secrets to the OS keychain (review R-02, the
/// `migrate_api_keys_to_keychain` pattern applied to the documented home of
/// MCP credentials): every secret-named `KEY=value` pair in
/// `mcp_servers.env` moves under account `mcp:<server id>:<KEY>`; on
/// success the pair leaves the column (move semantics — the value never
/// re-enters the DB), on keychain failure it stays (but is still scrubbed
/// from the import event). `McpServerDef` re-hydrates from the keychain at
/// spawn, so a moved secret keeps working.
fn migrate_mcp_env_to_keychain(conn: &Connection) -> Result<(usize, usize), EventError> {
    let mut stmt = conn.prepare("SELECT id, env FROM mcp_servers")?;
    let rows: Vec<(String, Option<String>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    drop(stmt);

    let mut moved = 0usize;
    let mut left = 0usize;
    for (server_id, env) in rows {
        let Some(env) = env.filter(|e| !e.trim().is_empty()) else {
            continue;
        };
        let mut kept: Vec<String> = Vec::new();
        let mut changed = false;
        for pair in env.split(',') {
            let Some((key, value)) = pair.split_once('=') else {
                kept.push(pair.to_string());
                continue;
            };
            let key = key.trim();
            if !is_secret_name(key) || value.trim().is_empty() {
                kept.push(pair.to_string());
                continue;
            }
            match keyring::Entry::new(KEYCHAIN_SERVICE, &mcp_keychain_account(&server_id, key))
                .and_then(|entry| entry.set_password(value.trim()))
            {
                Ok(()) => {
                    // Move semantics, with a tombstone: the pair stays as
                    // `KEY=` (empty value) so `McpServerDef`'s spawn knows
                    // to re-hydrate this key from the keychain — the value
                    // itself never re-enters the DB.
                    moved += 1;
                    changed = true;
                    kept.push(format!("{key}="));
                }
                Err(_) => {
                    left += 1; // stays in the column — scrubbed from the event
                    kept.push(pair.to_string());
                }
            }
        }
        if changed {
            conn.execute(
                "UPDATE mcp_servers SET env = ?2 WHERE id = ?1",
                rusqlite::params![server_id, kept.join(",")],
            )?;
        }
    }
    Ok((moved, left))
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
        // A UNIQUE account per run: the keychain prompts on access to an
        // item another (unsigned, differently-hashed) test binary created —
        // a fresh item never prompts, so the test stays headless-deterministic.
        let provider = format!("rc-unit-test-provider-{}", uuid::Uuid::new_v4().simple());

        let conn = legacy_conn();
        db::set_setting(&conn, "provider", &provider).unwrap();
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
        match keyring::Entry::new(KEYCHAIN_SERVICE, &provider) {
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
        // the broadened matcher (review R-13): credential-shaped names of
        // any spelling never enter the log
        assert!(is_secret_setting("apiToken"));
        assert!(is_secret_setting("openai.token"));
        assert!(is_secret_setting("zotero_secret"));
        assert!(is_secret_setting("db_password"));
        assert!(is_secret_setting("auth_credential"));
        assert!(is_secret_setting("anthropic_key"));
        assert!(is_secret_setting("KEY"));
        assert!(!is_secret_setting("provider"));
        assert!(!is_secret_setting("model"));
        assert!(!is_secret_setting("base_url"));
        assert!(!is_secret_setting("agent_path"));
    }

    /// MCP-server credentials never enter the event log (review R-02): the
    /// documented home of MCP secrets is `mcp_servers.env` — those values
    /// move to the keychain (or stay redacted), and `args`/`url` are
    /// scrubbed at the import boundary. Non-secret data survives.
    #[test]
    fn mcp_server_secrets_never_enter_the_event_log() {
        const SENTINEL: &str = "sk-mcp-NEVER-LEAK-THIS-9c2d";
        const URL_SENTINEL: &str = "url-secret-NEVER-LEAK-7e1f";
        // A UNIQUE server id per run: the keychain move creates only fresh
        // items (an item left by another test binary's run would prompt the
        // SecurityAgent — never in a headless test).
        let server = format!("rc-unit-test-mcp-{}", uuid::Uuid::new_v4().simple());

        let conn = legacy_conn();
        conn.execute(
            "INSERT INTO mcp_servers(id,name,transport,command,args,env,url,tags,connected,created_at)
             VALUES(?1,'Brave','stdio','npx','--token url-arg-placeholder -y @mcp/brave',?2,?3,'search',0,?4)",
            rusqlite::params![
                server,
                format!("BRAVE_API_KEY={SENTINEL},NODE_ENV=production"),
                format!("https://api.brave.example/v1?token={URL_SENTINEL}&client=rc"),
                chrono::Utc::now().to_rfc3339()
            ],
        )
        .unwrap();

        let report = run(&conn).unwrap();
        assert!(!report.skipped);
        assert_eq!(
            report.mcp_secrets_moved_to_keychain + report.mcp_secrets_left_in_settings,
            1,
            "exactly one MCP env secret was handled"
        );

        // No event — kind, payload, id, causes, anything — contains either
        // secret, and the import event kept the non-secret data.
        let events = EventStore::new(&conn).events_all().unwrap();
        assert!(!events.is_empty());
        let mut saw_mcp_import = false;
        for event in &events {
            let whole = serde_json::to_string(event).unwrap();
            assert!(
                !whole.contains(SENTINEL) && !whole.contains(URL_SENTINEL),
                "event {} ({}) leaked an MCP secret",
                event.seq,
                event.kind
            );
            if event.kind == "import.mcp_server"
                && event.payload.get("id").and_then(|v| v.as_str()) == Some(server.as_str())
            {
                saw_mcp_import = true;
                let env = event.payload.get("env").and_then(|v| v.as_str()).unwrap_or("");
                assert!(env.contains("NODE_ENV=production"), "non-secret env survives: {env}");
                assert!(
                    env.contains("BRAVE_API_KEY=[redacted]") || !env.contains("BRAVE_API_KEY"),
                    "the secret value never rides along: {env}"
                );
                let url = event.payload.get("url").and_then(|v| v.as_str()).unwrap_or("");
                assert!(url.contains("client=rc"), "non-secret query params survive: {url}");
                assert!(url.contains("token=[redacted]"), "secret query values are redacted: {url}");
                let args = event.payload.get("args").and_then(|v| v.as_str()).unwrap_or("");
                assert!(args.contains("[redacted]"), "secret flag values are redacted: {args}");
            }
        }
        assert!(saw_mcp_import, "the mcp_server row was imported (scrubbed)");

        // Keychain cleanup: delete the credential if the move succeeded.
        if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, &mcp_keychain_account(&server, "BRAVE_API_KEY")) {
            let _ = entry.delete_credential();
        }
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
