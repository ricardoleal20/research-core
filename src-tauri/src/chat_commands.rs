// Chat shell commands (Stories 5.4–5.6, FR-16): the assistant conversation
// surface — mission scoping with board context, attachments as context,
// and the per-conversation skill. The chats/messages tables are the legacy
// baseline; every scope/skill/attachment movement is an EVENT in the log
// (domain::chat) with the table columns as their projection (AD-1/AD-16).
// All LLM traffic goes through the provider layer via agent.rs (AD-9);
// attachment content lives in the digest-addressed store and leaves the
// machine only inside the user's chosen provider call (NFR-12).

use crate::agent::{self, AssistantCall};
use crate::db::{self, Db};
use crate::domain::chat::{
    self, folded_attachments, folded_scope, folded_skill, AttachmentAddedPayload,
    AttachmentContext, AttachmentKind, BoardContext, BoardHypothesis, ChatAttachment,
};
use crate::domain::evidence::{EvidenceProjection, PinKind};
use crate::domain::hypotheses::HypothesesProjection;
use crate::domain::missions::{Mission, MissionsProjection};
use crate::domain::skills::{self, Skill};
use crate::eventstore::{EventStore, NewEvent};
use crate::AppPaths;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;
use uuid::Uuid;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
fn uid() -> String {
    Uuid::new_v4().to_string()
}
fn err(e: impl ToString) -> String {
    e.to_string()
}

/// An attachment file as picked (desktop): its display name and absolute
/// path. The browser mock reads content client-side instead.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentPick {
    pub name: String,
    pub path: String,
}

/// The largest file stored as an attachment ref — bigger files are refused
/// (never quietly altered, FR-16.3).
pub const MAX_STORED_BYTES: u64 = 20 * 1024 * 1024;
/// The extracted-text cap that rides a provider call — longer text is
/// included up to the cap and VISIBLY flagged truncated (FR-16.3).
pub const MAX_INLINE_CHARS: usize = 64_000;

// ---------- skills registry (Story 5.6) ----------

/// All skills, builtins first (the curated six) then user-added, each name
/// once. Pure over the connection — the list endpoint the chat header
/// reads; extensible because a skill is a row, not code.
pub fn list_skills_inner(conn: &Connection) -> Result<Vec<Skill>, String> {
    let mut stmt = conn
        .prepare("SELECT name, provider, model, system_prompt, tools, builtin FROM skills ORDER BY builtin DESC, rowid")
        .map_err(err)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(rows
        .into_iter()
        .map(|(name, provider, model, system_prompt, tools, builtin)| Skill {
            name,
            provider,
            model,
            system_prompt,
            tools: serde_json::from_str(&tools).unwrap_or_default(),
            builtin: builtin != 0,
        })
        .collect())
}

/// One skill by name, when the registry holds it.
pub fn skill_by_name(conn: &Connection, name: &str) -> Result<Option<Skill>, String> {
    Ok(list_skills_inner(conn)?
        .into_iter()
        .find(|s| s.name == name.trim()))
}

#[tauri::command]
pub async fn list_skills(db: State<'_, Db>) -> Result<Vec<Skill>, String> {
    let c = db.0.lock().await;
    list_skills_inner(&c)
}

/// Add a skill definition (FR-16.8 extensibility): a skill is data — name,
/// (provider, model) pair (empty = the configured layer), system prompt,
/// and an allowed tool set drawn from the closed vocabulary. v1 ships the
/// six defaults; this is how the pool grows without code changes.
#[tauri::command]
pub async fn add_skill(
    db: State<'_, Db>,
    name: String,
    provider: String,
    model: String,
    system_prompt: String,
    tools: Vec<String>,
) -> Result<Skill, String> {
    let skill = Skill {
        name: name.trim().to_string(),
        provider: provider.trim().to_string(),
        model: model.trim().to_string(),
        system_prompt,
        tools,
        builtin: false,
    };
    skills::validate_skill(&skill)?;
    let c = db.0.lock().await;
    let existing = skill_by_name(&c, &skill.name)?;
    if existing.is_some() {
        return Err(format!(
            "skill `{}` already exists — a skill's name is its identity",
            skill.name
        ));
    }
    c.execute(
        "INSERT INTO skills(name,provider,model,system_prompt,tools,builtin) VALUES(?1,?2,?3,?4,?5,0)",
        params![
            skill.name,
            skill.provider,
            skill.model,
            skill.system_prompt,
            serde_json::to_string(&skill.tools).map_err(err)?
        ],
    )
    .map_err(err)?;
    Ok(skill)
}

// ---------- chats (moved from commands.rs; scope/skill aware) ----------

#[tauri::command]
pub async fn list_chats(
    db: State<'_, Db>,
    project_id: String,
    kind: Option<String>,
) -> Result<Vec<Value>, String> {
    let c = db.0.lock().await;
    match kind {
        Some(k) => db::query_all(&c, "SELECT * FROM chats WHERE project_id=?1 AND kind=?2 ORDER BY updated_at DESC", &[&project_id, &k]).map_err(err),
        None => db::query_all(&c, "SELECT * FROM chats WHERE project_id=?1 ORDER BY updated_at DESC", &[&project_id]).map_err(err),
    }
}

/// Validate a mission scope binding against the missions fold: a scope
/// event may only name a mission that exists (the fold is the read model,
/// AD-8). None is General — always valid.
fn validate_mission_scope(conn: &Connection, mission_id: &Option<String>) -> Result<Option<Uuid>, String> {
    let Some(raw) = mission_id else { return Ok(None) };
    let id: Uuid = raw
        .trim()
        .parse()
        .map_err(|e| format!("invalid mission id `{raw}`: {e}"))?;
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let missions = MissionsProjection::fold(&events).map_err(err)?;
    if !missions.iter().any(|m| m.id == id) {
        return Err(format!(
            "unknown mission `{raw}` — a conversation can only scope to a mission that exists"
        ));
    }
    Ok(Some(id))
}

/// Create a conversation, optionally scoped to a mission (FR-16.4),
/// running a skill (FR-16.8), and/or on a chosen model (FR-17.4). The
/// initial bindings are evented (chat.scoped / chat.skill_set /
/// chat.model_set, actor=user) with the row's columns as their projection.
#[tauri::command]
pub async fn create_chat(
    db: State<'_, Db>,
    project_id: String,
    kind: String,
    title: String,
    mission_id: Option<String>,
    skill: Option<String>,
    model: Option<String>,
) -> Result<Value, String> {
    let c = db.0.lock().await;
    create_chat_inner(&c, &project_id, &kind, &title, mission_id, skill, model)
}

pub fn create_chat_inner(
    c: &Connection,
    project_id: &str,
    kind: &str,
    title: &str,
    mission_id: Option<String>,
    skill: Option<String>,
    model: Option<String>,
) -> Result<Value, String> {
    let mission = validate_mission_scope(c, &mission_id)?;
    let id = uid();
    let skill = skill.unwrap_or_default();
    if !skill.trim().is_empty() && skill_by_name(c, &skill)?.is_none() {
        return Err(format!(
            "unknown skill `{skill}` — a conversation can only run a registered skill"
        ));
    }
    let skill = skill.trim().to_string();
    let model = model.unwrap_or_default().trim().to_string();
    c.execute(
        "INSERT INTO chats(id,project_id,kind,title,preview,mission_id,skill,model,created_at,updated_at) \
         VALUES(?1,?2,?3,?4,'',?5,?6,?7,?8,?9)",
        params![
            id,
            project_id,
            kind,
            title,
            mission.map(|m| m.to_string()),
            if skill.is_empty() { None } else { Some(skill.clone()) },
            if model.is_empty() { None } else { Some(model.clone()) },
            now(),
            now()
        ],
    )
    .map_err(err)?;
    // The initial bindings are events like every later movement (FR-16.4).
    let store = EventStore::new(c);
    if let Some(m) = mission {
        store
            .append(NewEvent::chat_scoped(&id, Some(m)).map_err(err)?)
            .map_err(err)?;
    }
    if !skill.is_empty() {
        store
            .append(NewEvent::chat_skill_set(&id, &skill).map_err(err)?)
            .map_err(err)?;
    }
    if !model.is_empty() {
        store
            .append(NewEvent::chat_model_set(&id, &model).map_err(err)?)
            .map_err(err)?;
    }
    db::query_one(c, "SELECT * FROM chats WHERE id=?1", &[&id])
        .map(|o| o.unwrap_or(Value::Null))
        .map_err(err)
}

/// The chat row plus its folded attachments (the chips the composer
/// renders).
#[tauri::command]
pub async fn get_chat(db: State<'_, Db>, id: String) -> Result<Value, String> {
    let c = db.0.lock().await;
    get_chat_inner(&c, &id)
}

fn get_chat_inner(c: &Connection, id: &str) -> Result<Value, String> {
    let chat = db::query_one(c, "SELECT * FROM chats WHERE id=?1", &[&id]).map_err(err)?;
    let messages =
        db::query_all(c, "SELECT * FROM messages WHERE chat_id=?1 ORDER BY created_at", &[&id])
            .unwrap_or_default();
    let events = EventStore::new(c).events_all().map_err(err)?;
    let attachments: Vec<ChatAttachment> = folded_attachments(&events, id);
    let mut out = chat.unwrap_or(Value::Null);
    if let Value::Object(ref mut m) = out {
        m.insert("messages".into(), Value::Array(messages));
        m.insert(
            "attachments".into(),
            serde_json::to_value(attachments).map_err(err)?,
        );
    }
    Ok(out)
}

#[tauri::command]
pub async fn delete_chat(db: State<'_, Db>, id: String) -> Result<(), String> {
    let c = db.0.lock().await;
    c.execute("DELETE FROM chats WHERE id=?1", params![id]).map_err(err)?;
    Ok(())
}

/// Re-scope a conversation (FR-16.4): one `chat.scoped` event (actor=user,
/// cause-linked to the mission when scoped) + the row's projection. The
/// message history is preserved untouched — a scope change is a binding,
/// not a rewrite.
#[tauri::command]
pub async fn set_chat_scope(
    db: State<'_, Db>,
    chat_id: String,
    mission_id: Option<String>,
) -> Result<Value, String> {
    let c = db.0.lock().await;
    set_chat_scope_inner(&c, &chat_id, mission_id)
}

pub fn set_chat_scope_inner(
    c: &Connection,
    chat_id: &str,
    mission_id: Option<String>,
) -> Result<Value, String> {
    let mission = validate_mission_scope(c, &mission_id)?;
    let n = c
        .execute(
            "UPDATE chats SET mission_id=?2, updated_at=?3 WHERE id=?1",
            params![chat_id, mission.map(|m| m.to_string()), now()],
        )
        .map_err(err)?;
    if n == 0 {
        return Err(format!("chat `{chat_id}` not found"));
    }
    EventStore::new(c)
        .append(NewEvent::chat_scoped(chat_id, mission).map_err(err)?)
        .map_err(err)?;
    db::query_one(c, "SELECT * FROM chats WHERE id=?1", &[&chat_id])
        .map(|o| o.unwrap_or(Value::Null))
        .map_err(err)
}

/// Choose (or clear) a conversation's skill (FR-16.8): one
/// `chat.skill_set` event + the row's projection. An empty skill returns
/// the conversation to the plain assistant persona.
#[tauri::command]
pub async fn set_chat_skill(
    db: State<'_, Db>,
    chat_id: String,
    skill: Option<String>,
) -> Result<Value, String> {
    let skill = skill.unwrap_or_default();
    let c = db.0.lock().await;
    set_chat_skill_inner(&c, &chat_id, &skill)
}

pub fn set_chat_skill_inner(
    c: &Connection,
    chat_id: &str,
    skill: &str,
) -> Result<Value, String> {
    if !skill.trim().is_empty() && skill_by_name(c, skill)?.is_none() {
        return Err(format!(
            "unknown skill `{skill}` — a conversation can only run a registered skill"
        ));
    }
    let skill = skill.trim().to_string();
    let n = c
        .execute(
            "UPDATE chats SET skill=?2, updated_at=?3 WHERE id=?1",
            params![chat_id, if skill.is_empty() { None } else { Some(skill.clone()) }, now()],
        )
        .map_err(err)?;
    if n == 0 {
        return Err(format!("chat `{chat_id}` not found"));
    }
    EventStore::new(c)
        .append(NewEvent::chat_skill_set(chat_id, &skill).map_err(err)?)
        .map_err(err)?;
    db::query_one(c, "SELECT * FROM chats WHERE id=?1", &[&chat_id])
        .map(|o| o.unwrap_or(Value::Null))
        .map_err(err)
}

/// Choose (or clear) a conversation's model (FR-17.4, Story 5.9): one
/// `chat.model_set` event + the row's projection. An empty model returns
/// the conversation to the provider's configured default. Changing the
/// model mid-conversation is history-preserving — earlier messages keep
/// the attribution they were produced with.
#[tauri::command]
pub async fn set_chat_model(
    db: State<'_, Db>,
    chat_id: String,
    model: Option<String>,
) -> Result<Value, String> {
    let c = db.0.lock().await;
    set_chat_model_inner(&c, &chat_id, model)
}

pub fn set_chat_model_inner(
    c: &Connection,
    chat_id: &str,
    model: Option<String>,
) -> Result<Value, String> {
    let model = model.unwrap_or_default().trim().to_string();
    let n = c
        .execute(
            "UPDATE chats SET model=?2, updated_at=?3 WHERE id=?1",
            params![chat_id, if model.is_empty() { None } else { Some(model.clone()) }, now()],
        )
        .map_err(err)?;
    if n == 0 {
        return Err(format!("chat `{chat_id}` not found"));
    }
    EventStore::new(c)
        .append(NewEvent::chat_model_set(chat_id, &model).map_err(err)?)
        .map_err(err)?;
    db::query_one(c, "SELECT * FROM chats WHERE id=?1", &[&chat_id])
        .map(|o| o.unwrap_or(Value::Null))
        .map_err(err)
}

// ---------- board context (FR-16.5, pure over the folds) ----------

/// Assemble a mission's board context from the existing read models —
/// hypotheses (statements + lifecycle statuses) with their evidence pins —
/// exactly the board the UI renders (listHypotheses + listEvidence).
pub fn board_context_for(events: &[crate::eventstore::StoredEvent], mission: &Mission) -> BoardContext {
    let hypotheses = HypothesesProjection::fold_for(events, mission.id).unwrap_or_default();
    let mut board = Vec::new();
    for h in &hypotheses {
        let claims = EvidenceProjection::fold_for(events, h.id).unwrap_or_default();
        let pins: Vec<String> = claims
            .iter()
            .filter_map(|c| {
                let pin = c.pin.as_ref()?;
                Some(match pin.kind {
                    PinKind::Citation => {
                        let source = pin
                            .ref_label
                            .clone()
                            .or_else(|| pin.ref_id.clone())
                            .unwrap_or_else(|| "sin fuente".into());
                        let removed = if pin.ref_removed { " [source removed]" } else { "" };
                        format!("citation — {source} (confianza {:.1}){removed}", pin.confidence)
                    }
                    PinKind::Numerical => format!(
                        "numerical — {} (confianza {:.1})",
                        pin.artifact_ref.clone().unwrap_or_default(),
                        pin.confidence
                    ),
                })
            })
            .collect();
        board.push(BoardHypothesis {
            label: format!("H-{}", h.seq),
            statement: h.statement.clone(),
            status: h.status.to_string(),
            pins,
        });
    }
    BoardContext {
        mission_label: format!("M-{}", mission.seq),
        question: mission.question.clone(),
        hypotheses: board,
    }
}

// ---------- attachments (Story 5.5) ----------

/// The digest-addressed store: attachment bytes live under
/// `<data_dir>/attachments/<sha-256>`; an includable attachment's extracted
/// text rides beside it as `<digest>.txt`. Content never enters the event
/// log (NFR-12: it leaves the machine only inside the provider call).
fn store_path(paths: &AppPaths, digest: &str, suffix: &str) -> std::path::PathBuf {
    paths.data_dir.join("attachments").join(format!("{digest}{suffix}"))
}

/// Cap extracted text at MAX_INLINE_CHARS on a char boundary, reporting
/// whether the cap bit (the visible truncation flag, FR-16.3).
fn cap_text(text: String) -> (String, bool) {
    if text.chars().count() <= MAX_INLINE_CHARS {
        return (text, false);
    }
    let capped: String = text.chars().take(MAX_INLINE_CHARS).collect();
    (capped, true)
}

/// Read + classify + store one picked file, and append its
/// `chat.attachment_added` event. Refusals (unreadable, oversized, text
/// that is not UTF-8) are returned, never swallowed.
#[allow(clippy::too_many_arguments)]
fn attach_one(
    c: &Connection,
    paths: &AppPaths,
    chat_id: &str,
    pick: &AttachmentPick,
) -> Result<ChatAttachment, String> {
    let bytes = std::fs::read(&pick.path)
        .map_err(|e| format!("`{}`: no se pudo leer ({e}) / could not read", pick.name))?;
    if bytes.len() as u64 > MAX_STORED_BYTES {
        return Err(format!(
            "`{}`: {} bytes exceeds the {} byte attachment cap / excede el límite",
            pick.name,
            bytes.len(),
            MAX_STORED_BYTES
        ));
    }
    let digest = hex::encode(Sha256::digest(&bytes));
    // The stored copy (deduplicated by digest — the attachment ref).
    std::fs::create_dir_all(paths.data_dir.join("attachments")).map_err(err)?;
    let raw = store_path(paths, &digest, "");
    if !raw.exists() {
        std::fs::write(&raw, &bytes).map_err(err)?;
    }
    let kind = chat::classify_attachment(&pick.name, &bytes);
    let (text, note) = match kind {
        AttachmentKind::Text => match String::from_utf8(bytes.clone()) {
            Ok(text) => (Some(text), String::new()),
            Err(_) => {
                return Err(format!(
                    "`{}`: no es UTF-8 válido — archivo de texto ilegible / unreadable text file",
                    pick.name
                ));
            }
        },
        AttachmentKind::Pdf => match pdf_extract::extract_text_from_mem(&bytes) {
            Ok(text) => (Some(text), String::new()),
            // Stored by ref, flagged honestly — never silently dropped.
            Err(_) => (None, "pdf-extraction-failed".into()),
        },
        AttachmentKind::Binary => (None, "unsupported-inline v1".into()),
    };
    let (text, truncated) = match text {
        Some(t) => {
            let (capped, truncated) = cap_text(t);
            (Some(capped), truncated)
        }
        None => (None, false),
    };
    let included = text.is_some();
    if let Some(t) = &text {
        std::fs::write(store_path(paths, &digest, ".txt"), t).map_err(err)?;
    }
    let attachment_id = Uuid::new_v4();
    EventStore::new(c)
        .append(
            NewEvent::chat_attachment_added(AttachmentAddedPayload {
                chat_id: chat_id.to_string(),
                attachment_id,
                name: pick.name.clone(),
                path: pick.path.clone(),
                kind: kind.as_str().to_string(),
                digest: digest.clone(),
                size_bytes: bytes.len() as u64,
                included,
                truncated,
                note: note.clone(),
            })
            .map_err(err)?,
        )
        .map_err(err)?;
    Ok(ChatAttachment {
        id: attachment_id,
        name: pick.name.clone(),
        kind: kind.as_str().to_string(),
        digest,
        size_bytes: bytes.len() as u64,
        included,
        truncated,
        note,
    })
}

/// The multi-file picker the composer's attach button opens (desktop).
#[tauri::command]
pub async fn pick_attachment_files(app: tauri::AppHandle) -> Result<Vec<AttachmentPick>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app
        .dialog()
        .file()
        .set_title("Adjuntar archivos / Attach files")
        .blocking_pick_files();
    Ok(picked
        .unwrap_or_default()
        .into_iter()
        .filter_map(|f| {
            let path = f.as_path()?.to_path_buf();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            Some(AttachmentPick {
                name,
                path: path.display().to_string(),
            })
        })
        .collect())
}

/// Attach picked files to a conversation (FR-16.1): each is read locally,
/// classified (text/pdf/binary), stored by its digest-addressed ref, and
/// evented. Per-file refusals ride the result — a bad file never blocks
/// the good ones, and nothing is silently dropped.
#[tauri::command]
pub async fn add_chat_attachments(
    db: State<'_, Db>,
    paths: State<'_, AppPaths>,
    chat_id: String,
    files: Vec<AttachmentPick>,
) -> Result<Value, String> {
    let c = db.0.lock().await;
    let exists: i64 = c
        .query_row("SELECT COUNT(*) FROM chats WHERE id=?1", params![&chat_id], |r| r.get(0))
        .map_err(err)?;
    if exists == 0 {
        return Err(format!("chat `{chat_id}` not found"));
    }
    let mut attached: Vec<ChatAttachment> = Vec::new();
    let mut refused: Vec<Value> = Vec::new();
    for pick in &files {
        match attach_one(&c, paths.inner(), &chat_id, pick) {
            Ok(a) => attached.push(a),
            Err(reason) => refused.push(json!({ "name": pick.name, "reason": reason })),
        }
    }
    Ok(json!({ "attached": attached, "refused": refused }))
}

/// Remove an attachment from a conversation (FR-16.1's removable chips):
/// one `chat.attachment_removed` event; the stored copy stays
/// (digest-addressed, deduplicated).
#[tauri::command]
pub async fn remove_chat_attachment(
    db: State<'_, Db>,
    chat_id: String,
    attachment_id: String,
) -> Result<Vec<ChatAttachment>, String> {
    let attachment_id: Uuid = attachment_id
        .parse()
        .map_err(|e| format!("invalid attachment id `{attachment_id}`: {e}"))?;
    let c = db.0.lock().await;
    EventStore::new(&c)
        .append(NewEvent::chat_attachment_removed(&chat_id, attachment_id).map_err(err)?)
        .map_err(err)?;
    let events = EventStore::new(&c).events_all().map_err(err)?;
    Ok(folded_attachments(&events, &chat_id))
}

// ---------- the send path (Stories 5.4–5.6 wired together) ----------

/// A chat's effective scope/skill: the folded events win over the row's
/// baseline columns (legacy chats have no events; the row is their only
/// record).
fn effective_scope(
    events: &[crate::eventstore::StoredEvent],
    row_mission: Option<&str>,
    chat_id: &str,
) -> Option<Uuid> {
    folded_scope(events, chat_id)
        .flatten()
        .or_else(|| row_mission.and_then(|m| m.trim().parse().ok()))
}

fn effective_skill(
    events: &[crate::eventstore::StoredEvent],
    row_skill: Option<&str>,
    chat_id: &str,
) -> Option<String> {
    folded_skill(events, chat_id)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            row_skill
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
}

/// Send a message: the user message lands with the conversation's CURRENT
/// mission scope (message events carry mission_id), then the assistant
/// call carries the board context, the skill, and the attachments into the
/// provider call. The review-kind branch is unchanged.
#[tauri::command]
pub async fn send_message(
    db: State<'_, Db>,
    paths: State<'_, AppPaths>,
    chat_id: String,
    content: String,
) -> Result<Value, String> {
    send_message_inner(db.inner(), paths.inner(), &chat_id, &content).await
}

pub async fn send_message_inner(
    db: &Db,
    paths: &AppPaths,
    chat_id: &str,
    content: &str,
) -> Result<Value, String> {
    let c = db.0.lock().await;
    let chat = db::query_one(&c, "SELECT project_id, kind, mission_id, skill, model FROM chats WHERE id=?1", &[&chat_id])
        .map_err(err)?
        .ok_or_else(|| format!("chat `{chat_id}` not found"))?;
    let project_id = chat.get("project_id").and_then(|v| v.as_str()).ok_or("no project")?.to_string();
    let kind = chat.get("kind").and_then(|v| v.as_str()).unwrap_or("asistente").to_string();
    let events = EventStore::new(&c).events_all().map_err(err)?;
    let mission_id = effective_scope(
        &events,
        chat.get("mission_id").and_then(|v| v.as_str()),
        chat_id,
    );
    let skill_name = effective_skill(&events, chat.get("skill").and_then(|v| v.as_str()), chat_id);
    let skill = match &skill_name {
        Some(name) => skill_by_name(&c, name)?,
        None => None,
    };
    // Story 5.7 (FR-17.1/NFR-11): the ASSISTANT path resolves its real
    // provider FIRST — an unconfigured workspace refuses the send with the
    // typed `no_provider_configured:` error BEFORE anything is stored (no
    // orphan user message, no fabricated reply). The review-kind branch
    // below keeps the simulated guarantee of Stories 1.6/2.1.
    let layer = if kind == "review" {
        None
    } else {
        Some(agent::resolve_assistant_layer(db, &c, skill.as_ref())?)
    };
    // Story 5.9 (FR-17.4): the conversation's chosen model — the folded
    // `chat.model_set` events win over the row's baseline column.
    let model = crate::domain::chat::folded_model(&events, chat_id)
        .or_else(|| {
            chat.get("model")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .map(str::to_string)
        });
    // store user message — carrying the conversation's current scope
    let user_id = uid();
    c.execute(
        "INSERT INTO messages(id,chat_id,role,content,mission_id,created_at) VALUES(?1,?2,'user',?3,?4,?5)",
        params![user_id, chat_id, content, mission_id.map(|m| m.to_string()), now()],
    )
    .map_err(err)?;
    c.execute(
        "UPDATE chats SET preview=?2, updated_at=?3 WHERE id=?1",
        params![chat_id, content, now()],
    )
    .map_err(err)?;

    if kind == "review" {
        // run a review and return an agent message summarizing it
        drop(c);
        let result = agent::run_review(db, &project_id, content).await?;
        let agent_text = format!(
            "Revisión #{} completa. Puntaje **{}/10**. Guardé la revisión y creé actions para los hallazgos de alta/media severidad.",
            result.get("number").and_then(|v| v.as_i64()).unwrap_or(0),
            result.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0)
        );
        let c = db.0.lock().await;
        let mid = uid();
        c.execute(
            "INSERT INTO messages(id,chat_id,role,content,classify_tag,meta,mission_id,created_at) VALUES(?1,?2,'agent',?3,'review',?4,?5,?6)",
            params![mid, chat_id, &agent_text, result.to_string(), mission_id.map(|m| m.to_string()), now()],
        )
        .map_err(err)?;
        c.execute("UPDATE chats SET updated_at=?2 WHERE id=?1", params![chat_id, now()]).map_err(err)?;
        return Ok(json!({ "user_id": user_id, "agent_id": mid, "agent_content": agent_text, "review": result }));
    }

    // asistente: assemble the scoped call (mission board + skill +
    // attachments + the chosen model) and reply through the provider layer.
    let layer = layer.expect("assistant path resolves its layer above");
    let missions = MissionsProjection::fold(&events).map_err(err)?;
    let board = mission_id
        .and_then(|id| missions.into_iter().find(|m| m.id == id))
        .map(|m| board_context_for(&events, &m));
    let attachments: Vec<AttachmentContext> = folded_attachments(&events, chat_id)
        .into_iter()
        .map(|a| {
            let text = if a.included {
                std::fs::read_to_string(store_path(paths, &a.digest, ".txt")).ok()
            } else {
                None
            };
            AttachmentContext {
                name: a.name,
                kind: match a.kind.as_str() {
                    "text" => AttachmentKind::Text,
                    "pdf" => AttachmentKind::Pdf,
                    _ => AttachmentKind::Binary,
                },
                text,
                truncated: a.truncated,
            }
        })
        .collect();
    let msgs_rows = db::query_all(&c, "SELECT role, content FROM messages WHERE chat_id=?1 ORDER BY created_at", &[&chat_id]).unwrap_or_default();
    drop(c);
    let history: Vec<agent::ChatMsg> = msgs_rows
        .into_iter()
        .filter_map(|r| {
            let role = r.get("role")?.as_str()?.to_string();
            let content = r.get("content")?.as_str()?.to_string();
            Some(agent::ChatMsg { role, content })
        })
        .collect();
    let call = AssistantCall {
        board,
        skill,
        attachments,
        model,
    };
    let reply = agent::assistant_reply(db, &layer, &project_id, history, call).await?;
    let c = db.0.lock().await;
    let mid = uid();
    // the reply carries its attribution (provider + model, Story 5.7/5.9) in
    // its meta — the message-render idiom shows it ("provider · model")
    let meta = json!({ "provider": reply.provider, "model": reply.model }).to_string();
    c.execute(
        "INSERT INTO messages(id,chat_id,role,content,classify_tag,meta,mission_id,created_at) VALUES(?1,?2,'agent',?3,?4,?5,?6,?7)",
        params![mid, chat_id, &reply.content, reply.tag, meta, mission_id.map(|m| m.to_string()), now()],
    )
    .map_err(err)?;
    c.execute("UPDATE chats SET updated_at=?2 WHERE id=?1", params![chat_id, now()]).map_err(err)?;
    Ok(json!({
        "user_id": user_id,
        "agent_id": mid,
        "agent_content": reply.content,
        "tag": reply.tag,
        "provider": reply.provider,
        "model": reply.model,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::hypotheses::HypothesesProjection;
    use rusqlite::Connection;

    /// An in-memory workspace: schema + eventstore + seeds (the demo project
    /// and the six default skills exist), wrapped in the Db handle.
    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::Db::migrate(&conn).unwrap();
        crate::db::Db::seed(&conn).unwrap();
        EventStore::init(&conn).unwrap();
        Db(std::sync::Arc::new(tokio::sync::Mutex::new(conn)))
    }

    fn test_paths() -> AppPaths {
        let mut dir = std::env::temp_dir();
        dir.push(format!("rc-chat-attach-test-{}", Uuid::new_v4()));
        AppPaths { data_dir: dir.clone(), log_dir: dir.join("logs") }
    }

    async fn conn(db: &Db) -> tokio::sync::MutexGuard<'_, Connection> {
        db.0.lock().await
    }

    /// A mission created through the typed constructor, returned with its
    /// folded read model.
    async fn make_mission(db: &Db) -> Mission {
        let c = conn(db).await;
        let stored = EventStore::new(&c)
            .append(
                NewEvent::mission_created(crate::domain::missions::MissionCreatedPayload {
                    question: "¿Reduce el RAG las citas fabricadas?".into(),
                    stop_condition: "Stop after $5.".into(),
                    success_criterion: "Un evaluador encuentra cero citas fabricadas.".into(),
                    autonomy: crate::domain::missions::Autonomy::Suggest,
                    spend_ceiling_cents: 500,
                    roles: vec![],
                    schedule: String::new(),
                })
                .unwrap(),
            )
            .unwrap();
        MissionsProjection::fold(&[stored]).unwrap().remove(0)
    }

    async fn project_id(db: &Db) -> String {
        let c = conn(db).await;
        db::query_one(&c, "SELECT id FROM projects LIMIT 1", ())
            .unwrap()
            .unwrap()
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn a_scoped_send_carries_mission_context_and_mission_id() {
        agent::use_fake_assistant_layer();
        let db = test_db();
        let paths = test_paths();
        let mission = make_mission(&db).await;
        let pid = project_id(&db).await;

        // a hypothesis + a pinned numerical claim on the mission's board
        {
            let c = conn(&db).await;
            let store = EventStore::new(&c);
            let hyp = store
                .append(
                    NewEvent::hypothesis_created(
                        "El RAG reduce las citas fabricadas en revisión ciega.",
                        mission.id,
                    )
                    .unwrap(),
                )
                .unwrap();
            let _hyp_id: Uuid = hyp.id;
            let claim = store
                .append(NewEvent::claim_registered("0.4% citas fabricadas vs 2.1% baseline", hyp.id, None).unwrap())
                .unwrap();
            store
                .append(
                    NewEvent::evidence_pinned_numerical(
                        claim.id,
                        hyp.id,
                        "runs/rag-citations.csv",
                        "0.4% vs 2.1%",
                        0.82,
                        "GLM-5.3",
                    )
                    .unwrap(),
                )
                .unwrap();
        }

        // create a mission-scoped chat (the initial binding is evented)
        let chat = {
            let c = conn(&db).await;
            create_chat_inner(&c, &pid, "asistente", "Scoped", Some(mission.id.to_string()), None, None).unwrap()
        };
        assert_eq!(chat["mission_id"], json!(mission.id.to_string()));

        // re-scope is explicit and history-preserving: scope to General and
        // back, then send — the LATEST binding (the mission) applies
        {
            let c = conn(&db).await;
            set_chat_scope_inner(&c, &chat_id(&chat), None).unwrap();
            set_chat_scope_inner(&c, &chat_id(&chat), Some(mission.id.to_string())).unwrap();
        }

        let out = send_message_inner(&db, &paths, &chat_id(&chat), "Resume el estado del tablero")
            .await
            .unwrap();

        // the user message landed with the mission scope on it
        let c = conn(&db).await;
        let fresh = get_chat_inner(&c, &chat_id(&chat)).unwrap();
        let messages = fresh["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["mission_id"], json!(mission.id.to_string()));
        assert_eq!(messages[1]["mission_id"], json!(mission.id.to_string()));

        // the board context rode the provider call — the simulated provider
        // echoes the marker it parsed from the prompt
        let reply = out["agent_content"].as_str().unwrap();
        assert!(
            reply.contains(&format!("tablero M-{}", mission.seq)),
            "the simulated reply must echo the board-context marker: {reply}"
        );
    }

    #[tokio::test]
    async fn a_general_send_carries_no_board_context() {
        agent::use_fake_assistant_layer();
        let db = test_db();
        let paths = test_paths();
        let mission = make_mission(&db).await; // exists, but the chat is General
        let pid = project_id(&db).await;
        assert_ne!(mission.id, Uuid::nil()); // the mission is simply not bound
        let chat = {
            let c = conn(&db).await;
            create_chat_inner(&c, &pid, "asistente", "General", None, None, None).unwrap()
        };
        let out = send_message_inner(&db, &paths, &chat_id(&chat), "hola")
            .await
            .unwrap();
        let reply = out["agent_content"].as_str().unwrap();
        assert!(!reply.contains("tablero M-"), "a General send carries no board context: {reply}");
        let c = conn(&db).await;
        let fresh = get_chat_inner(&c, &chat_id(&chat)).unwrap();
        assert_eq!(fresh["messages"][0]["mission_id"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn unknown_mission_and_skill_scopes_are_refused() {
        let db = test_db();
        let pid = project_id(&db).await;
        let c = conn(&db).await;
        assert!(create_chat_inner(&c, &pid, "asistente", "x", Some(Uuid::new_v4().to_string()), None, None).is_err());
        assert!(create_chat_inner(&c, &pid, "asistente", "x", None, Some("martian".into()), None).is_err());
        let chat = create_chat_inner(&c, &pid, "asistente", "x", None, None, None).unwrap();
        assert!(set_chat_scope_inner(&c, &chat_id(&chat), Some(Uuid::new_v4().to_string())).is_err());
        assert!(set_chat_skill_inner(&c, &chat_id(&chat), "martian").is_err());
    }

    /// Story 5.7 (FR-17.1/NFR-11): with NO real provider configured, an
    /// assistant send is refused with the typed `no_provider_configured:`
    /// error — never a simulated fallback, never a fabricated reply, and
    /// nothing stored (no orphan user message).
    #[tokio::test]
    async fn assistant_sends_refuse_without_a_real_provider() {
        let db = test_db();
        let paths = test_paths();
        let pid = project_id(&db).await;
        let chat = {
            let c = conn(&db).await;
            create_chat_inner(&c, &pid, "asistente", "Unconfigured", None, None, None).unwrap()
        };
        let out = send_message_inner(&db, &paths, &chat_id(&chat), "hola").await;
        let err = out.expect_err("the unconfigured assistant must refuse the send");
        assert!(
            err.starts_with("no_provider_configured:"),
            "the refusal is the typed no_provider_configured error, got: {err}"
        );
        // nothing was stored — the refusal came before the insert
        // (scoped: the guard must drop before the next acquisition)
        {
            let c = conn(&db).await;
            let fresh = get_chat_inner(&c, &chat_id(&chat)).unwrap();
            assert!(
                fresh["messages"].as_array().unwrap().is_empty(),
                "a refused send must not leave a stored message: {fresh}"
            );
        }
        // an explicit simulate mode is refused the same way (never a silent
        // mock on the assistant path)
        {
            let c = conn(&db).await;
            db::set_setting(&c, "llm_mode", "simulate").unwrap();
        }
        let err = send_message_inner(&db, &paths, &chat_id(&chat), "hola")
            .await
            .expect_err("simulate mode must not answer the assistant");
        assert!(err.starts_with("no_provider_configured:"), "got: {err}");
    }

    /// Story 5.9 (FR-17.4): the per-conversation model choice persists
    /// (row + `chat.model_set` event), the folded event wins over the row,
    /// and the chosen model rides the provider call — overriding the
    /// layer's configured default — and attributes the reply.
    #[tokio::test]
    async fn the_model_choice_persists_per_chat_and_overrides_the_default() {
        agent::use_fake_assistant_layer();
        let db = test_db();
        let paths = test_paths();
        let pid = project_id(&db).await;
        let c = conn(&db).await;
        // the initial choice is evented like every later movement
        let chat =
            create_chat_inner(&c, &pid, "asistente", "Modeloso", None, None, Some("glm-5.3-air".into()))
                .unwrap();
        assert_eq!(chat["model"], json!("glm-5.3-air"));
        let cid = chat_id(&chat);
        // a later movement wins: the folded event overrides the row
        set_chat_model_inner(&c, &cid, Some("claude-sonnet-4-5".into())).unwrap();
        let re_read = get_chat_inner(&c, &cid).unwrap();
        assert_eq!(re_read["model"], json!("claude-sonnet-4-5"));
        // clearing returns to the provider default
        set_chat_model_inner(&c, &cid, None).unwrap();
        let re_read = get_chat_inner(&c, &cid).unwrap();
        assert_eq!(re_read["model"], serde_json::Value::Null);

        // the chosen model rides the call: set it again and send — the
        // reply's attribution carries it (not the fake layer's default)
        set_chat_model_inner(&c, &cid, Some("glm-5.3-air".into())).unwrap();
        drop(c);
        let out = send_message_inner(&db, &paths, &cid, "redacta algo corto")
            .await
            .unwrap();
        assert_eq!(out["model"], json!("glm-5.3-air"));
        assert_eq!(out["provider"], json!("test-remote"));
        // the reply's stored message carries its attribution in meta
        let c = conn(&db).await;
        let fresh = get_chat_inner(&c, &cid).unwrap();
        let agent_msg = fresh["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == json!("agent"))
            .unwrap();
        let meta: Value =
            serde_json::from_str(agent_msg["meta"].as_str().unwrap()).unwrap();
        assert_eq!(meta["model"], json!("glm-5.3-air"));
        assert_eq!(meta["provider"], json!("test-remote"));
    }

    #[tokio::test]
    async fn attachments_are_classified_stored_and_included_or_flagged() {
        agent::use_fake_assistant_layer();
        let db = test_db();
        let paths = test_paths();
        let pid = project_id(&db).await;
        let chat = {
            let c = conn(&db).await;
            create_chat_inner(&c, &pid, "asistente", "Files", None, None, None).unwrap()
        };
        let cid = chat_id(&chat);

        // a text file: included
        let text_file = paths.data_dir.join("notas.md");
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        std::fs::write(&text_file, "# Notas\npunto clave").unwrap();
        // a binary file: stored by ref, flagged
        let bin_file = paths.data_dir.join("figura.png");
        std::fs::write(&bin_file, [0x89u8, 0x50, 0x4E, 0x47]).unwrap();
        // a fake .pdf (magic but unparseable): stored, extraction failed
        let pdf_file = paths.data_dir.join("paper.pdf");
        std::fs::write(&pdf_file, b"%PDF-1.7\nbroken").unwrap();

        let out = {
            let c = conn(&db).await;
            add_chat_attachments_inner(
                &c,
                &paths,
                &cid,
                &[
                    AttachmentPick { name: "notas.md".into(), path: text_file.display().to_string() },
                    AttachmentPick { name: "figura.png".into(), path: bin_file.display().to_string() },
                    AttachmentPick { name: "paper.pdf".into(), path: pdf_file.display().to_string() },
                ],
            )
            .unwrap()
        };
        assert_eq!(out["refused"].as_array().unwrap().len(), 0, "nothing silently refused: {}", out);
        let attached = out["attached"].as_array().unwrap();
        assert_eq!(attached.len(), 3);
        let by_name = |n: &str| attached.iter().find(|a| a["name"] == json!(n)).unwrap();
        assert_eq!(by_name("notas.md")["kind"], json!("text"));
        assert_eq!(by_name("notas.md")["included"], json!(true));
        assert_eq!(by_name("figura.png")["kind"], json!("binary"));
        assert_eq!(by_name("figura.png")["included"], json!(false));
        assert_eq!(by_name("figura.png")["note"], json!("unsupported-inline v1"));
        assert_eq!(by_name("paper.pdf")["kind"], json!("pdf"));
        assert_eq!(by_name("paper.pdf")["included"], json!(false));

        // the chips render from the fold; removal un-events one
        let c = conn(&db).await;
        let fresh = get_chat_inner(&c, &cid).unwrap();
        assert_eq!(fresh["attachments"].as_array().unwrap().len(), 3);

        // the send carries the text file's content into the provider call —
        // the simulated provider echoes the attachment count
        drop(c);
        let out = send_message_inner(&db, &paths, &cid, "usa las notas").await.unwrap();
        let reply = out["agent_content"].as_str().unwrap();
        assert!(reply.contains("adjuntos: 3"), "the simulated reply must echo attachments: {reply}");

        // an unreadable text file (invalid UTF-8) is refused loudly
        let bad = paths.data_dir.join("notas.md");
        std::fs::write(&bad, [0xFFu8, 0xFE, 0x00]).unwrap();
        let c = conn(&db).await;
        let out = add_chat_attachments_inner(
            &c,
            &paths,
            &cid,
            &[AttachmentPick { name: "notas.md".into(), path: bad.display().to_string() }],
        )
        .unwrap();
        assert_eq!(out["refused"].as_array().unwrap().len(), 1);
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[tokio::test]
    async fn the_default_skill_set_is_preinstalled_and_per_chat() {
        agent::use_fake_assistant_layer();
        let db = test_db();
        let pid = project_id(&db).await;
        let c = conn(&db).await;
        // FR-16.6: the six scientific skills ship pre-installed
        let skills = list_skills_inner(&c).unwrap();
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["drafter", "critic", "librarian", "verifier", "synthesizer", "note_taker"]
        );

        // the per-chat choice persists (row + event)
        let chat = create_chat_inner(&c, &pid, "asistente", "Skilled", None, Some("librarian".into()), None)
            .unwrap();
        assert_eq!(chat["skill"], json!("librarian"));
        let cid = chat_id(&chat);
        set_chat_skill_inner(&c, &cid, "verifier").unwrap();
        let re_read = get_chat_inner(&c, &cid).unwrap();
        assert_eq!(re_read["skill"], json!("verifier"));
        // clearing returns to the plain persona
        set_chat_skill_inner(&c, &cid, "").unwrap();
        let re_read = get_chat_inner(&c, &cid).unwrap();
        assert_eq!(re_read["skill"], serde_json::Value::Null);

        // a skilled send runs the skill's prompt and echoes the skill name
        drop(c);
        let paths = test_paths();
        let c = conn(&db).await;
        set_chat_skill_inner(&c, &cid, "librarian").unwrap();
        drop(c);
        let out = send_message_inner(&db, &paths, &cid, "busca literatura sobre scaling laws")
            .await
            .unwrap();
        let reply = out["agent_content"].as_str().unwrap();
        assert!(reply.contains("skill: librarian"), "the simulated reply must echo the skill: {reply}");
    }

    #[tokio::test]
    async fn the_system_prompt_merges_skill_board_and_attachments() {
        let call = AssistantCall {
            board: Some(BoardContext {
                mission_label: "M-9".into(),
                question: "q".into(),
                hypotheses: vec![BoardHypothesis {
                    label: "H-1".into(),
                    statement: "stmt".into(),
                    status: "supported".into(),
                    pins: vec!["numerical — a.csv (confianza 0.9)".into()],
                }],
            }),
            skill: Some(skills::default_skills().remove(2)), // librarian
            attachments: vec![AttachmentContext {
                name: "notas.md".into(),
                kind: AttachmentKind::Text,
                text: Some("contenido".into()),
                truncated: false,
            }],
            model: None,
        };
        let system = agent::assistant_system_prompt("Tesis", "- Ref A", &call);
        // the skill's role prompt and the marker'd name lead
        assert!(system.contains("Bibliotecario"));
        assert!(system.contains("Habilidad activa: «librarian»"));
        assert!(system.contains("Herramientas permitidas: search, read_library"));
        // the refs list keeps its marker contract (the mock parses it)
        assert!(system.contains("Referencias disponibles:\n- Ref A\nRedacta"));
        // board context + attachments ride after, between their markers
        assert!(system.contains("Contexto del tablero de la misión M-9"));
        assert!(system.contains("- H-1 [supported] stmt"));
        assert!(system.contains("· evidencia: numerical — a.csv (confianza 0.9)"));
        assert!(system.contains("Archivos adjuntos:\n- notas.md (text): incluido"));
        // a plain call keeps the exact original shape
        let plain = agent::assistant_system_prompt("Tesis", "- Ref A", &AssistantCall::default());
        assert!(plain.starts_with("Eres el Asistente de Research Core"));
        assert!(!plain.contains("Habilidad activa"));
        assert!(!plain.contains("Contexto del tablero"));
    }

    // the inner helpers the tests drive are the module-level create_chat_inner /
    // set_chat_scope_inner / set_chat_skill_inner (the #[tauri::command] layer
    // only unwraps State)
    fn chat_id(chat: &Value) -> String {
        chat.get("id").and_then(|v| v.as_str()).unwrap().to_string()
    }

    fn add_chat_attachments_inner(
        c: &Connection,
        paths: &AppPaths,
        chat_id: &str,
        files: &[AttachmentPick],
    ) -> Result<Value, String> {
        let mut attached: Vec<ChatAttachment> = Vec::new();
        let mut refused: Vec<Value> = Vec::new();
        for pick in files {
            match attach_one(c, paths, chat_id, pick) {
                Ok(a) => attached.push(a),
                Err(reason) => refused.push(json!({ "name": pick.name, "reason": reason })),
            }
        }
        Ok(json!({ "attached": attached, "refused": refused }))
    }

    /// The hypothesis fold is consulted by board_context_for — a sanity
    /// check that the board context derives from the read models the UI
    /// renders, not a parallel derivation.
    #[test]
    fn board_context_labels_derive_from_the_folds() {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        let store = EventStore::new(&conn);
        let mission = store
            .append(
                NewEvent::mission_created(crate::domain::missions::MissionCreatedPayload {
                    question: "q".into(),
                    stop_condition: "s".into(),
                    success_criterion: "c".into(),
                    autonomy: crate::domain::missions::Autonomy::Suggest,
                    spend_ceiling_cents: 500,
                    roles: vec![],
                    schedule: String::new(),
                })
                .unwrap(),
            )
            .unwrap();
        let hyp = store
            .append(NewEvent::hypothesis_created("stmt", mission.id).unwrap())
            .unwrap();
        let events = store.events_all().unwrap();
        let mission = MissionsProjection::fold(&events).unwrap().remove(0);
        let board = board_context_for(&events, &mission);
        assert_eq!(board.mission_label, format!("M-{}", mission.seq));
        assert_eq!(board.hypotheses.len(), 1);
        assert_eq!(board.hypotheses[0].label, format!("H-{}", hyp.seq));
        assert_eq!(board.hypotheses[0].status, "proposed");
        // the same fold the UI's listHypotheses renders
        assert_eq!(HypothesesProjection::fold_for(&events, mission.id).unwrap().len(), 1);
    }
}
