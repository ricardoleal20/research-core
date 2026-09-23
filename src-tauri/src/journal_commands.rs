// Journal-targeting shell commands (Story 6.11, FR-19.1/19.2): the
// venue-template read and the tier-2 "journal-ready" report. Both are
// READS over the bundled dataset + the shared log — the dataset is data
// (no table, no event, FR-13.2), the verdict a pure derived projection
// (asking again re-folds). Error strings lead with stable codes
// (`unknown_venue:`, `invalid_mission_id:`) and stay in code form —
// bilingual-safe by construction (EXPERIENCE.md).

use rusqlite::Connection;
use tauri::State;
use uuid::Uuid;

use crate::db::Db;
use crate::domain::journals::{self, VenueTemplate};
use crate::domain::manuscript::{build_scan, ManuscriptsProjection};
use crate::domain::readiness::{tier_two_report, TierTwoReport};
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

/// Every seeded venue template (FR-19.2): the Publication surface's list —
/// identity, scope tags, and the criteria (machine-checkable and
/// human-only), straight from the bundled local dataset (NFR-1: no cloud
/// fetches, ever).
#[tauri::command]
pub async fn list_venues() -> Result<Vec<VenueTemplate>, String> {
    Ok(journals::venues().to_vec())
}

/// The tier-2 journal-ready report of one venue (Story 6.11, FR-19.1):
/// `mission_id` scopes the gate to one mission's board (None = the whole
/// workspace); `venue_id` selects the venue whose checklist gates the
/// verdict. Every blocking item references the specific board or
/// manuscript object that blocks it; human-only items render pending
/// until the submission checklist carries their human confirmation
/// (Story 6.13).
#[tauri::command]
pub async fn get_journal_readiness(
    db: State<'_, Db>,
    mission_id: Option<String>,
    venue_id: String,
) -> Result<TierTwoReport, String> {
    let mission = parse_mission(mission_id.as_deref())?;
    let c = db.0.lock().await;
    journal_readiness_inner(&c, mission, &venue_id)
}

/// Plain inner (testable without Tauri state; the read-only server route
/// serves the same read over the shared core): fold the tier-2 report.
/// The manuscripts in scope are scanned from disk here — the ONLY place
/// the gate touches the filesystem (the `readiness_report_inner`
/// precedent) — and the library read feeds the references-resolved
/// check. The human confirmations come from the submission checklist's
/// evented human checks for this venue (Story 6.13); before any
/// submission mission exists the human items render pending, never
/// auto-passed.
pub(crate) fn journal_readiness_inner(
    conn: &Connection,
    mission: Option<Uuid>,
    venue_id: &str,
) -> Result<TierTwoReport, String> {
    let Some(venue) = journals::venue(venue_id) else {
        return Err(format!(
            "unknown_venue: `{venue_id}` — the bundled dataset carries no such venue"
        ));
    };
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let manuscripts: Vec<crate::domain::manuscript::Manuscript> =
        ManuscriptsProjection::fold(&events)
            .map_err(err)?
            .into_iter()
            .filter(|m| mission.is_none_or(|id| m.mission_id == id))
            .collect();
    let scans: Vec<_> = manuscripts.iter().map(build_scan).collect();
    let stats: Vec<_> = manuscripts.iter().map(journals::build_venue_stats).collect();
    let refs = crate::library_commands::list_refs_inner(conn, None, Some("active"))
        .map_err(err)?;
    // The human confirmations come from the submission checklist's
    // evented human checks for this venue (Story 6.13): a user-checked
    // human item confirms the venue's criterion. Before any submission
    // mission exists there is nothing to confirm against — the human
    // items render pending, never auto-passed.
    let human_confirmed: Vec<String> =
        crate::domain::submissions::human_confirmed_for_venue(&events, mission, &venue.id)
            .map_err(err)?;
    tier_two_report(&events, mission, venue, &scans, &stats, &refs, &human_confirmed)
        .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::missions::{Autonomy, MissionCreatedPayload};
    use crate::eventstore::NewEvent;

    fn test_db() -> (Db, std::path::PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("rc-journal-cmd-test-{}.sqlite", uuid::Uuid::new_v4()));
        let db = Db::open(&path).unwrap();
        (db, path)
    }

    fn cleanup(path: &std::path::PathBuf) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    }

    /// The venues command reads the bundled dataset — every seeded venue
    /// with its criteria counts.
    #[tokio::test]
    async fn the_venues_read_lists_the_bundled_dataset() {
        let venues = list_venues().await.unwrap();
        assert!(venues.len() >= 5);
        assert!(venues.iter().all(|v| !v.criteria.is_empty()));
    }

    /// An unknown venue fails with the stable code; a known venue folds
    /// the tier-2 report over the shared log (a dirty board is never
    /// journal-ready).
    #[tokio::test]
    async fn the_journal_readiness_folds_over_the_shared_log() {
        let (db, path) = test_db();
        {
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

            let e = journal_readiness_inner(&c, None, "no-such-venue").unwrap_err();
            assert!(e.starts_with("unknown_venue:"), "stable code first: {e}");

            let report =
                journal_readiness_inner(&c, Some(mission), "siam-jsc").unwrap();
            assert_eq!(report.venue_id, "siam-jsc");
            assert_eq!(report.scope, Some(mission));
            // the unpinned claim blocks tier 1 — tier 2 builds on it
            assert_eq!(
                report.tier_one,
                crate::domain::readiness::ReadinessVerdict::NotReady
            );
            assert_eq!(
                report.verdict,
                crate::domain::readiness::ReadinessVerdict::NotReady
            );
            // the manuscript-dependent checks fail honestly: no
            // manuscript registered for this scope
            assert!(report
                .items
                .iter()
                .any(|i| i.detail.as_deref() == Some("no_manuscript")));
        }
        cleanup(&path);
    }
}
