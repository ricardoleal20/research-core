// The bundled journal dataset + the venue stats the tier-2 gate consumes
// (Story 6.11, FR-19.2, NFR-1/NFR-7): venue templates are DATA — the
// FR-16.8 skills precedent applied to publishing. The dataset ships as
// `src-tauri/data/journals.toml` and is the ONLY thing the Journal Fit
// Finder (Story 6.12) and the tier-2 "journal-ready" gate read — no cloud
// fetches ever (NFR-1: fully local). Community-extensible by construction:
// a venue is a `[[venue]]` block, a checklist item is a criterion line —
// new venues and disciplines need no code change (NFR-7).
//
// FR-13.2 discipline: this module owns NO event, NO table — the dataset is
// data, verdicts over it are derived projections. The tier-2 computation
// itself lives in `domain/readiness.rs` (the gate's module); this module
// owns the data shape, the loader/validation, and the manuscript-side stats
// (word/figure/citation/statement counts — deterministic, non-LLM scans in
// the FR-20.4 spirit) that the gate consumes as pure inputs, built at the
// command seam like `manuscript::build_scan`.

use serde::{Deserialize, Serialize};
use once_cell::sync::Lazy;
use uuid::Uuid;

use crate::domain::manuscript::Manuscript;
use crate::eventstore::{Actor, EventError, NewEvent, StoredEvent};

pub const JOURNAL_FIT_COMPLETED: &str = "journal.fit_completed";

/// The bundled dataset — parsed once, validated once. A corrupt dataset is
/// a build/shipping bug and fails loudly on first use, never silently.
static VENUES: Lazy<Vec<VenueTemplate>> = Lazy::new(|| {
    parse_venues(include_str!("../../data/journals.toml")).expect(
        "the bundled journal dataset must parse and validate — a corrupt dataset is a shipping bug",
    )
});

// ---------------------------------------------------------------------------
// The data shape (FR-19.2): { name, criteria[] }, each criterion
// machine-checkable or human-only
// ---------------------------------------------------------------------------

/// One structural statement a venue requires (the `statement_present`
/// criterion's parameter): detected in the .tex by a keyword scan — the
/// presence of the section, never its content (that judgment stays human).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatementKind {
    DataAvailability,
    CodeAvailability,
    Ethics,
    Funding,
    ConflictOfInterest,
}

impl StatementKind {
    /// Parse the dataset's wire form.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "data_availability" => Self::DataAvailability,
            "code_availability" => Self::CodeAvailability,
            "ethics" => Self::Ethics,
            "funding" => Self::Funding,
            "conflict_of_interest" => Self::ConflictOfInterest,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::DataAvailability => "data_availability",
            Self::CodeAvailability => "code_availability",
            Self::Ethics => "ethics",
            Self::Funding => "funding",
            Self::ConflictOfInterest => "conflict_of_interest",
        }
    }

    /// The case-insensitive keyword fragments a .tex source must contain
    /// for the statement's section to count as present.
    fn keywords(self) -> &'static [&'static str] {
        match self {
            Self::DataAvailability => &["data availability", "availability of data"],
            Self::CodeAvailability => &["code availability", "software availability"],
            Self::Ethics => &["ethics approval", "ethical approval"],
            Self::Funding => &["funding", "acknowledg"],
            Self::ConflictOfInterest => &["conflict of interest", "competing interests"],
        }
    }
}

/// The bibliography reference style a venue requires (the
/// `reference_style` criterion's parameter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceStyle {
    Numeric,
    AuthorYear,
}

impl ReferenceStyle {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "numeric" => Self::Numeric,
            "author_year" => Self::AuthorYear,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Numeric => "numeric",
            Self::AuthorYear => "author_year",
        }
    }
}

/// One machine-checkable criterion (the closed vocabulary the tier-2 gate
/// computes; `[OWNER DECISION: seeded venues + generic item checks]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MachineCheck {
    /// The manuscript-board consistency check is clean (FR-20.4).
    ManuscriptConsistency,
    /// Every pinned claim in scope carries a `supported` support check
    /// (FR-23 — tier-2 requires support-verified claims).
    LoadBearingSupport,
    /// Every `\cite` key resolves: an entry in the repo's .bib AND a
    /// matching library reference (by DOI or arXiv id).
    ReferencesResolved,
    /// A compiled PDF exists (last compile outcome ok).
    CompiledPdf,
    /// A structural statement's section is present.
    StatementPresent { statement: StatementKind },
    /// The abstract's word count within the limit.
    AbstractWithinWords { max: u32 },
    /// The main text's total word count within the limit.
    MainTextWithinWords { max: u32 },
    /// The `\includegraphics` count within the limit.
    FiguresWithin { max: u32 },
    /// The compiled PDF's page count (from the compile log) within the
    /// limit.
    PagesWithin { max: u32 },
    /// The detected bibliography style matches.
    ReferenceStyle { style: ReferenceStyle },
}

impl MachineCheck {
    /// The stable code the wire and the dataset share.
    pub fn code(self) -> &'static str {
        match self {
            Self::ManuscriptConsistency => "manuscript_consistency",
            Self::LoadBearingSupport => "load_bearing_support",
            Self::ReferencesResolved => "references_resolved",
            Self::CompiledPdf => "compiled_pdf",
            Self::StatementPresent { .. } => "statement_present",
            Self::AbstractWithinWords { .. } => "abstract_within_words",
            Self::MainTextWithinWords { .. } => "main_text_within_words",
            Self::FiguresWithin { .. } => "figures_within",
            Self::PagesWithin { .. } => "pages_within",
            Self::ReferenceStyle { .. } => "reference_style",
        }
    }

    /// The code-form parameter the wire renders (mono, bilingual-safe by
    /// construction — the support `verdict_line` precedent).
    pub fn param(self) -> Option<String> {
        match self {
            Self::ManuscriptConsistency
            | Self::LoadBearingSupport
            | Self::ReferencesResolved
            | Self::CompiledPdf => None,
            Self::StatementPresent { statement } => Some(statement.as_str().to_string()),
            Self::AbstractWithinWords { max }
            | Self::MainTextWithinWords { max }
            | Self::FiguresWithin { max }
            | Self::PagesWithin { max } => Some(max.to_string()),
            Self::ReferenceStyle { style } => Some(style.as_str().to_string()),
        }
    }

    /// Parse from the wire pair (`code`, `param`) — the inverse of
    /// `code()`/`param()`. The param is required where the check takes
    /// one; a check that does not parse is refused (the loader validates
    /// the dataset through this same edge).
    pub fn parse_code(code: &str, param: Option<&str>) -> Option<Self> {
        Some(match code {
            "manuscript_consistency" => Self::ManuscriptConsistency,
            "load_bearing_support" => Self::LoadBearingSupport,
            "references_resolved" => Self::ReferencesResolved,
            "compiled_pdf" => Self::CompiledPdf,
            "statement_present" => Self::StatementPresent {
                statement: StatementKind::parse(param?)?,
            },
            "abstract_within_words" => Self::AbstractWithinWords {
                max: param?.parse().ok()?,
            },
            "main_text_within_words" => Self::MainTextWithinWords {
                max: param?.parse().ok()?,
            },
            "figures_within" => Self::FiguresWithin {
                max: param?.parse().ok()?,
            },
            "pages_within" => Self::PagesWithin {
                max: param?.parse().ok()?,
            },
            "reference_style" => Self::ReferenceStyle {
                style: ReferenceStyle::parse(param?)?,
            },
            _ => return None,
        })
    }

    /// Build from the dataset's raw fields into the wire pair
    /// (code, param); the param is required where the check takes one.
    fn from_parts(
        kind: &str,
        statement: Option<&str>,
        max: Option<u32>,
        style: Option<&str>,
    ) -> Option<(String, Option<String>)> {
        let (check, param) = match kind {
            "manuscript_consistency" => (Self::ManuscriptConsistency, None),
            "load_bearing_support" => (Self::LoadBearingSupport, None),
            "references_resolved" => (Self::ReferencesResolved, None),
            "compiled_pdf" => (Self::CompiledPdf, None),
            "statement_present" => (
                Self::StatementPresent {
                    statement: StatementKind::parse(statement?)?,
                },
                statement.map(str::to_string),
            ),
            "abstract_within_words" => (
                Self::AbstractWithinWords { max: max? },
                max.map(|m| m.to_string()),
            ),
            "main_text_within_words" => (
                Self::MainTextWithinWords { max: max? },
                max.map(|m| m.to_string()),
            ),
            "figures_within" => (
                Self::FiguresWithin { max: max? },
                max.map(|m| m.to_string()),
            ),
            "pages_within" => (
                Self::PagesWithin { max: max? },
                max.map(|m| m.to_string()),
            ),
            "reference_style" => (
                Self::ReferenceStyle {
                    style: ReferenceStyle::parse(style?)?,
                },
                style.map(str::to_string),
            ),
            _ => return None,
        };
        Some((check.code().to_string(), param))
    }
}

/// One venue-template criterion: machine-checkable (the gate computes
/// pass/fail) or human-only (rendered flagged for the researcher — never
/// auto-passed, never agent-checkable, FR-19.2). The wire shape is flat —
/// `check` is the machine check's stable code, `detail` its code-form
/// param (mono, bilingual-safe by construction).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VenueCriterion {
    /// The stable criterion id (machine: the check's code, param-qualified
    /// for statement checks; human: the dataset's `id` — the submission
    /// checklist keys items on it, Story 6.13).
    pub id: String,
    /// True = human-only: never auto-passed, never silently skipped.
    pub human: bool,
    /// The machine check's code (present iff `!human`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    /// The check's code-form param (the limit, the statement kind, the
    /// style).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Human-only: the bilingual label (the dataset owns the copy — the
    /// item is data, so is its label).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_en: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_es: Option<String>,
}

/// One venue template: the identity, the scope tags (the Fit Finder's
/// matching vocabulary, Story 6.12), and the criteria (FR-19.2's
/// `{name, criteria[]}` — data, community-extensible, NFR-7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VenueTemplate {
    pub id: String,
    pub name: String,
    pub family: String,
    pub scope: Vec<String>,
    pub description_en: String,
    pub description_es: String,
    pub criteria: Vec<VenueCriterion>,
}

impl VenueTemplate {
    pub fn machine_count(&self) -> u32 {
        self.criteria.iter().filter(|c| !c.human).count() as u32
    }

    pub fn human_count(&self) -> u32 {
        self.criteria.iter().filter(|c| c.human).count() as u32
    }
}

/// Every seeded venue, in dataset order — the one read of the bundled
/// data (parsed + validated once; see `VENUES`).
pub fn venues() -> &'static [VenueTemplate] {
    &VENUES
}

/// The venue with this id — `None` when the dataset carries no such id
/// (the command surfaces `unknown_venue:`).
pub fn venue(id: &str) -> Option<&'static VenueTemplate> {
    VENUES.iter().find(|v| v.id == id.trim())
}

// ---------------------------------------------------------------------------
// Dataset parsing + validation (the loader is the dataset's whole edge)
// ---------------------------------------------------------------------------

/// The raw `[[venue]]` block as it lies in the toml (params are optional
/// per criterion kind; validation pins the combination).
#[derive(Debug, Deserialize)]
struct VenueRaw {
    id: String,
    name: String,
    family: String,
    #[serde(default)]
    scope: Vec<String>,
    #[serde(default)]
    description_en: String,
    #[serde(default)]
    description_es: String,
    #[serde(default)]
    criteria: Vec<CriterionRaw>,
}

#[derive(Debug, Deserialize)]
struct CriterionRaw {
    kind: String,
    #[serde(default)]
    statement: Option<String>,
    #[serde(default)]
    max: Option<u32>,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    label_en: Option<String>,
    #[serde(default)]
    label_es: Option<String>,
}

// ---------------------------------------------------------------------------
// The Fit Finder's record (Story 6.12, FR-19.3): the advisory ranking is
// EVENTED — an agent run with model attribution and cited rationale —
// and the choice lands as a REVIEWABLE PROPOSAL (quarantine, AD-3). No
// autonomous submission action exists anywhere (PRD §10): approving the
// fit's proposal is the only way the ranking becomes a submission
// mission, and that merge is the human's.
// ---------------------------------------------------------------------------

/// One ranked candidate as the fit run recorded it (camelCase on the
/// wire, AD-8): the venue, the agent's score, the one-line rationale,
/// and the evidence ledger — every `H-{n}`/`CLAIMS-{n}` reference in the
/// rationale that RESOLVES to a board object is cited in `refs`; a
/// reference that resolves nowhere is flagged in `unverified_refs`
/// (FR-3.4: unpinned rationale claims are flagged, never silent).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FitCandidate {
    pub venue_id: String,
    /// The agent's fit score, 0–100.
    pub score: u8,
    pub rationale: String,
    /// The board objects the rationale rests on: "H-4", "CLAIMS-2".
    pub refs: Vec<String>,
    /// Rationale references that resolve to no board object — flagged.
    pub unverified_refs: Vec<String>,
}

/// The `journal.fit_completed` payload (FR-19.3): the evented ranking —
/// the advisory output of one agent run, with the model + provider that
/// produced it (receipt discipline, AD-10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalFitCompletedPayload {
    /// The research mission the fit derived from (None = workspace).
    #[serde(default)]
    pub mission_id: Option<Uuid>,
    pub provider: String,
    pub model: String,
    pub candidates: Vec<FitCandidate>,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `journal.fit_completed`
    /// event comes into being — actor is the proposing agent run (the
    /// ranking's receipt is attributed like every agent output). The
    /// venue ids must exist in the bundled dataset — a candidate that is
    /// not in the dataset is not a candidate (the ranking is over the
    /// bundled data, NFR-1).
    pub fn journal_fit_completed(
        run_id: &str,
        mission_id: Option<Uuid>,
        provider: &str,
        model: &str,
        candidates: &[FitCandidate],
    ) -> Result<Self, EventError> {
        if run_id.trim().is_empty() {
            return Err(EventError::Invalid(
                "journal.run_id must not be empty — a ranking names its run".into(),
            ));
        }
        if provider.trim().is_empty() || model.trim().is_empty() {
            return Err(EventError::Invalid(
                "journal.fit must name its provider and model — receipts are attributed".into(),
            ));
        }
        for candidate in candidates {
            if venue(&candidate.venue_id).is_none() {
                return Err(EventError::Invalid(format!(
                    "unknown_venue: `{}` — a fit candidate must be a bundled dataset venue (NFR-1)",
                    candidate.venue_id
                )));
            }
        }
        let payload = JournalFitCompletedPayload {
            mission_id,
            provider: provider.trim().to_string(),
            model: model.trim().to_string(),
            candidates: candidates.to_vec(),
        };
        let mut causes = Vec::new();
        if let Some(m) = mission_id {
            causes.push(m);
        }
        Ok(Self::new(
            JOURNAL_FIT_COMPLETED,
            Actor::Agent { run_id: run_id.to_string() },
            serde_json::to_value(&payload)?,
        )?
        .with_causes(causes))
    }
}

/// The latest live fit run for a scope, with its proposal (if still
/// pending) — the Fit Finder surfaces this rather than re-ranking
/// silently. `proposals` must be the live proposals fold.
pub fn latest_fit(
    events: &[StoredEvent],
    scope: Option<Uuid>,
) -> Result<Option<JournalFitRun>, EventError> {
    use crate::domain::proposals::ProposalsProjection;
    use crate::domain::submissions::SUBMISSION_CREATED;
    let cursor = crate::domain::checkpoints::FoldCursor::over(events);
    let live = cursor.live_owned(events);
    let proposals = ProposalsProjection::fold(&live)?;
    let mut latest: Option<JournalFitRun> = None;
    for event in &live {
        if event.kind != JOURNAL_FIT_COMPLETED {
            continue;
        }
        let payload: JournalFitCompletedPayload = serde_json::from_value(event.payload.clone())
            .map_err(|e| {
                EventError::Invalid(format!(
                    "corrupt {JOURNAL_FIT_COMPLETED} payload at seq {}: {e}",
                    event.seq
                ))
            })?;
        if payload.mission_id != scope {
            continue;
        }
        latest = Some(JournalFitRun {
            seq: event.seq,
            ts: event.ts,
            run_id: match &event.actor {
                Actor::Agent { run_id } => run_id.clone(),
                _ => String::new(),
            },
            provider: payload.provider,
            model: payload.model,
            candidates: payload.candidates,
            proposal: proposals
                .iter()
                .find(|p| {
                    // The fit's proposal targets the fit event; still
                    // reviewable when pending (or already decided).
                    p.target_entity == event.id && p.proposed_kind == SUBMISSION_CREATED
                })
                .cloned(),
        });
    }
    Ok(latest)
}

/// One recorded fit run as read from the log — the advisory ranking with
/// attribution and its reviewable proposal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalFitRun {
    pub seq: i64,
    pub ts: chrono::DateTime<chrono::Utc>,
    pub run_id: String,
    pub provider: String,
    pub model: String,
    pub candidates: Vec<FitCandidate>,
    /// The reviewable "choose the top venue" proposal (Story 6.12: the
    /// ranking is advisory; the merge IS the human's choice).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<crate::domain::proposals::Proposal>,
}

/// Parse + validate a dataset document. Pure — the unit tests parse
/// hand-built documents; the bundled file goes through the same edge.
fn parse_venues(raw: &str) -> Result<Vec<VenueTemplate>, EventError> {
    let file: toml::Value = raw
        .parse()
        .map_err(|e| EventError::Invalid(format!("journals: the dataset does not parse: {e}")))?;
    let venue_tables: Vec<VenueRaw> = file
        .get("venue")
        .and_then(|v| v.clone().try_into::<Vec<VenueRaw>>().ok())
        .ok_or_else(|| {
            EventError::Invalid("journals: the dataset carries no [[venue]] blocks".into())
        })?;
    let mut venues = Vec::with_capacity(venue_tables.len());
    let mut seen: Vec<String> = Vec::new();
    for raw in venue_tables {
        let id = raw.id.trim().to_string();
        if id.is_empty() {
            return Err(EventError::Invalid(
                "journals: a venue block requires a non-empty `id`".into(),
            ));
        }
        if seen.contains(&id) {
            return Err(EventError::Invalid(format!(
                "journals: duplicate venue id `{id}` — venue ids are stable keys"
            )));
        }
        if raw.name.trim().is_empty() {
            return Err(EventError::Invalid(format!(
                "journals: venue `{id}` requires a non-empty `name`"
            )));
        }
        let mut criteria = Vec::with_capacity(raw.criteria.len());
        let mut seen_criteria: Vec<String> = Vec::new();
        for c in &raw.criteria {
            if c.kind.trim() == "human" {
                let human_id = c.id.as_deref().map(str::trim).unwrap_or("").to_string();
                if human_id.is_empty() {
                    return Err(EventError::Invalid(format!(
                        "journals: venue `{id}` has a human criterion without an `id`"
                    )));
                }
                if c.label_en.as_deref().map(str::trim).unwrap_or("").is_empty()
                    || c.label_es.as_deref().map(str::trim).unwrap_or("").is_empty()
                {
                    return Err(EventError::Invalid(format!(
                        "journals: venue `{id}` human criterion `{human_id}` requires \
                         `label_en` and `label_es` — the item's copy is data"
                    )));
                }
                if seen_criteria.contains(&human_id) {
                    return Err(EventError::Invalid(format!(
                        "journals: venue `{id}` carries duplicate criterion `{human_id}`"
                    )));
                }
                seen_criteria.push(human_id.clone());
                criteria.push(VenueCriterion {
                    id: human_id,
                    human: true,
                    check: None,
                    detail: None,
                    label_en: c.label_en.clone(),
                    label_es: c.label_es.clone(),
                });
                continue;
            }
            let Some((code, detail)) = MachineCheck::from_parts(
                c.kind.trim(),
                c.statement.as_deref(),
                c.max,
                c.style.as_deref(),
            ) else {
                return Err(EventError::Invalid(format!(
                    "journals: venue `{id}` carries a criterion kind `{}` that is neither a \
                     known machine check nor `human`",
                    c.kind.trim()
                )));
            };
            // The stable criterion id: the check's code, param-qualified
            // for statement checks (a venue may require several
            // statements — each is its own checklist item).
            let criterion_id = match detail.as_deref() {
                Some(param) if code == "statement_present" => {
                    format!("statement_present:{param}")
                }
                _ => code.clone(),
            };
            if seen_criteria.contains(&criterion_id) {
                return Err(EventError::Invalid(format!(
                    "journals: venue `{id}` carries duplicate criterion `{criterion_id}`"
                )));
            }
            seen_criteria.push(criterion_id.clone());
            criteria.push(VenueCriterion {
                id: criterion_id,
                human: false,
                check: Some(code),
                detail,
                label_en: None,
                label_es: None,
            });
        }
        if criteria.is_empty() {
            return Err(EventError::Invalid(format!(
                "journals: venue `{id}` carries no criteria — a venue template is its checklist"
            )));
        }
        seen.push(id.clone());
        venues.push(VenueTemplate {
            id,
            name: raw.name.trim().to_string(),
            family: raw.family.trim().to_string(),
            scope: raw.scope,
            description_en: raw.description_en,
            description_es: raw.description_es,
            criteria,
        });
    }
    Ok(venues)
}

// ---------------------------------------------------------------------------
// Venue stats (the manuscript-side pure inputs the tier-2 gate consumes)
// ---------------------------------------------------------------------------

/// One .bib entry as far as the references-resolved check needs it: the
/// key the `\cite`s reference, plus the identifiers that can match a
/// library reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BibEntry {
    pub key: String,
    pub doi: Option<String>,
    pub arxiv: Option<String>,
}

/// The manuscript-side stats of ONE registered manuscript (Story 6.11):
/// deterministic, non-LLM scans — counts and keyword presences, never
/// semantics claims. Built at the command seam (the only place the gate
/// machinery touches the filesystem, the `build_scan` precedent); the
/// tier-2 computation over it is pure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManuscriptVenueStats {
    pub mission_id: uuid::Uuid,
    /// An unreadable manuscript dir carries its error honestly — every
    /// manuscript-dependent check fails with `no_manuscript` semantics,
    /// never silently skipped.
    pub error: Option<String>,
    /// Total whitespace-token word count across the .tex files (the
    /// `scan_text` honesty rule: comments included, never semantics).
    pub words: u32,
    /// The `\begin{abstract}` environment's word count — None when the
    /// manuscript has no abstract environment.
    pub abstract_words: Option<u32>,
    /// The `\includegraphics` count.
    pub figures: u32,
    /// Every `\cite`/`\citep`/`\citet` key, deduped, in first-use order.
    pub cite_keys: Vec<String>,
    /// The repo's .bib entries (every `.bib` file in the manuscript dir).
    pub bib_entries: Vec<BibEntry>,
    /// The structural statements whose sections are present.
    pub statements: Vec<StatementKind>,
    /// The detected bibliography style — None when undetectable.
    pub reference_style: Option<ReferenceStyle>,
}

/// Build the venue stats of a registered manuscript (the builder is the
/// only place the stats touch the filesystem; the checks over them are
/// pure). An unreadable dir carries its error honestly.
pub fn build_venue_stats(ms: &Manuscript) -> ManuscriptVenueStats {
    let root = std::path::Path::new(&ms.dir);
    let mut stats = ManuscriptVenueStats {
        mission_id: ms.mission_id,
        error: None,
        words: 0,
        abstract_words: None,
        figures: 0,
        cite_keys: Vec::new(),
        bib_entries: Vec::new(),
        statements: Vec::new(),
        reference_style: None,
    };
    let Ok(tex_scans) = crate::domain::manuscript::scan_manuscript(root) else {
        stats.error = Some(format!("unreadable manuscript dir `{}`", ms.dir));
        return stats;
    };
    let mut content_all = String::new();
    for scan in &tex_scans {
        let content = crate::domain::manuscript::read_tex_file(root, &scan.path)
            .unwrap_or_default();
        stats.words += scan.words;
        stats.figures += count_figures(&content);
        collect_cite_keys(&content, &mut stats.cite_keys);
        content_all.push('\n');
        content_all.push_str(&content);
    }
    stats.abstract_words = abstract_words(&content_all);
    stats.statements = STATEMENT_KINDS
        .iter()
        .copied()
        .filter(|k| statement_present(&content_all, *k))
        .collect();
    stats.reference_style = detect_reference_style(&content_all);
    stats.bib_entries = scan_bib_entries(root);
    stats
}

/// Every statement kind the detector knows — the `statements` field's
/// vocabulary.
pub const STATEMENT_KINDS: [StatementKind; 5] = [
    StatementKind::DataAvailability,
    StatementKind::CodeAvailability,
    StatementKind::Ethics,
    StatementKind::Funding,
    StatementKind::ConflictOfInterest,
];

/// The abstract environment's word count (pure): the first
/// `\begin{abstract}`…`\end{abstract}` span's whitespace tokens — None
/// when the manuscript has no abstract environment.
pub fn abstract_words(content: &str) -> Option<u32> {
    let start = content.find("\\begin{abstract}")? + "\\begin{abstract}".len();
    let end = start + content[start..].find("\\end{abstract}")?;
    Some(content[start..end].split_whitespace().count() as u32)
}

/// The `\includegraphics` count (pure).
pub fn count_figures(content: &str) -> u32 {
    content.matches("\\includegraphics").count() as u32
}

/// Collect `\cite`/`\citep`/`\citet` keys (pure, deduped, first-use
/// order). Optional `[…]` arguments are skipped; keys split on commas.
pub fn collect_cite_keys(content: &str, out: &mut Vec<String>) {
    for needle in ["\\citep{", "\\citet{", "\\cite{"] {
        let mut from = 0usize;
        while let Some(found) = content[from..].find(needle) {
            let start = from + found + needle.len();
            if let Some(end) = content[start..].find('}').map(|e| start + e) {
                for key in content[start..end].split(',') {
                    let key = key.trim();
                    if !key.is_empty() && !out.iter().any(|k| k == key) {
                        out.push(key.to_string());
                    }
                }
            }
            from = start;
        }
    }
}

/// Is the statement's section present? (pure — a case-insensitive keyword
/// scan; the presence of the section, never its content).
pub fn statement_present(content: &str, kind: StatementKind) -> bool {
    let lower = content.to_lowercase();
    kind.keywords().iter().any(|kw| lower.contains(kw))
}

/// Detect the bibliography style (pure): `natbib` with an `authoryear`
/// option (or biblatex `style=authoryear`) → AuthorYear; `natbib` with
/// `numbers`/no option, biblatex `style=numeric`, or a numeric
/// `\bibliographystyle` → Numeric; undetectable → None (the check fails
/// honestly with `style unknown`, never guessed).
pub fn detect_reference_style(content: &str) -> Option<ReferenceStyle> {
    let lower = content.to_lowercase();
    if lower.contains("natbib") {
        if lower.contains("authoryear") {
            return Some(ReferenceStyle::AuthorYear);
        }
        return Some(ReferenceStyle::Numeric);
    }
    if lower.contains("style=authoryear") || lower.contains("style = authoryear") {
        return Some(ReferenceStyle::AuthorYear);
    }
    if lower.contains("style=numeric") || lower.contains("style = numeric") {
        return Some(ReferenceStyle::Numeric);
    }
    if let Some(pos) = lower.find("\\bibliographystyle") {
        let tail = &lower[pos..];
        let numeric_styles = ["plain", "unsrt", "ieeetr", "siam", "acm", "apalike"];
        if numeric_styles.iter().any(|s| tail.contains(&format!("{{{s}}}"))) {
            return Some(ReferenceStyle::Numeric);
        }
    }
    None
}

/// Parse the PDF's page count from a compile log tail (pure): pdflatex's
/// `Output written on main.pdf (25 pages` line — the honest page count,
/// from the toolchain itself.
pub fn pdf_pages(log_tail: &str) -> Option<u32> {
    let pos = log_tail.find("Output written on")?;
    let tail = &log_tail[pos..];
    let open = tail.find('(')?;
    let after = &tail[open + 1..];
    let digits: String = after
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let pages: u32 = digits.parse().ok()?;
    if pages == 0 {
        None
    } else {
        Some(pages)
    }
}

/// Scan the repo's `.bib` files into entries (pure over each file's
/// content; the walker mirrors `scan_manuscript`'s — hidden directories
/// skipped, deterministic order). An entry needs its key; `doi`/`eprint`
/// match a library reference when present.
fn scan_bib_entries(root: &std::path::Path) -> Vec<BibEntry> {
    let mut entries = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(dir) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in dir.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let path = entry.path();
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if name.to_ascii_lowercase().ends_with(".bib") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    parse_bib(&content, &mut entries);
                }
            }
        }
    }
    entries
}

/// Parse a .bib document's entries: every `@type{key,` opens an entry;
/// inside, `doi = {…}` / `doi = "…"` and `eprint = {…}` set the
/// identifiers. Line-based and forgiving — a malformed entry still yields
/// its key, with None identifiers (an honest unresolved-in-library).
fn parse_bib(content: &str, out: &mut Vec<BibEntry>) {
    let mut current: Option<BibEntry> = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(entry) = current.take() {
            if trimmed.starts_with('}') {
                out.push(entry);
                continue;
            }
            // still inside the entry — look for identifiers
            let entry = Some(match field_value(trimmed, "doi") {
                Some(doi) => BibEntry { doi: Some(doi), ..entry },
                None => match field_value(trimmed, "eprint") {
                    Some(arxiv) => BibEntry { arxiv: Some(arxiv), ..entry },
                    None => entry,
                },
            });
            current = entry;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('@') {
            if let Some((_, key)) = rest.split_once('{') {
                let key = key.trim_end_matches(',').trim().to_string();
                if !key.is_empty() {
                    current = Some(BibEntry {
                        key,
                        doi: None,
                        arxiv: None,
                    });
                }
            }
        }
    }
    if let Some(entry) = current.take() {
        out.push(entry);
    }
}

/// `field = {value}` / `field = "value"` — the field name anchored at the
/// line's start (a leading comma tolerated), so an identifier never
/// matches inside a title.
fn field_value(line: &str, field: &str) -> Option<String> {
    let trimmed = line.trim().trim_start_matches(',');
    if !trimmed.to_lowercase().starts_with(field) {
        return None;
    }
    let after = &trimmed[field.len()..];
    let after = after.trim_start().strip_prefix('=')?.trim_start();
    let value = after
        .strip_prefix('{')
        .and_then(|v| v.split('}').next())
        .or_else(|| after.strip_prefix('"').and_then(|v| v.split('"').next()))?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

// ---------------------------------------------------------------------------
// Tests (NFR-8)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_dataset_parses_and_validates() {
        let venues = venues();
        assert!(venues.len() >= 5, "the seed list covers the founding user's fields");
        let ids: Vec<&str> = venues.iter().map(|v| v.id.as_str()).collect();
        for expected in ["siam-jsc", "siam-sinum", "acm-toms", "physrev-e", "jcp-chem", "jcp-comp"] {
            assert!(ids.contains(&expected), "seed venue `{expected}` missing");
        }
        for venue in venues {
            assert!(!venue.criteria.is_empty());
            assert!(venue.machine_count() >= 5, "each venue carries the machine core");
            assert!(venue.human_count() >= 1, "each venue carries at least one human item");
            // every venue carries the FR-19.2 machine core
            let codes: Vec<&str> = venue
                .criteria
                .iter()
                .filter(|c| !c.human)
                .filter_map(|c| c.check.as_deref())
                .collect();
            for core in ["manuscript_consistency", "load_bearing_support", "references_resolved", "compiled_pdf"] {
                assert!(codes.contains(&core), "{} misses {}", venue.id, core);
            }
            // human items never carry a machine check and always carry labels
            for c in venue.criteria.iter().filter(|c| c.human) {
                assert!(c.check.is_none());
                assert!(c.label_en.as_deref().is_some_and(|s| !s.is_empty()));
                assert!(c.label_es.as_deref().is_some_and(|s| !s.is_empty()));
            }
        }
    }

    #[test]
    fn venue_lookup_finds_and_misses() {
        assert_eq!(venue("siam-jsc").map(|v| v.name.as_str()), Some("SIAM Journal on Scientific Computing"));
        assert!(venue("does-not-exist").is_none());
    }

    #[test]
    fn a_duplicate_venue_id_is_refused() {
        let raw = r#"
[[venue]]
id = "dup"
name = "One"
family = "F"
[[venue.criteria]]
kind = "compiled_pdf"
[[venue]]
id = "dup"
name = "Two"
family = "F"
[[venue.criteria]]
kind = "compiled_pdf"
"#;
        assert!(parse_venues(raw).is_err());
    }

    #[test]
    fn an_unknown_criterion_kind_is_refused() {
        let raw = r#"
[[venue]]
id = "v"
name = "V"
family = "F"
[[venue.criteria]]
kind = "semantic_novelty"
"#;
        let err = parse_venues(raw).unwrap_err().to_string();
        assert!(err.contains("neither a known machine check nor `human`"), "{err}");
    }

    #[test]
    fn a_limit_check_without_its_param_is_refused() {
        let raw = r#"
[[venue]]
id = "v"
name = "V"
family = "F"
[[venue.criteria]]
kind = "abstract_within_words"
"#;
        assert!(parse_venues(raw).is_err());
    }

    #[test]
    fn a_human_item_without_labels_is_refused() {
        let raw = r#"
[[venue]]
id = "v"
name = "V"
family = "F"
[[venue.criteria]]
kind = "human"
id = "ethics"
"#;
        assert!(parse_venues(raw).is_err());
    }

    #[test]
    fn abstract_figures_and_citations_scan_pure() {
        let tex = r#"
\documentclass{article}
\usepackage[numbers]{natbib}
\begin{abstract}
El método converge cuadráticamente.
\end{abstract}
\section{Data availability}
Los datos están disponibles.
\begin{figure}\includegraphics{f1}\end{figure}
\begin{figure}\includegraphics{f2}\end{figure}
Como muestra \cite{smith2020, jones2021} y también \citep{smith2020}.
\bibliographystyle{siam}
"#;
        assert_eq!(abstract_words(tex), Some(4));
        assert_eq!(count_figures(tex), 2);
        let mut keys = Vec::new();
        collect_cite_keys(tex, &mut keys);
        assert_eq!(keys, ["smith2020", "jones2021"]);
        assert!(statement_present(tex, StatementKind::DataAvailability));
        assert!(!statement_present(tex, StatementKind::Ethics));
        assert_eq!(detect_reference_style(tex), Some(ReferenceStyle::Numeric));
    }

    #[test]
    fn an_author_year_preamble_is_detected() {
        let tex = r#"
\usepackage[authoryear]{natbib}
\begin{document}\citep{smith2020}
"#;
        assert_eq!(detect_reference_style(tex), Some(ReferenceStyle::AuthorYear));
    }

    #[test]
    fn an_undetectable_style_is_none_never_guessed() {
        assert_eq!(detect_reference_style("\\begin{document}x\\end{document}"), None);
        assert_eq!(abstract_words("no abstract here"), None);
    }

    #[test]
    fn bib_entries_parse_keys_and_identifiers() {
        let bib = r#"
@article{smith2020,
  author = {Smith, A.},
  doi = {10.1234/example},
}
@misc{jones2021,
  eprint = {2103.05021},
}
@book{plain2022,
  title = {Plain},
}
"#;
        let mut entries = Vec::new();
        parse_bib(bib, &mut entries);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].key, "smith2020");
        assert_eq!(entries[0].doi.as_deref(), Some("10.1234/example"));
        assert_eq!(entries[1].arxiv.as_deref(), Some("2103.05021"));
        assert_eq!(entries[2].doi, None);
        assert_eq!(entries[2].arxiv, None);
    }

    #[test]
    fn pdf_pages_parse_from_a_compile_log_tail() {
        let log = "Latexmk: All targets () are up to date\nOutput written on main.pdf (25 pages, 400000 bytes).\n";
        assert_eq!(pdf_pages(log), Some(25));
        assert_eq!(pdf_pages("no output line"), None);
    }

    #[test]
    fn build_venue_stats_scans_a_tex_repo() {
        let dir = std::env::temp_dir().join(format!("rc-journal-stats-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("main.tex"),
            r#"
\documentclass{article}
\usepackage{natbib}
\begin{abstract}
We show convergence.
\end{abstract}
\section*{Data availability}
Data available.
\section{Intro}
Words here \cite{smith2020} and \includegraphics{fig1}.
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("refs.bib"),
            "@article{smith2020,\n  doi = {10.1/x},\n}\n",
        )
        .unwrap();
        let ms = Manuscript {
            mission_id: uuid::Uuid::new_v4(),
            seq: 1,
            ts: chrono::Utc::now(),
            dir: dir.display().to_string(),
            main_file: "main.tex".into(),
        };
        let stats = build_venue_stats(&ms);
        assert_eq!(stats.error, None);
        assert_eq!(stats.figures, 1);
        assert_eq!(stats.cite_keys, ["smith2020".to_string()]);
        assert_eq!(stats.abstract_words, Some(3));
        assert!(stats.statements.contains(&StatementKind::DataAvailability));
        assert_eq!(stats.reference_style, Some(ReferenceStyle::Numeric));
        assert_eq!(stats.bib_entries.len(), 1);
        assert_eq!(stats.bib_entries[0].doi.as_deref(), Some("10.1/x"));
        assert!(stats.words >= 15, "the honest whitespace-token count of the seeded main.tex");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
