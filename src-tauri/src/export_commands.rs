// Export shell commands (Story 3.1, FR-7.1/7.2, AD-11): the open-export
// surface the header's Export composer calls. Exporting renders the pure
// projections to open git-friendly files at ONE named seq cut — a READER
// of the log (nothing appended, nothing mutated); the inspect read powers
// the composer's stale-warning state (Story 2.6's staleness signal).
// Error strings lead with stable codes (`invalid_scope:`, `io:`) and stay
// in code form — bilingual-safe by construction (EXPERIENCE.md).

use crate::db::Db;
use crate::domain::export::{inspect_export as inspect_export_at, ExportInspect, ExportOutcome};
use crate::eventstore::EventStore;
use tauri::State;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// Export the workspace to a folder (FR-7.1): renders every requested
/// scope's fold at ONE named seq cut — the log head at render start —
/// recorded in the export manifest. Markdown first (FR-7.2: the exported
/// content is fully readable and usable without the app); when the folder
/// already holds a stale previous export (a rollback landed after its
/// cut), the manifest and the affected entity files carry the visible
/// bilingual STALE marker naming both cuts.
#[tauri::command]
pub async fn export_workspace(
    db: State<'_, Db>,
    dir: String,
    scope: String,
) -> Result<ExportOutcome, String> {
    let dir = dir.trim();
    if dir.is_empty() {
        return Err(
            "invalid_dir: an export names the folder it writes — pick a destination".into(),
        );
    }
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    crate::domain::export::export_workspace(&store, std::path::Path::new(dir), &scope)
        .map_err(err)
}

/// Read an export folder at open (EXPERIENCE.md's composer): the cut its
/// manifest records, and whether a rollback after that cut has staled it
/// against the current log — the stale-warning state. `None` when the
/// folder holds no parseable export. Read-only.
#[tauri::command]
pub async fn inspect_export(
    db: State<'_, Db>,
    dir: String,
) -> Result<Option<ExportInspect>, String> {
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    inspect_export_at(&store, std::path::Path::new(dir.trim())).map_err(err)
}
