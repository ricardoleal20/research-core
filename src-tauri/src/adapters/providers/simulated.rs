// The simulated provider (Story 1.6): the existing offline mock, routed
// through the same `ProviderClient` interface as every real provider so the
// caller shape is uniform — with no key configured, all AI features keep
// working through this adapter.
//
// The mock is prompt-format-aware by design: agent.rs builds its prompts
// from the marker constants below (single source of truth, they cannot
// drift), and the simulator derives its contextual answer from them. It
// costs nothing and records no spend.

use serde_json::json;

use crate::domain::chat::{
    ATTACHMENTS_END, ATTACHMENTS_MARKER, BOARD_CONTEXT_MARKER, SKILL_MARKER,
};

use super::{
    ChatRequest, ChatResponse, Message, ProviderClient, ProviderError, Usage,
};

// ---- Prompt markers (shared with agent.rs's prompt builders) ----

/// Marks the review-orchestrator system prompt.
pub const REVIEW_MARKER: &str = "Eres un orquestador de revisión académica";
/// Followed by the comma-separated judge names, ending before ". ".
pub const JUDGES_MARKER: &str = "Usa los jueces: ";
/// Followed by the reference count, ending before " referencias".
pub const REFS_COUNT_MARKER: &str = "Hay ";
/// Followed by the previous score, ending before "/10".
pub const PREV_SCORE_MARKER: &str = "Puntaje de la revisión anterior: ";
/// The review focus sits between this marker and the closing "»".
pub const FOCUS_MARKER: &str = "Foco de la revisión: «";
/// The assistant prompt's reference list starts here …
pub const REFS_LIST_MARKER: &str = "Referencias disponibles:\n";
/// … and ends here.
pub const REFS_LIST_END: &str = "\nRedacta";
/// Marks the hypothesis-candidates generator system prompt (onboarding,
/// Story 1.9). The paper title sits between `PAPER_TITLE_MARKER` and the
/// closing "»"; the output language follows `LANG_MARKER`.
pub const CANDIDATES_MARKER: &str = "Eres el generador de candidatos de hipótesis";
/// The paper title sits between this marker and `PAPER_TITLE_END`.
pub const PAPER_TITLE_MARKER: &str = "Artículo: «";
/// … and ends here.
pub const PAPER_TITLE_END: &str = "»";
/// Followed by the output language code ("es" | "en").
pub const LANG_MARKER: &str = "Idioma de salida: ";

pub struct Simulated;

impl ProviderClient for Simulated {
    fn name(&self) -> &str {
        "simulated"
    }

    fn chat(
        &self,
        req: ChatRequest,
    ) -> super::BoxFuture<'_, Result<ChatResponse, ProviderError>> {
        Box::pin(async move {
            let system = req
                .messages
                .iter()
                .find(|m| m.role == "system")
                .map(|m| m.content.as_str())
                .unwrap_or("");
            let last_user = Message::last_user(&req.messages)
                .map(|m| m.content.clone())
                .unwrap_or_default();
            let content = if system.contains(REVIEW_MARKER) {
                review_json(system)
            } else if system.contains(CANDIDATES_MARKER) {
                candidates_json(system)
            } else {
                // The assistant reply carries the honest context echo: the
                // markers the scoped prompt carries (board context, skill,
                // attachments) are read back so `vite` dev demonstrates the
                // wiring (Stories 5.4–5.6 ACs).
                let mut reply = assistant_reply(&last_user, refs_from_system(system));
                reply.push_str(&context_echo(system));
                reply
            };
            // Simulated calls are free: no usage to report, no spend to record.
            Ok(ChatResponse { content, usage: Usage::ZERO })
        })
    }
}

/// The reference list embedded in the assistant system prompt.
fn refs_from_system(system: &str) -> &str {
    match system.split_once(REFS_LIST_MARKER) {
        Some((_, rest)) => rest.split_once(REFS_LIST_END).map(|(refs, _)| refs).unwrap_or(rest),
        None => "",
    }
}

/// The honest context echo (Stories 5.4–5.6): whatever scoped context the
/// prompt carried — board context (with its mission label), the active
/// skill, the attachments — is echoed back as a receipt line, so the mock
/// demonstrates the wiring instead of pretending context traveled.
fn context_echo(system: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some((_, rest)) = system.split_once(BOARD_CONTEXT_MARKER) {
        if let Some(label) = rest.split_once(" «").map(|(label, _)| label.trim()) {
            if !label.is_empty() {
                parts.push(format!("contexto: tablero {label} sincronizado"));
            }
        }
    }
    if let Some((_, rest)) = system.split_once(SKILL_MARKER) {
        if let Some(skill) = rest.split_once('»').map(|(s, _)| s.trim()) {
            if !skill.is_empty() {
                parts.push(format!("skill: {skill}"));
            }
        }
    }
    if let Some((_, rest)) = system.split_once(ATTACHMENTS_MARKER) {
        let block = rest.split_once(ATTACHMENTS_END).map(|(b, _)| b).unwrap_or(rest);
        let n = block.lines().filter(|l| l.starts_with("- ")).count();
        if n > 0 {
            parts.push(format!("adjuntos: {n}"));
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("\n\n— {}", parts.join(" · "))
    }
}

/// The simulated Asistente reply — keyword heuristics over the last user
/// message (the existing mock, preserved).
pub fn assistant_reply(user_msg: &str, refs: &str) -> String {
    let lower = user_msg.to_lowercase();
    if lower.contains("resum") || lower.contains("summary") {
        return "Aquí un resumen estructurado en 4 puntos clave, listo para §2:\n\n1. **Contexto** — el problema se motiva en los costos de inferencia de attention.\n2. **Método** — se propone una optimización basada en sparsificación.\n3. **Resultados** — ganancia de 1.8× sin pérdida significativa de calidad.\n4. **Limitaciones** — validez acotada al régimen evaluado.\n\n¿Lo expando a un párrafo continuo para el manuscrito?".to_string();
    }
    if lower.contains("redacta")
        || lower.contains("párrafo")
        || lower.contains("parrafo")
        || lower.contains("escribe")
    {
        return "Propuesta para el manuscrito:\n\n> Empleamos un learning rate de 3×10⁻⁴ con tamaño de lote B = 256, una elección consistente con los regímenes de escalamiento descritos por Kaplan et al. (2020). Bajo la regla de escalamiento lineal, este par se ubica en una región de convergencia estable sin sacrificar generalización.\n\n¿Quieres que cite también Hoffmann et al. (2022) sobre Chinchilla?".to_string();
    }
    format!("Entendido. Trabajando en el contexto de tu proyecto. Puedo redactar párrafos, resumir referencias, comparar baselines o sugerir citations. Referencias cargadas:\n{}\n\n¿Qué redactamos primero?", refs.lines().take(5).collect::<Vec<_>>().join("\n"))
}

/// The simulated review, as the JSON the review orchestrator asks providers
/// for — reconstructed from the review prompt's markers (judges, reference
/// count, previous score, focus).
pub fn review_json(system: &str) -> String {
    let judges = judges_from_prompt(system);
    let refs_count = after_marker(system, REFS_COUNT_MARKER)
        .and_then(|s| s.split(' ').next())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let prev = after_marker(system, PREV_SCORE_MARKER)
        .and_then(|s| s.split('/').next())
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(0.0);
    let focus = system
        .split_once(FOCUS_MARKER)
        .and_then(|(_, rest)| rest.split_once('»'))
        .map(|(f, _)| f.to_string())
        .unwrap_or_default();

    let mut dims: Vec<serde_json::Value> = Vec::new();
    for (i, j) in judges.iter().enumerate() {
        let base = 7.2 + (i as f64 * 0.3);
        let s = (base + 0.6).min(9.5);
        dims.push(json!({ "name": j, "score": (s * 10.0).round() / 10.0 }));
    }
    if dims.is_empty() {
        dims.push(json!({ "name": "General", "score": 7.6 }));
    }
    let score = dims
        .iter()
        .map(|d| d["score"].as_f64().unwrap_or(0.0))
        .sum::<f64>()
        / dims.len() as f64;
    let score = (score * 10.0).round() / 10.0;
    let findings = json!([
        { "severity": "high", "location": focus_sect(&focus, "§4.2"), "text": "Añadir varianza e intervalos de confianza a los ablations (3-5 semillas por configuración)." },
        { "severity": "med", "location": focus_sect(&focus, "§3.1"), "text": "Conectar la justificación del learning rate con Scaling Laws (Kaplan 2020)." },
        { "severity": "low", "location": "§3", "text": "Suavizar la transición entre §3.1 y §3.2." },
    ]);
    let delta = if score >= prev {
        format!("+{:.1}", score - prev)
    } else {
        format!("{:.1}", score - prev)
    };
    let verdict = format!(
        "Buen estado general. Puntaje {}/10 (cambio {} desde la revisión anterior). Rigor metodológico sólido y cobertura de {} referencias. Prioridad: atender los hallazgos de {}.",
        score,
        delta,
        refs_count,
        focus_sect(&focus, "§4.2")
    );
    json!({ "dims": dims, "findings": findings, "verdict": verdict }).to_string()
}

fn judges_from_prompt(system: &str) -> Vec<String> {
    after_marker(system, JUDGES_MARKER)
        .map(|s| s.split(". ").next().unwrap_or("").to_string())
        .unwrap_or_default()
        .split(", ")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// The simulated hypothesis candidates (onboarding, Story 1.9), as the JSON
/// the generator prompt asks providers for — reconstructed from the prompt's
/// markers (paper title, output language). Sensible in both languages:
/// falsifiable statements about the paper, not placeholders.
pub fn candidates_json(system: &str) -> String {
    let title = system
        .split_once(PAPER_TITLE_MARKER)
        .and_then(|(_, rest)| rest.split_once(PAPER_TITLE_END))
        .map(|(t, _)| t.trim())
        .filter(|t| !t.is_empty())
        .unwrap_or("el artículo");
    let lang = after_marker(system, LANG_MARKER)
        .map(|s| {
            s.trim()
                .trim_end_matches('.')
                .split_whitespace()
                .next()
                .unwrap_or("es")
                .to_string()
        })
        .unwrap_or_else(|| "es".into());
    let candidates: Vec<serde_json::Value> = if lang == "en" {
        vec![
            json!({ "statement": format!("The central result of «{title}» replicates under independent evaluation"), "confidence": 0.78 }),
            json!({ "statement": format!("The method of «{title}» outperforms the baselines it is compared against"), "confidence": 0.71 }),
            json!({ "statement": format!("The claims of «{title}» hold only within the regimes its authors evaluate"), "confidence": 0.65 }),
        ]
    } else {
        vec![
            json!({ "statement": format!("El resultado central de «{title}» se replica bajo una evaluación independiente"), "confidence": 0.78 }),
            json!({ "statement": format!("El método de «{title}» supera a los baselines con los que se compara"), "confidence": 0.71 }),
            json!({ "statement": format!("Las afirmaciones de «{title}» solo se sostienen dentro de los regímenes que sus autores evalúan"), "confidence": 0.65 }),
        ]
    };
    json!({ "candidates": candidates }).to_string()
}

fn after_marker<'a>(s: &'a str, marker: &str) -> Option<&'a str> {
    s.split_once(marker).map(|(_, rest)| rest)
}

fn focus_sect(focus: &str, def: &str) -> String {
    if focus.is_empty() {
        def.to_string()
    } else {
        // pick the first §x.y token found, else default
        focus
            .split_whitespace()
            .find(|t| t.starts_with("§"))
            .map(String::from)
            .unwrap_or_else(|| def.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review_system() -> String {
        format!(
            "{REVIEW_MARKER}. Devuelve EXCLUSIVAMENTE JSON válido. \
             {JUDGES_MARKER}Rigor metodológico, Originalidad, Claridad. \
             {REFS_COUNT_MARKER}6 referencias. \
             {PREV_SCORE_MARKER}7.2/10. \
             {FOCUS_MARKER}§4.2 y §3»."
        )
    }

    #[test]
    fn assistant_reply_matches_the_last_user_message_and_refs() {
        let r = assistant_reply("Redacta un párrafo sobre scaling laws", "");
        assert!(r.contains("Propuesta para el manuscrito"));
        let r = assistant_reply("dame un resumen", "");
        assert!(r.contains("resumen estructurado"));
        let r = assistant_reply("hola", "- Ref A\n- Ref B\n- Ref C\n- Ref D\n- Ref E\n- Ref F");
        assert!(r.contains("Referencias cargadas:"));
        assert!(r.contains("- Ref E"));
        assert!(!r.contains("- Ref F"), "only the first 5 refs are listed");
    }

    #[test]
    fn review_json_reconstructs_the_prompt_context() {
        let raw = review_json(&review_system());
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let dims = v["dims"].as_array().unwrap();
        assert_eq!(dims.len(), 3);
        assert_eq!(dims[0]["name"], "Rigor metodológico");
        assert_eq!(dims[0]["score"], 7.8);
        assert_eq!(dims[1]["score"], 8.1);
        assert_eq!(dims[2]["score"], 8.4);
        let findings = v["findings"].as_array().unwrap();
        assert_eq!(findings.len(), 3);
        assert_eq!(findings[0]["severity"], "high");
        // the first §x.y token of the focus drives the finding locations
        assert_eq!(findings[0]["location"], "§4.2");
        assert_eq!(findings[1]["location"], "§4.2");
        let verdict = v["verdict"].as_str().unwrap();
        assert!(verdict.contains("8.1/10"), "verdict carries the mean score: {verdict}");
        assert!(verdict.contains("6 referencias"));
    }

    #[test]
    fn review_json_without_judges_falls_back_to_general() {
        let system = format!(
            "{REVIEW_MARKER}. JSON. {JUDGES_MARKER}. {REFS_COUNT_MARKER}0 referencias. \
             {PREV_SCORE_MARKER}0.0/10. {FOCUS_MARKER}revisión general»."
        );
        let v: serde_json::Value = serde_json::from_str(&review_json(&system)).unwrap();
        let dims = v["dims"].as_array().unwrap();
        assert_eq!(dims.len(), 1);
        assert_eq!(dims[0]["name"], "General");
    }

    #[test]
    fn refs_list_is_parsed_from_the_system_prompt() {
        let system = format!(
            "Eres el Asistente. {}- Ref A (x, 2020) — V\n{}Redacta en español.",
            REFS_LIST_MARKER,
            ""
        );
        assert_eq!(refs_from_system(&system), "- Ref A (x, 2020) — V");
        assert_eq!(refs_from_system("sin marcador"), "");
    }

    /// Stories 5.4–5.6: the mock echoes the scoped context it was handed —
    /// board context (mission label), skill, attachments — and stays silent
    /// for a plain unscoped conversation.
    #[test]
    fn the_mock_echoes_the_scoped_context_honestly() {
        use crate::domain::chat::{
            assemble_attachments_context, assemble_board_context, AttachmentContext,
            AttachmentKind, BoardContext, BoardHypothesis,
        };
        let scoped = format!(
            "Eres el Bibliotecario. {}librarian{}. {}{}\nRedacta en español.\n\n{}\n\n{}",
            SKILL_MARKER,
            "».",
            REFS_LIST_MARKER,
            "- Ref A",
            assemble_board_context(Some(&BoardContext {
                mission_label: "M-3".into(),
                question: "q".into(),
                hypotheses: vec![BoardHypothesis {
                    label: "H-1".into(),
                    statement: "s".into(),
                    status: "proposed".into(),
                    pins: vec![],
                }],
            })),
            assemble_attachments_context(&[AttachmentContext {
                name: "notas.md".into(),
                kind: AttachmentKind::Text,
                text: Some("x".into()),
                truncated: false,
            }]),
        );
        let echo = context_echo(&scoped);
        assert!(echo.contains("contexto: tablero M-3 sincronizado"), "echo: {echo}");
        assert!(echo.contains("skill: librarian"));
        assert!(echo.contains("adjuntos: 1"));
        // a plain conversation echoes nothing
        let plain = format!("Eres el Asistente. {}- Ref A\n{}Redacta en español.", REFS_LIST_MARKER, "");
        assert_eq!(context_echo(&plain), "");
    }

    #[test]
    fn candidates_json_derives_sensible_candidates_from_the_paper() {
        // Spanish (the app's default language): topic-aware falsifiable statements.
        let system = format!(
            "{CANDIDATES_MARKER} de Research Core. JSON. {PAPER_TITLE_MARKER}Attention Is All You Need{PAPER_TITLE_END}. {LANG_MARKER}es."
        );
        let v: serde_json::Value = serde_json::from_str(&candidates_json(&system)).unwrap();
        let cands = v["candidates"].as_array().unwrap();
        assert_eq!(cands.len(), 3);
        for c in cands {
            assert!(c["statement"].as_str().unwrap().contains("Attention Is All You Need"));
            let conf = c["confidence"].as_f64().unwrap();
            assert!((0.0..=1.0).contains(&conf));
        }
        // English output follows the language marker.
        let system = format!(
            "{CANDIDATES_MARKER} de Research Core. JSON. {PAPER_TITLE_MARKER}Attention Is All You Need{PAPER_TITLE_END}. {LANG_MARKER}en."
        );
        let v: serde_json::Value = serde_json::from_str(&candidates_json(&system)).unwrap();
        assert!(v["candidates"][0]["statement"]
            .as_str()
            .unwrap()
            .starts_with("The central result"));
    }
}
