// Manuscript domain (FR-20, Stories 6.6–6.8): the .tex repo IS the
// manuscript. A mission registers a .tex project directory (an absolute
// path on the user's disk); the `manuscript.registered` event REFERENCES
// the directory, it never duplicates it — the files on disk are the
// artifact (AD-11 spirit) and no parallel manuscript store exists. Edits
// through the app target the user's files directly (source-control
// friendly: the app is just another editor over the repo).
//
// Story 6.6 (FR-20.1/20.2, NFR-9): registration is evented (actor=user,
// typed command, AD-2 envelope); the surface lists/scans the .tex sources
// (per-file word + marker counts); compile runs on demand through a
// configured binary — toolchain detection is HONEST: the default search
// order is tectonic → pdflatex → xelatex (the `tex_compiler` setting may
// name any known compiler, incl. latexmk), and a missing toolchain is the
// explicit `tex_not_found` state with an install hint, never a fake
// render. Every compile run is evented (`manuscript.compiled`, outcome +
// log tail); compile errors render the log honestly in receipt voice.
// Manuscript edits are desktop-only (AD-14): the served browser view reads
// the manuscript read-only and every mutation is refused with the standard
// bilingual message at the api.ts layer.
//
// Stories 6.7/6.8 (FR-20.3/20.4) extend this module below: agent edits
// land as quarantined LaTeX diffs (before/after hunks — never a freeform
// overwrite) merged only by the human, and the deterministic
// manuscript-board consistency check links the paper's inline `\hyp{…}` /
// `\claim{…}` markers to the board.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::domain::checkpoints::FoldCursor;
use crate::eventstore::{Actor, EventError, NewEvent, StoredEvent};

pub const MANUSCRIPT_REGISTERED: &str = "manuscript.registered";
pub const MANUSCRIPT_COMPILED: &str = "manuscript.compiled";

/// The default toolchain search order (FR-20.2): first found wins. The
/// `tex_compiler` setting may override with any name in `TEX_TOOLCHAINS`.
pub const TEX_TOOLCHAIN_SEARCH: &[&str] = &["tectonic", "pdflatex", "xelatex"];
/// Every compiler the `tex_compiler` setting may name.
pub const TEX_TOOLCHAINS: &[&str] = &["tectonic", "pdflatex", "xelatex", "latexmk"];

/// Everything that can go wrong on the manuscript path — typed, with
/// stable codes the UI never translates (EXPERIENCE.md).
#[derive(Debug, thiserror::Error)]
pub enum ManuscriptError {
    #[error("not_found: no manuscript registered for mission `{0}`")]
    NotRegistered(Uuid),
    #[error("not_found: mission `{0}` does not exist — a manuscript registers to a real mission")]
    MissionNotFound(Uuid),
    #[error("invalid_dir: `{0}` — the manuscript directory must exist on disk")]
    InvalidDir(String),
    #[error("invalid_main_file: `{0}` — the main file must be a relative .tex path inside the manuscript directory (no `..`, no leading `/`) that exists on disk")]
    InvalidMainFile(String),
    #[error("unsafe_path: `{0}` escapes the manuscript directory — refused")]
    UnsafePath(String),
    #[error("file_missing: `{0}` — the manuscript file is not on disk (moved or deleted outside the app)")]
    FileMissing(String),
    #[error(transparent)]
    Store(#[from] EventError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// Registration (Story 6.6, FR-20.1)
// ---------------------------------------------------------------------------

/// The `manuscript.registered` payload: the mission the manuscript belongs
/// to, the ABSOLUTE directory of the .tex project on disk (referenced,
/// never copied — the files are the artifact), and the main .tex file to
/// compile (a repo-relative path, e.g. `main.tex` or `paper/main.tex`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManuscriptRegisteredPayload {
    pub mission_id: Uuid,
    pub dir: String,
    pub main_file: String,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `manuscript.registered`
    /// event comes into being. The mission must be real, the dir non-empty
    /// (existing on disk is validated at the command seam, which holds the
    /// filesystem), and the main file a safe relative .tex path.
    pub fn manuscript_registered(
        mission_id: Uuid,
        dir: &str,
        main_file: &str,
    ) -> Result<Self, EventError> {
        if mission_id.is_nil() {
            return Err(EventError::Invalid(
                "manuscript.mission_id must not be nil — a manuscript registers to a mission".into(),
            ));
        }
        if dir.trim().is_empty() {
            return Err(EventError::Invalid(
                "manuscript.dir must not be empty — a manuscript names its .tex project directory"
                    .into(),
            ));
        }
        validate_main_file(main_file)?;
        let payload = ManuscriptRegisteredPayload {
            mission_id,
            dir: dir.trim().to_string(),
            main_file: main_file.trim().to_string(),
        };
        Ok(Self::new(
            MANUSCRIPT_REGISTERED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?
        .with_causes(vec![mission_id]))
    }
}

/// A main file (or any manuscript file reference) must be a relative path
/// inside the repo: no leading `/`, no `..` component, `.tex` extension.
pub(crate) fn validate_main_file(main_file: &str) -> Result<(), EventError> {
    let f = main_file.trim();
    if f.is_empty() {
        return Err(EventError::Invalid(
            "manuscript.main_file must not be empty — e.g. `main.tex`".into(),
        ));
    }
    if f.starts_with('/') || f.starts_with('\\') {
        return Err(EventError::Invalid(format!(
            "manuscript.main_file `{f}` must be relative to the manuscript directory"
        )));
    }
    if f.split(['/', '\\']).any(|c| c == "..") {
        return Err(EventError::Invalid(format!(
            "manuscript.main_file `{f}` must not escape the manuscript directory"
        )));
    }
    if !f.to_ascii_lowercase().ends_with(".tex") {
        return Err(EventError::Invalid(format!(
            "manuscript.main_file `{f}` must be a .tex file"
        )));
    }
    Ok(())
}

/// One mission's registered manuscript — the read model the surface
/// renders (camelCase on the wire, AD-8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manuscript {
    pub mission_id: Uuid,
    /// The `manuscript.registered` event's seq — the registration receipt.
    pub seq: i64,
    pub ts: chrono::DateTime<chrono::Utc>,
    /// The ABSOLUTE .tex project directory on disk (referenced, not owned).
    pub dir: String,
    /// The repo-relative main .tex file to compile.
    pub main_file: String,
}

/// Pure fold of the log into the registered-manuscript read model. A
/// re-registration of the same mission REPLACES the earlier one (latest
/// wins, by seq); nothing is ever removed. The fold respects the shared
/// FoldCursor (AD-1): a rollback that orphaned a registration means it
/// never happened.
pub struct ManuscriptsProjection;

impl ManuscriptsProjection {
    pub fn fold(events: &[StoredEvent]) -> Result<Vec<Manuscript>, EventError> {
        let cursor = FoldCursor::over(events);
        let live = cursor.live_owned(events);
        let mut by_mission: std::collections::BTreeMap<Uuid, Manuscript> = Default::default();
        for event in &live {
            if event.kind != MANUSCRIPT_REGISTERED {
                continue;
            }
            let payload: ManuscriptRegisteredPayload = serde_json::from_value(event.payload.clone())
                .map_err(|e| {
                    EventError::Invalid(format!(
                        "corrupt {MANUSCRIPT_REGISTERED} payload at seq {}: {e}",
                        event.seq
                    ))
                })?;
            by_mission.insert(
                payload.mission_id,
                Manuscript {
                    mission_id: payload.mission_id,
                    seq: event.seq,
                    ts: event.ts,
                    dir: payload.dir,
                    main_file: payload.main_file,
                },
            );
        }
        Ok(by_mission.into_values().collect())
    }

    /// The LATEST registered manuscript of one mission, if any.
    pub fn for_mission(
        events: &[StoredEvent],
        mission_id: Uuid,
    ) -> Result<Option<Manuscript>, EventError> {
        Ok(Self::fold(events)?
            .into_iter()
            .find(|m| m.mission_id == mission_id))
    }
}

// ---------------------------------------------------------------------------
// The .tex scan (Story 6.6, FR-20.2): the files on disk are the artifact
// ---------------------------------------------------------------------------

/// One .tex source as the manuscript file list renders it: the
/// repo-relative path, the word count, and the board-link marker counts
/// (`\hyp{…}` / `\claim{…}` — the FR-20.4 convention, counted here so the
/// file list can label claim-bearing sources).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TexFileScan {
    pub path: String,
    pub words: u32,
    pub hyp_markers: u32,
    pub claim_markers: u32,
    pub bytes: u64,
}

/// Word + marker counts of one .tex source (pure — the listing read and the
/// consistency scan share it). Words are whitespace-separated tokens,
/// comments included — an honest count, never a semantics claim.
pub fn scan_text(content: &str) -> (u32, u32, u32) {
    let words = content.split_whitespace().count() as u32;
    let hyp_markers = count_marker(content, "hyp");
    let claim_markers = count_marker(content, "claim");
    (words, hyp_markers, claim_markers)
}

pub(crate) fn count_marker(content: &str, name: &str) -> u32 {
    let needle = format!("\\{name}{{");
    content.matches(&needle).count() as u32
}

/// Resolve a repo-relative file reference to an absolute path, refusing
/// anything that escapes the manuscript directory (the editor and the diff
/// merge both go through this — no traversal, ever).
pub fn safe_file_path(root: &Path, rel: &str) -> Result<PathBuf, ManuscriptError> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Err(ManuscriptError::UnsafePath(rel.into()));
    }
    if rel.starts_with('/') || rel.starts_with('\\') || rel.split(['/', '\\']).any(|c| c == "..") {
        return Err(ManuscriptError::UnsafePath(rel.into()));
    }
    Ok(root.join(rel))
}

/// List and scan every .tex file of the manuscript directory (recursive,
/// sorted by path — a deterministic listing). Hidden directories (`.git`,
/// etc.) are skipped: the repo is the manuscript, its metadata is not.
pub fn scan_manuscript(dir: &Path) -> Result<Vec<TexFileScan>, ManuscriptError> {
    if !dir.is_dir() {
        return Err(ManuscriptError::InvalidDir(dir.display().to_string()));
    }
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)?;
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let path = entry.path();
            if name.starts_with('.') {
                continue; // .git and friends are not the manuscript
            }
            if path.is_dir() {
                stack.push(path);
            } else if name.to_ascii_lowercase().ends_with(".tex") {
                let rel = path
                    .strip_prefix(dir)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| path.display().to_string());
                let content = std::fs::read_to_string(&path)?;
                let (words, hyp_markers, claim_markers) = scan_text(&content);
                files.push(TexFileScan {
                    path: rel,
                    words,
                    hyp_markers,
                    claim_markers,
                    bytes: entry.metadata()?.len(),
                });
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Read one manuscript file (traversal-safe).
pub fn read_tex_file(root: &Path, rel: &str) -> Result<String, ManuscriptError> {
    let path = safe_file_path(root, rel)?;
    if !path.is_file() {
        return Err(ManuscriptError::FileMissing(rel.into()));
    }
    Ok(std::fs::read_to_string(&path)?)
}

/// Write one manuscript file (traversal-safe) — the editing surface's
/// save. The repo is the manuscript: this writes the USER's file, exactly
/// as an external editor would (source-control friendly).
pub fn write_tex_file(root: &Path, rel: &str, content: &str) -> Result<(), ManuscriptError> {
    let path = safe_file_path(root, rel)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(std::fs::write(&path, content)?)
}

// ---------------------------------------------------------------------------
// Toolchain + compile (Story 6.6, FR-20.2, NFR-9)
// ---------------------------------------------------------------------------

/// A detected LaTeX toolchain: the compiler's name and absolute path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TexToolchain {
    pub name: String,
    pub path: String,
}

/// Detect the toolchain honestly (NFR-9): the `preferred` setting's name
/// first (if it names a known compiler), then the default search order
/// (tectonic → pdflatex → xelatex) over `PATH`. None is the honest
/// `tex_not_found` state — never a fake render.
pub fn detect_toolchain(preferred: Option<&str>) -> Option<TexToolchain> {
    let order: Vec<&str> = match preferred.map(str::trim) {
        Some(name) if !name.is_empty() => {
            if !TEX_TOOLCHAINS.contains(&name) {
                return None; // an unknown configured compiler is tex_not_found, honestly
            }
            vec![name]
        }
        _ => TEX_TOOLCHAIN_SEARCH.to_vec(),
    };
    for name in order {
        if let Some(path) = which(name) {
            return Some(TexToolchain {
                name: name.to_string(),
                path: path.display().to_string(),
            });
        }
    }
    None
}

/// Minimal `which`: search `PATH` for an executable. (The dev-dependency
/// `which` crate is not a runtime dep; this stays dependency-free.)
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// The compile run's outcome (NFR-9): `ok` produced a PDF; `error` ran the
/// compiler and it failed (the log tail is the receipt); `tex_not_found`
/// is the honest missing-toolchain state with an install hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompileOutcome {
    Ok,
    Error,
    TexNotFound,
}

impl CompileOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::TexNotFound => "tex_not_found",
        }
    }
}

/// The `manuscript.compiled` payload: what ran, how it ended, and the log
/// tail (the receipt — compile errors render it honestly, mono).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManuscriptCompiledPayload {
    pub mission_id: Uuid,
    pub outcome: CompileOutcome,
    /// The compiler that ran (None when `tex_not_found`).
    pub tool: Option<String>,
    /// The last lines of the compile log (capped) — empty never: even the
    /// missing-toolchain state carries its one-line reason.
    pub log_tail: String,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `manuscript.compiled`
    /// event comes into being. Every compile attempt is evented — success,
    /// failure, AND the honest tex_not_found state.
    pub fn manuscript_compiled(
        mission_id: Uuid,
        outcome: CompileOutcome,
        tool: Option<&str>,
        log_tail: &str,
    ) -> Result<Self, EventError> {
        if mission_id.is_nil() {
            return Err(EventError::Invalid(
                "manuscript.mission_id must not be nil".into(),
            ));
        }
        let payload = ManuscriptCompiledPayload {
            mission_id,
            outcome,
            tool: tool.map(str::to_string),
            log_tail: log_tail.chars().take(4_000).collect(),
        };
        Ok(Self::new(
            MANUSCRIPT_COMPILED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?
        .with_causes(vec![mission_id]))
    }
}

/// One compile run as the surface renders it (the last compile of a
/// mission is what the PDF pane + log receipt show).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledRun {
    pub seq: i64,
    pub ts: chrono::DateTime<chrono::Utc>,
    pub mission_id: Uuid,
    pub outcome: CompileOutcome,
    pub tool: Option<String>,
    pub log_tail: String,
}

/// The LAST compile run of one mission (the PDF pane's state), folded from
/// the log — None when the manuscript has never been compiled.
pub fn last_compile(events: &[StoredEvent], mission_id: Uuid) -> Option<CompiledRun> {
    let cursor = FoldCursor::over(events);
    let live = cursor.live_owned(events);
    let mut last = None;
    for event in &live {
        if event.kind != MANUSCRIPT_COMPILED {
            continue;
        }
        let Ok(payload) =
            serde_json::from_value::<ManuscriptCompiledPayload>(event.payload.clone())
        else {
            continue; // a corrupt compile event never blocks the surface
        };
        if payload.mission_id == mission_id {
            last = Some(CompiledRun {
                seq: event.seq,
                ts: event.ts,
                mission_id,
                outcome: payload.outcome,
                tool: payload.tool,
                log_tail: payload.log_tail,
            });
        }
    }
    last
}

/// The compiler argv for one tool (the outdir is always explicit — the
/// temp outdir, never the repo: the app leaves no build noise in the
/// user's source tree).
pub fn compile_args(tool: &str, outdir: &Path, main_file: &str) -> Vec<String> {
    let outdir = outdir.display().to_string();
    match tool {
        "tectonic" => vec!["--outdir".into(), outdir, main_file.into()],
        "latexmk" => vec![
            "-pdf".into(),
            "-interaction=nonstopmode".into(),
            "-output-directory".into(),
            outdir,
            main_file.into(),
        ],
        // pdflatex / xelatex
        _ => vec![
            "-interaction=nonstopmode".into(),
            "-halt-on-error".into(),
            "-output-directory".into(),
            outdir,
            main_file.into(),
        ],
    }
}

/// The compile result before it is evented: the outcome, the (capped) log
/// tail, and the PDF bytes when the run produced one.
#[derive(Debug, Clone, PartialEq)]
pub struct CompileRun {
    pub outcome: CompileOutcome,
    pub log_tail: String,
    pub pdf: Option<Vec<u8>>,
}

/// Run one compile (cwd = the manuscript dir; the PDF is expected at
/// `{outdir}/{main stem}.pdf`). A failing spawn (missing binary mid-run)
/// is an honest `error` with the OS reason in the log tail — the
/// `tex_not_found` STATE is decided by `detect_toolchain` before this
/// runs, never guessed here.
pub fn run_compile(
    tool: &TexToolchain,
    dir: &Path,
    main_file: &str,
    outdir: &Path,
) -> CompileRun {
    std::fs::create_dir_all(outdir).ok();
    let output = std::process::Command::new(&tool.path)
        .args(compile_args(&tool.name, outdir, main_file))
        .current_dir(dir)
        .output();
    let output = match output {
        Ok(o) => o,
        Err(e) => {
            return CompileRun {
                outcome: CompileOutcome::Error,
                log_tail: format!("failed to run `{}`: {e}", tool.name),
                pdf: None,
            }
        }
    };
    let mut log = String::from_utf8_lossy(&output.stdout).into_owned();
    log.push_str(&String::from_utf8_lossy(&output.stderr));
    // the tail: the last 4,000 chars, whole
    let log_tail: String = {
        let chars: Vec<char> = log.chars().collect();
        let start = chars.len().saturating_sub(4_000);
        chars[start..].iter().collect()
    };
    let pdf_path = outdir.join(
        Path::new(main_file)
            .file_stem()
            .map(|s| format!("{}.pdf", s.to_string_lossy()))
            .unwrap_or_else(|| "main.pdf".into()),
    );
    let pdf = if output.status.success() {
        std::fs::read(&pdf_path).ok()
    } else {
        None
    };
    let outcome = if pdf.is_some() {
        CompileOutcome::Ok
    } else {
        CompileOutcome::Error
    };
    CompileRun {
        outcome,
        log_tail,
        pdf,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::missions::{Autonomy, MissionCreatedPayload};
    use crate::eventstore::EventStore;
    use rusqlite::Connection;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    fn seed_mission(store: &EventStore) -> Uuid {
        store
            .append(NewEvent::mission_created(MissionCreatedPayload {
                question: "Does X hold up?".into(),
                stop_condition: "Stop after $5.".into(),
                success_criterion: "A blind rater agrees.".into(),
                autonomy: Autonomy::Suggest,
                spend_ceiling_cents: 500,
                schedule: "daily-03:00".into(),
                roles: vec![],
            })
            .unwrap())
            .unwrap()
            .id
    }

    // ---------- registration (FR-20.1) ----------

    #[test]
    fn registration_is_evented_and_validated_at_the_edges() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let ev =
            NewEvent::manuscript_registered(m, "/Users/ricardo/papers/stiff", "paper/main.tex")
                .unwrap();
        assert_eq!(ev.kind, MANUSCRIPT_REGISTERED);
        assert_eq!(ev.actor, Actor::User);
        assert_eq!(ev.causes, vec![m]);
        assert_eq!(
            ev.payload,
            serde_json::json!({
                "mission_id": m.to_string(),
                "dir": "/Users/ricardo/papers/stiff",
                "main_file": "paper/main.tex",
            })
        );
        // the edges: nil mission, empty dir, absolute/traversing/non-tex main
        assert!(NewEvent::manuscript_registered(Uuid::nil(), "/d", "main.tex").is_err());
        assert!(NewEvent::manuscript_registered(m, "  ", "main.tex").is_err());
        for bad in ["/abs/main.tex", "../up/main.tex", "main.org"] {
            assert!(
                NewEvent::manuscript_registered(m, "/d", bad).is_err(),
                "main_file `{bad}` must be refused"
            );
        }
    }

    #[test]
    fn the_fold_reads_the_latest_registration_per_mission() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m1 = seed_mission(&store);
        let m2 = seed_mission(&store);
        store
            .append(
                NewEvent::manuscript_registered(m1, "/papers/one", "main.tex").unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::manuscript_registered(m2, "/papers/two", "paper.tex").unwrap(),
            )
            .unwrap();
        let all = ManuscriptsProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(all.len(), 2);
        // re-registration replaces (latest wins)
        store
            .append(
                NewEvent::manuscript_registered(m1, "/papers/one-v2", "v2.tex").unwrap(),
            )
            .unwrap();
        let m1_ms = ManuscriptsProjection::for_mission(&store.events_all().unwrap(), m1)
            .unwrap()
            .expect("registered");
        assert_eq!(m1_ms.dir, "/papers/one-v2");
        assert_eq!(m1_ms.main_file, "v2.tex");
        // an unregistered mission reads None
        assert!(ManuscriptsProjection::for_mission(&store.events_all().unwrap(), Uuid::new_v4())
            .unwrap()
            .is_none());
    }

    // ---------- the scan ----------

    #[test]
    fn scan_text_counts_words_and_board_markers() {
        let tex = "% a comment\n\\section{Results}\nThe gain is 12.3\\% (n=48) \\hyp{H-3}.\n\\claim{CLAIMS-5} holds \\hyp{7} too.\n";
        let (words, hyp, claims) = scan_text(tex);
        assert!(words > 10, "an honest token count: {words}");
        assert_eq!(hyp, 2, "\\hyp markers: H-3 and 7");
        assert_eq!(claims, 1, "\\claim markers: CLAIMS-5");
    }

    #[test]
    fn safe_file_path_refuses_traversal() {
        let root = Path::new("/papers/one");
        assert_eq!(
            safe_file_path(root, "chapters/intro.tex").unwrap(),
            PathBuf::from("/papers/one/chapters/intro.tex")
        );
        for bad in ["../secrets.tex", "/etc/passwd", ""] {
            assert!(safe_file_path(root, bad).is_err(), "`{bad}` must refuse");
        }
    }

    #[test]
    fn scan_manuscript_lists_tex_files_deterministically() {
        let dir = std::env::temp_dir().join(format!("rc-ms-scan-{}", Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("chapters")).unwrap();
        std::fs::write(dir.join("main.tex"), "one two \\hyp{H-1}\n").unwrap();
        std::fs::write(dir.join("chapters/intro.tex"), "alpha beta\n").unwrap();
        std::fs::write(dir.join(".git/config.tex"), "must be skipped\n").unwrap();
        std::fs::write(dir.join("refs.bib"), "not tex\n").unwrap();
        let files = scan_manuscript(&dir).unwrap();
        assert_eq!(
            files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            vec!["chapters/intro.tex", "main.tex"],
            "sorted, .tex only, hidden dirs skipped"
        );
        assert_eq!(files[1].words, 3, "one two \\hyp{{H-1}} — three tokens");
        assert_eq!(files[1].hyp_markers, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---------- toolchain + compile (NFR-9) ----------

    #[test]
    fn detect_toolchain_searches_path_honestly() {
        // an empty PATH finds nothing — the honest tex_not_found state
        let saved = std::env::var_os("PATH");
        // SAFETY: test-only PATH mutation (this suite's only env touch)
        unsafe {
            std::env::set_var("PATH", "");
        }
        assert!(detect_toolchain(None).is_none());
        assert!(detect_toolchain(Some("latexmk")).is_none());
        unsafe {
            std::env::set_var(
                "PATH",
                saved.clone().unwrap_or_else(|| std::ffi::OsString::from("/usr/bin")),
            );
        }
        // an unknown configured compiler is tex_not_found, honestly
        assert!(detect_toolchain(Some("not-a-compiler")).is_none());
    }

    #[test]
    fn the_compile_args_name_the_temp_outdir() {
        let args = compile_args("pdflatex", Path::new("/tmp/out"), "main.tex");
        assert_eq!(
            args,
            vec![
                "-interaction=nonstopmode".to_string(),
                "-halt-on-error".to_string(),
                "-output-directory".to_string(),
                "/tmp/out".to_string(),
                "main.tex".to_string(),
            ]
        );
        let tectonic = compile_args("tectonic", Path::new("/tmp/o"), "paper.tex");
        assert_eq!(tectonic[0], "--outdir");
    }

    /// A fake `tectonic` on PATH: a shell script that creates the expected
    /// PDF in its outdir — proves the honest ok path end to end without a
    /// real TeX install.
    #[test]
    fn run_compile_classifies_ok_and_error_honestly() {
        let bin_dir = std::env::temp_dir().join(format!("rc-tex-bin-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&bin_dir).unwrap();
        let work = std::env::temp_dir().join(format!("rc-tex-work-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(work.join("main.tex"), "\\documentclass{article}\n").unwrap();

        // ok: writes the pdf into the outdir (arg 2 of tectonic's argv)
        let ok_bin = bin_dir.join("tectonic");
        std::fs::write(
            &ok_bin,
            "#!/bin/sh\nout=\"\"\nfor a in \"$@\"; do case \"$prev\" in --outdir) out=\"$a\";; esac; prev=\"$a\"; done\nmkdir -p \"$out\"\nprintf '%%PDF-1.4 fake' > \"$out/main.pdf\"\necho 'compiled 1 page'\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&ok_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let tool = TexToolchain {
            name: "tectonic".into(),
            path: ok_bin.display().to_string(),
        };
        let outdir = std::env::temp_dir().join(format!("rc-tex-out-{}", Uuid::new_v4()));
        let run = run_compile(&tool, &work, "main.tex", &outdir);
        assert_eq!(run.outcome, CompileOutcome::Ok);
        assert!(run.pdf.as_deref().map(|p| !p.is_empty()).unwrap_or(false));
        assert!(run.log_tail.contains("compiled 1 page"));

        // error: a compiler that fails without producing the pdf
        let fail_bin = bin_dir.join("pdflatex");
        std::fs::write(&fail_bin, "#!/bin/sh\necho '! Emergency stop.' >&2\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fail_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let tool = TexToolchain {
            name: "pdflatex".into(),
            path: fail_bin.display().to_string(),
        };
        let run = run_compile(&tool, &work, "main.tex", &outdir);
        assert_eq!(run.outcome, CompileOutcome::Error);
        assert!(run.pdf.is_none());
        assert!(run.log_tail.contains("Emergency stop"), "the honest log: {}", run.log_tail);

        let _ = std::fs::remove_dir_all(&bin_dir);
        let _ = std::fs::remove_dir_all(&work);
        let _ = std::fs::remove_dir_all(&outdir);
    }

    #[test]
    fn every_compile_attempt_is_evented_including_tex_not_found() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        // the honest missing-toolchain state carries its one-line reason
        let ev = NewEvent::manuscript_compiled(
            m,
            CompileOutcome::TexNotFound,
            None,
            "tex_not_found: no LaTeX toolchain found (searched tectonic, pdflatex, xelatex)",
        )
        .unwrap();
        store.append(ev).unwrap();
        store
            .append(NewEvent::manuscript_compiled(
                m,
                CompileOutcome::Ok,
                Some("tectonic"),
                "compiled 1 page",
            )
            .unwrap())
            .unwrap();
        let last = last_compile(&store.events_all().unwrap(), m).expect("folded");
        assert_eq!(last.outcome, CompileOutcome::Ok);
        assert_eq!(last.tool.as_deref(), Some("tectonic"));
        assert_eq!(last.log_tail, "compiled 1 page");
        // another mission's compiles never leak in
        let m2 = seed_mission(&store);
        assert!(last_compile(&store.events_all().unwrap(), m2).is_none());
    }
}
