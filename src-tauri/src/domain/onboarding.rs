// Onboarding domain (FR-8, Story 1.9 — the sixty-second first value): one
// pasted arXiv URL (or one library ref via the Zotero connector stub) becomes
// a starter mission plus three hypothesis candidates, automatically.
//
// The flow: (1) the paper is upserted into the refs library (matched by url
// or doi — re-running the same paste never duplicates a ref); (2) three
// hypothesis candidates are generated from the title/abstract THROUGH the
// provider layer (AD-9 — the simulated fallback fires when no key is
// configured, so first value never requires configuration, FR-8.1); (3) a
// starter mission is created with a pre-filled stop condition and falsifiable
// success criterion (AD-12 — non-empty by construction, the typed constructor
// refuses blanks); (4) the candidates become `hypothesis.created` events on
// the mission — the simplest correct board mapping: candidates ARE proposed
// hypotheses (status `proposed` is stamped by the constructor, matching the
// "proposed" chips of the onboarding result moment).
//
// Generated content follows the app's `lang` setting (bilingual EN/ES,
// NFR-6). Errors are honest and bilingual-safe coded — every error string
// leads with a stable code (`invalid_url:`, `fetch_failed:`,
// `generation_failed:`), never a translated sentence (codes are never
// translated, EXPERIENCE.md).
//
// Candidate confidence is the generator's self-assessed confidence, shown in
// the generation receipt and labeled with the assessing model (Story 1.7
// attribution conventions). Persisted confidence lives on evidence pins
// (`evidence.pinned`), never here — a generation-time score is not evidence.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::adapters::providers::simulated as sim;
use crate::adapters::providers::{
    pricing, ChatRequest, Kind, Message, ProviderError, ProviderLayer,
};
use crate::db::Db;
use crate::domain::hypotheses::{HypothesisStatus, HypothesesProjection};
use crate::domain::missions::{
    Autonomy, Mission, MissionCreatedPayload, MissionsProjection,
};
use crate::eventstore::{EventError, EventStore, NewEvent};

use rusqlite::params;
use thiserror::Error;

/// Everything that can go wrong in the first-value flow — typed, coded,
/// bilingual-safe by construction.
#[derive(Debug, Error)]
pub enum OnboardingError {
    #[error("invalid_url: `{0}` — paste a valid arXiv URL (https://arxiv.org/abs/1706.03762)")]
    InvalidUrl(String),
    #[error("fetch_failed: {0}")]
    FetchFailed(String),
    #[error("generation_failed: {0}")]
    Generation(String),
    #[error("not_found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Event(#[from] EventError),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
}

/// Parse a pasted arXiv URL into the bare arXiv id. Accepts
/// `https://arxiv.org/abs/<id>`, `…/pdf/<id>(.pdf)`, the export host, and a
/// bare id (`1706.03762`, `1706.03762v2`, old-style `cs/0601011`).
/// Anything else is `invalid_url:` — honestly, before any fetch.
pub fn parse_arxiv_url(input: &str) -> Result<String, OnboardingError> {
    let s = input.trim();
    let bad = || OnboardingError::InvalidUrl(s.to_string());
    let after_scheme = match s.split_once("://") {
        Some((scheme, rest)) => {
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
                rest
            } else {
                return Err(bad());
            }
        }
        None => s,
    };
    let lower = after_scheme.to_lowercase();
    let id = if lower.starts_with("arxiv.org/")
        || lower.starts_with("www.arxiv.org/")
        || lower.starts_with("export.arxiv.org/")
    {
        let path = after_scheme
            .split_once('/')
            .map(|(_, rest)| rest)
            .unwrap_or("");
        let segments: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        // `abs/<id>` or `pdf/<id>(.pdf)` — the id may itself be old-style
        // (`cs/0601011`), so everything after the kind segment is the id.
        match segments.split_first() {
            Some((kind, rest)) if (*kind == "abs" || *kind == "pdf") && !rest.is_empty() => {
                rest.join("/").trim_end_matches(".pdf").to_string()
            }
            _ => return Err(bad()),
        }
    } else if looks_like_arxiv_id(after_scheme) {
        after_scheme.to_string()
    } else {
        return Err(bad());
    };
    if !valid_id_chars(&id) {
        return Err(bad());
    }
    Ok(id)
}

/// `1706.03762`, `1706.03762v2`, `cs/0601011`, `math.GT/0309136` — the two
/// arXiv id shapes. Version suffixes are tolerated.
fn looks_like_arxiv_id(s: &str) -> bool {
    if s.is_empty() || s.len() > 40 || !valid_id_chars(s) {
        return false;
    }
    // strip a trailing version (…v7)
    let base = match s.rsplit_once('v') {
        Some((b, ver)) if !ver.is_empty() && ver.chars().all(|c| c.is_ascii_digit()) => b,
        _ => s,
    };
    if let Some((head, tail)) = base.split_once('/') {
        // old-style: subject-class/yy mm nn
        return !head.is_empty()
            && !tail.is_empty()
            && head.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
            && tail.chars().all(|c| c.is_ascii_digit());
    }
    // new-style: yy mm .nnnnn
    match base.split_once('.') {
        Some((head, tail)) => {
            head.len() == 4
                && head.chars().all(|c| c.is_ascii_digit())
                && (4..=5).contains(&tail.len())
                && tail.chars().all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

fn valid_id_chars(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '/' | '_'))
}

/// The paper the first-value flow works from — either fetched from arXiv or
/// read from the migrated library (the Zotero connector stub).
#[derive(Debug, Clone, PartialEq)]
pub struct Paper {
    pub title: String,
    pub authors: String,
    pub year: Option<i64>,
    pub venue: String,
    pub doi: String,
    pub url: String,
    pub arxiv_id: String,
    pub abstract_text: Option<String>,
}

/// The generation receipt (mono, honesty — DESIGN.md onboarding result
/// moment): who generated, with which model, and what it cost. Simulated
/// generation says so and costs nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationReceipt {
    pub provider: String,
    pub model: String,
    pub simulated: bool,
    pub cost_cents: u64,
}

/// The paper as the result moment renders it — the library ref id above all
/// (the paste upserted it; the Zotero path matched it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperInfo {
    pub ref_id: String,
    pub arxiv_id: String,
    pub title: String,
    pub authors: String,
    pub year: Option<i64>,
    pub url: String,
}

/// One hypothesis candidate: the created hypothesis (its board identity) plus
/// the generation-time confidence, attributed to the assessing model (Story
/// 1.7 conventions — never anonymous, never "verified").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HypothesisCandidate {
    pub hypothesis_id: Uuid,
    pub seq: i64,
    pub statement: String,
    pub status: HypothesisStatus,
    pub confidence: f64,
    pub assessing_model: String,
}

/// Everything the result moment renders (FR-8.1): the receipt, the paper, the
/// starter mission, and the candidates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirstValueResult {
    pub receipt: GenerationReceipt,
    pub paper: PaperInfo,
    pub mission: Mission,
    pub candidates: Vec<HypothesisCandidate>,
}

/// The assessing-model label for attribution: the provider's model
/// identifier, falling back to the adapter name (the simulated provider
/// attributes to "simulated" — honestly, not to a model it is not).
fn assessing_model(layer: &ProviderLayer) -> String {
    let model = layer.model().trim();
    if model.is_empty() {
        layer.name().to_string()
    } else {
        model.to_string()
    }
}

/// The candidate-generation prompt. Marker constants come from the simulated
/// provider (single source of truth — the simulator derives its answer from
/// them; real providers simply follow the instructions).
fn candidates_prompt(paper: &Paper, lang: &str) -> String {
    let mut system = format!(
        "{marker} de Research Core. Devuelve EXCLUSIVAMENTE JSON válido con la forma \
         {{\"candidates\":[{{\"statement\":\"…\",\"confidence\":0.0}}]}} — exactamente \
         3 candidatos, redactados como afirmaciones falsables y verificables sobre el \
         artículo, sin numerar. {title_marker}{title}{title_end}.",
        marker = sim::CANDIDATES_MARKER,
        title_marker = sim::PAPER_TITLE_MARKER,
        title = paper.title,
        title_end = sim::PAPER_TITLE_END,
    );
    if let Some(abstract_text) = paper.abstract_text.as_deref() {
        let abstract_text = abstract_text.trim();
        if !abstract_text.is_empty() {
            system.push_str(&format!(" Resumen: «{abstract_text}»."));
        }
    }
    system.push_str(&format!(" {}{}.", sim::LANG_MARKER, lang));
    system
}

#[derive(Debug)]
struct CandidateDraft {
    statement: String,
    confidence: f64,
}

/// Generate the candidates THROUGH the provider layer (AD-9): one chat call,
/// the reply parsed leniently (markdown fences stripped, confidence clamped,
/// at most 3 candidates, at least 1 or the flow fails honestly).
async fn generate_candidates(
    layer: &ProviderLayer,
    paper: &Paper,
    lang: &str,
) -> Result<(Vec<CandidateDraft>, u64), OnboardingError> {
    let req = ChatRequest::new(vec![
        Message::system(candidates_prompt(paper, lang)),
        Message::user("Genera los 3 candidatos de hipótesis."),
    ])
    .with_temperature(0.3);
    let resp = layer.chat(req).await?;
    let drafts = parse_candidates(&resp.content)?;
    let cost_cents = if layer.kind() == Kind::Remote {
        pricing::cost_cents(layer.name(), layer.model(), &resp.usage)
    } else {
        0
    };
    Ok((drafts, cost_cents))
}

fn parse_candidates(content: &str) -> Result<Vec<CandidateDraft>, OnboardingError> {
    let trimmed = content.trim();
    // providers often wrap JSON in markdown fences — strip them.
    let body = if trimmed.starts_with("```") {
        trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
    } else {
        trimmed
    };
    let value: serde_json::Value = serde_json::from_str(body).map_err(|e| {
        OnboardingError::Generation(format!("the provider's reply is not valid JSON ({e})"))
    })?;
    let items = value
        .get("candidates")
        .and_then(|c| c.as_array())
        .cloned()
        .or_else(|| value.as_array().cloned())
        .unwrap_or_default();
    let mut drafts = Vec::new();
    for item in items.iter().take(3) {
        let statement = item
            .get("statement")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if statement.is_empty() {
            continue;
        }
        let confidence = item
            .get("confidence")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.5)
            .clamp(0.0, 1.0);
        drafts.push(CandidateDraft { statement, confidence });
    }
    if drafts.is_empty() {
        return Err(OnboardingError::Generation(
            "the provider returned no hypothesis candidates".into(),
        ));
    }
    Ok(drafts)
}

/// The starter mission's payload (AD-12): a pre-filled stop condition and a
/// falsifiable success criterion derived from the paper's topic — the mission
/// is terminable from the first second. Autonomy starts at `suggest` and the
/// ceiling at $1.00 (DESIGN.md onboarding result moment).
fn starter_mission_payload(paper: &Paper, lang: &str) -> MissionCreatedPayload {
    let (question, stop_condition, success_criterion) = if lang == "en" {
        (
            format!("Check the claims in \u{201C}{}\u{201D}", paper.title),
            "Stop after 3 runs or 20 sources reviewed, whichever comes first.".to_string(),
            "Every surviving candidate has at least 3 pinned sources agreeing at \
             70%+ confidence, or it is refuted."
                .to_string(),
        )
    } else {
        (
            format!("Comprueba las afirmaciones de «{}»", paper.title),
            "Detente tras 3 corridas o 20 fuentes revisadas, lo que ocurra primero.".to_string(),
            "Cada candidato que sobreviva tiene al menos 3 anclas de evidencia citadas \
             que coinciden con ≥70 % de confianza, o queda refutado."
                .to_string(),
        )
    };
    MissionCreatedPayload {
        question,
        stop_condition,
        success_criterion,
        autonomy: Autonomy::Suggest,
        spend_ceiling_cents: 100,
    }
}

/// The project a first-run ref lands under: the active project, else the
/// first project (a fresh workspace always has its seeded demo project), else
/// one is created — the paste never fails on library plumbing.
fn ensure_project(conn: &rusqlite::Connection) -> Result<String, OnboardingError> {
    use rusqlite::OptionalExtension;
    let active: Option<String> = conn
        .query_row(
            "SELECT id FROM projects WHERE is_active = 1 LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = active {
        return Ok(id);
    }
    let any: Option<String> = conn
        .query_row(
            "SELECT id FROM projects ORDER BY created_at LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = any {
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO projects(id,name,folder,kind,tags,color,chapter_index,chapter_count,created_at,updated_at,is_active)
         VALUES(?1,'Research','~','paper','','#3B5BDB',1,1,?2,?2,1)",
        params![id, now],
    )?;
    Ok(id)
}

/// Upsert the paper into the refs library: an existing ref with the same url
/// or doi is matched (the paste never duplicates), otherwise the ref is
/// created under the workspace project.
fn upsert_ref(conn: &rusqlite::Connection, paper: &Paper) -> Result<String, OnboardingError> {
    use rusqlite::OptionalExtension;
    let existing = conn
        .query_row(
            "SELECT id FROM refs WHERE (?1 != '' AND url = ?1) OR (?2 != '' AND doi = ?2) LIMIT 1",
            params![paper.url, paper.doi],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let project = ensure_project(conn)?;
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO refs(id,project_id,title,authors,year,venue,doi,url,tags,status,used,citation_count,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'arXiv,onboarding','unread',0,0,?9)",
        params![
            id,
            project,
            paper.title,
            paper.authors,
            paper.year,
            paper.venue,
            paper.doi,
            paper.url,
            now
        ],
    )?;
    Ok(id)
}

/// Read a paper from the migrated library (the Zotero connector stub,
/// FR-8.1's second door): the ref's metadata becomes the paper. Refs carry no
/// abstract, so generation works from the title — the flow is identical.
pub fn paper_from_ref(
    conn: &rusqlite::Connection,
    ref_id: &str,
) -> Result<Paper, OnboardingError> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT title, authors, year, venue, doi, url FROM refs WHERE id = ?1",
        params![ref_id],
        |r| {
            Ok(Paper {
                title: r.get(0)?,
                authors: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                year: r.get(2)?,
                venue: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                doi: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                url: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                arxiv_id: String::new(),
                abstract_text: None,
            })
        },
    )
    .optional()?
    .ok_or_else(|| OnboardingError::NotFound(format!("no ref with id `{ref_id}` in the library")))
}

/// The sixty-second first value (FR-8.1): paper → library ref → candidates
/// through the provider layer → starter mission + candidate hypotheses.
/// Phased so the connection lock is never held across the provider call (the
/// layer locks internally to record spend).
pub async fn run_first_value(
    db: &Db,
    layer: &ProviderLayer,
    paper: Paper,
) -> Result<FirstValueResult, OnboardingError> {
    // Phase 1 (locked): locale + library upsert.
    let (lang, ref_id) = {
        let conn = db.0.lock().await;
        let lang = crate::db::get_setting(&conn, "lang");
        let ref_id = upsert_ref(&conn, &paper)?;
        (lang, ref_id)
    };

    // Phase 2 (provider call — AD-9; never hold the lock across it).
    let model = assessing_model(layer);
    let (drafts, cost_cents) = generate_candidates(layer, &paper, &lang).await?;

    // Phase 3 (locked): mission.created + one hypothesis.created per candidate.
    let (mission, candidates) = {
        let conn = db.0.lock().await;
        let store = EventStore::new(&conn);
        let mission_stored = store
            .append(NewEvent::mission_created(starter_mission_payload(
                &paper, &lang,
            ))?)?;
        let mission = MissionsProjection::fold(&[mission_stored])?
            .pop()
            .expect("the fold of one creation event yields one mission");
        let mut candidates = Vec::new();
        for draft in drafts {
            let stored = store.append(NewEvent::hypothesis_created(draft.statement, mission.id)?)?;
            let hypothesis = HypothesesProjection::fold(&[stored])?
                .pop()
                .expect("the fold of one creation event yields one hypothesis");
            candidates.push(HypothesisCandidate {
                hypothesis_id: hypothesis.id,
                seq: hypothesis.seq,
                statement: hypothesis.statement,
                status: hypothesis.status,
                confidence: draft.confidence,
                assessing_model: model.clone(),
            });
        }
        (mission, candidates)
    };

    Ok(FirstValueResult {
        receipt: GenerationReceipt {
            provider: layer.name().to_string(),
            model,
            simulated: layer.kind() == Kind::Simulated,
            cost_cents,
        },
        paper: PaperInfo {
            ref_id,
            arxiv_id: paper.arxiv_id.clone(),
            title: paper.title.clone(),
            authors: paper.authors.clone(),
            year: paper.year,
            url: paper.url.clone(),
        },
        mission,
        candidates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::providers::ProviderSettings;
    use crate::eventstore::EventStore;
    use rusqlite::Connection;
    use std::sync::Arc;

    /// An in-memory workspace: schema + eventstore, wrapped in the Db handle.
    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::Db::migrate(&conn).unwrap();
        EventStore::init(&conn).unwrap();
        Db(Arc::new(tokio::sync::Mutex::new(conn)))
    }

    /// The simulated layer the way first run resolves it: legacy auto mode
    /// with no key configured — the fallback keeps first value working
    /// (FR-8.1: zero configuration beyond the paste).
    fn sim_layer(db: &Db) -> ProviderLayer {
        ProviderLayer::from_settings(
            db,
            ProviderSettings {
                mode: String::new(),
                name: "openai".into(),
                base_url: String::new(),
                api_key: String::new(),
                model: String::new(),
                cli: String::new(),
                cli_model: String::new(),
            },
        )
        .unwrap()
    }

    fn paper() -> Paper {
        Paper {
            title: "Attention Is All You Need".into(),
            authors: "Vaswani et al.".into(),
            year: Some(2017),
            venue: "arXiv".into(),
            doi: "10.48550/arXiv.1706.03762".into(),
            url: "https://arxiv.org/abs/1706.03762".into(),
            arxiv_id: "1706.03762".into(),
            abstract_text: Some(
                "The dominant sequence transduction models are based on recurrent networks."
                    .into(),
            ),
        }
    }

    #[tokio::test]
    async fn first_value_from_a_paper_produces_a_mission_and_attributed_candidates() {
        let db = test_db();
        let layer = sim_layer(&db);
        assert_eq!(layer.kind(), Kind::Simulated, "no key => simulated fallback");

        let result = run_first_value(&db, &layer, paper()).await.unwrap();

        // The starter mission passes AD-12: non-empty, falsifiable, bounded.
        let mission = &result.mission;
        assert!(!mission.question.trim().is_empty());
        assert!(
            !mission.stop_condition.trim().is_empty()
                && mission.stop_condition.to_lowercase().contains("detente"),
            "stop condition pre-filled and bounded: {}",
            mission.stop_condition
        );
        assert!(
            mission.success_criterion.contains("70")
                && (mission.success_criterion.contains("refutad")
                    || mission.success_criterion.contains("falsif")),
            "the criterion is falsifiable and countable: {}",
            mission.success_criterion
        );
        assert_eq!(mission.autonomy, Autonomy::Suggest);
        assert_eq!(mission.spend_ceiling_cents, 100);

        // Three candidates, each attributed to the assessing model.
        assert_eq!(result.candidates.len(), 3);
        for candidate in &result.candidates {
            assert!(!candidate.statement.trim().is_empty());
            assert!((0.0..=1.0).contains(&candidate.confidence));
            assert_eq!(candidate.assessing_model, "simulated");
            assert_eq!(candidate.status, HypothesisStatus::Proposed);
        }
        assert!(
            result.candidates.iter().any(|c| c.statement.contains("Attention Is All You Need")),
            "simulated candidates derive from the paper's title"
        );

        // The receipt is honest: simulated says so and costs nothing.
        assert!(result.receipt.simulated);
        assert_eq!(result.receipt.cost_cents, 0);
        assert_eq!(result.receipt.model, "simulated");

        // The log holds exactly what the read models claim (AD-1): the
        // mission and its three cause-linked candidate hypotheses.
        let conn = db.0.lock().await;
        let store = EventStore::new(&conn);
        let events = store.events_all().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|e| e.kind == crate::domain::missions::MISSION_CREATED)
                .count(),
            1
        );
        let on_board =
            HypothesesProjection::fold_for(&events, mission.id).unwrap();
        assert_eq!(on_board.len(), 3);
        for h in &on_board {
            assert!(h.mission_id == mission.id);
            assert!(events.iter().any(|e| e.id == h.id && e.causes.contains(&mission.id)));
        }
        // The paste upserted the paper into the library.
        let ref_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM refs WHERE url = ?1",
                params!["https://arxiv.org/abs/1706.03762"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ref_count, 1);
    }

    #[tokio::test]
    async fn pasting_the_same_paper_twice_never_duplicates_the_ref() {
        let db = test_db();
        let layer = sim_layer(&db);
        let first = run_first_value(&db, &layer, paper()).await.unwrap();
        let second = run_first_value(&db, &layer, paper()).await.unwrap();
        assert_eq!(first.paper.ref_id, second.paper.ref_id);
        let conn = db.0.lock().await;
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM refs", [], |r| r.get(0)).unwrap();
        assert_eq!(total, 1, "the url match reuses the library ref");
        // and the second run is a second mission (the user pasted again)
        assert_ne!(first.mission.id, second.mission.id);
    }

    #[tokio::test]
    async fn generated_content_follows_the_interface_language() {
        let db = test_db();
        {
            let conn = db.0.lock().await;
            crate::db::set_setting(&conn, "lang", "en").unwrap();
        }
        let layer = sim_layer(&db);
        let result = run_first_value(&db, &layer, paper()).await.unwrap();
        assert!(
            result.mission.question.starts_with("Check the claims in"),
            "English interface gets English content: {}",
            result.mission.question
        );
        assert!(
            result.candidates[0].statement.starts_with("The "),
            "English candidates: {}",
            result.candidates[0].statement
        );
    }

    #[test]
    fn non_arxiv_urls_are_typed_invalid_url_errors() {
        for bad in [
            "https://example.com/paper",
            "not a url at all",
            "https://arxiv.org/abs/",
            "ftp://arxiv.org/abs/1706.03762",
        ] {
            let err = parse_arxiv_url(bad)
                .expect_err("a non-arXiv paste must fail before any fetch");
            assert!(
                err.to_string().starts_with("invalid_url:"),
                "{bad:?} => {err}"
            );
        }
    }

    #[test]
    fn arxiv_url_forms_parse_to_the_bare_id() {
        for (input, id) in [
            ("https://arxiv.org/abs/1706.03762", "1706.03762"),
            ("http://arxiv.org/abs/1706.03762v7", "1706.03762v7"),
            ("https://www.arxiv.org/pdf/1706.03762", "1706.03762"),
            ("https://arxiv.org/pdf/1706.03762v2.pdf", "1706.03762v2"),
            ("https://export.arxiv.org/abs/cs/0601011", "cs/0601011"),
            ("  1706.03762  ", "1706.03762"),
            ("cs/0601011", "cs/0601011"),
        ] {
            assert_eq!(parse_arxiv_url(input).unwrap(), id, "{input}");
        }
    }

    #[test]
    fn candidate_parsing_is_lenient_but_honest() {
        // fenced JSON with a bare array is accepted; confidence is clamped
        let drafts = parse_candidates(
            "```json\n[{\"statement\":\"s1\",\"confidence\":1.7},{\"statement\":\"\",\"confidence\":0.5},{\"statement\":\"s2\",\"confidence\":0.4}]\n```",
        )
        .unwrap();
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].confidence, 1.0);
        // no candidates at all is a typed failure
        let err = parse_candidates("{\"candidates\":[]}").unwrap_err();
        assert!(err.to_string().starts_with("generation_failed:"), "{err}");
        // garbage is a typed failure too
        let err = parse_candidates("una respuesta cualquiera").unwrap_err();
        assert!(err.to_string().starts_with("generation_failed:"), "{err}");
    }

    #[test]
    fn the_starter_mission_payload_is_falsifiable_in_both_languages() {
        for (lang, needle) in [("es", "Comprueba las afirmaciones"), ("en", "Check the claims")] {
            let payload = starter_mission_payload(&paper(), lang);
            assert!(payload.question.contains(needle), "{lang}: {}", payload.question);
            assert!(!payload.stop_condition.trim().is_empty());
            assert!(payload.success_criterion.contains("70"), "{lang}");
            // AD-12 is enforced at the typed constructor: blanks never append.
            assert!(NewEvent::mission_created(payload).is_ok());
        }
        let mut blank = starter_mission_payload(&paper(), "es");
        blank.success_criterion = "   ".into();
        assert!(NewEvent::mission_created(blank).is_err());
    }

    #[tokio::test]
    async fn the_zotero_stub_reads_a_library_ref_as_the_paper() {
        let db = test_db();
        let conn = db.0.lock().await;
        // a migrated library ref (the Zotero connector's output)
        conn.execute(
            "INSERT INTO projects(id,name,folder,kind,tags,color,chapter_index,chapter_count,created_at,updated_at,is_active)
             VALUES('p1','Research','~','paper','','#3B5BDB',1,1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO refs(id,project_id,title,authors,year,venue,doi,url,tags,status,used,citation_count,created_at)
             VALUES('r1','p1','Attention Is All You Need','Vaswani et al.',2017,'NeurIPS','10.48550/arXiv.1706.03762','https://arxiv.org/abs/1706.03762','','read',1,2,'2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        let paper = paper_from_ref(&conn, "r1").unwrap();
        assert_eq!(paper.title, "Attention Is All You Need");
        // unknown ref ids are typed not_found errors
        let err = paper_from_ref(&conn, "nope").unwrap_err();
        assert!(err.to_string().starts_with("not_found:"), "{err}");
        drop(conn);

        // the full flow runs from the library ref (the Zotero door)
        let layer = sim_layer(&db);
        let result = run_first_value(&db, &layer, paper).await.unwrap();
        assert_eq!(result.paper.ref_id, "r1", "the library ref is matched, not duplicated");
        assert_eq!(result.candidates.len(), 3);
    }
}
