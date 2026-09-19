// Mission shell commands (AD-15a): the typed core APIs the UI calls. They
// construct events via the domain's typed constructor and read state via the
// projection — never raw JSON appends, never direct writes. Args arrive
// camelCase from the frontend (Tauri 2 convention) and are parsed into domain
// types before construction.

use crate::adapters::providers::ProviderSettings;
use crate::db::Db;
use crate::domain::missions::{
    Autonomy, Mission, MissionCreatedPayload, MissionRun, MissionsProjection, RoleConfig,
};
use crate::eventstore::{EventStore, NewEvent};
use crate::runtime;
use crate::runtime::{AgentRuntime, AgentStepResult};
use tauri::State;
use uuid::Uuid;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// Create a mission: append one `mission.created` event (actor=user) and
/// return the folded mission read model. Fails loudly when terminator fields
/// are empty (AD-12), the autonomy stop is unknown, or the resolved role
/// config violates the different-model critic rule (NFR-3 — the typed
/// constructor rejects it BEFORE any event is appended).
#[tauri::command]
pub async fn create_mission(
    db: State<'_, Db>,
    question: String,
    stop_condition: String,
    success_criterion: String,
    autonomy: String,
    spend_ceiling_cents: u64,
    roles: Option<Vec<RoleConfig>>,
) -> Result<Mission, String> {
    let autonomy = Autonomy::parse(&autonomy)
        .ok_or_else(|| format!("unknown autonomy stop `{autonomy}` — expected watch | suggest | act_with_receipts"))?;
    // Resolve the mission's agent-role config: the layer's defaults, with the
    // caller's overrides applied by role name (Story 2.1). The typed
    // constructor validates the resolved set — a same-model critic fails
    // creation before anything is appended.
    let roles = {
        let c = db.0.lock().await;
        runtime::merge_roles(
            runtime::default_roles(&ProviderSettings::load(&c)),
            roles,
        )
    };
    let event = NewEvent::mission_created(MissionCreatedPayload {
        question,
        stop_condition,
        success_criterion,
        autonomy,
        spend_ceiling_cents,
        roles,
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

/// The run list of one mission (FR-1.3): every log event referencing it, in
/// `seq` order — the basic drill-down the missions home renders. The full
/// receipts timeline is a later story.
#[tauri::command]
pub async fn get_mission_runs(
    db: State<'_, Db>,
    mission_id: String,
) -> Result<Vec<MissionRun>, String> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|e| format!("invalid mission id `{mission_id}`: {e}"))?;
    let c = db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(err)?;
    Ok(MissionsProjection::runs_for(&events, mission_id))
}

/// Run ONE agent step for a mission's role (Story 2.1): the role's
/// (provider, model) resolves through the provider layer (AD-9), the
/// step's spend is recorded role-tagged (AD-10), and the result carries
/// what the role produced. Fails loudly on unknown mission / role or an
/// empty task.
#[tauri::command]
pub async fn run_agent_step(
    db: State<'_, Db>,
    mission_id: String,
    role: String,
    task: String,
) -> Result<AgentStepResult, String> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|e| format!("invalid mission id `{mission_id}`: {e}"))?;
    AgentRuntime::new(db.inner().clone())
        .run_step(mission_id, &role, &task)
        .await
        .map_err(err)
}
