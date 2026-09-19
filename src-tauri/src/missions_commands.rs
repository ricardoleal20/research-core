// Mission shell commands (AD-15a): the typed core APIs the UI calls. They
// construct events via the domain's typed constructor and read state via the
// projection — never raw JSON appends, never direct writes. Args arrive
// camelCase from the frontend (Tauri 2 convention) and are parsed into domain
// types before construction.

use crate::db::Db;
use crate::domain::missions::{
    Autonomy, Mission, MissionCreatedPayload, MissionsProjection,
};
use crate::eventstore::{EventStore, NewEvent};
use tauri::State;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// Create a mission: append one `mission.created` event (actor=user) and
/// return the folded mission read model. Fails loudly when terminator fields
/// are empty (AD-12) or the autonomy stop is unknown.
#[tauri::command]
pub async fn create_mission(
    db: State<'_, Db>,
    question: String,
    stop_condition: String,
    success_criterion: String,
    autonomy: String,
    spend_ceiling_cents: u64,
) -> Result<Mission, String> {
    let autonomy = Autonomy::parse(&autonomy)
        .ok_or_else(|| format!("unknown autonomy stop `{autonomy}` — expected watch | suggest | act_with_receipts"))?;
    let event = NewEvent::mission_created(MissionCreatedPayload {
        question,
        stop_condition,
        success_criterion,
        autonomy,
        spend_ceiling_cents,
    })
    .map_err(err)?;
    let c = db.0.lock().await;
    let stored = EventStore::new(&c).append(event).map_err(err)?;
    // Return exactly what the log now holds — the read model, not the input.
    let missions = MissionsProjection::fold(&[stored]).map_err(err)?;
    Ok(missions.into_iter().next().expect("fold of one creation event yields one mission"))
}

/// All missions, folded from the log in `seq` order (oldest first).
#[tauri::command]
pub async fn list_missions(db: State<'_, Db>) -> Result<Vec<Mission>, String> {
    let c = db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(err)?;
    MissionsProjection::fold(&events).map_err(err)
}
