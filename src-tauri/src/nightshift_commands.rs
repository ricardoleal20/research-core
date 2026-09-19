// Night Shift shell commands (AD-15a, Story 2.3): the typed core APIs the
// Morning Digest view calls. The manual trigger and the schedule change are
// mutations (Tauri command path only, AD-14); the digest is a read over the
// shared core. Error strings lead with stable codes (`not_found:`,
// `invalid_schedule:`) and stay in code form — bilingual-safe by
// construction (EXPERIENCE.md).

use crate::db::Db;
use crate::domain::digest::MorningDigest;
use crate::domain::missions::{Mission, MissionsProjection};
use crate::eventstore::{EventStore, NewEvent};
use crate::nightshift::{morning_digest, NightShift};
use tauri::State;
use uuid::Uuid;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// The morning digest over the shared core (FR-4.4): ≤10 one-line rows, the
/// outcome badge, the spend-vs-ceiling line, and the dead-run alerts —
/// rendered even when (especially when) runs failed (FR-4.3).
#[tauri::command]
pub async fn get_morning_digest(db: State<'_, Db>) -> Result<MorningDigest, String> {
    morning_digest(db.inner()).await.map_err(err)
}

/// Run the Night Shift now (FR-4.1 manual trigger — testing/UX seam): every
/// ACTIVE mission's literature scan runs once, its output lands as
/// quarantined proposals (FR-4.2), and the fresh digest is returned. Failed
/// scans are honest rows in that digest, never command errors (FR-4.3).
#[tauri::command]
pub async fn run_night_shift_now(db: State<'_, Db>) -> Result<MorningDigest, String> {
    NightShift::new(db.inner().clone())
        .run_all()
        .await
        .map_err(err)?;
    morning_digest(db.inner()).await.map_err(err)
}

/// Change a mission's Night Shift schedule (FR-4.1): `off` or `daily-HH:MM`.
/// Appends one `mission.scheduled` event (latest wins in the fold) and
/// returns the folded mission — the read model, not the input.
#[tauri::command]
pub async fn set_mission_schedule(
    db: State<'_, Db>,
    mission_id: String,
    schedule: String,
) -> Result<Mission, String> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|e| format!("invalid mission id `{mission_id}`: {e}"))?;
    let schedule = schedule.trim().to_string();
    // Validate at the edge: an unparseable schedule never becomes an event.
    if crate::domain::missions::Schedule::parse(&schedule).is_none() {
        return Err(format!(
            "invalid_schedule: `{schedule}` — expected `off` or `daily-HH:MM` (e.g. daily-03:00)"
        ));
    }
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    let events = store.events_all().map_err(err)?;
    let missions = MissionsProjection::fold(&events).map_err(err)?;
    let mission = missions
        .iter()
        .find(|m| m.id == mission_id)
        .ok_or_else(|| format!("not_found: no mission with id `{mission_id}`"))?;
    let mut updated = mission.clone();
    store
        .append(NewEvent::mission_scheduled(&schedule, mission_id).map_err(err)?)
        .map_err(err)?;
    updated.schedule = schedule;
    Ok(updated)
}
