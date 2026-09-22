// Proposal shell commands (AD-15a, Story 2.2): the typed core APIs the
// quarantine review surface calls. Mutations flow ONLY through the domain's
// approve()/reject() — the merge constructors are crate-private, so there is
// no path from the shell to a blind merge (AD-13). Error strings lead with
// stable codes (`not_pending:`, `basis_stale:`, `not_found:`) and stay in
// code form — bilingual-safe by construction (EXPERIENCE.md).

use crate::db::Db;
use crate::domain::proposals::{self, ApproveOutcome, Proposal, ProposalsProjection};
use crate::eventstore::EventStore;
use rusqlite::Connection;
use tauri::State;
use uuid::Uuid;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// The quarantine read model (shared with the read-only server, AD-14):
/// all proposals, or one mission's, folded from the log in `seq` order.
pub(crate) fn list_proposals_inner(
    conn: &Connection,
    mission_id: Option<Uuid>,
) -> Result<Vec<Proposal>, String> {
    let events = EventStore::new(conn).events_all().map_err(err)?;
    match mission_id {
        Some(mission_id) => ProposalsProjection::fold_for(&events, mission_id),
        None => ProposalsProjection::fold(&events),
    }
    .map_err(err)
}

/// All proposals (or one mission's when `mission_id` is given): pending
/// first-class, decided ones with their receipts — nothing disappears.
#[tauri::command]
pub async fn list_proposals(
    db: State<'_, Db>,
    mission_id: Option<String>,
) -> Result<Vec<Proposal>, String> {
    let mission_id = match mission_id {
        Some(raw) => Some(
            raw.parse()
                .map_err(|e| format!("invalid mission id `{raw}`: {e}"))?,
        ),
        None => None,
    };
    let c = db.0.lock().await;
    list_proposals_inner(&c, mission_id)
}

/// Approve (merge) a proposal (AD-13): the domain validates the proposal is
/// pending and its basis against the entity's current state — a stale basis
/// is refused with `basis_stale:` unless `force`, and a forced merge records
/// the basis-stale marker the UI surfaces. Conflicting pending siblings are
/// superseded and returned so the surface can show what was displaced.
#[tauri::command]
pub async fn approve_proposal(
    db: State<'_, Db>,
    proposal_id: String,
    force: bool,
) -> Result<ApproveOutcome, String> {
    let proposal_id: Uuid = proposal_id
        .parse()
        .map_err(|e| format!("invalid proposal id `{proposal_id}`: {e}"))?;
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    proposals::approve(&store, proposal_id, force).map_err(err)
}

/// Reject a pending proposal — the change never applies; the proposal stays
/// in the log with its rejection receipt. A decided proposal is refused with
/// `not_pending:`.
#[tauri::command]
pub async fn reject_proposal(
    db: State<'_, Db>,
    proposal_id: String,
) -> Result<Proposal, String> {
    let proposal_id: Uuid = proposal_id
        .parse()
        .map_err(|e| format!("invalid proposal id `{proposal_id}`: {e}"))?;
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    proposals::reject(&store, proposal_id).map_err(err)
}
