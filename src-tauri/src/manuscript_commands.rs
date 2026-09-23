// Manuscript shell commands (AD-15a, Stories 6.6–6.8, FR-20): the typed
// core APIs the manuscript surface calls. The repo IS the manuscript —
// reads scan the user's files on disk, writes edit them in place
// (source-control friendly), and every mutation is evented. Error strings
// lead with stable codes (`not_found:`, `unsafe_path:`, `tex_not_found:`,
// `basis_stale:`, `hunk_mismatch:`) and stay in code form — bilingual-safe
// by construction (EXPERIENCE.md).
//
// Manuscript edits are DESKTOP-ONLY (AD-14): the served browser view reads
// the manuscript through the read-only server routes; every mutation here
// has no server counterpart, and the api.ts layer refuses them in the
// served view with the standard bilingual message.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;
use uuid::Uuid;

use crate::db::Db;
use crate::domain::manuscript::{
    self, detect_toolchain, diff_basis_stale, last_compile, scan_manuscript, CompileOutcome,
    CompiledRun, DiffProposal, Manuscript, ManuscriptsProjection, TexFileScan,
};
use crate::eventstore::{EventStore, NewEvent};
use crate::AppPaths;

fn err(e: impl ToString) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------
// Views (camelCase on the wire, AD-8)
// ---------------------------------------------------------------------------

/// The toolchain row: what detection found — None is the honest
/// `tex_not_found` state (never a fake render, NFR-9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolchainView {
    pub name: String,
    pub path: String,
    /// True when the `tex_compiler` setting named this compiler (vs. the
    /// default search order).
    pub configured: bool,
}

/// One compile run as the PDF pane renders it: the outcome, the log tail
/// (receipt voice, mono), and the served PDF url when one exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileView {
    pub seq: i64,
    pub ts: chrono::DateTime<chrono::Utc>,
    pub outcome: CompileOutcome,
    pub tool: Option<String>,
    pub log_tail: String,
    pub pdf_url: Option<String>,
}

/// The manuscript surface's one read: the registration, the scanned file
/// list (word + marker counts), the detected toolchain, and the last
/// compile. None when the mission has no manuscript — progressive
/// disclosure: no manuscript vocabulary until one is registered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManuscriptView {
    pub manuscript: Manuscript,
    pub files: Vec<TexFileScan>,
    pub toolchain: Option<ToolchainView>,
    pub last_compile: Option<CompileView>,
}

/// One manuscript file's content (the editing surface's read).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManuscriptFileView {
    pub path: String,
    pub content: String,
}

// ---------------------------------------------------------------------------
// Shared inners (the read-only server serves the same reads, AD-14)
// ---------------------------------------------------------------------------

/// Where the last compiled PDF of a mission lives (app data, never the
/// user's repo — the app leaves no build noise in the source tree).
pub(crate) fn pdf_path(data_dir: &std::path::Path, mission_id: Uuid) -> PathBuf {
    data_dir.join("manuscripts").join(format!("{mission_id}.pdf"))
}

/// The served PDF url (the in-process server shell, AD-7): the same app
/// in the browser renders the identical PDF the desktop webview does.
pub(crate) fn pdf_url(mission_id: Uuid) -> String {
    format!(
        "http://localhost:{}/api/manuscript/{}/pdf",
        crate::server::port(),
        mission_id
    )
}

fn toolchain_view(conn: &rusqlite::Connection) -> Option<ToolchainView> {
    let preferred = crate::db::get_setting(conn, "tex_compiler");
    let configured = !preferred.trim().is_empty();
    detect_toolchain(if configured {
        Some(preferred.as_str())
    } else {
        None
    })
    .map(|t| ToolchainView {
        name: t.name,
        path: t.path,
        configured,
    })
}

fn compile_view(
    run: &CompiledRun,
    data_dir: &std::path::Path,
) -> CompileView {
    CompileView {
        seq: run.seq,
        ts: run.ts,
        outcome: run.outcome,
        tool: run.tool.clone(),
        log_tail: run.log_tail.clone(),
        pdf_url: if run.outcome == CompileOutcome::Ok
            && pdf_path(data_dir, run.mission_id).is_file()
        {
            Some(pdf_url(run.mission_id))
        } else {
            None
        },
    }
}

/// The manuscript read of one mission: None when nothing is registered.
pub(crate) fn get_manuscript_inner(
    conn: &rusqlite::Connection,
    data_dir: &std::path::Path,
    mission_id: Uuid,
) -> Result<Option<ManuscriptView>, String> {
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let Some(ms) = ManuscriptsProjection::for_mission(&events, mission_id).map_err(err)? else {
        return Ok(None);
    };
    let dir = PathBuf::from(&ms.dir);
    // A dir that vanished from disk scans as empty — the file list renders
    // the honest emptiness, never a crash.
    let files = scan_manuscript(&dir).unwrap_or_default();
    let last = last_compile(&events, mission_id);
    Ok(Some(ManuscriptView {
        toolchain: toolchain_view(conn),
        last_compile: last.map(|run| compile_view(&run, data_dir)),
        manuscript: ms,
        files,
    }))
}

pub(crate) fn read_manuscript_file_inner(
    conn: &rusqlite::Connection,
    mission_id: Uuid,
    path: &str,
) -> Result<ManuscriptFileView, String> {
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let ms = ManuscriptsProjection::for_mission(&events, mission_id)
        .map_err(err)?
        .ok_or_else(|| manuscript::ManuscriptError::NotRegistered(mission_id).to_string())?;
    let content =
        manuscript::read_tex_file(&PathBuf::from(&ms.dir), path).map_err(err)?;
    Ok(ManuscriptFileView {
        path: path.trim().to_string(),
        content,
    })
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Register a mission's manuscript (Story 6.6, FR-20.1): one
/// `manuscript.registered` event referencing the .tex project directory on
/// disk — the files ARE the manuscript, nothing is copied. The dir and
/// main file must exist (validated here, where the filesystem lives).
#[tauri::command]
pub async fn register_manuscript(
    db: State<'_, Db>,
    mission_id: String,
    dir: String,
    main_file: String,
) -> Result<Manuscript, String> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|e| format!("invalid mission id `{mission_id}`: {e}"))?;
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    let events = store.events_all().map_err(err)?;
    if !store_events_have_mission(&events, mission_id) {
        return Err(manuscript::ManuscriptError::MissionNotFound(mission_id).to_string());
    }
    let dir_path = PathBuf::from(dir.trim());
    if !dir_path.is_dir() {
        return Err(manuscript::ManuscriptError::InvalidDir(dir.trim().to_string()).to_string());
    }
    let main = manuscript::safe_file_path(&dir_path, &main_file)
        .map_err(err)?
        .is_file()
        .then(|| main_file.trim().to_string())
        .ok_or_else(|| {
            manuscript::ManuscriptError::InvalidMainFile(main_file.trim().to_string()).to_string()
        })?;
    let event = NewEvent::manuscript_registered(mission_id, dir.trim(), &main).map_err(err)?;
    store.append(event).map_err(err)?;
    let events = store.events_all().map_err(err)?;
    ManuscriptsProjection::for_mission(&events, mission_id)
        .map_err(err)?
        .ok_or_else(|| "internal: the registration just appended".to_string())
}

fn store_events_have_mission(events: &[crate::eventstore::StoredEvent], mission_id: Uuid) -> bool {
    use crate::domain::missions::MISSION_CREATED;
    events
        .iter()
        .any(|e| e.kind == MISSION_CREATED && e.id == mission_id)
}

/// The manuscript read of one mission: None when nothing is registered
/// (progressive disclosure — the surface never renders uninvited).
#[tauri::command]
pub async fn get_manuscript(
    db: State<'_, Db>,
    paths: State<'_, AppPaths>,
    mission_id: String,
) -> Result<Option<ManuscriptView>, String> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|e| format!("invalid mission id `{mission_id}`: {e}"))?;
    let c = db.0.lock().await;
    get_manuscript_inner(&c, &paths.data_dir, mission_id)
}

/// Every registered manuscript (the Ajustes registration surface's read —
/// which missions already carry one).
#[tauri::command]
pub async fn list_manuscripts(db: State<'_, Db>) -> Result<Vec<Manuscript>, String> {
    let c = db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(err)?;
    ManuscriptsProjection::fold(&events).map_err(err)
}

/// Read one manuscript file (the editing surface's read — the served
/// browser view shares it read-only).
#[tauri::command]
pub async fn read_manuscript_file(
    db: State<'_, Db>,
    mission_id: String,
    path: String,
) -> Result<ManuscriptFileView, String> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|e| format!("invalid mission id `{mission_id}`: {e}"))?;
    let c = db.0.lock().await;
    read_manuscript_file_inner(&c, mission_id, &path)
}

/// Save one manuscript file (desktop-only, AD-14): the edit targets the
/// USER's file in place — the repo is the manuscript, the app is just
/// another editor over it.
#[tauri::command]
pub async fn write_manuscript_file(
    db: State<'_, Db>,
    mission_id: String,
    path: String,
    content: String,
) -> Result<ManuscriptFileView, String> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|e| format!("invalid mission id `{mission_id}`: {e}"))?;
    let c = db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(err)?;
    let ms = ManuscriptsProjection::for_mission(&events, mission_id)
        .map_err(err)?
        .ok_or_else(|| manuscript::ManuscriptError::NotRegistered(mission_id).to_string())?;
    let rel = path.trim().to_string();
    manuscript::write_tex_file(&PathBuf::from(&ms.dir), &rel, &content).map_err(err)?;
    Ok(ManuscriptFileView {
        path: rel,
        content,
    })
}

/// Compile the manuscript (Story 6.6, FR-20.2, NFR-9): toolchain detection
/// is honest — no toolchain is the evented `tex_not_found` state with the
/// install hint, never a fake render. A successful run stores the PDF in
/// app data (never the repo) and serves it through the server shell.
#[tauri::command]
pub async fn compile_manuscript(
    db: State<'_, Db>,
    paths: State<'_, AppPaths>,
    mission_id: String,
) -> Result<CompileView, String> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|e| format!("invalid mission id `{mission_id}`: {e}"))?;
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    let events = store.events_all().map_err(err)?;
    let ms = ManuscriptsProjection::for_mission(&events, mission_id)
        .map_err(err)?
        .ok_or_else(|| manuscript::ManuscriptError::NotRegistered(mission_id).to_string())?;
    let dir = PathBuf::from(&ms.dir);
    let main = manuscript::safe_file_path(&dir, &ms.main_file)
        .map_err(err)?
        .is_file()
        .then(|| ms.main_file.clone())
        .ok_or_else(|| {
            manuscript::ManuscriptError::FileMissing(ms.main_file.clone()).to_string()
        })?;
    let preferred = crate::db::get_setting(&c, "tex_compiler");
    let tool = detect_toolchain(if preferred.trim().is_empty() {
        None
    } else {
        Some(preferred.as_str())
    });
    let Some(tool) = tool else {
        // the honest state: evented, with the install hint in the log tail
        let log = format!(
            "tex_not_found: no LaTeX toolchain found (searched: {}) — install tectonic \
             (tectonic-typesetting.github.io) or a TeX distribution (pdflatex/xelatex), \
             or configure one in Ajustes",
            manuscript::TEX_TOOLCHAIN_SEARCH.join(", ")
        );
        store
            .append(
                NewEvent::manuscript_compiled(
                    mission_id,
                    CompileOutcome::TexNotFound,
                    None,
                    &log,
                )
                .map_err(err)?,
            )
            .map_err(err)?;
        let events = store.events_all().map_err(err)?;
        let run = last_compile(&events, mission_id).expect("the compile just appended");
        return Ok(compile_view(&run, &paths.data_dir));
    };
    // compile into a temp outdir — the repo stays clean (source-control
    // friendly); the PDF lands in app data for the served pane
    let outdir = std::env::temp_dir().join(format!("rc-compile-{}-{}", mission_id, uuid::Uuid::new_v4()));
    let run = manuscript::run_compile(&tool, &dir, &main, &outdir);
    let log_tail = if run.log_tail.trim().is_empty() {
        format!("`{}` exited without output", tool.name)
    } else {
        run.log_tail.clone()
    };
    store
        .append(
            NewEvent::manuscript_compiled(
                mission_id,
                run.outcome,
                Some(&tool.name),
                &log_tail,
            )
            .map_err(err)?,
        )
        .map_err(err)?;
    if let Some(pdf_bytes) = &run.pdf {
        let pdf = pdf_path(&paths.data_dir, mission_id);
        if let Some(parent) = pdf.parent() {
            std::fs::create_dir_all(parent).map_err(err)?;
        }
        std::fs::write(&pdf, pdf_bytes).map_err(err)?;
    }
    let events = store.events_all().map_err(err)?;
    let compiled = last_compile(&events, mission_id).expect("the compile just appended");
    Ok(compile_view(&compiled, &paths.data_dir))
}

// ---------------------------------------------------------------------------
// Quarantined LaTeX diffs (Story 6.7, FR-20.3, AD-3/AD-13)
// ---------------------------------------------------------------------------

/// The diffs of one mission with the derived pending basis-staleness (the
/// review card's warning variant) — the read the surface and the server
/// route share.
pub(crate) fn list_manuscript_diffs_inner(
    conn: &rusqlite::Connection,
    mission_id: Uuid,
) -> Result<Vec<DiffProposal>, String> {
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let ms = ManuscriptsProjection::for_mission(&events, mission_id)
        .map_err(err)?
        .ok_or_else(|| manuscript::ManuscriptError::NotRegistered(mission_id).to_string())?;
    let root = PathBuf::from(&ms.dir);
    let mut diffs = manuscript::DiffProjection::fold_for(&events, mission_id).map_err(err)?;
    for diff in diffs.iter_mut().filter(|d| d.status == manuscript::DiffStatus::Pending) {
        diff.basis_stale = diff_basis_stale(&root, diff);
    }
    Ok(diffs)
}

/// The quarantined agent diffs of one mission (decided ones included —
/// history is honest).
#[tauri::command]
pub async fn list_manuscript_diffs(
    db: State<'_, Db>,
    mission_id: String,
) -> Result<Vec<DiffProposal>, String> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|e| format!("invalid mission id `{mission_id}`: {e}"))?;
    let c = db.0.lock().await;
    list_manuscript_diffs_inner(&c, mission_id)
}

/// The agent seam (FR-20.3, AD-3): an agent run's manuscript edit lands
/// as a quarantined diff — the hunks and the basis digest are derived in
/// the core (the digest is computed at proposal time, never sent), the
/// file is untouched until a human merges. `run_id` names the proposing
/// agent run (the skills/roles surface passes it).
#[tauri::command]
pub async fn propose_manuscript_diff(
    db: State<'_, Db>,
    mission_id: String,
    file: String,
    hunks: Vec<manuscript::ManuscriptDiffHunk>,
    note: String,
    run_id: String,
) -> Result<DiffProposal, String> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|e| format!("invalid mission id `{mission_id}`: {e}"))?;
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    let events = store.events_all().map_err(err)?;
    let ms = ManuscriptsProjection::for_mission(&events, mission_id)
        .map_err(err)?
        .ok_or_else(|| manuscript::ManuscriptError::NotRegistered(mission_id).to_string())?;
    let root = PathBuf::from(&ms.dir);
    manuscript::propose_diff(&store, &run_id, mission_id, &root, &file, &hunks, &note)
        .map_err(err)
}

/// Merge (approve) a diff (AD-13): validated against the CURRENT file —
/// a stale basis refuses with `basis_stale:` unless forced (the marker is
/// recorded and surfaced); a hunk that cannot apply cleanly refuses with
/// `hunk_mismatch:` (force does not help). Before applying, the merge
/// takes its checkpoint: a log checkpoint event + a file-level backup
/// outside the repo (app data), recorded on the merged event.
#[tauri::command]
pub async fn approve_manuscript_diff(
    db: State<'_, Db>,
    paths: State<'_, AppPaths>,
    proposal_id: String,
    force: bool,
) -> Result<manuscript::MergeDiffOutcome, String> {
    let proposal_id: Uuid = proposal_id
        .parse()
        .map_err(|e| format!("invalid proposal id `{proposal_id}`: {e}"))?;
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    let events = store.events_all().map_err(err)?;
    // the diff's manuscript (its dir is the merge root)
    let diffs = manuscript::DiffProjection::fold(&events).map_err(err)?;
    let diff = diffs
        .iter()
        .find(|d| d.id == proposal_id)
        .ok_or_else(|| manuscript::DiffError::NotFound(proposal_id).to_string())?;
    let ms = ManuscriptsProjection::for_mission(&events, diff.mission_id)
        .map_err(err)?
        .ok_or_else(|| {
            manuscript::ManuscriptError::NotRegistered(diff.mission_id).to_string()
        })?;
    let root = PathBuf::from(&ms.dir);
    let backup_dir = paths.data_dir.join("manuscript-backups");
    manuscript::merge_diff(&store, &root, &backup_dir, proposal_id, force).map_err(err)
}

/// Reject a pending diff — the change never applies, the file is
/// untouched, the rejection stays in the log with its receipt.
#[tauri::command]
pub async fn reject_manuscript_diff(
    db: State<'_, Db>,
    proposal_id: String,
) -> Result<DiffProposal, String> {
    let proposal_id: Uuid = proposal_id
        .parse()
        .map_err(|e| format!("invalid proposal id `{proposal_id}`: {e}"))?;
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    manuscript::reject_diff(&store, proposal_id).map_err(err)
}
