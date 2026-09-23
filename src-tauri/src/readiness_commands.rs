// Readiness shell commands (AD-15a, Story 4.3, FR-13): the read-model entry
// the Readiness Report view calls — the preprint-tier gate, derived from
// board state (FR-13.1) with NO new data model (FR-13.2): no readiness
// table, no readiness event, nothing but the pure fold over the log. Asking
// again re-folds; replaying the same events yields the same report.
//
// This command is a READ (Tauri command + the read-only server route the
// served browser view renders from — the same core instance, one writer,
// AD-14): the gate cannot be "run" into existence, only asked. Error
// strings lead with stable codes (`invalid_mission_id:`) and stay in code
// form — bilingual-safe by construction (EXPERIENCE.md).

use rusqlite::Connection;
use tauri::State;
use uuid::Uuid;

use crate::db::Db;
use crate::domain::manuscript::{build_scan, ManuscriptsProjection};
use crate::domain::readiness::{readiness_report_with_manuscript, ReadinessReport};
use crate::eventstore::EventStore;

fn err(e: impl ToString) -> String {
    e.to_string()
}

fn parse_mission(mission_id: Option<&str>) -> Result<Option<Uuid>, String> {
    match mission_id.map(str::trim) {
        None | Some("") => Ok(None),
        Some(raw) => raw
            .parse()
            .map(Some)
            .map_err(|e| format!("invalid_mission_id: `{raw}`: {e}")),
    }
}

/// The readiness report of a scope (FR-13.1/13.2): `mission_id` scopes the
/// gate to one mission's board; None asks the whole workspace. Every
/// blocking item references the specific board object that blocks it; a
/// clean scope reports `ready` with the evidence trail that justifies it.
#[tauri::command]
pub async fn get_readiness_report(
    db: State<'_, Db>,
    mission_id: Option<String>,
) -> Result<ReadinessReport, String> {
    let mission = parse_mission(mission_id.as_deref())?;
    let c = db.0.lock().await;
    readiness_report_inner(&c, mission)
}

/// Plain inner (testable without Tauri state; the server route serves the
/// same read over the shared core): fold the report. The manuscript scope
/// (Story 6.8, FR-20.5): every registered manuscript in scope is scanned
/// from disk here — the ONLY place the gate touches the filesystem — and
/// the pure fold consumes the scan (same log + same files, same report,
/// FR-13.2). No manuscript registered, no manuscript scope (progressive
/// disclosure).
pub(crate) fn readiness_report_inner(
    conn: &Connection,
    mission: Option<Uuid>,
) -> Result<ReadinessReport, String> {
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let scans: Vec<crate::domain::manuscript::ManuscriptScan> = ManuscriptsProjection::fold(&events)
        .map_err(err)?
        .iter()
        .filter(|m| mission.is_none_or(|id| m.mission_id == id))
        .map(build_scan)
        .collect();
    readiness_report_with_manuscript(&events, mission, &scans).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::missions::{Autonomy, MissionCreatedPayload};
    use crate::eventstore::NewEvent;

    fn test_db() -> (Db, std::path::PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("rc-readiness-cmd-test-{}.sqlite", uuid::Uuid::new_v4()));
        let db = Db::open(&path).unwrap();
        (db, path)
    }

    fn cleanup(path: &std::path::PathBuf) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    }

    /// Seed one mission + a testing hypothesis + an unpinned claim (a dirty
    /// board); returns (db, path, mission id, hypothesis id).
    async fn seed_dirty(db: &Db) -> (uuid::Uuid, uuid::Uuid) {
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        let mission = store
            .append(
                NewEvent::mission_created(MissionCreatedPayload {
                    question: "Does X hold up?".into(),
                    stop_condition: "Stop after $5.".into(),
                    success_criterion: "A blind rater agrees.".into(),
                    autonomy: Autonomy::Suggest,
                    spend_ceiling_cents: 500,
                    schedule: "daily-03:00".into(),
                    roles: vec![],
                })
                .unwrap(),
            )
            .unwrap()
            .id;
        let h = store
            .append(NewEvent::hypothesis_created("X holds.", mission).unwrap())
            .unwrap()
            .id;
        store
            .append(NewEvent::claim_registered("X holds at 32k.", h, None).unwrap())
            .unwrap();
        (mission, h)
    }

    /// The command inner folds the report off the shared log — mission-scoped
    /// and workspace-wide, with the blocking objects referenced by id.
    #[tokio::test]
    async fn the_command_folds_the_report_over_the_shared_log() {
        let (db, path) = test_db();
        let (mission, h) = seed_dirty(&db).await;
        {
            let c = db.0.lock().await;
            let report = readiness_report_inner(&c, None).unwrap();
            assert_eq!(report.verdict, crate::domain::readiness::ReadinessVerdict::NotReady);
            // the unpinned claim references its hypothesis
            let unpinned = report
                .blockers
                .iter()
                .find(|b| b.kind == crate::domain::readiness::ReadinessItemKind::UnpinnedClaim)
                .unwrap();
            assert_eq!(unpinned.hypothesis_id, Some(h));
            // scoped: the same board, the same blockers
            let scoped = readiness_report_inner(&c, Some(mission)).unwrap();
            assert_eq!(scoped.scope, Some(mission));
            assert_eq!(scoped.blockers.len(), report.blockers.len());
        }
        cleanup(&path);
    }

    /// A malformed mission id fails with the stable code, folding nothing.
    #[tokio::test]
    async fn a_bad_scope_fails_with_a_coded_error() {
        let (db, path) = test_db();
        // an unknown scope is an honest empty read, not an error
        {
            let c = db.0.lock().await;
            let empty = readiness_report_inner(&c, Some(uuid::Uuid::new_v4())).unwrap();
            assert_eq!(empty.verdict, crate::domain::readiness::ReadinessVerdict::Ready);
            assert!(empty.blockers.is_empty());
        }
        let e = parse_mission(Some("not-a-uuid")).unwrap_err();
        assert!(e.starts_with("invalid_mission_id:"), "stable code first: {e}");
        cleanup(&path);
    }
}
