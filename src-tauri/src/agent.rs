// Agent flows: prompt building + persistence around the provider adapter
// layer (AD-9). Every LLM call — real BYOK providers, local CLIs, and the
// simulated fallback — goes through `adapters::providers::ProviderLayer`;
// this module makes no HTTP calls and spawns no LLM itself. Spend for real
// provider calls is appended by the layer (AD-10), not here.
use crate::adapters::providers::{self, simulated, ChatRequest, Message};
use crate::db::{self, Db};
use crate::domain::chat::{
    assemble_attachments_context, assemble_board_context, AttachmentContext, BoardContext,
    SKILL_MARKER, SKILL_END,
};
use crate::domain::missions::RoleConfig;
use crate::domain::skills::Skill;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The chat-message shape the UI speaks — the provider layer's `Message`.
pub type ChatMsg = Message;

/// The scoped context one assistant call carries (Stories 5.4–5.9): the
/// mission board context when the conversation is mission-scoped, the
/// per-conversation skill, the conversation's attachments, and the
/// per-conversation model choice (FR-17.4). Everything is optional — a
/// General, skill-less, attachment-less conversation on the configured
/// default model is the plain assistant of before.
#[derive(Debug, Clone, Default)]
pub struct AssistantCall {
    /// The mission's board context (FR-16.5) — label, question, hypotheses
    /// and pins; None for General chats (no board context at all).
    pub board: Option<BoardContext>,
    /// The per-conversation skill (FR-16.8); None = the plain persona.
    pub skill: Option<Skill>,
    /// The conversation's attachments (FR-16.2) with their extracted text.
    pub attachments: Vec<AttachmentContext>,
    /// The conversation's chosen model (FR-17.4, Story 5.9); None = the
    /// layer's configured model (or the skill's default). When set it
    /// overrides both — the picker wins.
    pub model: Option<String>,
}

/// One assistant reply with its provider attribution (Story 5.7/5.9): the
/// reply text, the classify tag, and the (provider, model) pair that
/// produced it — so messages can carry their attribution ("GLM-5.3 · …").
#[derive(Debug, Clone)]
pub struct AssistantReply {
    pub content: String,
    pub tag: Option<String>,
    pub provider: String,
    pub model: String,
}

/// The assistant system prompt (pure): the persona (the skill's role when
/// one is active, the plain assistant otherwise), the project, the
/// reference list between the simulated provider's markers, and — appended
/// after — the board-context and attachments blocks between their own
/// markers. Single source of truth: the markers are the contract the mock
/// reads back.
pub fn assistant_system_prompt(
    proj_name: &str,
    refs_list: &str,
    call: &AssistantCall,
) -> String {
    let (persona, skill_line) = match &call.skill {
        Some(skill) => {
            let name = skill.name.trim();
            let tools = if skill.tools.is_empty() {
                "ninguna".to_string()
            } else {
                skill.tools.join(", ")
            };
            (
                skill.system_prompt.trim().to_string(),
                format!(" {SKILL_MARKER}{name}{SKILL_END} Herramientas permitidas: {tools}."),
            )
        }
        None => (
            "Eres el Asistente de Research Core, un copiloto de escritura académica.".into(),
            String::new(),
        ),
    };
    let mut system = format!(
        "{persona} Proyecto activo: «{proj_name}».{skill_line} {}{}\nRedacta en español, tono académico sobrio. Sé conciso y útil. \
        Cuando propongas texto para el manuscrito, inclúyelo en un bloque citado.",
        simulated::REFS_LIST_MARKER,
        refs_list,
    );
    let board = assemble_board_context(call.board.as_ref());
    if !board.is_empty() {
        system.push_str("\n\n");
        system.push_str(&board);
    }
    let attachments = assemble_attachments_context(&call.attachments);
    if !attachments.is_empty() {
        system.push_str("\n\n");
        system.push_str(&attachments);
    }
    system
}

/// Test seam (Stories 5.7–5.9): the chat send-path tests run the assistant
/// against a remote-SHAPED layer over the simulated echo client — no
/// network, but a REAL provider kind, so the real-only rule (NFR-11) holds
/// in tests exactly as in production. Thread-local: each test thread opts
/// in for itself, so the refusal tests (no flag) stay unconfigured even
/// while send tests run in parallel.
#[cfg(test)]
thread_local! {
    static FAKE_ASSISTANT_LAYER: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn use_fake_assistant_layer() {
    FAKE_ASSISTANT_LAYER.with(|f| f.set(true));
}

#[cfg(test)]
fn fake_assistant_layer(db: &Db) -> providers::ProviderLayer {
    providers::test_remote_simulated(db)
}

/// Resolve the layer an assistant call runs on (AD-9, Stories 5.6/5.7): a
/// skill with its own (provider, model) pair resolves through the SAME
/// `for_role` path the mission roles use; a skill with an empty provider —
/// the default set's shape — runs on the configured layer. The ASSISTANT
/// rule (FR-17.1/NFR-11): this path can never resolve to the simulated
/// provider — no matter how it got there (auto mode, simulate mode, or a
/// skill named "simulated"), the typed `no_provider_configured:` refusal
/// comes back instead.
pub fn resolve_assistant_layer(
    db: &Db,
    conn: &rusqlite::Connection,
    skill: Option<&Skill>,
) -> Result<providers::ProviderLayer, String> {
    #[cfg(test)]
    if FAKE_ASSISTANT_LAYER.with(std::cell::Cell::get) {
        return Ok(fake_assistant_layer(db));
    }
    let layer = match skill {
        Some(s) if !s.provider.trim().is_empty() => {
            providers::ProviderLayer::for_role(
                db,
                conn,
                &RoleConfig {
                    name: s.name.clone(),
                    provider: s.provider.clone(),
                    model: s.model.clone(),
                },
            )
            .map_err(|e| e.to_string())?
        }
        _ => providers::ProviderLayer::resolve_real(db, conn).map_err(|e| e.to_string())?,
    };
    if layer.kind() == providers::Kind::Simulated {
        return Err(providers::ProviderError::NoProviderConfigured.to_string());
    }
    Ok(layer)
}

/// Reply in an assistant chat, on an ALREADY-RESOLVED layer (the send path
/// resolves first so an unconfigured provider refuses BEFORE any message is
/// stored). The conversation's chosen model (Story 5.9) overrides the
/// skill's and the layer's default. Returns the reply + its (provider,
/// model) attribution.
pub async fn assistant_reply(
    db: &Db,
    layer: &providers::ProviderLayer,
    project_id: &str,
    history: Vec<ChatMsg>,
    call: AssistantCall,
) -> Result<AssistantReply, String> {
    let conn = db.0.lock().await;
    let project = db::query_one(&conn, "SELECT name, folder FROM projects WHERE id=?1", &[&project_id]).ok().flatten();
    let refs = db::query_all(&conn,
        "SELECT title, authors, year, venue FROM refs WHERE project_id=?1 ORDER BY year DESC LIMIT 20",
        &[&project_id]).unwrap_or_default();
    drop(conn);

    let proj_name = project.as_ref().and_then(|p| p.get("name")).and_then(|v| v.as_str()).unwrap_or("");
    let ref_list: Vec<String> = refs.iter().filter_map(|r| {
        let t = r.get("title").and_then(|v| v.as_str())?;
        let a = r.get("authors").and_then(|v| v.as_str()).unwrap_or("");
        let y = r.get("year").and_then(|v| v.as_i64()).unwrap_or(0);
        Some(format!("- {} ({}, {}) — {}", t, a, y, r.get("venue").and_then(|v| v.as_str()).unwrap_or("")))
    }).collect();

    // The reference list travels in the system prompt between the simulated
    // provider's markers (single source of truth — the mock reads them back);
    // the board context and attachments ride in their own marker blocks.
    let system = assistant_system_prompt(proj_name, &ref_list.join("\n"), &call);

    let mut messages = vec![Message::system(system)];
    messages.extend(history);
    // The trust dispatch (Story 2.4, AD-10): user-initiated, so the dial does
    // not gate it — but the kill switch and the ceilings do, and a real call
    // still needs its runtime reservation. A skilled call tags its spend with
    // the skill's role name (per-role receipts, Story 2.1).
    let mut req = ChatRequest::new(messages).with_temperature(0.4);
    if let Some(model) = call.model.as_deref() {
        if !model.trim().is_empty() {
            // the picker wins (FR-17.4): the chat's model overrides the
            // skill's default and the layer's configured default
            req = req.with_model(model.trim().to_string());
        }
    }
    if let Some(skill) = &call.skill {
        req = req.with_role(skill.name.clone());
    }
    let attribution_model = req.model.clone();
    let resp = crate::trust::reserve_and_chat(
        db,
        layer,
        req,
        assistant_plan(layer, "assistant"),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(AssistantReply {
        content: resp.content,
        tag: None,
        provider: layer.name().to_string(),
        model: attribution_model,
    })
}

/// The call plan for a user-initiated assistant flow (Story 2.4): never
/// dial-gated (the user pressed the button), still kill-switched and
/// ceiling-checked, still reserved when it costs.
fn assistant_plan(layer: &providers::ProviderLayer, prefix: &str) -> crate::trust::CallPlan {
    crate::trust::CallPlan {
        run_id: format!("{prefix}-{}", uuid::Uuid::new_v4().simple()),
        target: layer.name().to_string(),
        model: layer.model().to_string(),
        mission_id: None,
        estimate_cents: crate::trust::estimate_cents(layer, layer.model()),
        autonomous: false,
    }
}

/// Run an AI review over the project. Creates a review record, findings, and
/// actions in the DB. Returns the new review id. One code path for every
/// mode: the prompt goes to the provider layer; the layer decides whether a
/// real provider, a local CLI, or the simulated fallback answers.
pub async fn run_review(
    db: &Db,
    project_id: &str,
    focus: &str,
) -> Result<Value, String> {
    let conn = db.0.lock().await;
    let layer = providers::ProviderLayer::resolve(db, &conn).map_err(|e| e.to_string())?;
    // enabled judges
    let judges = db::query_all(&conn,
        "SELECT key, name, description FROM agents WHERE kind='judge' AND enabled=1", ()).unwrap_or_default();
    let last_review = db::query_one(&conn,
        "SELECT number, score FROM reviews WHERE project_id=?1 ORDER BY number DESC LIMIT 1", &[&project_id]).ok().flatten();
    let prev_num = last_review.as_ref().and_then(|r| r.get("number")).and_then(|v| v.as_i64()).unwrap_or(0);
    let prev_score = last_review.as_ref().and_then(|r| r.get("score")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let refs_count: i64 = conn.query_row("SELECT COUNT(*) FROM refs WHERE project_id=?1", params![project_id], |r| r.get(0)).unwrap_or(0);
    drop(conn);

    let judge_names: Vec<String> = judges.iter().filter_map(|j| j.get("name").and_then(|v| v.as_str()).map(String::from)).collect();

    // The review context travels in the system prompt between the simulated
    // provider's markers (judges, reference count, previous score, focus).
    let system = format!(
        "{}. Devuelve EXCLUSIVAMENTE JSON válido con esta forma: \
        {{\"dims\":[{{\"name\":string,\"score\":number}}],\"findings\":[{{\"severity\":\"high|med|low\",\"location\":string,\"text\":string}}],\"verdict\":string}}. \
        {}{}. {}{} referencias. {}{:.1}/10. {}{}». \
        Responde solo con el JSON, sin texto adicional ni fences.",
        simulated::REVIEW_MARKER,
        simulated::JUDGES_MARKER,
        judge_names.join(", "),
        simulated::REFS_COUNT_MARKER,
        refs_count,
        simulated::PREV_SCORE_MARKER,
        prev_score,
        simulated::FOCUS_MARKER,
        focus,
    );
    let messages = vec![
        Message::system(system),
        Message::user(format!("Revisa: {}", focus)),
    ];
    // The trust dispatch (Story 2.4): same contract as the assistant reply.
    let resp = crate::trust::reserve_and_chat(
        db,
        &layer,
        ChatRequest::new(messages).with_temperature(0.4),
        assistant_plan(&layer, "review"),
    )
    .await
    .map_err(|e| e.to_string())?;

    // Parse the JSON every mode returns (real providers and CLIs verbatim;
    // the simulated provider reconstructs it from the prompt markers).
    let parsed: Value = serde_json::from_str(&extract_json(&resp.content))
        .unwrap_or_else(|_| json!({}));
    let dims: Vec<Dim> = parsed.get("dims").cloned()
        .map(|v| serde_json::from_value(v).unwrap_or_default()).unwrap_or_default();
    let findings: Vec<Finding> = parsed.get("findings").cloned()
        .map(|v| serde_json::from_value(v).unwrap_or_default()).unwrap_or_default();
    let verdict = parsed.get("verdict").and_then(|v| v.as_str()).unwrap_or("Revisión completada.").to_string();
    let score = if dims.is_empty() { 7.5 } else { dims.iter().map(|d| d.score).sum::<f64>() / dims.len() as f64 };

    // persist review
    let review_id = uuid::Uuid::new_v4().to_string();
    let new_num = prev_num + 1;
    let dims_json = serde_json::to_string(&dims).map_err(|e| e.to_string())?;
    {
        let conn = db.0.lock().await;
        conn.execute(
            "INSERT INTO reviews(id,project_id,number,score,focus,dims,verdict,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![review_id, project_id, new_num, score, focus, dims_json, verdict, now()],
        ).map_err(|e| e.to_string())?;
        // persist findings
        for f in &findings {
            conn.execute(
                "INSERT INTO findings(id,review_id,severity,location,text,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
                params![uuid::Uuid::new_v4().to_string(), review_id, f.severity, f.location, f.text, now()],
            ).map_err(|e| e.to_string())?;
        }
        // create actions from high/med findings
        let mut next_code = next_action_code(&conn, project_id);
        for f in findings.iter().filter(|f| f.severity != "low") {
            conn.execute(
                "INSERT INTO actions(id,project_id,code,title,description,priority,origin,due,location,source_review_id,source_finding,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,'review','',?7,?8,?9,?10)",
                params![uuid::Uuid::new_v4().to_string(), project_id, next_code, f.text, f.text,
                    if f.severity == "high" { "high" } else { "med" },
                    f.location, review_id, f.severity, now()],
            ).map_err(|e| e.to_string())?;
            next_code = inc_code(next_code);
        }
    }

    Ok(json!({
        "review_id": review_id,
        "number": new_num,
        "score": score,
        "prev_score": prev_score,
        "dims": dims,
        "findings": findings,
        "verdict": verdict
    }))
}

fn next_action_code(conn: &rusqlite::Connection, project_id: &str) -> String {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM actions WHERE project_id=?1", params![project_id], |r| r.get(0)).unwrap_or(0);
    format!("A-{:03}", n + 1)
}
fn inc_code(c: String) -> String {
    if let Some(num) = c.strip_prefix("A-").and_then(|s| s.parse::<u32>().ok()) {
        format!("A-{:03}", num + 1)
    } else { c }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Finding {
    severity: String,
    location: String,
    text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Dim {
    name: String,
    score: f64,
}

fn now() -> String { chrono::Utc::now().to_rfc3339() }

/// Extract the first balanced `{...}` JSON object from a free-text response.
fn extract_json(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut start = None;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate() {
        if in_str {
            if esc { esc = false; }
            else if b == b'\\' { esc = true; }
            else if b == b'"' { in_str = false; }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => {
                if start.is_none() { start = Some(i); }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(st) = start {
                        return s[st..=i].to_string();
                    }
                }
            }
            _ => {}
        }
    }
    s.trim().trim_matches('`').to_string()
}
