use crate::agent;
use crate::db::{self, Db};
use crate::mcp::{self, McpRegistry, McpServerDef};
use crate::AppPaths;
use rusqlite::params;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;

fn now() -> String { chrono::Utc::now().to_rfc3339() }
fn uid() -> String { uuid::Uuid::new_v4().to_string() }
fn err(e: impl ToString) -> String { e.to_string() }

// ---------- Projects ----------
#[tauri::command]
pub async fn list_projects(db: State<'_, Db>) -> Result<Vec<Value>, String> {
    let c = db.0.lock().await;
    db::query_all(&c, "SELECT * FROM projects ORDER BY is_active DESC, updated_at DESC", ())
        .map_err(err)
}
#[tauri::command]
pub async fn get_active_project(db: State<'_, Db>) -> Result<Option<Value>, String> {
    let c = db.0.lock().await;
    db::query_one(&c, "SELECT * FROM projects WHERE is_active=1", ()).map_err(err)
}
#[tauri::command]
pub async fn set_active_project(db: State<'_, Db>, id: String) -> Result<(), String> {
    let c = db.0.lock().await;
    c.execute("UPDATE projects SET is_active=0", []).map_err(err)?;
    c.execute("UPDATE projects SET is_active=1, updated_at=?2 WHERE id=?1", params![id, now()]).map_err(err)?;
    Ok(())
}
#[tauri::command]
pub async fn create_project(db: State<'_, Db>, name: String, folder: String, kind: String, tags: String, color: String) -> Result<Value, String> {
    let id = uid();
    let c = db.0.lock().await;
    c.execute("UPDATE projects SET is_active=0", []).map_err(err)?;
    c.execute(
        "INSERT INTO projects(id,name,folder,kind,tags,color,chapter_index,chapter_count,created_at,updated_at,is_active)
         VALUES(?1,?2,?3,?4,?5,?6,1,1,?7,?8,1)",
        params![id, name, folder, kind, tags, color, now(), now()],
    ).map_err(err)?;
    db::query_one(&c, "SELECT * FROM projects WHERE id=?1", &[&id]).map(|o| o.unwrap_or(Value::Null)).map_err(err)
}
#[tauri::command]
pub async fn update_project(db: State<'_, Db>, id: String, name: String, folder: String, tags: String) -> Result<(), String> {
    let c = db.0.lock().await;
    c.execute("UPDATE projects SET name=?2, folder=?3, tags=?4, updated_at=?5 WHERE id=?1",
        params![id, name, folder, tags, now()]).map_err(err)?;
    Ok(())
}
#[tauri::command]
pub async fn delete_project(db: State<'_, Db>, id: String) -> Result<(), String> {
    let c = db.0.lock().await;
    c.execute("DELETE FROM projects WHERE id=?1", params![id]).map_err(err)?;
    Ok(())
}

// ---------- Dashboard ----------
#[tauri::command]
pub async fn get_dashboard(db: State<'_, Db>, project_id: String) -> Result<Value, String> {
    let c = db.0.lock().await;
    let total: i64 = c.query_row("SELECT COUNT(*) FROM refs WHERE project_id=?1", params![project_id], |r| r.get(0)).unwrap_or(0);
    let used: i64 = c.query_row("SELECT COUNT(*) FROM refs WHERE project_id=?1 AND used=1", params![project_id], |r| r.get(0)).unwrap_or(0);
    let in_text: i64 = c.query_row("SELECT COALESCE(SUM(citation_count),0) FROM refs WHERE project_id=?1", params![project_id], |r| r.get(0)).unwrap_or(0);
    let last_review = db::query_one(&c, "SELECT * FROM reviews WHERE project_id=?1 ORDER BY number DESC LIMIT 1", &[&project_id]).unwrap_or(None);
    let review_history = db::query_all(&c, "SELECT number, score FROM reviews WHERE project_id=?1 ORDER BY number ASC", &[&project_id]).unwrap_or_default();
    let active_actions: i64 = c.query_row("SELECT COUNT(*) FROM actions WHERE project_id=?1 AND done=0", params![project_id], |r| r.get(0)).unwrap_or(0);
    let done_actions: i64 = c.query_row("SELECT COUNT(*) FROM actions WHERE project_id=?1 AND done=1", params![project_id], |r| r.get(0)).unwrap_or(0);
    let timeline = db::query_all(&c,
        "SELECT 'review' AS type, verdict AS text, created_at FROM reviews WHERE project_id=?1
         UNION ALL SELECT 'ref' AS type, title AS text, created_at FROM refs WHERE project_id=?1
         ORDER BY created_at DESC LIMIT 6", &[&project_id]).unwrap_or_default();
    Ok(json!({
        "refs_total": total, "refs_used": used, "citations_in_text": in_text,
        "last_review": last_review, "review_history": review_history,
        "active_actions": active_actions, "done_actions": done_actions,
        "timeline": timeline
    }))
}

// ---------- Refs ----------
// The library read re-folds the evented library projection (FR-15, Epic 5):
// the legacy `refs` table is the implicit baseline, `ref.added` /
// `ref.removed` / `ref.restored` events apply on top — archived refs read
// with their `removed` flag. The legacy `create_ref`/`delete_ref` paths
// stay dead (AD-16): mutations are evented in domain/library.
#[tauri::command]
pub async fn list_refs(db: State<'_, Db>, project_id: String, filter: Option<String>) -> Result<Vec<crate::domain::library::LibraryRef>, String> {
    let c = db.0.lock().await;
    crate::library_commands::list_refs_inner(&c, Some(&project_id), filter.as_deref())
}
#[tauri::command]
pub async fn get_ref(db: State<'_, Db>, id: String) -> Result<Value, String> {
    let c = db.0.lock().await;
    let r = db::query_one(&c, "SELECT * FROM refs WHERE id=?1", &[&id]).map_err(err)?;
    let usages = db::query_all(&c, "SELECT * FROM ref_usages WHERE ref_id=?1 ORDER BY created_at", &[&id]).unwrap_or_default();
    let mut out = r.unwrap_or(Value::Null);
    if let Value::Object(ref mut m) = out { m.insert("usages".into(), Value::Array(usages)); }
    Ok(out)
}
#[tauri::command]
pub async fn create_ref(db: State<'_, Db>, project_id: String, title: String, authors: String, year: i64, venue: String, doi: String, url: String, tags: String) -> Result<Value, String> {
    let id = uid();
    let c = db.0.lock().await;
    c.execute(
        "INSERT INTO refs(id,project_id,title,authors,year,venue,doi,url,tags,status,used,citation_count,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'unread',0,0,?10)",
        params![id, project_id, title, authors, year, venue, doi, url, tags, now()],
    ).map_err(err)?;
    db::query_one(&c, "SELECT * FROM refs WHERE id=?1", &[&id]).map(|o| o.unwrap_or(Value::Null)).map_err(err)
}
#[tauri::command]
pub async fn update_ref(db: State<'_, Db>, id: String, title: String, authors: String, year: i64, venue: String, doi: String, url: String, tags: String, status: String) -> Result<(), String> {
    let c = db.0.lock().await;
    c.execute("UPDATE refs SET title=?2,authors=?3,year=?4,venue=?5,doi=?6,url=?7,tags=?8,status=?9 WHERE id=?1",
        params![id, title, authors, year, venue, doi, url, tags, status]).map_err(err)?;
    Ok(())
}
#[tauri::command]
pub async fn delete_ref(db: State<'_, Db>, id: String) -> Result<(), String> {
    let c = db.0.lock().await;
    c.execute("DELETE FROM refs WHERE id=?1", params![id]).map_err(err)?;
    Ok(())
}
#[tauri::command]
pub async fn search_refs(db: State<'_, Db>, project_id: String, q: String) -> Result<Vec<Value>, String> {
    let c = db.0.lock().await;
    let pat = format!("%{}%", q);
    db::query_all(&c,
        "SELECT * FROM refs WHERE project_id=?1 AND (title LIKE ?2 OR authors LIKE ?2 OR doi LIKE ?2) ORDER BY year DESC",
        &[&project_id, &pat]).map_err(err)
}
#[tauri::command]
pub async fn search_refs_external(_db: State<'_, Db>, q: String) -> Result<Vec<Value>, String> {
    let results = mcp::arxiv_search(&q, 8).await?;
    Ok(results.into_iter().map(|r| json!({
        "title": r.title, "authors": r.authors, "year": r.year,
        "venue": r.venue, "doi": r.doi, "url": r.url, "abstract": r.abstract_text
    })).collect())
}

// ---------- Collections ----------
#[tauri::command]
pub async fn list_collections(db: State<'_, Db>, project_id: String) -> Result<Vec<Value>, String> {
    let c = db.0.lock().await;
    db::query_all(&c,
        "SELECT c.*, (SELECT COUNT(*) FROM refs r WHERE r.collection_id=c.id) AS count FROM collections c WHERE c.project_id=?1 ORDER BY c.name",
        &[&project_id]).map_err(err)
}

// ---------- Reviews ----------
#[tauri::command]
pub async fn list_reviews(db: State<'_, Db>, project_id: String) -> Result<Vec<Value>, String> {
    let c = db.0.lock().await;
    let mut reviews = db::query_all(&c, "SELECT * FROM reviews WHERE project_id=?1 ORDER BY number DESC", &[&project_id]).map_err(err)?;
    for r in reviews.iter_mut() {
        if let Some(d) = r.get("dims").and_then(|v| v.as_str()) {
            if let Ok(parsed) = serde_json::from_str::<Value>(d) {
                if let Value::Object(m) = r { m.insert("dims".into(), parsed); }
            }
        }
        let rid = r.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let findings = db::query_all(&c, "SELECT * FROM findings WHERE review_id=?1 ORDER BY rowid", &[&rid]).unwrap_or_default();
        if let Value::Object(m) = r { m.insert("findings".into(), Value::Array(findings)); }
    }
    Ok(reviews)
}
#[tauri::command]
pub async fn run_review(db: State<'_, Db>, project_id: String, focus: String) -> Result<Value, String> {
    agent::run_review(&db, &project_id, &focus).await
}

// ---------- Actions ----------
#[tauri::command]
pub async fn list_actions(db: State<'_, Db>, project_id: String, done: bool) -> Result<Vec<Value>, String> {
    let c = db.0.lock().await;
    let done_i64: i64 = done as i64;
    db::query_all(&c,
        "SELECT * FROM actions WHERE project_id=?1 AND done=?2 ORDER BY (due=''), due, created_at DESC",
        params![project_id, done_i64]).map_err(err)
}
#[tauri::command]
pub async fn create_action(db: State<'_, Db>, project_id: String, title: String, priority: String, origin: String, due: String, location: String, description: String) -> Result<Value, String> {
    let id = uid();
    let c = db.0.lock().await;
    let code: String = c.query_row("SELECT COALESCE(MAX(CAST(SUBSTR(code,3) AS INTEGER)),0)+1 FROM actions WHERE project_id=?1", params![project_id], |r| r.get::<_, i64>(0))
        .map(|n| format!("A-{:03}", n)).unwrap_or_else(|_| "A-001".into());
    c.execute(
        "INSERT INTO actions(id,project_id,code,title,description,priority,origin,due,location,done,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,0,?10)",
        params![id, project_id, code, title, description, priority, origin, due, location, now()],
    ).map_err(err)?;
    db::query_one(&c, "SELECT * FROM actions WHERE id=?1", &[&id]).map(|o| o.unwrap_or(Value::Null)).map_err(err)
}
#[tauri::command]
pub async fn toggle_action(db: State<'_, Db>, id: String, done: bool) -> Result<(), String> {
    let c = db.0.lock().await;
    c.execute("UPDATE actions SET done=?2, done_at=CASE WHEN ?2=1 THEN ?3 ELSE NULL END WHERE id=?1",
        params![id, done as i64, now()]).map_err(err)?;
    Ok(())
}
#[tauri::command]
pub async fn update_action(db: State<'_, Db>, id: String, title: String, description: String, priority: String, due: String, notes: String) -> Result<(), String> {
    let c = db.0.lock().await;
    c.execute("UPDATE actions SET title=?2,description=?3,priority=?4,due=?5,notes=?6 WHERE id=?1",
        params![id, title, description, priority, due, notes]).map_err(err)?;
    Ok(())
}
#[tauri::command]
pub async fn delete_action(db: State<'_, Db>, id: String) -> Result<(), String> {
    let c = db.0.lock().await;
    c.execute("DELETE FROM actions WHERE id=?1", params![id]).map_err(err)?;
    Ok(())
}

// ---------- Chats ----------
// The chat surface lives in chat_commands.rs (Epic 5): mission scoping with
// board context, attachments, and the per-conversation skill — the
// chats/messages tables are the legacy baseline, the scope/skill/attachment
// movements are events in the log.

// ---------- Agents ----------
#[tauri::command]
pub async fn list_agents(db: State<'_, Db>) -> Result<Vec<Value>, String> {
    let c = db.0.lock().await;
    db::query_all(&c, "SELECT * FROM agents ORDER BY kind, name", ()).map_err(err)
}
#[tauri::command]
pub async fn toggle_agent(db: State<'_, Db>, id: String, enabled: bool) -> Result<(), String> {
    let c = db.0.lock().await;
    c.execute("UPDATE agents SET enabled=?2 WHERE id=?1", params![id, enabled as i64]).map_err(err)?;
    Ok(())
}

// ---------- MCP ----------
#[tauri::command]
pub async fn list_mcp_servers(db: State<'_, Db>) -> Result<Vec<Value>, String> {
    let c = db.0.lock().await;
    db::query_all(&c, "SELECT * FROM mcp_servers ORDER BY name", ()).map_err(err)
}
#[tauri::command]
pub async fn add_mcp_server(db: State<'_, Db>, name: String, transport: String, command: String, args: String, env: String, url: String, tags: String) -> Result<Value, String> {
    let id = uid();
    let c = db.0.lock().await;
    c.execute("INSERT INTO mcp_servers(id,name,transport,command,args,env,url,tags,connected,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,0,?9)",
        params![id, name, transport, command, args, env, url, tags, now()]).map_err(err)?;
    db::query_one(&c, "SELECT * FROM mcp_servers WHERE id=?1", &[&id]).map(|o| o.unwrap_or(Value::Null)).map_err(err)
}
#[tauri::command]
pub async fn update_mcp_server(db: State<'_, Db>, id: String, name: String, transport: String, command: String, args: String, env: String, url: String, tags: String, connected: bool) -> Result<(), String> {
    let c = db.0.lock().await;
    c.execute("UPDATE mcp_servers SET name=?2,transport=?3,command=?4,args=?5,env=?6,url=?7,tags=?8,connected=?9 WHERE id=?1",
        params![id, name, transport, command, args, env, url, tags, connected as i64]).map_err(err)?;
    Ok(())
}
#[tauri::command]
pub async fn delete_mcp_server(db: State<'_, Db>, id: String) -> Result<(), String> {
    let c = db.0.lock().await;
    c.execute("DELETE FROM mcp_servers WHERE id=?1", params![id]).map_err(err)?;
    Ok(())
}
#[tauri::command]
pub async fn test_mcp_server(db: State<'_, Db>, registry: State<'_, McpRegistry>, id: String) -> Result<Value, String> {
    let def = {
        let c = db.0.lock().await;
        let row = db::query_one(&c, "SELECT * FROM mcp_servers WHERE id=?1", &[&id]).map_err(err)?.ok_or("not found")?;
        McpServerDef {
            id: row.get("id").and_then(|v| v.as_str()).unwrap_or("").into(),
            name: row.get("name").and_then(|v| v.as_str()).unwrap_or("").into(),
            transport: row.get("transport").and_then(|v| v.as_str()).unwrap_or("stdio").into(),
            command: row.get("command").and_then(|v| v.as_str()).map(String::from),
            args: row.get("args").and_then(|v| v.as_str()).map(String::from),
            env: row.get("env").and_then(|v| v.as_str()).map(String::from),
            url: row.get("url").and_then(|v| v.as_str()).map(String::from),
            tags: row.get("tags").and_then(|v| v.as_str()).map(String::from),
        }
    };
    if def.transport == "stdio" {
        match registry.connect(&def).await {
            Ok(tools) => {
                let c = db.0.lock().await;
                c.execute("UPDATE mcp_servers SET connected=1 WHERE id=?1", params![id]).map_err(err)?;
                Ok(json!({ "ok": true, "tools": tools.len(), "tool_names": tools.iter().map(|t| t.name.clone()).collect::<Vec<_>>() }))
            }
            Err(e) => {
                let c = db.0.lock().await;
                c.execute("UPDATE mcp_servers SET connected=0 WHERE id=?1", params![id]).ok();
                Err(e)
            }
        }
    } else {
        // http: mark connected optimistically (REST adapters handled in-app)
        let c = db.0.lock().await;
        c.execute("UPDATE mcp_servers SET connected=1 WHERE id=?1", params![id]).map_err(err)?;
        Ok(json!({ "ok": true, "tools": "rest-adapter" }))
    }
}
#[tauri::command]
pub async fn list_mcp_tools(registry: State<'_, McpRegistry>) -> Result<Value, String> {
    let sessions = registry.sessions.lock().await;
    let mut out = serde_json::Map::new();
    for (id, s) in sessions.iter() {
        let tools = s.tools().await;
        out.insert(id.clone(), json!(tools));
    }
    Ok(Value::Object(out))
}

// ---------- Settings ----------
#[tauri::command]
pub async fn get_settings(db: State<'_, Db>) -> Result<Value, String> {
    let c = db.0.lock().await;
    let rows = db::query_all(&c, "SELECT key, value FROM settings", ()).map_err(err)?;
    let mut map = serde_json::Map::new();
    for r in rows {
        if let (Some(k), Some(v)) = (r.get("key").and_then(|v| v.as_str()), r.get("value").and_then(|v| v.as_str())) {
            map.insert(k.to_string(), Value::String(v.to_string()));
        }
    }
    Ok(Value::Object(map))
}
#[tauri::command]
pub async fn update_setting(db: State<'_, Db>, key: String, value: String) -> Result<(), String> {
    let c = db.0.lock().await;
    db::set_setting(&c, &key, &value).map_err(err)
}

/// Store the provider API key in the OS keychain (AD-16 / Story 1.6:
/// keychain-only credentials) under the currently configured provider's
/// account, and clear the legacy `settings.api_key` row so the value never
/// (re-)enters the database. An empty key clears the stored credential
/// (the wizard's simulate/cli modes).
#[tauri::command]
pub async fn set_provider_key(db: State<'_, Db>, key: String) -> Result<(), String> {
    let c = db.0.lock().await;
    let provider = db::get_setting(&c, "provider");
    let account = crate::eventstore::migration::keychain_account(&provider);
    let entry = keyring::Entry::new(crate::eventstore::migration::KEYCHAIN_SERVICE, &account)
        .map_err(err)?;
    let key = key.trim();
    if key.is_empty() {
        // Clearing: best effort — a keychain we cannot delete from is the
        // same keychain we cannot read from, so the key stays unusable.
        let _ = entry.delete_credential();
    } else {
        entry.set_password(key).map_err(err)?;
    }
    // Move semantics: the key never (re-)enters the database.
    if !db::get_setting(&c, "api_key").trim().is_empty() {
        db::set_setting(&c, "api_key", "").map_err(err)?;
    }
    Ok(())
}

/// Danger zone: drop every table and rebuild the schema + seed defaults.
/// Wipes projects, refs, reviews, actions, chats AND settings (onboarding,
/// lock hash, LLM config) so the app re-runs the first-run setup wizard.
#[tauri::command]
pub async fn reset_database(db: State<'_, Db>) -> Result<(), String> {
    let c = db.0.lock().await;
    db::recreate(&c).map_err(err)
}

// ---------- Logging ----------
/// Append a timestamped line to the app's log file (in the hidden data dir).
/// Replaces the temporary /tmp/rc-diag.log mechanism with a persistent log.
#[tauri::command]
pub async fn app_log(paths: State<'_, AppPaths>, message: String) -> Result<(), String> {
    use std::io::Write;
    let line = format!("[{}] {}\n", chrono::Utc::now().to_rfc3339(), message);
    let path = paths.log_dir.join("app.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
    Ok(())
}

/// Return the resolved data + log dir paths so the UI can display them.
#[tauri::command]
pub async fn get_app_paths(paths: State<'_, AppPaths>) -> Result<Value, String> {
    Ok(json!({
        "data_dir": paths.data_dir.display().to_string(),
        "log_dir": paths.log_dir.display().to_string(),
    }))
}

/// Reveal a path in Finder (used by the setup wizard "Mostrar en Finder").
#[tauri::command]
pub async fn reveal_path(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Native folder picker for the setup wizard / project creation.
#[tauri::command]
pub async fn pick_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app.dialog().file().set_title("Selecciona una carpeta").blocking_pick_folder();
    Ok(picked.map(|p| p.to_string()))
}

// ---------- Local access key (lock policy) ----------
/// Hash a key with a fresh random salt. Returns (hex_hash, hex_salt).
/// Used at wizard Step 3 to persist the local access key without storing it
/// in plaintext.
pub fn hash_key(key: &str) -> (String, String) {
    let mut salt = [0u8; 16];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut salt);
    let salt_hex = hex::encode(&salt);
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(key.as_bytes());
    (hex::encode(hasher.finalize()), salt_hex)
}

/// Verify a key against stored hash+salt.
pub fn verify_hash(key: &str, hash: &str, salt_hex: &str) -> bool {
    let salt = match hex::decode(salt_hex) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize()) == hash
}

/// Verify the supplied key against the persisted lock_hash + lock_salt.
#[tauri::command]
pub async fn verify_key(db: State<'_, Db>, key: String) -> Result<bool, String> {
    let (hash, salt) = {
        let c = db.0.lock().await;
        (db::get_setting(&c, "lock_hash"), db::get_setting(&c, "lock_salt"))
    };
    // No lock configured yet -> always allow (first-run / wizard not done).
    if hash.is_empty() || salt.is_empty() {
        return Ok(true);
    }
    Ok(verify_hash(&key, &hash, &salt))
}

/// Hash + persist the local access key (called once from the wizard Step 3).
#[tauri::command]
pub async fn set_lock_key(db: State<'_, Db>, key: String) -> Result<(), String> {
    // If the user left the key blank at login, don't configure a lock.
    if key.trim().is_empty() {
        return Ok(());
    }
    let (hash, salt) = hash_key(&key);
    let c = db.0.lock().await;
    db::set_setting(&c, "lock_hash", &hash).map_err(err)?;
    db::set_setting(&c, "lock_salt", &salt).map_err(err)?;
    Ok(())
}

/// Return the current lock policy + whether a lock is configured, so the
/// frontend can decide whether to show a lock screen on boot.
#[tauri::command]
pub async fn lock_state(db: State<'_, Db>) -> Result<Value, String> {
    let c = db.0.lock().await;
    Ok(json!({
        "policy": db::get_setting(&c, "lock_policy"),         // never|on_launch|idle|sensitive
        "idle_min": db::get_setting(&c, "lock_idle_min"),
        "configured": !db::get_setting(&c, "lock_hash").is_empty(),
    }))
}

// ---------- LLM CLI detection (wizard Step 4) ----------
/// Probe whether a given CLI binary (claude/codex/opencode) is reachable on
/// PATH. Returns its resolved absolute path, or null if not found.
#[tauri::command]
pub async fn test_cli(command: String) -> Result<Value, String> {
    let path = which_cli(&command);
    Ok(json!({ "command": command, "path": path }))
}

fn which_cli(cmd: &str) -> Option<String> {
    // Use the same augmented PATH the MCP spawner uses, so GUI .app bundles
    // can find Homebrew/nvm-installed CLIs.
    let path_env = mcp::augmented_path();
    for dir in path_env.split(':') {
        if dir.is_empty() { continue; }
        let candidate = std::path::Path::new(dir).join(cmd);
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    None
}

