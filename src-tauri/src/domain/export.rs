// Open export at a consistent cut (FR-7.1/7.2, AD-11, Story 3.1): every
// research object renders to open git-friendly files — markdown first,
// JSON where a tool needs it — at ONE named seq cut recorded in the export
// manifest. The exporter is a READER, never a writer of the log (AD-11):
// it folds the same pure projections the app renders, so the exported
// content is fully readable and usable WITHOUT the app (FR-7.2) — the
// no-app test below reads the written folder with std::fs alone.
//
// The single-cut guarantee (AD-11): the cut is the log head at render
// start; every entity's file renders from the SAME captured event slice
// (the folds are pure functions of the slice, so one slice ⇒ one
// consistent position across every entity — never one entity at one head
// and another at a later one). Events appended after the capture cannot
// leak into the render; the single-cut test appends between per-scope
// renders and asserts they never do.
//
// Staleness (Story 2.6's signal, consumed here): the manifest records the
// cut, and `checkpoints::export_is_stale` decides whether a rollback
// appended after the cut changes what that cut means. When the exporter
// re-renders into a folder that already holds a STALE export (a rollback
// orphaned events it rendered), the new manifest and the affected entity
// files carry a visible bilingual STALE marker naming both cuts in mono —
// so a git commit of a pre-rollback export is flagged outside the app.

use std::collections::HashSet;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::checkpoints::{
    export_is_stale, FoldCursor, CHECKPOINT_CREATED, CHECKPOINT_ROLLED_BACK,
};
use crate::domain::digest::render_digest;
use crate::domain::evidence::{Claim, EvidenceProjection, PinKind};
use crate::domain::hypotheses::HypothesesProjection;
use crate::domain::missions::{MissionStatus, MissionsProjection, SpendState};
use crate::domain::nightshift::{RUN_STARTED, SCAN_STEP};
use crate::domain::proposals::ProposalsProjection;
use crate::domain::receipts::{render_receipt, ReceiptAction, RunOutcome};
use crate::domain::search::search_disclosure;
use crate::eventstore::{Actor, EventError, EventStore, StoredEvent};

pub const MANIFEST_FILE: &str = "MANIFEST.md";
/// The machine-greppable cut marker — the line `read_manifest_cut` parses
/// first, and the line anyone (or any tool) can grep outside the app.
pub const CUT_MARKER: &str = "<!-- export-cut:";

pub const SCOPE_ALL: &str = "all";
pub const SCOPE_MISSIONS: &str = "missions";
pub const SCOPE_HYPOTHESES: &str = "hypotheses";
pub const SCOPE_EVIDENCE: &str = "evidence";
pub const SCOPE_TIMELINE: &str = "timeline";
pub const SCOPE_SEARCH_LOG: &str = "search_log";

/// The exporter's scope vocabulary (EXPERIENCE.md's composer): whole
/// workspace, or one entity family at a time.
pub const EXPORT_SCOPES: &[&str] = &[
    SCOPE_ALL,
    SCOPE_MISSIONS,
    SCOPE_HYPOTHESES,
    SCOPE_EVIDENCE,
    SCOPE_TIMELINE,
    SCOPE_SEARCH_LOG,
];

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------------------
// Read models (AD-8) — camelCase on the wire (Tauri 2)
// ---------------------------------------------------------------------------

/// The single named seq cut an export renders at (AD-11): the log head at
/// render start, plus that position's timestamp (`None` for the empty
/// log's `e-0` beginning).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportCut {
    pub seq: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<DateTime<Utc>>,
}

impl ExportCut {
    /// The cut of a captured slice: the max seq (the head) and that
    /// event's timestamp. Pure — the caller owns the slice.
    pub fn of(events: &[StoredEvent]) -> Self {
        match events.last() {
            Some(last) => Self { seq: last.seq, ts: Some(last.ts) },
            None => Self { seq: 0, ts: None },
        }
    }

    /// The mono cut label every file and the manifest stamp: `e-42`.
    pub fn label(&self) -> String {
        format!("e-{}", self.seq)
    }
}

/// Why the previous export at the target folder is stale (AD-11): its cut,
/// and the rollback appended after it that changed what that cut means.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaleNotice {
    pub previous_cut: i64,
    pub rollback_seq: i64,
    pub rollback_ts: DateTime<Utc>,
}

/// The parsed export manifest — the summary the result moment renders and
/// the tests assert on; the on-disk form is `MANIFEST.md` (below).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportManifest {
    pub cut_seq: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cut_ts: Option<DateTime<Utc>>,
    pub rendered_ts: DateTime<Utc>,
    pub scope: String,
    pub app_version: String,
    pub file_count: u32,
    /// The previous export at this folder was stale — this render
    /// supersedes it (both cuts render on the manifest, EXPERIENCE.md).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_notice: Option<StaleNotice>,
}

/// One rendered file: its folder-relative path (forward slashes) and its
/// full text content. Markdown first (FR-7.2); `.jsonl` where a tool
/// needs it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFile {
    pub path: String,
    pub content: String,
}

/// What `export_workspace` did: where it wrote, the manifest summary, and
/// every file it rendered (paths relative to the folder).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOutcome {
    pub dir: String,
    pub manifest: ExportManifest,
    pub files: Vec<String>,
}

/// The "at open" read of an existing export folder (EXPERIENCE.md: the
/// composer's stale-warning state): the cut its manifest records, and
/// whether a rollback after that cut has staled it against the current
/// log (Story 2.6's signal).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportInspect {
    pub cut_seq: i64,
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback_seq: Option<i64>,
}

/// Everything that can go wrong exporting — typed, with stable codes the
/// UI never translates (EXPERIENCE.md).
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("invalid_scope: `{0}` — export scopes are all, missions, hypotheses, evidence, timeline, search_log")]
    InvalidScope(String),
    #[error("io: {0}")]
    Io(String),
    #[error(transparent)]
    Store(#[from] EventError),
}

// ---------------------------------------------------------------------------
// The exporter — a pure render over one captured slice, then one write
// ---------------------------------------------------------------------------

impl From<std::io::Error> for ExportError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// Render an export at ONE cut (AD-11): `events` is the captured slice —
/// the log up to the head at render start; every scope renders from this
/// same slice, so every file shares the manifest's cut by construction.
/// Pure — no store, no IO. `stale_notice` (present when re-rendering over
/// a stale previous export) stamps the visible bilingual STALE marker on
/// the manifest and the affected entity files.
pub fn render_export(
    events: &[StoredEvent],
    scope: &str,
    stale_notice: Option<&StaleNotice>,
    now: DateTime<Utc>,
) -> Result<(ExportManifest, Vec<ExportFile>), ExportError> {
    let cut = ExportCut::of(events);
    let affected =
        stale_notice.map(|n| affected_entities(events, n)).unwrap_or_default();
    let ctx = RenderCtx {
        cut: &cut,
        notice: stale_notice,
        affected: &affected,
        now,
    };
    let mut files = Vec::new();
    for sub in wanted_scopes(scope)? {
        // Every sub-scope folds the SAME slice — the single-cut guarantee
        // (AD-11): appends between these renders (the test does exactly
        // that) cannot leak in, because the slice is already captured.
        files.extend(render_subscope(sub, events, &ctx)?);
    }
    let manifest = ExportManifest {
        cut_seq: cut.seq,
        cut_ts: cut.ts,
        rendered_ts: now,
        scope: scope.to_string(),
        app_version: APP_VERSION.to_string(),
        file_count: files.len() as u32 + 1, // + the manifest itself
        stale_notice: stale_notice.cloned(),
    };
    let manifest_file = manifest_markdown(&manifest, scope);
    files.push(ExportFile { path: MANIFEST_FILE.to_string(), content: manifest_file });
    Ok((manifest, files))
}

/// Render ONE sub-scope's files from the captured slice. Public so the
/// single-cut test can append events to the store BETWEEN renders and
/// assert the captured cut never moves and the new events never leak.
pub fn render_subscope(
    scope: &str,
    events: &[StoredEvent],
    ctx: &RenderCtx<'_, '_>,
) -> Result<Vec<ExportFile>, ExportError> {
    match scope {
        SCOPE_MISSIONS => render_missions(events, ctx),
        SCOPE_HYPOTHESES => render_hypotheses(events, ctx),
        SCOPE_EVIDENCE => render_evidence(events, ctx),
        SCOPE_TIMELINE => render_timeline(events, ctx),
        SCOPE_SEARCH_LOG => render_search_log(events, ctx),
        _ => Err(ExportError::InvalidScope(scope.to_string())),
    }
}

/// The shared render context: the one cut, the stale notice (if any), the
/// entities the noticed rollback affected, and the render clock.
pub struct RenderCtx<'a, 'b> {
    pub cut: &'a ExportCut,
    pub notice: Option<&'b StaleNotice>,
    pub affected: &'a HashSet<String>,
    pub now: DateTime<Utc>,
}

/// Which sub-scopes a requested scope renders.
fn wanted_scopes(scope: &str) -> Result<Vec<&'static str>, ExportError> {
    match scope {
        SCOPE_ALL => Ok(vec![
            SCOPE_MISSIONS,
            SCOPE_HYPOTHESES,
            SCOPE_EVIDENCE,
            SCOPE_TIMELINE,
            SCOPE_SEARCH_LOG,
        ]),
        s if EXPORT_SCOPES.contains(&s) => Ok(vec![match s {
            SCOPE_MISSIONS => SCOPE_MISSIONS,
            SCOPE_HYPOTHESES => SCOPE_HYPOTHESES,
            SCOPE_EVIDENCE => SCOPE_EVIDENCE,
            SCOPE_TIMELINE => SCOPE_TIMELINE,
            _ => SCOPE_SEARCH_LOG,
        }]),
        other => Err(ExportError::InvalidScope(other.to_string())),
    }
}

/// Export the workspace (the command the shell calls): capture the log
/// once (the cut is the head at render start), detect whether the folder
/// already holds a stale previous export (Story 2.6's signal), render
/// every requested scope from the one slice, and write the folder. A
/// READER of the log — nothing is appended, nothing is mutated.
pub fn export_workspace(
    store: &EventStore<'_>,
    dir: &Path,
    scope: &str,
) -> Result<ExportOutcome, ExportError> {
    let events = store.events_all()?;
    let stale_notice = read_manifest_cut(&dir.join(MANIFEST_FILE))
        .and_then(|previous_cut| stale_notice(&events, previous_cut));
    export_events(&events, dir, scope, stale_notice, Utc::now())
}

/// The store-free export core (the no-runtime capability check, FR-7.2):
/// render from a captured slice and write the folder. The same pure folds
/// the app renders — no server, no scheduler, no runtime pieces.
pub fn export_events(
    events: &[StoredEvent],
    dir: &Path,
    scope: &str,
    stale_notice: Option<StaleNotice>,
    now: DateTime<Utc>,
) -> Result<ExportOutcome, ExportError> {
    let (manifest, files) = render_export(events, scope, stale_notice.as_ref(), now)?;
    write_export(dir, &manifest, &files)
}

/// Read an existing export folder at open (EXPERIENCE.md's composer): the
/// cut its manifest records, and whether that cut is stale against the
/// current log. `None` when the folder holds no parseable manifest.
/// Read-only — nothing is written.
pub fn inspect_export(
    store: &EventStore<'_>,
    dir: &Path,
) -> Result<Option<ExportInspect>, ExportError> {
    let Some(cut_seq) = read_manifest_cut(&dir.join(MANIFEST_FILE)) else {
        return Ok(None);
    };
    let events = store.events_all()?;
    Ok(Some(inspect_cut(&events, cut_seq)))
}

/// The staleness read against a current slice (Story 2.6's
/// `export_is_stale`, enriched with the rollback's seq for the marker).
pub fn inspect_cut(events: &[StoredEvent], cut_seq: i64) -> ExportInspect {
    let rollback_seq = events
        .iter()
        .find(|e| e.kind == CHECKPOINT_ROLLED_BACK && e.seq > cut_seq)
        .map(|e| e.seq);
    ExportInspect {
        cut_seq,
        stale: export_is_stale(events, cut_seq),
        rollback_seq,
    }
}

/// Is a previous export at `previous_cut` stale against `events`? When it
/// is, the notice names the first rollback appended after that cut — the
/// event that changed what the cut means (AD-11).
pub fn stale_notice(
    events: &[StoredEvent],
    previous_cut: i64,
) -> Option<StaleNotice> {
    if !export_is_stale(events, previous_cut) {
        return None;
    }
    events
        .iter()
        .find(|e| e.kind == CHECKPOINT_ROLLED_BACK && e.seq > previous_cut)
        .map(|rb| StaleNotice {
            previous_cut,
            rollback_seq: rb.seq,
            rollback_ts: rb.ts,
        })
}

/// Parse the cut a written manifest records. The machine marker first
/// (`<!-- export-cut: 42 -->`), then the human mono line — so the cut is
/// recoverable both by tools and by eye, inside or outside the app.
pub fn read_manifest_cut(manifest_path: &Path) -> Option<i64> {
    let text = std::fs::read_to_string(manifest_path).ok()?;
    parse_manifest_cut(&text)
}

/// The pure parser over manifest text (test surface).
pub fn parse_manifest_cut(text: &str) -> Option<i64> {
    // the machine marker: `<!-- export-cut: 42 -->`
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix(CUT_MARKER) {
            let digits: String =
                rest.trim().trim_end_matches("-->").trim().to_string();
            if let Ok(seq) = digits.parse::<i64>() {
                return Some(seq);
            }
        }
    }
    // the human mono line: - **Cut / Corte:** `e-42`
    for line in text.lines() {
        if line.contains("**Cut / Corte:**") {
            if let Some(start) = line.find('`') {
                if let Some(end) = line[start + 1..].find('`') {
                    let label = &line[start + 1..start + 1 + end];
                    if let Some(digits) = label.strip_prefix("e-") {
                        if let Ok(seq) = digits.parse::<i64>() {
                            return Some(seq);
                        }
                    }
                }
            }
        }
    }
    None
}

/// The entities a stale notice's rollback affected: every id the orphaned
/// events (those the rollback at `notice.rollback_seq` orphaned after
/// `notice.previous_cut`) reference — creation ids, causes, and the
/// payload ids (`mission_id`, `hypothesis_id`, …). Entity files whose id
/// is in this set carry the visible STALE marker on re-render.
fn affected_entities(events: &[StoredEvent], notice: &StaleNotice) -> HashSet<String> {
    const ID_KEYS: &[&str] = &[
        "mission_id",
        "hypothesis_id",
        "claim_id",
        "proposal_id",
        "target_entity",
        "checkpoint_id",
        "run_id",
    ];
    let mut ids = HashSet::new();
    for event in events
        .iter()
        .filter(|e| e.seq > notice.previous_cut && e.seq <= notice.rollback_seq)
    {
        if event.kind == CHECKPOINT_CREATED || event.kind == CHECKPOINT_ROLLED_BACK {
            continue; // bookkeeping — positions in history, not entities
        }
        ids.insert(event.id.to_string());
        for cause in &event.causes {
            ids.insert(cause.to_string());
        }
        for key in ID_KEYS {
            if let Some(id) = event.payload.get(*key).and_then(|v| v.as_str()) {
                ids.insert(id.to_string());
            }
        }
    }
    ids
}

/// Write the rendered files into `dir` (creating it and every subfolder).
/// Files are written in order; the manifest last (the folder is only
/// complete when `MANIFEST.md` lands).
fn write_export(
    dir: &Path,
    manifest: &ExportManifest,
    files: &[ExportFile],
) -> Result<ExportOutcome, ExportError> {
    std::fs::create_dir_all(dir)?;
    let mut written = Vec::new();
    for file in files {
        let path = dir.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, &file.content)?;
        written.push(file.path.clone());
    }
    Ok(ExportOutcome {
        dir: dir.display().to_string(),
        manifest: manifest.clone(),
        files: written,
    })
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

fn scope_label(scope: &str) -> String {
    match scope {
        SCOPE_ALL => "all — the whole workspace / todo el espacio de trabajo".into(),
        SCOPE_MISSIONS => "missions / misiones".into(),
        SCOPE_HYPOTHESES => "hypotheses (the board) / hipótesis (el tablero)".into(),
        SCOPE_EVIDENCE => "evidence pins / anclas de evidencia".into(),
        SCOPE_TIMELINE => "timeline (events, receipts, digest) / cronología (eventos, recibos, resumen)".into(),
        SCOPE_SEARCH_LOG => "search disclosures / divulgación de búsquedas".into(),
        _ => scope.to_string(),
    }
}

fn manifest_markdown(manifest: &ExportManifest, scope: &str) -> String {
    let cut_ts = manifest
        .cut_ts
        .as_ref()
        .map(|ts| ts.to_rfc3339())
        .unwrap_or_else(|| "the empty log's beginning / el inicio del log vacío".into());
    let mut md = format!(
        "# ResearchCore Export — MANIFEST\n\n\
         **Own your research / Tu investigación es tuya.**\n\n\
         - **Cut / Corte:** `e-{cut}` · {cut_ts}\n\
         - **Rendered / Renderizado:** {rendered}\n\
         - **Scope / Alcance:** {scope}\n\
         - **App / Aplicación:** ResearchCore v{version}\n\
         - **Files / Archivos:** {files}\n\n\
         ## Staleness / Desactualización\n\n\
         This export renders the read model at the single seq cut `e-{cut}` — \
         every file in this folder was rendered from the same event-log \
         position (AD-11). It is **stale** if the log later records a rollback \
         after that cut (`checkpoint.rolled_back` with seq > {cut}): re-export \
         to refresh.\n\n\
         Este export renderiza el modelo de lectura en el corte único `e-{cut}` \
         — todos los archivos de esta carpeta se renderizaron desde la misma \
         posición del log. Queda **desactualizado** si el log registra un \
         retroceso posterior a ese corte: re-exporta para refrescar.\n",
        cut = manifest.cut_seq,
        cut_ts = cut_ts,
        rendered = manifest.rendered_ts.to_rfc3339(),
        scope = scope_label(scope),
        version = manifest.app_version,
        files = manifest.file_count,
    );
    if let Some(notice) = &manifest.stale_notice {
        md.push_str(&format!(
            "\n> ⚠ **STALE — rolled back at `e-{rb}` on {rb_ts}; re-export to refresh / \
             DESACTUALIZADO.** The previous export at this folder (cut `e-{prev}`) is \
             stale — the rollback at `e-{rb}` orphaned events it rendered. This \
             re-render supersedes it at cut `e-{cut}`.\n",
            rb = notice.rollback_seq,
            rb_ts = notice.rollback_ts.to_rfc3339(),
            prev = notice.previous_cut,
            cut = manifest.cut_seq,
        ));
    }
    // The machine-greppable cut — `read_manifest_cut` and any outside tool.
    md.push_str(&format!("\n{CUT_MARKER} {} -->\n", manifest.cut_seq));
    md
}

// ---------------------------------------------------------------------------
// Per-scope renderers — markdown first (FR-7.2)
// ---------------------------------------------------------------------------

/// The footer every entity file stamps: the ONE cut it rendered at — the
/// single-cut consistency marker the tests (and any reader) can check.
fn footer(ctx: &RenderCtx<'_, '_>) -> String {
    let ts = ctx
        .cut
        .ts
        .as_ref()
        .map(|t| format!(" · {}", t.to_rfc3339()))
        .unwrap_or_default();
    format!(
        "\n---\n*Rendered at cut / Renderizado en el corte `{}`{} · ResearchCore v{}*\n",
        ctx.cut.label(),
        ts,
        APP_VERSION,
    )
}

/// The visible stale banner for entity files the noticed rollback
/// affected (AD-11): both cuts in mono, bilingual, outside-the-app
/// detectable.
fn stale_banner(ctx: &RenderCtx<'_, '_>) -> String {
    match ctx.notice {
        Some(notice) => format!(
            "> ⚠ **STALE — rolled back at `e-{rb}` on {rb_ts}; re-export to refresh / \
             DESACTUALIZADO.** The previous export at this folder (cut `e-{prev}`) \
             rendered this entity from events that rollback orphaned; this file \
             re-renders the current state at cut `e-{cut}`.\n\n",
            rb = notice.rollback_seq,
            rb_ts = notice.rollback_ts.to_rfc3339(),
            prev = notice.previous_cut,
            cut = ctx.cut.seq,
        ),
        None => String::new(),
    }
}

fn is_affected(ctx: &RenderCtx<'_, '_>, id: &str) -> bool {
    ctx.affected.contains(id)
}

fn mission_status_label(status: MissionStatus) -> &'static str {
    match status {
        MissionStatus::Active => "active",
        MissionStatus::AwaitingReview => "awaiting_review",
        MissionStatus::Completed => "completed",
        MissionStatus::Stopped => "stopped",
        MissionStatus::Failed => "failed",
    }
}

fn spend_state_label(state: SpendState) -> &'static str {
    match state {
        SpendState::Ok => "ok",
        SpendState::Near => "near",
        SpendState::Blocked => "blocked",
    }
}

fn outcome_label(outcome: &RunOutcome) -> &'static str {
    match outcome {
        RunOutcome::Finished => "finished",
        RunOutcome::Failed => "failed",
        RunOutcome::Open => "open",
    }
}

fn actor_label(actor: &Actor) -> String {
    match actor {
        Actor::User => "user".into(),
        Actor::Agent { .. } => "agent".into(),
        Actor::System { component } => {
            format!("system:{}", format!("{component:?}").to_lowercase())
        }
    }
}

/// A filesystem-safe, human-readable slug: lowercase, alphanumerics and
/// dashes, capped (git-friendly names stay scannable in a diff).
fn slug(text: &str) -> String {
    let mut slug = String::new();
    let mut dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !dash && !slug.is_empty() {
            slug.push('-');
            dash = true;
        }
    }
    let trimmed = slug.trim_matches('-').to_string();
    if trimmed.is_empty() {
        return "untitled".into();
    }
    trimmed.chars().take(40).collect()
}

fn render_missions(
    events: &[StoredEvent],
    ctx: &RenderCtx<'_, '_>,
) -> Result<Vec<ExportFile>, ExportError> {
    let missions = MissionsProjection::fold(events)?;
    let mut files = Vec::new();
    for mission in &missions {
        let runs = MissionsProjection::runs_for(events, mission.id);
        let mut md = format!(
            "# M-{seq} · {question}\n\n{stale}\
             - **Status / Estado:** {status}\n\
             - **Created / Creada:** {ts}\n\
             - **Stop condition / Condición de paro:** {stop}\n\
             - **Success criterion / Criterio de éxito:** {success}\n\
             - **Autonomy / Autonomía:** {autonomy}\n\
             - **Spend / Gasto:** {spend}¢ of {ceiling}¢ ({state})\n\
             - **Schedule / Horario:** `{schedule}`\n\n",
            seq = mission.seq,
            question = mission.question,
            stale = if is_affected(ctx, &mission.id.to_string()) {
                stale_banner(ctx)
            } else {
                String::new()
            },
            status = mission_status_label(mission.status),
            ts = mission.ts.to_rfc3339(),
            stop = mission.stop_condition,
            success = mission.success_criterion,
            autonomy = mission.autonomy.as_str(),
            spend = mission.spend_cents,
            ceiling = mission.spend_ceiling_cents,
            state = spend_state_label(mission.spend_state),
            schedule = mission.schedule,
        );
        md.push_str("## Runs / Ejecuciones\n\n");
        if runs.is_empty() {
            md.push_str("_No runs yet / Aún sin ejecuciones._\n");
        }
        for run in &runs {
            let role = run
                .role
                .as_deref()
                .map(|r| format!(" · {r}"))
                .unwrap_or_default();
            md.push_str(&format!(
                "- `e-{seq}` · {ts} · `{kind}` · {actor}{role}\n",
                seq = run.seq,
                ts = run.ts.to_rfc3339(),
                kind = run.kind,
                actor = run.actor,
            ));
        }
        md.push_str(&footer(ctx));
        files.push(ExportFile {
            path: format!(
                "missions/M-{}-{}.md",
                mission.seq,
                slug(&mission.question)
            ),
            content: md,
        });
    }
    Ok(files)
}

/// The shared claims-and-pins section (the hypotheses board and the
/// evidence scope render the same detail — each scope stays standalone).
fn claims_section(claims: &[Claim]) -> String {
    let mut md = String::new();
    if claims.is_empty() {
        md.push_str("_No claims yet / Aún sin afirmaciones._\n");
        return md;
    }
    for claim in claims {
        md.push_str(&format!(
            "\n### Claim `e-{seq}` — {pin_state}\n\n> {text}\n\n",
            seq = claim.seq,
            pin_state = if claim.pinned {
                "pinned / anclada"
            } else {
                "**UNPINNED / SIN ANCLA**"
            },
            text = claim.text,
        ));
        if let Some(pin) = &claim.pin {
            match pin.kind {
                PinKind::Citation => {
                    let label = pin
                        .ref_label
                        .as_deref()
                        .map(|l| format!(" — {l}"))
                        .unwrap_or_default();
                    md.push_str(&format!(
                        "- **Citation / Cita:** ref `{ref_id}`{label}\n  \
                           > {excerpt}\n",
                        ref_id = pin.ref_id.as_deref().unwrap_or("?"),
                        excerpt = pin.excerpt,
                    ));
                }
                PinKind::Numerical => {
                    md.push_str(&format!(
                        "- **Numerical artifact / Artefacto numérico:** `{artifact}`\n",
                        artifact = pin.artifact_ref.as_deref().unwrap_or("?"),
                    ));
                }
            }
            md.push_str(&format!(
                "- **Confidence / Confianza:** {confidence} · assessed by / evaluada por `{model}`\n\
                   - **Digest / Huella:** `sha256:{digest}`\n",
                confidence = pin.confidence,
                model = pin.assessing_model,
                digest = pin.digest,
            ));
        }
    }
    md
}

fn render_hypotheses(
    events: &[StoredEvent],
    ctx: &RenderCtx<'_, '_>,
) -> Result<Vec<ExportFile>, ExportError> {
    let board = HypothesesProjection::fold(events)?;
    let missions = MissionsProjection::fold(events)?;
    let mut files = Vec::new();
    for hyp in &board {
        let mission_label = missions
            .iter()
            .find(|m| m.id == hyp.mission_id)
            .map(|m| format!("M-{} — {}", m.seq, m.question))
            .unwrap_or_else(|| "unknown mission / misión desconocida".into());
        let claims = EvidenceProjection::fold_for(events, hyp.id)?;
        let mut md = format!(
            "# H-{seq} · {statement}\n\n{stale}\
             - **Status / Estado:** {status}\n\
             - **Mission / Misión:** {mission}\n\
             - **Created / Creada:** {ts}\n\
             - **Last audit / Última auditoría:** `e-{auseq}` · {auts} · {auactor} — \"{basis}\"\n\n",
            seq = hyp.seq,
            statement = hyp.statement,
            stale = if is_affected(ctx, &hyp.id.to_string()) {
                stale_banner(ctx)
            } else {
                String::new()
            },
            status = hyp.status.as_str(),
            mission = mission_label,
            ts = hyp.ts.to_rfc3339(),
            auseq = hyp.audit.seq,
            auts = hyp.audit.ts.to_rfc3339(),
            auactor = hyp.audit.actor,
            basis = hyp.audit.basis,
        );
        md.push_str("## Relations / Relaciones\n\n");
        if hyp.relations.is_empty() {
            md.push_str("_None / Ninguna._\n");
        }
        for rel in &hyp.relations {
            md.push_str(&format!(
                "- {kind} ({direction}) → H-{other_seq} · \"{other}\"\n",
                kind = rel.kind.as_str(),
                direction = match rel.direction {
                    crate::domain::hypotheses::RelationDirection::Outgoing => "outgoing",
                    crate::domain::hypotheses::RelationDirection::Incoming => "incoming",
                },
                other_seq = rel.other_seq,
                other = rel.other_statement,
            ));
        }
        md.push_str("\n## Claims & evidence pins / Afirmaciones y anclas de evidencia\n");
        md.push_str(&claims_section(&claims));
        md.push_str(&footer(ctx));
        files.push(ExportFile {
            path: format!("hypotheses/H-{}-{}.md", hyp.seq, slug(&hyp.statement)),
            content: md,
        });
    }
    Ok(files)
}

fn render_evidence(
    events: &[StoredEvent],
    ctx: &RenderCtx<'_, '_>,
) -> Result<Vec<ExportFile>, ExportError> {
    let board = HypothesesProjection::fold(events)?;
    let mut files = Vec::new();
    for hyp in &board {
        let claims = EvidenceProjection::fold_for(events, hyp.id)?;
        let mut md = format!(
            "# Evidence / Evidencia — H-{seq} · {statement}\n\n{stale}\
             - **Hypothesis status / Estado de la hipótesis:** {status}\n",
            seq = hyp.seq,
            statement = hyp.statement,
            stale = if is_affected(ctx, &hyp.id.to_string()) {
                stale_banner(ctx)
            } else {
                String::new()
            },
            status = hyp.status.as_str(),
        );
        md.push_str(&claims_section(&claims));
        md.push_str(&footer(ctx));
        files.push(ExportFile {
            path: format!("evidence/H-{}-{}.md", hyp.seq, slug(&hyp.statement)),
            content: md,
        });
    }
    Ok(files)
}

fn receipt_row_line(row: &crate::domain::receipts::ReceiptRow) -> String {
    let detail = match &row.action {
        ReceiptAction::RunStart { step, schedule } => {
            format!("run start — `{step}` (`{schedule}`)")
        }
        ReceiptAction::Search { query } => format!("search — \"{query}\""),
        ReceiptAction::Call {
            provider,
            model,
            role,
            input_tokens,
            output_tokens,
            cost_cents,
        } => format!(
            "call — `{provider}/{model}`{} · {in_t}→{out_t} tokens · {cost}¢",
            role.as_deref().map(|r| format!(" ({r})")).unwrap_or_default(),
            in_t = input_tokens,
            out_t = output_tokens,
            cost = cost_cents,
        ),
        ReceiptAction::Claim { text } => format!("claim — \"{text}\""),
        ReceiptAction::Proposal {
            proposal_seq, to, status, ..
        } => {
            format!("proposal `pr-{proposal_seq}` → {to} ({status})")
        }
        ReceiptAction::Decision {
            proposal_seq,
            decision,
            ..
        } => format!("decision on `pr-{proposal_seq}` — {decision}"),
        ReceiptAction::Refused {
            scope,
            ceiling_cents,
            would_be_cost_cents,
        } => format!(
            "refused ({scope}) — would be {would}¢ over the {ceiling}¢ ceiling",
            would = would_be_cost_cents,
            ceiling = ceiling_cents,
        ),
        ReceiptAction::Released { reason } => format!("released — {reason}"),
        ReceiptAction::RunEnd {
            outcome,
            reason,
            verdict,
        } => match outcome {
            RunOutcome::Finished => {
                format!("run finished — \"{}\"", verdict.as_deref().unwrap_or("?"))
            }
            RunOutcome::Failed => {
                format!("run failed — {}", reason.as_deref().unwrap_or("?"))
            }
            RunOutcome::Open => "run open".into(),
        },
    };
    format!("- `e-{}` · {} · {}", row.seq, row.ts.to_rfc3339(), detail)
}

fn run_ids(events: &[StoredEvent]) -> Vec<String> {
    let cursor = FoldCursor::over(events);
    cursor
        .live(events)
        .filter(|e| e.kind == RUN_STARTED)
        .filter_map(|e| {
            e.payload
                .get("run_id")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect()
}

fn render_timeline(
    events: &[StoredEvent],
    ctx: &RenderCtx<'_, '_>,
) -> Result<Vec<ExportFile>, ExportError> {
    let mut files = Vec::new();
    // The raw timeline at the cut — one JSON object per line, git-friendly
    // by construction (a diff shows exactly which events landed).
    let mut jsonl = String::new();
    for event in events {
        let line = serde_json::json!({
            "seq": event.seq,
            "ts": event.ts.to_rfc3339(),
            "actor": actor_label(&event.actor),
            "kind": event.kind,
            "payload": event.payload,
            "causes": event.causes,
        });
        jsonl.push_str(&line.to_string());
        jsonl.push('\n');
    }
    files.push(ExportFile {
        path: "timeline/events.jsonl".into(),
        content: jsonl,
    });
    // One receipt per run — the audit ledger, replayed at the cut.
    for run_id in run_ids(events) {
        let Some(receipt) = render_receipt(events, &run_id)? else {
            continue;
        };
        let mut md = format!(
            "# Run receipt / Recibo de ejecución — `{run_id}`\n\n{stale}\
             - **Mission / Misión:** M-{mission_seq}\n\
             - **Outcome / Resultado:** {outcome}\n\
             - **Started / Iniciado:** {started}\n",
            run_id = receipt.run_id,
            stale = if is_affected(ctx, &receipt.run_id) {
                stale_banner(ctx)
            } else {
                String::new()
            },
            mission_seq = receipt.mission_seq,
            outcome = outcome_label(&receipt.outcome),
            started = receipt.started_ts.to_rfc3339(),
        );
        if let Some(ended) = &receipt.ended_ts {
            md.push_str(&format!(
                "- **Ended / Terminó:** {ended} · {dur}s\n",
                ended = ended.to_rfc3339(),
                dur = receipt.duration_secs.unwrap_or(0),
            ));
        }
        if let Some(reason) = &receipt.reason {
            md.push_str(&format!("- **Reason / Razón:** `{reason}`\n",));
        }
        if let Some(verdict) = &receipt.verdict {
            md.push_str(&format!("- **Verdict / Veredicto:** \"{verdict}\"\n"));
        }
        md.push_str(&format!(
            "- **Spend / Gasto:** {spend}¢ of {ceiling}¢\n\
             - **Models / Modelos:** {models}\n",
            spend = receipt.spend_cents,
            ceiling = receipt.ceiling_cents,
            models = receipt
                .models
                .iter()
                .map(|m| format!("`{m}`"))
                .collect::<Vec<_>>()
                .join(", "),
        ));
        md.push_str("\n## Ledger / Libro mayor\n\n");
        for row in &receipt.rows {
            md.push_str(&receipt_row_line(row));
            md.push('\n');
        }
        md.push_str(&footer(ctx));
        files.push(ExportFile {
            path: format!("receipts/{}.md", slug(&receipt.run_id)),
            content: md,
        });
    }
    // The morning digest as rendered at export time.
    let digest = render_digest(events, ctx.now)?;
    let mut md = format!(
        "# Morning Digest / Resumen matutino — at cut / en el corte `{cut}`\n\n",
        cut = ctx.cut.label(),
    );
    md.push_str(&format!(
        "- **Outcome / Resultado:** {}\n\
         - **Spend / Gasto:** {}¢ of {}¢\n",
        match digest.outcome {
            crate::domain::digest::DigestOutcome::NoRuns => "no runs",
            crate::domain::digest::DigestOutcome::AllFinished => "all finished",
            crate::domain::digest::DigestOutcome::PartialSuccess => "partial success",
            crate::domain::digest::DigestOutcome::AllFailed => "all failed",
        },
        digest.spend_cents,
        digest.ceiling_cents,
    ));
    if !digest.rows.is_empty() {
        md.push_str("\n## Missions / Misiones\n\n");
        for row in &digest.rows {
            md.push_str(&format!("- {}\n", row.verdict_line()));
        }
    }
    if !digest.alerts.is_empty() {
        md.push_str("\n## Dead-run alerts / Alertas de ejecuciones muertas\n\n");
        for alert in &digest.alerts {
            md.push_str(&format!(
                "- M-{} · run `{}` · last heartbeat / último latido: {}\n",
                alert.mission_seq,
                alert.run_id,
                alert.heartbeat_ts.to_rfc3339(),
            ));
        }
    }
    if !digest.connection_alerts.is_empty() {
        md.push_str("\n## Connection alerts / Alertas de conexiones\n\n");
        for alert in &digest.connection_alerts {
            md.push_str(&format!(
                "- {} — `{}` since / desde {}\n",
                alert.connection,
                alert.error_code,
                alert.failed_ts.to_rfc3339(),
            ));
        }
    }
    md.push_str(&footer(ctx));
    files.push(ExportFile {
        path: "digest/digest.md".into(),
        content: md,
    });
    // The quarantine's pending + superseded history exports with the run
    // history that produced it (EXPERIENCE.md: never hidden).
    files.extend(render_proposals(events, ctx)?);
    Ok(files)
}

/// The search disclosure (FR-12.1/12.2, Story 4.1): every search the app
/// performed, PRISMA-style, from the dedicated `search.run` events —
/// query, database, filters, date, result count — with null results
/// visibly marked "**0 results / 0 resultados**", never hidden. Pre-4.1
/// logs (scans that predate dedicated search events) fall back to the
/// run-started scan rows, so a review that searched never renders an
/// empty disclosure; the empty note is only for logs that truly never
/// searched. Rendered at the export's single seq cut like every scope.
fn render_search_log(
    events: &[StoredEvent],
    ctx: &RenderCtx<'_, '_>,
) -> Result<Vec<ExportFile>, ExportError> {
    let missions = MissionsProjection::fold(events)?;
    let mut md = format!(
        "# Search disclosure / Divulgación de búsquedas — at cut / en el corte `{cut}`\n\n",
        cut = ctx.cut.label(),
    );
    let disclosure = search_disclosure(events, None)?;
    if !disclosure.rows.is_empty() {
        md.push_str(&format!(
            "Every search the runs and the user performed, PRISMA-style — query, \
             database, filters, date, result count — including null results, \
             logged identically (FR-12.1).\n\n\
             Cada búsqueda que ejecutaron las ejecuciones y la usuaria, estilo \
             PRISMA — consulta, base de datos, filtros, fecha, número de \
             resultados — incluidos los resultados nulos, registrados \
             idénticamente.\n\n\
             **{total} searches / búsquedas · {nulls} null results / resultados nulos**\n\n",
            total = disclosure.total,
            nulls = disclosure.null_result_count,
        ));
        for row in &disclosure.rows {
            let mission_ref = row
                .mission_id
                .and_then(|id| missions.iter().find(|m| m.id == id))
                .map(|m| format!("M-{}", m.seq))
                .unwrap_or_else(|| "—".into());
            let filters = if row.filters.is_empty() {
                "—".to_string()
            } else {
                row.filters
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let order = row.order.clone().unwrap_or_else(|| "default".into());
            let count = if row.null_result {
                "**0 results / 0 resultados** (null / nula)".to_string()
            } else {
                format!("{} results / resultados", row.result_count)
            };
            md.push_str(&format!(
                "- {ts} · e-{seq} · {mission} · \"{query}\" · `{database}` · filters / filtros: \
                 {filters} · order / orden: {order} · {count}\n",
                ts = row.started_at.to_rfc3339(),
                seq = row.seq,
                mission = mission_ref,
                query = row.query,
                database = row.database,
            ));
        }
    } else {
        // Legacy logs (pre-4.1): scans ran without dedicated search
        // events — their run-started scan rows are the honest disclosure,
        // because a review that searched cannot render as empty.
        let cursor = FoldCursor::over(events);
        let mut rows = Vec::new();
        for event in cursor.live(events).filter(|e| e.kind == RUN_STARTED) {
            let (Some(run_id), Some(mission_id)) = (
                event.payload.get("run_id").and_then(|v| v.as_str()),
                event
                    .payload
                    .get("mission_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .or_else(|| event.causes.first().copied()),
            ) else {
                continue;
            };
            let Some(mission) = missions.iter().find(|m| m.id == mission_id) else {
                continue;
            };
            if event.payload.get("step").and_then(|v| v.as_str()) != Some(SCAN_STEP) {
                continue;
            }
            rows.push(format!(
                "- {ts} · M-{seq} · run `{run_id}` · query / consulta: \"{query}\"\n",
                ts = event.ts.to_rfc3339(),
                seq = mission.seq,
                query = mission.question,
            ));
        }
        if rows.is_empty() {
            md.push_str("_No searches recorded / No hay búsquedas registradas._\n");
        } else {
            md.push_str(
                "Searches recorded by the scan step (a log from before dedicated \
                 search events — FR-12.1 made them first-class).\n\n\
                 Búsquedas registradas por el paso de escaneo (un log anterior a \
                 los eventos de búsqueda dedicados).\n\n",
            );
            for row in rows {
                md.push_str(&row);
            }
        }
    }
    md.push_str(&footer(ctx));
    Ok(vec![ExportFile {
        path: "search-log/search-log.md".into(),
        content: md,
    }])
}

// ---------------------------------------------------------------------------
// Proposals (the quarantine read model): pending + superseded history —
// never hidden (EXPERIENCE.md). Rendered with the timeline scope, whose
// run history produced them.
// ---------------------------------------------------------------------------

fn render_proposals(
    events: &[StoredEvent],
    ctx: &RenderCtx<'_, '_>,
) -> Result<Vec<ExportFile>, ExportError> {
    let proposals = ProposalsProjection::fold(events)?;
    let mut files = Vec::new();
    for proposal in &proposals {
        let target = match (&proposal.target_seq, &proposal.target_label) {
            (Some(seq), Some(label)) => format!("H-{seq} — \"{label}\""),
            _ => "unknown target / objetivo desconocido".into(),
        };
        let to = proposal
            .proposed_payload
            .get("to")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let basis = proposal
            .proposed_payload
            .get("basis")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let mut md = format!(
            "# Proposal / Propuesta `pr-{seq}`\n\n{stale}\
             - **Status / Estado:** {status}{superseded}\n\
             - **Target / Objetivo:** {target}\n\
             - **Proposed / Propuesto:** → `{to}`\n\
             - **Basis / Base:** \"{basis}\"\n\
             - **Derived from / Derivada de:** `e-{basis_seq}`{stale_basis}\n\
             - **Run / Ejecución:** `{run_id}`\n",
            seq = proposal.seq,
            stale = if is_affected(ctx, &proposal.id.to_string()) {
                stale_banner(ctx)
            } else {
                String::new()
            },
            status = proposal.status.as_str(),
            superseded = if proposal.orphaned_by_rollback {
                format!(
                    " — **SUPERSEDED — orphaned by rollback at `e-{}` / huérfana por retroceso; \
                     superseded history, never mergeable**",
                    proposal.rolled_back_seq.unwrap_or(0),
                )
            } else {
                String::new()
            },
            target = target,
            to = to,
            basis = basis,
            basis_seq = proposal.basis_seq,
            stale_basis = if proposal.basis_stale {
                " — basis stale / base desactualizada"
            } else {
                ""
            },
            run_id = proposal.run_id,
        );
        if let Some(decided) = &proposal.decided {
            md.push_str(&format!(
                "- **Decided / Decidida:** `e-{seq}` · {ts} · {actor}\n",
                seq = decided.seq,
                ts = decided.ts.to_rfc3339(),
                actor = decided.actor,
            ));
        }
        md.push_str(&footer(ctx));
        files.push(ExportFile {
            path: format!(
                "proposals/pr-{}-{}.md",
                proposal.seq,
                slug(&format!("{to} {basis}"))
            ),
            content: md,
        });
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::hypotheses::HypothesisStatus;
    use crate::domain::missions::{Autonomy, MissionCreatedPayload};
    use crate::domain::proposals::propose_transition;
    use crate::domain::spend::SpendRecordedPayload;
    use crate::eventstore::{EventStore, NewEvent};
    use chrono::TimeZone;
    use rusqlite::Connection;
    use std::path::PathBuf;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rc-export-{tag}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 19, 8, 0, 0).unwrap()
    }

    /// Parse `"seq":N` from a jsonl line — plain string ops, the way an
    /// outside-the-app reader would.
    fn seq_of_line(line: &str) -> Option<i64> {
        let i = line.find("\"seq\":")? + "\"seq\":".len();
        let digits: String =
            line[i..].chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse().ok()
    }

    fn seed_full_workspace(store: &EventStore<'_>) -> (Uuid, Uuid) {
        // the mission's identity is its creation event's id (AD-2)
        let mission = store
            .append(
                NewEvent::mission_created(MissionCreatedPayload {
                    question: "Does sparse attention hold at 32k?".into(),
                    stop_condition: "Stop after $5 of spend.".into(),
                    success_criterion: "A rater agrees the evidence is decisive.".into(),
                    autonomy: Autonomy::Suggest,
                    spend_ceiling_cents: 100,
                    roles: vec![],
                    schedule: "daily-03:00".into(),
                })
                .unwrap(),
            )
            .unwrap()
            .id;
        let hyp = store
            .append(
                NewEvent::hypothesis_created(
                    "Sparse attention retains accuracy at 32k.",
                    mission,
                )
                .unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::hypothesis_status_changed(
                    HypothesisStatus::Proposed,
                    HypothesisStatus::Testing,
                    "the first scans look promising",
                )
                .unwrap()
                .with_causes(vec![hyp.id]),
            )
            .unwrap();
        // a claim + a citation pin + a numerical pin
        let claim = store
            .append(
                NewEvent::claim_registered(
                    "Three 2025 papers report no accuracy loss at 32k.",
                    hyp.id,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::evidence_pinned_citation(
                    claim.id,
                    hyp.id,
                    "ref-42",
                    "Accuracy is retained within 0.3 points at 32k context.",
                    0.82,
                    "gpt-5",
                )
                .unwrap(),
            )
            .unwrap();
        let num_claim = store
            .append(
                NewEvent::claim_registered(
                    "Our rerun matches the reported numbers.",
                    hyp.id,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::evidence_pinned_numerical(
                    num_claim.id,
                    hyp.id,
                    "figures/fig2-accuracy.csv",
                    "context=32768,accuracy_delta=0.27",
                    0.9,
                    "claude-opus-4",
                )
                .unwrap(),
            )
            .unwrap();
        // a night run with spend and a proposal in quarantine
        store
            .append(
                NewEvent::run_started("run-32k-a", mission, "daily-03:00", SCAN_STEP).unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::spend_recorded(SpendRecordedPayload {
                    provider: "simulated".into(),
                    model: "simulated".into(),
                    input_tokens: 1200,
                    output_tokens: 300,
                    cost_cents: 12,
                    mission_id: Some(mission),
                    role: Some("drafter".into()),
                    run_id: Some("run-32k-a".into()),
                })
                .unwrap(),
            )
            .unwrap();
        propose_transition(
            store,
            "run-32k-a",
            hyp.id,
            HypothesisStatus::Supported,
            "three pinned papers agree",
        )
        .unwrap();
        store
            .append(NewEvent::run_finished("run-32k-a", mission, "scan found 3 agreeing papers", 1).unwrap())
            .unwrap();
        (mission, hyp.id)
    }

    #[test]
    fn export_renders_the_workspace_at_one_cut_in_open_files() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let (mission, hyp) = seed_full_workspace(&store);
        let events = store.events_all().unwrap();
        let dir = tmp_dir("full");
        let outcome =
            export_events(&events, &dir, SCOPE_ALL, None, fixed_now()).unwrap();

        // the manifest records the cut, the scope, the app version
        assert_eq!(outcome.manifest.cut_seq, events.last().unwrap().seq);
        assert_eq!(outcome.manifest.scope, SCOPE_ALL);
        assert!(outcome.manifest.app_version.contains(char::is_numeric));
        assert!(outcome.files.contains(&MANIFEST_FILE.to_string()));

        // per-entity git-friendly files exist
        for prefix in [
            "missions/M-1-",
            "hypotheses/H-2-",
            "evidence/H-2-",
            "timeline/events.jsonl",
            "receipts/run-32k-a.md",
            "digest/digest.md",
            "search-log/search-log.md",
            "proposals/pr-",
        ] {
            assert!(
                outcome.files.iter().any(|p| p.starts_with(prefix)),
                "expected a file under {prefix}, got {:?}",
                outcome.files
            );
        }

        // content: the mission file carries the terminator pair + status
        let mission_file = outcome
            .files
            .iter()
            .find(|p| p.starts_with("missions/"))
            .unwrap();
        let mission_md = std::fs::read_to_string(dir.join(mission_file)).unwrap();
        assert!(mission_md.contains("Stop after $5 of spend."));
        assert!(mission_md.contains("A rater agrees the evidence is decisive."));
        assert!(mission_md.contains("**Status / Estado:** active"));

        // content: the hypothesis file carries statement, audit, pins
        let hyp_file = outcome
            .files
            .iter()
            .find(|p| p.starts_with("hypotheses/"))
            .unwrap();
        let hyp_md = std::fs::read_to_string(dir.join(hyp_file)).unwrap();
        assert!(hyp_md.contains("Sparse attention retains accuracy at 32k."));
        assert!(hyp_md.contains("**Status / Estado:** testing"));
        assert!(hyp_md.contains("the first scans look promising"));
        assert!(hyp_md.contains("Accuracy is retained within 0.3 points"));
        assert!(hyp_md.contains("0.82"));
        assert!(hyp_md.contains("`gpt-5`"));
        assert!(hyp_md.contains("figures/fig2-accuracy.csv"));
        assert!(hyp_md.contains("sha256:"));

        // the receipt replays the run's ledger at the cut
        let receipt_md =
            std::fs::read_to_string(dir.join("receipts/run-32k-a.md")).unwrap();
        assert!(receipt_md.contains("run finished"));
        assert!(receipt_md.contains("search"));

        // the search disclosure names the scan's query
        let search_md = std::fs::read_to_string(dir.join("search-log/search-log.md"))
            .unwrap();
        assert!(search_md.contains("Does sparse attention hold at 32k?"));

        // every entity file stamps the SAME cut footer; the raw jsonl's
        // cut consistency is that its last line IS the cut and no line
        // exceeds it
        let cut = events.last().unwrap().seq;
        for path in &outcome.files {
            if path == MANIFEST_FILE {
                continue;
            }
            let text = std::fs::read_to_string(dir.join(path)).unwrap();
            if path == "timeline/events.jsonl" {
                let last = text.lines().last().unwrap();
                assert!(last.contains(&format!("\"seq\":{cut}")));
                assert!(text.lines().all(|l| !seq_of_line(l).is_some_and(|s| s > cut)));
            } else {
                assert!(
                    text.contains(&format!(
                        "cut / Renderizado en el corte `e-{cut}`"
                    )),
                    "{path} misses the single-cut footer"
                );
            }
        }
        let _ = (mission, hyp);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_search_log_renders_the_prisma_disclosure_at_the_cut() {
        // FR-12.1/12.2 (Story 4.1): dedicated `search.run` events render
        // PRISMA-style — query, database, filters, date, count — with
        // null results visibly marked, never hidden, at the single cut.
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let mission_id = seed_full_workspace(&store).0;
        // one search with results, one null — both through the ONE seam
        crate::domain::search::run_search(
            &store,
            &crate::domain::search::SearchParams {
                query: "Does sparse attention hold at 32k?".into(),
                database: "arxiv".into(),
                filters: std::collections::BTreeMap::from([(
                    "from_year".into(),
                    serde_json::json!(2017),
                )]),
                order: Some("relevance".into()),
                first_page: true,
                mission_id: Some(mission_id),
                run_id: Some("nightshift-32k".into()),
            },
        )
        .unwrap();
        crate::domain::search::run_search(
            &store,
            &crate::domain::search::SearchParams {
                query: "qqqq zzzz".into(),
                database: "semantic-scholar".into(),
                filters: Default::default(),
                order: None,
                first_page: true,
                mission_id: Some(mission_id),
                run_id: None,
            },
        )
        .unwrap();
        let events = store.events_all().unwrap();
        let dir = tmp_dir("search-disclosure");
        export_events(&events, &dir, SCOPE_SEARCH_LOG, None, fixed_now()).unwrap();
        let md = std::fs::read_to_string(dir.join("search-log/search-log.md")).unwrap();
        // the PRISMA fields render per row
        assert!(md.contains("\"Does sparse attention hold at 32k?\""));
        assert!(md.contains("`arxiv`"));
        assert!(md.contains("from_year=2017"));
        assert!(md.contains("`semantic-scholar`"));
        // the null result is visible, never hidden
        assert!(md.contains("**0 results / 0 resultados**"));
        // the summary counts both, nulls included
        assert!(md.contains("2 searches / búsquedas"));
        assert!(md.contains("1 null results / resultados nulos"));
        // the disclosure stamps the export's single seq cut
        let cut = events.last().unwrap().seq;
        assert!(md.contains(&format!("cut / Renderizado en el corte `e-{cut}`")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn events_appended_between_renders_never_leak_into_the_export() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        seed_full_workspace(&store);
        // the capture: the cut is the head at render start (AD-11)
        let captured = store.events_all().unwrap();
        let cut = ExportCut::of(&captured);
        let ctx = RenderCtx { cut: &cut, notice: None, affected: &HashSet::new(), now: fixed_now() };

        // appends BETWEEN the per-scope renders — a whole new mission and
        // hypothesis landing after the capture
        let late_mission = store
            .append(
                NewEvent::mission_created(MissionCreatedPayload {
                    question: "A late mission that must never appear.".into(),
                    stop_condition: "late.".into(),
                    success_criterion: "late.".into(),
                    autonomy: Autonomy::Watch,
                    spend_ceiling_cents: 0,
                    roles: vec![],
                    schedule: "off".into(),
                })
                .unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::hypothesis_created(
                    "A late hypothesis that must never appear.",
                    late_mission.id,
                )
                .unwrap(),
            )
            .unwrap();
        assert!(store.head_seq().unwrap() > cut.seq, "the appends must land after the cut");

        // every scope renders from the SAME captured slice — the new
        // events cannot leak, and every file stamps the captured cut
        let mut files = Vec::new();
        for sub in wanted_scopes(SCOPE_ALL).unwrap() {
            files.extend(render_subscope(sub, &captured, &ctx).unwrap());
        }
        let text: String = files
            .iter()
            .flat_map(|f| vec![f.content.clone(), f.path.clone()])
            .collect();
        assert!(
            !text.contains("never appear"),
            "an event appended after the capture leaked into the export"
        );
        for file in &files {
            if file.path == "timeline/events.jsonl" {
                let last = file.content.lines().last().unwrap();
                assert!(last.contains(&format!("\"seq\":{}", cut.seq)));
                assert!(file
                    .content
                    .lines()
                    .all(|l| !seq_of_line(l).is_some_and(|s| s > cut.seq)));
            } else {
                assert!(
                    file.content.contains(&format!("`e-{}`", cut.seq)),
                    "{} misses the cut stamp",
                    file.path
                );
            }
        }
        // the head moved — but the manifest cut stays at the capture
        let (manifest, _) =
            render_export(&captured, SCOPE_ALL, None, fixed_now()).unwrap();
        assert_eq!(manifest.cut_seq, cut.seq);
        assert!(manifest.cut_seq < store.head_seq().unwrap());
    }

    /// FR-7.2 — the no-app readability test. This module reads the export
    /// with std::fs and string parsing ONLY: no EventStore, no folds, no
    /// domain types — exactly what a researcher with nothing but the files
    /// (and git) has.
    #[test]
    fn the_export_is_readable_and_usable_without_the_app() {
        // (setup uses the core, as any export does; the READS below do not)
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        seed_full_workspace(&store);
        let events = store.events_all().unwrap();
        let dir = tmp_dir("no-app");
        export_events(&events, &dir, SCOPE_ALL, None, fixed_now()).unwrap();
        let cut = events.last().unwrap().seq;
        drop(store);
        drop(conn);
        drop(events);

        // --- from here on: std only -------------------------------------
        use std::fs;

        let manifest = fs::read_to_string(dir.join("MANIFEST.md")).unwrap();
        assert!(manifest.contains("**Cut / Corte:**"));
        assert!(manifest.contains(&format!("e-{cut}")));
        assert!(manifest.contains("ResearchCore v"));
        // the cut is machine-recoverable with plain string ops
        let marker = manifest
            .lines()
            .find(|l| l.contains("<!-- export-cut:"))
            .unwrap();
        let cut_parsed: i64 = marker
            .trim_start_matches("<!-- export-cut:")
            .trim_end_matches("-->")
            .trim()
            .parse()
            .unwrap();
        assert_eq!(cut_parsed, cut);

        // every file is open-format text (markdown/jsonl), valid UTF-8
        let mut stack = vec![dir.clone()];
        let mut seen = 0;
        while let Some(current) = stack.pop() {
            for entry in fs::read_dir(&current).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    stack.push(entry.path());
                } else {
                    let path: PathBuf = entry.path();
                    let name = path.file_name().unwrap().to_string_lossy().into_owned();
                    assert!(
                        name.ends_with(".md") || name.ends_with(".jsonl"),
                        "non-open file in the export: {name}"
                    );
                    let text = fs::read_to_string(&path).unwrap(); // UTF-8 or die
                    assert!(!text.trim().is_empty(), "{name} is empty");
                    seen += 1;
                }
            }
        }
        assert!(seen >= 8, "expected the full workspace, saw {seen} files");

        // the research reads without the app: the mission's terminator
        // pair, the hypothesis statement, the pinned excerpt + digest
        let mut all = String::new();
        let mut stack = vec![dir.clone()];
        while let Some(current) = stack.pop() {
            for entry in fs::read_dir(&current).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    stack.push(entry.path());
                } else {
                    all.push_str(&fs::read_to_string(entry.path()).unwrap());
                }
            }
        }
        assert!(all.contains("Stop after $5 of spend."));
        assert!(all.contains("A rater agrees the evidence is decisive."));
        assert!(all.contains("Sparse attention retains accuracy at 32k."));
        assert!(all.contains("Accuracy is retained within 0.3 points"));
        assert!(all.contains("sha256:"));
        assert!(all.contains("Does sparse attention hold at 32k?"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_stale_export_is_flagged_on_re_render_and_a_fresh_one_is_not() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let (_, hyp) = seed_full_workspace(&store);
        let dir = tmp_dir("stale");

        // export at cut C
        let first =
            export_workspace(&store, &dir, SCOPE_ALL).unwrap();
        let cut_c = first.manifest.cut_seq;
        assert!(first.manifest.stale_notice.is_none());

        // a checkpoint, post-cut work, then the rollback
        let cp = store
            .append(NewEvent::checkpoint_created("pre-trial", cut_c).unwrap())
            .unwrap();
        store
            .append(
                NewEvent::hypothesis_status_changed(
                    HypothesisStatus::Testing,
                    HypothesisStatus::Supported,
                    "the pinned evidence is decisive",
                )
                .unwrap()
                .with_causes(vec![hyp]),
            )
            .unwrap();
        crate::domain::checkpoints::rollback(&store, cp.id).unwrap();

        // the pre-rollback export is stale — detectable at open
        let inspected = inspect_export(&store, &dir).unwrap().unwrap();
        assert_eq!(inspected.cut_seq, cut_c);
        assert!(inspected.stale);
        assert!(inspected.rollback_seq.unwrap() > cut_c);

        // the re-render supersedes it: the manifest and the affected
        // entity files carry the visible bilingual STALE marker with both
        // cuts in mono
        let second = export_workspace(&store, &dir, SCOPE_ALL).unwrap();
        let notice = second.manifest.stale_notice.as_ref().unwrap();
        assert_eq!(notice.previous_cut, cut_c);
        assert!(notice.rollback_seq > cut_c);
        let manifest_md = std::fs::read_to_string(dir.join(MANIFEST_FILE)).unwrap();
        assert!(manifest_md.contains("STALE"));
        assert!(manifest_md.contains("DESACTUALIZADO"));
        assert!(manifest_md.contains(&format!("`e-{}`", cut_c)));
        assert!(manifest_md.contains(&format!("`e-{}`", notice.rollback_seq)));

        // the affected entity (the hypothesis the orphaned transition
        // referenced) carries the banner; its file re-renders the
        // post-rollback state (back to `testing`)
        let hyp_file = second
            .files
            .iter()
            .find(|p| p.starts_with("hypotheses/"))
            .unwrap();
        let hyp_md = std::fs::read_to_string(dir.join(hyp_file)).unwrap();
        assert!(hyp_md.contains("STALE"));
        assert!(hyp_md.contains("**Status / Estado:** testing"));

        // and the re-render itself is FRESH: no rollback after its cut
        assert!(!export_is_stale(&store.events_all().unwrap(), second.manifest.cut_seq));
        let third = export_workspace(&store, &dir, SCOPE_ALL).unwrap();
        assert!(third.manifest.stale_notice.is_none());
        // the parse round-trips: the written manifest's cut is readable back
        assert_eq!(parse_manifest_cut(&manifest_md), Some(second.manifest.cut_seq));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scope_filtering_renders_only_the_requested_entities() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        seed_full_workspace(&store);
        let events = store.events_all().unwrap();
        let dir = tmp_dir("scope");
        let outcome =
            export_events(&events, &dir, SCOPE_MISSIONS, None, fixed_now()).unwrap();

        assert!(outcome.files.iter().any(|p| p.starts_with("missions/")));
        for absent in [
            "hypotheses/",
            "evidence/",
            "timeline/",
            "receipts/",
            "digest/",
            "search-log/",
            "proposals/",
        ] {
            assert!(
                !outcome.files.iter().any(|p| p.starts_with(absent)),
                "scope=missions must not render {absent}"
            );
        }
        // an unknown scope is refused with a stable code
        let err = export_events(&events, &dir, "everything", None, fixed_now())
            .unwrap_err();
        assert!(err.to_string().starts_with("invalid_scope:"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn exporting_an_empty_log_renders_an_honest_manifest() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let dir = tmp_dir("empty");
        let outcome = export_workspace(&store, &dir, SCOPE_ALL).unwrap();
        assert_eq!(outcome.manifest.cut_seq, 0);
        assert!(outcome.manifest.cut_ts.is_none());
        // no entity files — only the always-on renders (the empty timeline,
        // the no-runs digest, the no-searches disclosure) + the manifest
        assert_eq!(
            outcome.files,
            vec![
                "timeline/events.jsonl".to_string(),
                "digest/digest.md".to_string(),
                "search-log/search-log.md".to_string(),
                MANIFEST_FILE.to_string(),
            ]
        );
        let manifest_md = std::fs::read_to_string(dir.join(MANIFEST_FILE)).unwrap();
        assert!(manifest_md.contains("`e-0`"));
        assert_eq!(parse_manifest_cut(&manifest_md), Some(0));
        // an empty-log export is never stale
        assert!(!export_is_stale(&store.events_all().unwrap(), 0));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unparseable_previous_manifest_is_not_stale_and_never_crashes() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        seed_full_workspace(&store);
        let dir = tmp_dir("garbage");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MANIFEST_FILE), "not a researchcore manifest").unwrap();
        // the inspect read reports nothing (no parseable cut)
        assert!(inspect_export(&store, &dir).unwrap().is_none());
        // and the export overwrites the garbage without a stale notice
        let outcome = export_workspace(&store, &dir, SCOPE_ALL).unwrap();
        assert!(outcome.manifest.stale_notice.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_manifest_cut_parses_from_both_the_marker_and_the_mono_line() {
        let with_marker = "# x\n\n<!-- export-cut: 42 -->\n";
        assert_eq!(parse_manifest_cut(with_marker), Some(42));
        let mono_only = "# x\n\n- **Cut / Corte:** `e-7` · 2026-01-01\n";
        assert_eq!(parse_manifest_cut(mono_only), Some(7));
        assert_eq!(parse_manifest_cut("nothing here"), None);
    }
}
