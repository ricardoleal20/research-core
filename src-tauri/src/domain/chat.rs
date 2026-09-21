// Chat intelligence domain (FR-16, Epic 5, Stories 5.4–5.6): the evented
// conversation scope — the mission binding (5.4), the per-conversation skill
// (5.6), and conversation attachments (5.5) — plus the PURE context
// assembly that feeds the provider call. The chats/messages tables remain
// the legacy baseline (AD-16); the events below are the auditable way these
// bindings move after creation — the table columns are their projection,
// never an independent source of truth.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::eventstore::{Actor, EventError, NewEvent, StoredEvent};

pub const CHAT_SCOPED: &str = "chat.scoped";
pub const CHAT_SKILL_SET: &str = "chat.skill_set";
pub const CHAT_MODEL_SET: &str = "chat.model_set";
pub const CHAT_ATTACHMENT_ADDED: &str = "chat.attachment_added";
pub const CHAT_ATTACHMENT_REMOVED: &str = "chat.attachment_removed";

// ---- Prompt markers (single source of truth; the simulated provider
// reads them back to echo its context honestly — same contract as
// REFS_LIST_MARKER) ----

/// The board-context block starts here; the mission label sits between this
/// marker and the following " «".
pub const BOARD_CONTEXT_MARKER: &str = "Contexto del tablero de la misión ";
/// …and the block ends here.
pub const BOARD_CONTEXT_END: &str = "Fin del contexto del tablero.";
/// The attachments block starts here …
pub const ATTACHMENTS_MARKER: &str = "Archivos adjuntos:\n";
/// …and ends here.
pub const ATTACHMENTS_END: &str = "Fin de los adjuntos.";
/// The active skill's name sits between this marker and the closing "»".
pub const SKILL_MARKER: &str = "Habilidad activa: «";
/// …and ends here.
pub const SKILL_END: &str = "».";

// ---------- 5.4: mission scoping ----------

/// The `chat.scoped` payload: a conversation's mission binding. `None` is
/// the explicit "General" scope — re-scoping to General is as much an event
/// as binding a mission (FR-16.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatScopedPayload {
    pub chat_id: String,
    /// The mission the conversation is scoped to; `None` = General.
    #[serde(default)]
    pub mission_id: Option<Uuid>,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a conversation's mission
    /// binding moves — a `chat.scoped` event, actor=user, cause-linked to
    /// the mission's creation when scoped.
    pub fn chat_scoped(chat_id: &str, mission_id: Option<Uuid>) -> Result<Self, EventError> {
        let chat_id = chat_id.trim().to_string();
        if chat_id.is_empty() {
            return Err(EventError::Invalid(
                "chat.scoped: chat_id must not be empty — a scope event names its conversation"
                    .into(),
            ));
        }
        let payload = ChatScopedPayload { chat_id, mission_id };
        let event = Self::new(
            CHAT_SCOPED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?;
        Ok(match mission_id {
            Some(id) => event.with_causes(vec![id]),
            None => event,
        })
    }

    /// Typed constructor (AD-15, Story 5.6): the one way a conversation's
    /// skill moves — a `chat.skill_set` event, actor=user. An empty skill
    /// clears the choice (the plain assistant persona).
    pub fn chat_skill_set(chat_id: &str, skill: &str) -> Result<Self, EventError> {
        let chat_id = chat_id.trim().to_string();
        let skill = skill.trim().to_string();
        if chat_id.is_empty() {
            return Err(EventError::Invalid(
                "chat.skill_set: chat_id must not be empty — a skill event names its conversation"
                    .into(),
            ));
        }
        Ok(Self::new(
            CHAT_SKILL_SET,
            Actor::User,
            serde_json::json!({ "chat_id": chat_id, "skill": skill }),
        )?)
    }

    /// Typed constructor (AD-15, Story 5.9): the one way a conversation's
    /// model choice moves — a `chat.model_set` event, actor=user. An empty
    /// model clears the choice (the provider's configured default). The
    /// choice is per-conversation and history-preserving: earlier messages
    /// keep the attribution they were produced with (FR-17.4).
    pub fn chat_model_set(chat_id: &str, model: &str) -> Result<Self, EventError> {
        let chat_id = chat_id.trim().to_string();
        let model = model.trim().to_string();
        if chat_id.is_empty() {
            return Err(EventError::Invalid(
                "chat.model_set: chat_id must not be empty — a model event names its conversation"
                    .into(),
            ));
        }
        Ok(Self::new(
            CHAT_MODEL_SET,
            Actor::User,
            serde_json::json!({ "chat_id": chat_id, "model": model }),
        )?)
    }
}

/// The conversation an event belongs to (its `chat_id` payload), if any.
fn event_chat_id(event: &StoredEvent) -> Option<&str> {
    event
        .payload
        .get("chat_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// A chat's current mission scope, folded from the log: the LATEST
/// `chat.scoped` event for the chat wins; no event ⇒ the caller's baseline
/// (the table column / creation value).
pub fn folded_scope(events: &[StoredEvent], chat_id: &str) -> Option<Option<Uuid>> {
    events.iter().rev().find_map(|e| {
        if e.kind == CHAT_SCOPED && event_chat_id(e) == Some(chat_id.trim()) {
            Some(
                serde_json::from_value::<ChatScopedPayload>(e.payload.clone())
                    .ok()
                    .map(|p| p.mission_id)
                    .flatten(),
            )
        } else {
            None
        }
    })
}

/// A chat's current skill, folded from the log: the latest `chat.skill_set`
/// event wins; an empty skill string clears the choice.
pub fn folded_skill(events: &[StoredEvent], chat_id: &str) -> Option<String> {
    events.iter().rev().find_map(|e| {
        if e.kind == CHAT_SKILL_SET && event_chat_id(e) == Some(chat_id.trim()) {
            e.payload
                .get("skill")
                .and_then(serde_json::Value::as_str)
                .map(|s| s.trim().to_string())
        } else {
            None
        }
    })
}

/// A chat's current model choice, folded from the log (Story 5.9): the
/// LATEST `chat.model_set` event for the chat wins — an empty model in it
/// is the explicit "back to the provider default" (never a resurrection of
/// an older choice); no event at all ⇒ `None` (the caller's baseline — the
/// table column / the provider's configured default).
pub fn folded_model(events: &[StoredEvent], chat_id: &str) -> Option<String> {
    events.iter().rev().find_map(|e| {
        if e.kind == CHAT_MODEL_SET && event_chat_id(e) == Some(chat_id.trim()) {
            e.payload
                .get("model")
                .and_then(serde_json::Value::as_str)
                .map(|s| s.trim().to_string())
        } else {
            None
        }
    })
}

// ---------- 5.4: board context assembly (pure) ----------

/// One hypothesis of the board context, as the provider call renders it.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardHypothesis {
    /// The display label ("H-3" — derived from the creation seq).
    pub label: String,
    pub statement: String,
    /// The lifecycle status as the board renders it (proposed | testing |
    /// supported | refuted | revised).
    pub status: String,
    /// One line per pinned evidence claim.
    pub pins: Vec<String>,
}

/// The mission board context a scoped conversation carries into the
/// provider call (FR-16.5): the mission's question plus its hypotheses
/// (statements + lifecycle statuses) and their evidence pins — assembled
/// from the existing board read models, never re-derived here.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardContext {
    /// The display label ("M-2" — derived from the creation seq).
    pub mission_label: String,
    pub question: String,
    pub hypotheses: Vec<BoardHypothesis>,
}

/// Assemble the board-context block (pure, FR-16.5). `None` (a General
/// conversation) assembles NOTHING — General sends carry no board context.
pub fn assemble_board_context(ctx: Option<&BoardContext>) -> String {
    let Some(ctx) = ctx else {
        return String::new();
    };
    let mut out = format!(
        "{BOARD_CONTEXT_MARKER}{} «{}» sincronizado:\n",
        ctx.mission_label, ctx.question
    );
    if ctx.hypotheses.is_empty() {
        out.push_str("  (sin hipótesis aún)\n");
    }
    for h in &ctx.hypotheses {
        out.push_str(&format!("- {} [{}] {}\n", h.label, h.status, h.statement));
        for pin in &h.pins {
            out.push_str(&format!("    · evidencia: {pin}\n"));
        }
    }
    out.push_str(BOARD_CONTEXT_END);
    out
}

// ---------- 5.5: attachments ----------

/// An attachment's kind (FR-16.2/16.3): text rides the provider call as
/// context; PDF text is extracted locally and rides the same way; binary is
/// stored by digest-addressed ref and flagged — never silently dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Text,
    Pdf,
    Binary,
}

impl AttachmentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Pdf => "pdf",
            Self::Binary => "binary",
        }
    }
}

/// Classify an attachment from its file name and bytes (pure). Extension
/// first (.md/.txt/.tex ⇒ text, .pdf ⇒ pdf), with an honest magic check: a
/// `.pdf` that does not start with `%PDF` is NOT a pdf — it classifies as
/// binary rather than pretending extractable. Everything else is binary.
pub fn classify_attachment(name: &str, bytes: &[u8]) -> AttachmentKind {
    let lower = name.to_lowercase();
    if lower.ends_with(".pdf") {
        return if bytes.starts_with(b"%PDF") {
            AttachmentKind::Pdf
        } else {
            AttachmentKind::Binary
        };
    }
    if lower.ends_with(".md") || lower.ends_with(".txt") || lower.ends_with(".tex") {
        return AttachmentKind::Text;
    }
    AttachmentKind::Binary
}

/// The `chat.attachment_added` payload (FR-16.1–16.3): the attachment's
/// ref — name, original path, kind, the sha-256 digest of the stored copy,
/// and the honest inclusion flags. Content never enters the event log; it
/// lives in the digest-addressed store and leaves the machine only inside
/// the user's chosen provider call (NFR-12).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttachmentAddedPayload {
    pub chat_id: String,
    pub attachment_id: Uuid,
    pub name: String,
    pub path: String,
    /// text | pdf | binary.
    pub kind: String,
    /// sha-256 hex of the stored copy — the digest-addressed ref.
    pub digest: String,
    pub size_bytes: u64,
    /// Whether the attachment's (extracted) text rides the provider call.
    pub included: bool,
    /// Whether the included text was visibly capped (never quietly
    /// truncated — FR-16.3).
    pub truncated: bool,
    /// Honest exclusion reason for non-included attachments ("" when
    /// included): "unsupported-inline v1" for binary, "pdf-extraction
    /// failed", …
    pub note: String,
}

/// The attachment read model the chat UI renders (chips: filename + kind +
/// flags). camelCase on the wire (Tauri 2 convention).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAttachment {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub digest: String,
    pub size_bytes: u64,
    pub included: bool,
    pub truncated: bool,
    pub note: String,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way an attachment lands — a
    /// `chat.attachment_added` event, actor=user. The shell has already
    /// read, classified, digested, and stored the file; this event records
    /// the ref and the honest inclusion flags.
    pub fn chat_attachment_added(payload: AttachmentAddedPayload) -> Result<Self, EventError> {
        if payload.chat_id.trim().is_empty() {
            return Err(EventError::Invalid(
                "chat.attachment_added: chat_id must not be empty".into(),
            ));
        }
        if payload.name.trim().is_empty() {
            return Err(EventError::Invalid(
                "chat.attachment_added: name must not be empty — an attachment names its file"
                    .into(),
            ));
        }
        if payload.digest.trim().is_empty() {
            return Err(EventError::Invalid(
                "chat.attachment_added: digest must not be empty — an attachment is stored by its digest-addressed ref"
                    .into(),
            ));
        }
        Self::new(
            CHAT_ATTACHMENT_ADDED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )
    }

    /// Typed constructor (AD-15): the one way an attachment leaves — a
    /// `chat.attachment_removed` event, actor=user. The stored copy stays
    /// (digest-addressed, deduplicated); the conversation stops carrying it.
    pub fn chat_attachment_removed(
        chat_id: &str,
        attachment_id: Uuid,
    ) -> Result<Self, EventError> {
        if chat_id.trim().is_empty() {
            return Err(EventError::Invalid(
                "chat.attachment_removed: chat_id must not be empty".into(),
            ));
        }
        Ok(Self::new(
            CHAT_ATTACHMENT_REMOVED,
            Actor::User,
            serde_json::json!({ "chat_id": chat_id.trim(), "attachment_id": attachment_id }),
        )?)
    }
}

/// A chat's active attachments, folded from the log in `seq` order: added
/// minus removed (removal is un-eventing, not deletion — AD-1).
pub fn folded_attachments(events: &[StoredEvent], chat_id: &str) -> Vec<ChatAttachment> {
    let chat_id = chat_id.trim();
    let mut removed: Vec<Uuid> = Vec::new();
    let mut out: Vec<ChatAttachment> = Vec::new();
    for event in events {
        if event_chat_id(event) != Some(chat_id) {
            continue;
        }
        match event.kind.as_str() {
            CHAT_ATTACHMENT_ADDED => {
                let Ok(payload) =
                    serde_json::from_value::<AttachmentAddedPayload>(event.payload.clone())
                else {
                    continue; // a corrupt payload never breaks the chat read
                };
                out.push(ChatAttachment {
                    id: payload.attachment_id,
                    name: payload.name,
                    kind: payload.kind,
                    digest: payload.digest,
                    size_bytes: payload.size_bytes,
                    included: payload.included,
                    truncated: payload.truncated,
                    note: payload.note,
                });
            }
            CHAT_ATTACHMENT_REMOVED => {
                if let Some(id) = event
                    .payload
                    .get("attachment_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
                {
                    removed.push(id);
                }
            }
            _ => {}
        }
    }
    out.retain(|a| !removed.contains(&a.id));
    out
}

/// One attachment's contribution to the provider call (pure assembly input):
/// the extracted text when the attachment rides the call.
#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentContext {
    pub name: String,
    pub kind: AttachmentKind,
    /// The (extracted) text — `None` for stored-only attachments.
    pub text: Option<String>,
    pub truncated: bool,
}

/// Assemble the attachments context block (pure, FR-16.2/16.3): text and
/// extracted PDF text are included verbatim (fenced); binary and failed
/// extractions are flagged by name — never silently dropped. Empty input
/// assembles nothing.
pub fn assemble_attachments_context(items: &[AttachmentContext]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut out = format!("{ATTACHMENTS_MARKER}");
    for a in items {
        let kind = a.kind.as_str();
        match (&a.text, a.kind) {
            (Some(text), _) => {
                let flag = if a.truncated { " (truncado)" } else { "" };
                out.push_str(&format!(
                    "- {} ({}): incluido{flag}\n<<<contenido\n{}\n>>>\n",
                    a.name, kind, text
                ));
            }
            (None, AttachmentKind::Binary) => out.push_str(&format!(
                "- {} (binary): almacenado por referencia — formato no inline en v1 / unsupported-inline v1\n",
                a.name
            )),
            (None, _) => out.push_str(&format!(
                "- {} ({}): almacenado — no se pudo extraer texto ({}), no incluido\n",
                a.name,
                kind,
                if a.truncated { "truncado" } else { "sin texto" }
            )),
        }
    }
    out.push_str(ATTACHMENTS_END);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventstore::EventStore;
    use rusqlite::Connection;
    use serde_json::json;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    // ---- 5.4 scope ----

    #[test]
    fn scoped_events_carry_the_mission_and_the_general_unscope() {
        let mission: Uuid = Uuid::new_v4();
        let ev = NewEvent::chat_scoped("chat-1", Some(mission)).unwrap();
        assert_eq!(ev.kind, "chat.scoped");
        assert_eq!(ev.actor, Actor::User);
        assert_eq!(ev.causes, vec![mission], "scoped events cause-link the mission");
        // General is an event too — the explicit un-scope
        let ev = NewEvent::chat_scoped("chat-1", None).unwrap();
        assert_eq!(ev.payload["mission_id"], serde_json::Value::Null);
        assert!(ev.causes.is_empty());
        // a scope event names its conversation
        assert!(NewEvent::chat_scoped("  ", Some(mission)).is_err());
    }

    #[test]
    fn folded_scope_latest_event_wins() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m1: Uuid = Uuid::new_v4();
        let m2: Uuid = Uuid::new_v4();
        store.append(NewEvent::chat_scoped("c1", Some(m1)).unwrap()).unwrap();
        store.append(NewEvent::chat_scoped("c1", Some(m2)).unwrap()).unwrap();
        store.append(NewEvent::chat_scoped("c2", Some(m1)).unwrap()).unwrap();
        store.append(NewEvent::chat_scoped("c1", None).unwrap()).unwrap();
        let events = store.events_all().unwrap();
        assert_eq!(folded_scope(&events, "c1"), Some(None), "General (the latest) wins");
        assert_eq!(folded_scope(&events, "c2"), Some(Some(m1)));
        assert_eq!(folded_scope(&events, "unknown"), None, "no events ⇒ the table baseline");
    }

    #[test]
    fn folded_skill_latest_event_wins_and_empty_clears() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        store.append(NewEvent::chat_skill_set("c1", "librarian").unwrap()).unwrap();
        store.append(NewEvent::chat_skill_set("c1", "  ").unwrap()).unwrap();
        let events = store.events_all().unwrap();
        assert_eq!(folded_skill(&events, "c1"), Some(String::new()));
        assert_eq!(folded_skill(&events, "other"), None);
    }

    // ---- board context ----

    #[test]
    fn board_context_assembles_hypotheses_statuses_and_pins() {
        let ctx = BoardContext {
            mission_label: "M-2".into(),
            question: "¿Reduce el RAG las citas fabricadas?".into(),
            hypotheses: vec![
                BoardHypothesis {
                    label: "H-1".into(),
                    statement: "El RAG reduce las citas fabricadas".into(),
                    status: "supported".into(),
                    pins: vec![
                        "citation — Vaswani et al. 2017 (confianza 0.8)".into(),
                    ],
                },
                BoardHypothesis {
                    label: "H-2".into(),
                    statement: "El efecto se acota al régimen evaluado".into(),
                    status: "testing".into(),
                    pins: vec![],
                },
            ],
        };
        let block = assemble_board_context(Some(&ctx));
        assert!(block.starts_with(BOARD_CONTEXT_MARKER));
        assert!(block.contains("M-2 «¿Reduce el RAG las citas fabricadas?» sincronizado"));
        assert!(block.contains("- H-1 [supported] El RAG reduce las citas fabricadas"));
        assert!(block.contains("· evidencia: citation — Vaswani et al. 2017 (confianza 0.8)"));
        assert!(block.ends_with(BOARD_CONTEXT_END));
        // General carries no board context at all
        assert_eq!(assemble_board_context(None), "");
        // an empty board still marks the sync honestly
        let empty = assemble_board_context(Some(&BoardContext {
            mission_label: "M-3".into(),
            question: "q".into(),
            hypotheses: vec![],
        }));
        assert!(empty.contains("(sin hipótesis aún)"));
    }

    // ---- attachments ----

    #[test]
    fn attachments_classify_by_extension_with_an_honest_pdf_magic_check() {
        use AttachmentKind::*;
        assert_eq!(classify_attachment("notas.md", b"hola"), Text);
        assert_eq!(classify_attachment("datos.TXT", b"hola"), Text);
        assert_eq!(classify_attachment("main.tex", b"\\doc"), Text);
        assert_eq!(classify_attachment("paper.pdf", b"%PDF-1.7\n..."), Pdf);
        // a .pdf without the %PDF magic is NOT extractable — classify it
        // honestly as binary rather than pretending
        assert_eq!(classify_attachment("fake.pdf", b"not a pdf"), Binary);
        assert_eq!(classify_attachment("figure.png", &[0x89, 0x50]), Binary);
        assert_eq!(classify_attachment("datos.bin", b""), Binary);
    }

    #[test]
    fn attachments_context_includes_text_and_pdf_and_flags_binary() {
        let block = assemble_attachments_context(&[
            AttachmentContext {
                name: "notas.md".into(),
                kind: AttachmentKind::Text,
                text: Some("# Notas\npunto 1".into()),
                truncated: false,
            },
            AttachmentContext {
                name: "paper.pdf".into(),
                kind: AttachmentKind::Pdf,
                text: Some("Texto extraído del pdf.".into()),
                truncated: false,
            },
            AttachmentContext {
                name: "figura.png".into(),
                kind: AttachmentKind::Binary,
                text: None,
                truncated: false,
            },
            AttachmentContext {
                name: "largo.md".into(),
                kind: AttachmentKind::Text,
                text: Some("abc".into()),
                truncated: true,
            },
        ]);
        assert!(block.starts_with(ATTACHMENTS_MARKER));
        assert!(block.contains("- notas.md (text): incluido\n<<<contenido\n# Notas\npunto 1\n>>>"));
        assert!(block.contains("- paper.pdf (pdf): incluido\n<<<contenido\nTexto extraído del pdf."));
        // binary is flagged, never silently dropped
        assert!(block.contains(
            "- figura.png (binary): almacenado por referencia — formato no inline en v1 / unsupported-inline v1"
        ));
        // truncation is visible, never quiet
        assert!(block.contains("- largo.md (text): incluido (truncado)"));
        assert!(block.ends_with(ATTACHMENTS_END));
        assert_eq!(assemble_attachments_context(&[]), "");
    }

    #[test]
    fn attachment_events_fold_added_minus_removed() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let a1 = Uuid::new_v4();
        let a2 = Uuid::new_v4();
        let add = |chat: &str, id: Uuid, name: &str, kind: &str, included: bool| {
            NewEvent::chat_attachment_added(AttachmentAddedPayload {
                chat_id: chat.into(),
                attachment_id: id,
                name: name.into(),
                path: format!("/{name}"),
                kind: kind.into(),
                digest: format!("sha-{id}"),
                size_bytes: 10,
                included,
                truncated: false,
                note: String::new(),
            })
            .unwrap()
        };
        store.append(add("c1", a1, "notas.md", "text", true)).unwrap();
        store.append(add("c1", a2, "fig.png", "binary", false)).unwrap();
        store
            .append(NewEvent::chat_attachment_removed("c1", a1).unwrap())
            .unwrap();
        // another chat's attachments never leak
        store.append(add("c2", Uuid::new_v4(), "otro.md", "text", true)).unwrap();

        let events = store.events_all().unwrap();
        let active = folded_attachments(&events, "c1");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, a2);
        assert_eq!(active[0].name, "fig.png");
        assert!(!active[0].included);
        // constructor guards
        assert!(NewEvent::chat_attachment_added(AttachmentAddedPayload {
            chat_id: " ".into(),
            attachment_id: a1,
            name: "x".into(),
            path: String::new(),
            kind: "text".into(),
            digest: "d".into(),
            size_bytes: 1,
            included: true,
            truncated: false,
            note: String::new(),
        })
        .is_err());
        // a corrupt payload never breaks the fold
        store
            .append(NewEvent::new(CHAT_ATTACHMENT_ADDED, Actor::User, json!({ "chat_id": "c1" })).unwrap())
            .unwrap();
        assert_eq!(folded_attachments(&store.events_all().unwrap(), "c1").len(), 1);
    }
}
