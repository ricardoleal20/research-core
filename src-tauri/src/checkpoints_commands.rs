// Checkpoints shell commands (AD-15a, Story 2.6): the typed core APIs the
// board header's checkpoint control calls. Creating checkpoints, previewing
// a rollback, and rolling back are the checkpoint surface (FR-10.1):
// creation and rollback are mutations (Tauri command path only, AD-14);
// listing and previewing are reads over the shared core. Error strings lead
// with stable codes (`not_found:`, `invalid_name:`, `orphaned_checkpoint:`)
// and stay in code form — bilingual-safe by construction (EXPERIENCE.md).

use crate::db::Db;
use crate::domain::checkpoints::{
    fold_checkpoints, rollback, rollback_plan, Checkpoint, CheckpointsView, RollbackOutcome,
    RollbackPlan,
};
use crate::eventstore::{EventStore, NewEvent};
use tauri::State;
use uuid::Uuid;

fn err(e: impl ToString) -> String {
    e.to_string()
}

fn parse_id(checkpoint_id: &str) -> Result<Uuid, String> {
    checkpoint_id
        .parse()
        .map_err(|e| format!("invalid checkpoint id `{checkpoint_id}`: {e}"))
}

/// Create a checkpoint (FR-10.1): the user names the current log head as a
/// restore point. Appends one `checkpoint.created` event and returns the
/// folded checkpoint — the read model, not the input.
#[tauri::command]
pub async fn create_checkpoint(db: State<'_, Db>, name: String) -> Result<Checkpoint, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("invalid_name: a checkpoint is named — the restore-point list renders names (FR-10.1)".into());
    }
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    let head = store.head_seq().map_err(err)?;
    let stored = store
        .append(NewEvent::checkpoint_created(&name, head).map_err(err)?)
        .map_err(err)?;
    Ok(Checkpoint { id: stored.id, seq: head, ts: stored.ts, name })
}

/// The restore points and the rollback history (FR-10.1) — the checkpoint
/// control's read model over the shared core.
#[tauri::command]
pub async fn list_checkpoints(db: State<'_, Db>) -> Result<CheckpointsView, String> {
    let c = db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(err)?;
    fold_checkpoints(&events).map_err(err)
}

/// Preview a rollback (EXPERIENCE.md: the confirmation names every orphaned
/// proposal, never a summary): what rolling back to this checkpoint WOULD
/// orphan right now — every orphaned event, every orphaned proposal by
/// name. A read: nothing is appended.
#[tauri::command]
pub async fn preview_rollback(
    db: State<'_, Db>,
    checkpoint_id: String,
) -> Result<RollbackPlan, String> {
    let id = parse_id(&checkpoint_id)?;
    let c = db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(err)?;
    rollback_plan(&events, id).map_err(err)
}

/// Roll back to a checkpoint (AD-1): appends ONE `checkpoint.rolled_back`
/// event — history is never rewritten; the projections' shared fold cursor
/// does the returning. Returns the outcome with every orphaned event and
/// proposal listed (the superseded history the confirmation and the
/// quarantine's superseded view render).
#[tauri::command]
pub async fn rollback_to_checkpoint(
    db: State<'_, Db>,
    checkpoint_id: String,
) -> Result<RollbackOutcome, String> {
    let id = parse_id(&checkpoint_id)?;
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    rollback(&store, id).map_err(err)
}
