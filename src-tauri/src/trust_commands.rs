// Trust shell commands (AD-15a, Story 2.4): the typed core APIs the Settings
// trust center calls. The dial, ceiling, and kill-switch settings are
// mutations (Tauri command path only, AD-14) appended as events through the
// domain's typed constructors; the status is a read over the shared core.
// Error strings lead with stable codes and stay in code form — bilingual-safe
// by construction (EXPERIENCE.md).

use crate::db::Db;
use crate::trust::{self, TrustStatus};
use tauri::State;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// The trust status (FR-5): runtime state, effective dials and ceilings at
/// every scope, current spend vs ceiling per scope, and the last run's spend
/// line — everything the trust center renders.
#[tauri::command]
pub async fn get_trust_status(db: State<'_, Db>) -> Result<TrustStatus, String> {
    trust::status(db.inner()).await
}

/// Configure the autonomy dial at one scope (FR-5.1): `watch | suggest |
/// act_with_receipts` at `global | mission | target`. Appends one
/// `autonomy.configured` event (latest wins per scope; across scopes,
/// most-restrictive-wins — AD-15d) and returns the fresh status.
#[tauri::command]
pub async fn configure_autonomy(
    db: State<'_, Db>,
    scope: String,
    scope_id: Option<String>,
    mode: String,
) -> Result<TrustStatus, String> {
    trust::configure_autonomy(db.inner(), &scope, scope_id, &mode).await
}

/// Configure a spend ceiling at one scope (FR-5.2): cents, at
/// `global | mission | target`. Appends one `ceiling.configured` event — a
/// mission's configured ceiling can only tighten its creation ceiling — and
/// returns the fresh status.
#[tauri::command]
pub async fn configure_ceiling(
    db: State<'_, Db>,
    scope: String,
    scope_id: Option<String>,
    ceiling_cents: u64,
) -> Result<TrustStatus, String> {
    trust::configure_ceiling(db.inner(), &scope, scope_id, ceiling_cents).await
}

/// Fire the kill switch (AD-15e): appends `runtime.killed` — a runtime-owned
/// core command. While it is the latest runtime-state event by seq, every
/// dispatch is refused (including the Night Shift tick); the command path is
/// the only way it fires.
#[tauri::command]
pub async fn kill_runtime(db: State<'_, Db>) -> Result<TrustStatus, String> {
    trust::kill_runtime(db.inner()).await.map_err(err)
}

/// Resume a killed runtime (AD-15e): appends `runtime.resumed` — dispatch is
/// restored.
#[tauri::command]
pub async fn resume_runtime(db: State<'_, Db>) -> Result<TrustStatus, String> {
    trust::resume_runtime(db.inner()).await.map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::missions::{Autonomy, MissionCreatedPayload, SpendState};
    use crate::eventstore::{EventStore, NewEvent};
    use rusqlite::Connection;

    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::Db::migrate(&conn).unwrap();
        EventStore::init(&conn).unwrap();
        Db(std::sync::Arc::new(tokio::sync::Mutex::new(conn)))
    }

    #[tokio::test]
    async fn the_full_command_surface_round_trips_through_the_log() {
        let db = test_db();
        let mission = {
            let conn = db.0.lock().await;
            EventStore::new(&conn)
                .append(
                    NewEvent::mission_created(MissionCreatedPayload {
                        question: "Does X hold?".into(),
                        stop_condition: "Stop after $5.".into(),
                        success_criterion: "A rater agrees.".into(),
                        autonomy: Autonomy::Suggest,
                        spend_ceiling_cents: 100,
                        roles: vec![],
                        schedule: "off".into(),
                    })
                    .unwrap(),
                )
                .unwrap()
        };
        // dial + ceiling at every scope
        let status = trust::configure_autonomy(&db, "global", None, "suggest")
            .await
            .unwrap();
        assert_eq!(status.global_autonomy, Some(Autonomy::Suggest));
        let status = trust::configure_autonomy(
            &db,
            "mission",
            Some(mission.id.to_string()),
            "watch",
        )
        .await
        .unwrap();
        assert_eq!(status.mission_dials.len(), 1);
        assert_eq!(status.mission_dials[0].mode, Autonomy::Watch);
        let status = trust::configure_ceiling(&db, "target", Some("openai".into()), 500)
            .await
            .unwrap();
        assert_eq!(status.target_ceilings.len(), 1);
        assert_eq!(status.target_ceilings[0].ceiling_cents, 500);
        // the mission meter renders against the creation ceiling
        assert_eq!(status.missions.len(), 1);
        assert_eq!(status.missions[0].ceiling_cents, 100);
        assert_eq!(status.missions[0].state, SpendState::Ok);
        // kill → refused dispatch; resume → restored (the command path)
        let status = trust::kill_runtime(&db).await.unwrap();
        assert_eq!(status.runtime_state, trust::RuntimeState::Killed);
        let status = trust::resume_runtime(&db).await.unwrap();
        assert_eq!(status.runtime_state, trust::RuntimeState::Running);
    }
}
