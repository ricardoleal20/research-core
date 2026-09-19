// Agent loop: OpenAI-compatible chat completions with a graceful simulated
// fallback when no API key is configured. Powers both the Asistente chat and
// the AI Review orchestrator (which also writes reviews + actions to the DB).
use crate::db::{self, Db};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMsg {
    pub role: String,
    pub content: String,
}

pub struct AgentConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub agent_path: String,
    /// "cli" | "provider" | "simulate" (empty/missing => legacy auto behavior).
    pub llm_mode: String,
    pub llm_cli: String,
    pub llm_cli_model: String,
}

pub fn load_config(conn: &rusqlite::Connection) -> AgentConfig {
    AgentConfig {
        base_url: db::get_setting(conn, "base_url"),
        api_key: load_api_key(conn),
        model: db::get_setting(conn, "model"),
        agent_path: db::get_setting(conn, "agent_path"),
        llm_mode: db::get_setting(conn, "llm_mode"),
        llm_cli: db::get_setting(conn, "llm_cli"),
        llm_cli_model: db::get_setting(conn, "llm_cli_model"),
    }
}

/// API keys live in the OS keychain after the one-time migration (AD-16);
/// the legacy `settings.api_key` row remains as the fallback for keys that
/// could not be moved.
fn load_api_key(conn: &rusqlite::Connection) -> String {
    let provider = db::get_setting(conn, "provider");
    let account = crate::eventstore::migration::keychain_account(&provider);
    if let Ok(entry) = keyring::Entry::new(crate::eventstore::migration::KEYCHAIN_SERVICE, &account) {
        if let Ok(key) = entry.get_password() {
            if !key.trim().is_empty() {
                return key;
            }
        }
    }
    db::get_setting(conn, "api_key")
}

/// Resolve the effective mode. Empty/legacy settings fall back to the original
/// auto behavior: real provider if key+base_url set, else simulate.
fn effective_mode(cfg: &AgentConfig) -> &'static str {
    match cfg.llm_mode.as_str() {
        "cli" => "cli",
        "provider" => "provider",
        "simulate" => "simulate",
        _ => {
            if !cfg.api_key.trim().is_empty() && !cfg.base_url.trim().is_empty() {
                "provider"
            } else {
                "simulate"
            }
        }
    }
}

fn has_real_provider(cfg: &AgentConfig) -> bool {
    !cfg.api_key.trim().is_empty() && !cfg.base_url.trim().is_empty()
}

/// Reply in an assistant chat. Returns (content, classify_tag).
pub async fn assistant_reply(
    db: &Db,
    project_id: &str,
    history: Vec<ChatMsg>,
) -> Result<(String, Option<String>), String> {
    let conn = db.0.lock().await;
    let cfg = load_config(&conn);
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

    let system = format!(
        "Eres el Asistente de Research Core, un copiloto de escritura académica. \
        Proyecto activo: «{}». Referencias disponibles:\n{}\n\
        Redacta en español, tono académico sobrio. Sé conciso y útil. \
        Cuando propongas texto para el manuscrito, inclúyelo en un bloque citado.",
        proj_name, ref_list.join("\n")
    );

    match effective_mode(&cfg) {
        "provider" if has_real_provider(&cfg) => {
            chat_completion(&cfg, &system, history).await.map(|c| (c, None))
        }
        "cli" => {
            chat_via_cli(&cfg, &system, history).await.map(|c| (c, None))
        }
        _ => {
            // Simulated but contextual reply
            let last = history.last().map(|m| m.content.as_str()).unwrap_or("");
            let reply = simulate_assistant(last, &ref_list.join("\n"));
            Ok((reply, None))
        }
    }
}

fn simulate_assistant(user_msg: &str, refs: &str) -> String {
    let lower = user_msg.to_lowercase();
    if lower.contains("resum") || lower.contains("summary") {
        return "Aquí un resumen estructurado en 4 puntos clave, listo para §2:\n\n1. **Contexto** — el problema se motiva en los costos de inferencia de attention.\n2. **Método** — se propone una optimización basada en sparsificación.\n3. **Resultados** — ganancia de 1.8× sin pérdida significativa de calidad.\n4. **Limitaciones** — validez acotada al régimen evaluado.\n\n¿Lo expando a un párrafo continuo para el manuscrito?".to_string();
    }
    if lower.contains("redacta") || lower.contains("párrafo") || lower.contains("parrafo") || lower.contains("escribe") {
        return "Propuesta para el manuscrito:\n\n> Empleamos un learning rate de 3×10⁻⁴ con tamaño de lote B = 256, una elección consistente con los regímenes de escalamiento descritos por Kaplan et al. (2020). Bajo la regla de escalamiento lineal, este par se ubica en una región de convergencia estable sin sacrificar generalización.\n\n¿Quieres que cite también Hoffmann et al. (2022) sobre Chinchilla?".to_string();
    }
    format!("Entendido. Trabajando en el contexto de tu proyecto. Puedo redactar párrafos, resumir referencias, comparar baselines o sugerir citations. Referencias cargadas:\n{}\n\n¿Qué redactamos primero?", refs.lines().take(5).collect::<Vec<_>>().join("\n"))
}

/// Run an AI review over the project. Creates a review record, findings, and
/// actions in the DB. Returns the new review id.
pub async fn run_review(
    db: &Db,
    project_id: &str,
    focus: &str,
) -> Result<Value, String> {
    let conn = db.0.lock().await;
    let cfg = load_config(&conn);
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

    let (score, dims, findings, verdict) = match effective_mode(&cfg) {
        "provider" if has_real_provider(&cfg) => {
            run_review_via_llm(&cfg, project_id, focus, &judge_names, refs_count).await?
        }
        "cli" => run_review_via_cli(&cfg, project_id, focus, &judge_names, refs_count).await?,
        _ => simulate_review(focus, &judge_names, refs_count, prev_score),
    };

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

async fn run_review_via_llm(
    cfg: &AgentConfig, _project_id: &str, focus: &str,
    judges: &[String], refs_count: i64,
) -> Result<(f64, Vec<Dim>, Vec<Finding>, String), String> {
    let system = format!(
        "Eres un orquestador de revisión académica. Devuelve EXCLUSIVAMENTE JSON válido con esta forma: \
        {{\"dims\":[{{\"name\":string,\"score\":number}}],\"findings\":[{{\"severity\":\"high|med|low\",\"location\":string,\"text\":string}}],\"verdict\":string}}. \
        Usa los jueces: {}. Hay {} referencias. Foco de la revisión: «{}».",
        judges.join(", "), refs_count, focus
    );
    let msgs = vec![ChatMsg { role: "user".into(), content: format!("Revisa: {}", focus) }];
    let raw = chat_completion(cfg, &system, msgs).await?;
    let parsed: Value = serde_json::from_str(&extract_json(&raw))
        .unwrap_or_else(|_| json!({}));
    let dims: Vec<Dim> = parsed.get("dims").cloned()
        .map(|v| serde_json::from_value(v).unwrap_or_default()).unwrap_or_default();
    let findings: Vec<Finding> = parsed.get("findings").cloned()
        .map(|v| serde_json::from_value(v).unwrap_or_default()).unwrap_or_default();
    let verdict = parsed.get("verdict").and_then(|v| v.as_str()).unwrap_or("Revisión completada.").to_string();
    let score = if dims.is_empty() { 7.5 } else { dims.iter().map(|d| d.score).sum::<f64>() / dims.len() as f64 };
    Ok((score, dims, findings, verdict))
}

fn simulate_review(focus: &str, judges: &[String], refs_count: i64, prev: f64) -> (f64, Vec<Dim>, Vec<Finding>, String) {
    let mut dims = Vec::new();
    for (i, j) in judges.iter().enumerate() {
        let base = 7.2 + (i as f64 * 0.3);
        let s = (base + 0.6).min(9.5);
        dims.push(Dim { name: j.clone(), score: (s * 10.0).round() / 10.0 });
    }
    if dims.is_empty() {
        dims.push(Dim { name: "General".into(), score: 7.6 });
    }
    let score = (dims.iter().map(|d| d.score).sum::<f64>() / dims.len() as f64 * 10.0).round() / 10.0;
    let findings = vec![
        Finding { severity: "high".into(), location: focus_sect(focus, "§4.2"), text: "Añadir varianza e intervalos de confianza a los ablations (3-5 semillas por configuración).".into() },
        Finding { severity: "med".into(), location: focus_sect(focus, "§3.1"), text: "Conectar la justificación del learning rate con Scaling Laws (Kaplan 2020).".into() },
        Finding { severity: "low".into(), location: "§3".into(), text: "Suavizar la transición entre §3.1 y §3.2.".into() },
    ];
    let delta = if score >= prev { format!("+{:.1}", score - prev) } else { format!("{:.1}", score - prev) };
    let verdict = format!(
        "Buen estado general. Puntaje {}/10 (cambio {} desde la revisión anterior). Rigor metodológico sólido y cobertura de {} referencias. Prioridad: atender los hallazgos de {}.",
        score, delta, refs_count, focus_sect(focus, "§4.2")
    );
    (score, dims, findings, verdict)
}

fn focus_sect(focus: &str, def: &str) -> String {
    if focus.is_empty() { def.to_string() } else {
        // pick the first §x.y token found, else default
        focus.split_whitespace().find(|t| t.starts_with("§")).map(String::from).unwrap_or_else(|| def.to_string())
    }
}

async fn chat_completion(cfg: &AgentConfig, system: &str, history: Vec<ChatMsg>) -> Result<String, String> {
    let mut messages: Vec<Value> = vec![json!({ "role": "system", "content": system })];
    for m in history {
        messages.push(json!({ "role": m.role, "content": m.content }));
    }
    let body = json!({ "model": cfg.model, "messages": messages, "temperature": 0.4 });
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build().map_err(|e| e.to_string())?;
    let resp = client.post(&url)
        .bearer_auth(&cfg.api_key)
        .json(&body)
        .send().await.map_err(|e| format!("request: {e}"))?;
    if !resp.status().is_success() {
        let st = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("provider error {st}: {}", txt.chars().take(300).collect::<String>()));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    let content = v["choices"][0]["message"]["content"].as_str()
        .ok_or("empty completion")?.to_string();
    Ok(content)
}

fn now() -> String { chrono::Utc::now().to_rfc3339() }

// ---------- CLI LLM adapter ----------
// Spawns a local coding CLI (claude / codex / opencode) to generate completions.
// Local-first: no HTTP, no API key. The system prompt + conversation are passed
// as the prompt; stdout is captured as the completion.

/// Which CLI binary to use, falling back to a sensible default.
fn cli_command(cfg: &AgentConfig) -> &str {
    let c = cfg.llm_cli.trim();
    if c.is_empty() { "claude" } else { c }
}

/// Build the prompt string the CLI receives (system + history collapsed).
fn cli_prompt(system: &str, history: &[ChatMsg]) -> String {
    let mut out = String::new();
    if !system.trim().is_empty() {
        out.push_str(system);
        out.push_str("\n\n");
    }
    for m in history {
        let who = match m.role.as_str() {
            "assistant" => "Asistente",
            _ => "Tú",
        };
        out.push_str(&format!("{}: {}\n", who, m.content));
    }
    out.push_str("Asistente:");
    out
}

async fn chat_via_cli(cfg: &AgentConfig, system: &str, history: Vec<ChatMsg>) -> Result<String, String> {
    let cmd = cli_command(cfg);
    let prompt = cli_prompt(system, &history);
    run_cli(cmd, &cfg.llm_cli_model, &prompt).await
}

/// Run a CLI: `claude -p "<prompt>"` (with optional `--model`). For codex /
/// opencode we use the same shape (`codex exec`, `opencode`), since all three
/// accept a prompt and print the result to stdout.
async fn run_cli(cmd: &str, model: &str, prompt: &str) -> Result<String, String> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    // Resolve the binary via the augmented PATH (finds Homebrew/nvm installs
    // even when launched as a GUI .app bundle).
    let resolved = crate::mcp::augmented_path();
    let mut command = match cmd {
        "codex" => {
            let mut c = tokio::process::Command::new("codex");
            c.arg("exec");
            c
        }
        "opencode" => {
            let mut c = tokio::process::Command::new("opencode");
            c.arg("run");
            c
        }
        _ => {
            // claude (default): `claude -p "<prompt>" [--model X]`
            let mut c = tokio::process::Command::new("claude");
            c.arg("-p");
            if !model.trim().is_empty() {
                c.arg("--model").arg(model);
            }
            c
        }
    };
    command.env("PATH", resolved);
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    // For claude/opencode the prompt is a CLI arg; for codex it's on stdin.
    let prompt_owned = prompt.to_string();
    let model_owned = model.to_string();
    let cmd_owned = cmd.to_string();
    let mut child = command.spawn().map_err(|e| format!("spawn {cmd_owned}: {e} (¿instalado y en PATH?)"))?;

    // codex reads the prompt from stdin; the others got it as an arg already.
    if cmd_owned == "codex" {
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt_owned.as_bytes()).await.map_err(|e| e.to_string())?;
            stdin.flush().await.map_err(|e| e.to_string())?;
            // dropping stdin closes it
        }
    }
    let _ = model_owned;
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| format!("{cmd_owned} timed out (>120s)"))?
    .map_err(|e| format!("{cmd_owned}: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{cmd_owned} failed: {}", err.chars().take(300).collect::<String>()));
    }
    let out = String::from_utf8_lossy(&output.stdout).to_string();
    if out.trim().is_empty() {
        return Err(format!("{cmd_owned} produced no output"));
    }
    Ok(out)
}

/// Mirror of run_review_via_llm but routing through the CLI adapter.
async fn run_review_via_cli(
    cfg: &AgentConfig, _project_id: &str, focus: &str,
    judges: &[String], refs_count: i64,
) -> Result<(f64, Vec<Dim>, Vec<Finding>, String), String> {
    let system = format!(
        "Eres un orquestador de revisión académica. Devuelve EXCLUSIVAMENTE JSON válido con esta forma: \
        {{\"dims\":[{{\"name\":string,\"score\":number}}],\"findings\":[{{\"severity\":\"high|med|low\",\"location\":string,\"text\":string}}],\"verdict\":string}}. \
        Usa los jueces: {}. Hay {} referencias. Foco de la revisión: «{}». \
        Responde solo con el JSON, sin texto adicional ni fences.",
        judges.join(", "), refs_count, focus
    );
    let msgs = vec![ChatMsg { role: "user".into(), content: format!("Revisa: {}", focus) }];
    let raw = chat_via_cli(cfg, &system, msgs).await?;
    // Tolerate surrounding prose: extract the first {...} block.
    let json_str = extract_json(&raw);
    let parsed: Value = serde_json::from_str(&json_str).unwrap_or_else(|_| json!({}));
    let dims: Vec<Dim> = parsed.get("dims").cloned()
        .map(|v| serde_json::from_value(v).unwrap_or_default()).unwrap_or_default();
    let findings: Vec<Finding> = parsed.get("findings").cloned()
        .map(|v| serde_json::from_value(v).unwrap_or_default()).unwrap_or_default();
    let verdict = parsed.get("verdict").and_then(|v| v.as_str()).unwrap_or("Revisión completada.").to_string();
    let score = if dims.is_empty() { 7.5 } else { dims.iter().map(|d| d.score).sum::<f64>() / dims.len() as f64 };
    Ok((score, dims, findings, verdict))
}

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
