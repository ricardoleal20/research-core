// Submission checklist shell commands (Story 6.13, FR-19.4): choosing a
// venue spawns the submission mission; the human checks items directly
// (evented, actor=user); the agent pre-check NEVER appends — each
// machine item it can verify lands as a proposal with evidence, awaiting
// the owner's merge (AD-3, AD-15d). The mission completes only when
// every item is checked (AD-12 — no checklist rests without a terminal
// state), and the "ready to submit?" verdict rides the tier-2 gate
// (Story 6.11). Error strings lead with stable codes and stay in code
// form — bilingual-safe by construction (EXPERIENCE.md).

use tauri::State;
use uuid::Uuid;

use crate::db::Db;
use crate::domain::journals;
use crate::domain::manuscript::{build_scan, ManuscriptsProjection};
use crate::domain::missions::MissionStatus;
use crate::domain::nightshift::{RUN_FINISHED, RUN_STARTED};
use crate::domain::proposals::ProposalsProjection;
use crate::domain::readiness::{tier_two_report, TierTwoReport};
use crate::domain::submissions::{
    SubmissionMission, SubmissionProjection, SubmissionCreatedPayload,
};
use crate::eventstore::{EventStore, NewEvent};

fn err(e: impl ToString) -> String {
    e.to_string()
}

fn parse_mission(mission_id: &str) -> Result<Uuid, String> {
    mission_id
        .trim()
        .parse()
        .map_err(|e| format!("invalid_mission_id: `{mission_id}`: {e}"))
}

/// One submission mission's full view (the checklist mission card's
/// read): the mission with its per-item state + the "ready to submit?"
/// verdict via the tier-2 gate — the same derived fold the readiness
/// drawer renders, scoped to the submission's source mission, with the
/// human confirmations taken from THIS checklist.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionView {
    pub submission: SubmissionMission,
    pub readiness: TierTwoReport,
    /// AD-12, derived: every item checked — the mission's success
    /// criterion, satisfied or not.
    pub all_checked: bool,
}

/// Choose a venue (FR-19.4): spawn the submission mission whose items
/// come from the venue template. `mission_id` is the research mission
/// the submission derives from (the checklist's board scope); None
/// spawns a workspace-level checklist. Actor is always the user — the
/// agent path (the Fit Finder, Story 6.12) proposes this same event and
/// the merge applies it.
#[tauri::command]
pub async fn create_submission_mission(
    db: State<'_, Db>,
    mission_id: Option<String>,
    venue_id: String,
) -> Result<SubmissionView, String> {
    let source = match mission_id.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => Some(
            raw.parse()
                .map_err(|e| format!("invalid_mission_id: `{raw}`: {e}"))?,
        ),
    };
    let c = db.0.lock().await;
    let venue = journals::venue(&venue_id).ok_or_else(|| {
        format!("unknown_venue: `{venue_id}` — the bundled dataset carries no such venue")
    })?;
    let event = NewEvent::submission_created(SubmissionCreatedPayload {
        question: format!("Envío a {}", venue.name),
        stop_condition: "submission-ready".into(),
        success_criterion: "todos los elementos del checklist marcados".into(),
        venue_id: venue.id.to_string(),
        source_mission_id: source,
    })
    .map_err(err)?;
    let stored = EventStore::new(&c).append(event).map_err(err)?;
    submission_view_inner(&c, stored.id)
}

/// Every submission mission, in creation order (the Publication
/// surface's checklist section).
#[tauri::command]
pub async fn list_submissions(
    db: State<'_, Db>,
) -> Result<Vec<SubmissionMission>, String> {
    let c = db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(err)?;
    SubmissionProjection::fold(&events).map_err(err)
}

/// One submission mission's view — the checklist + the tier-2 "ready to
/// submit?" verdict.
#[tauri::command]
pub async fn get_submission(
    db: State<'_, Db>,
    mission_id: String,
) -> Result<SubmissionView, String> {
    let id = parse_mission(&mission_id)?;
    let c = db.0.lock().await;
    submission_view_inner(&c, id)
}

/// The human check (FR-19.4): the owner checks an item — machine or
/// human — directly, evented with the user's audit stamp.
#[tauri::command]
pub async fn check_submission_item(
    db: State<'_, Db>,
    mission_id: String,
    item_id: String,
    note: Option<String>,
) -> Result<SubmissionView, String> {
    let id = parse_mission(&mission_id)?;
    let c = db.0.lock().await;
    let submission = require_submission(&c, id)?;
    let event = NewEvent::submission_item_checked(
        id,
        &submission.venue_id,
        &item_id,
        note.as_deref(),
    )
    .map_err(err)?;
    EventStore::new(&c).append(event).map_err(err)?;
    submission_view_inner(&c, id)
}

/// The human correction: uncheck an item (evented — nothing disappears,
/// the log keeps both moves).
#[tauri::command]
pub async fn uncheck_submission_item(
    db: State<'_, Db>,
    mission_id: String,
    item_id: String,
) -> Result<SubmissionView, String> {
    let id = parse_mission(&mission_id)?;
    let c = db.0.lock().await;
    let submission = require_submission(&c, id)?;
    let event =
        NewEvent::submission_item_unchecked(id, &submission.venue_id, &item_id, None)
            .map_err(err)?;
    EventStore::new(&c).append(event).map_err(err)?;
    submission_view_inner(&c, id)
}

/// The agent pre-check (FR-19.4, AD-3/AD-15d): for every MACHINE item of
/// the checklist that the tier-2 gate verifies as passing and that is
/// not yet checked, propose the check — one proposal per item, each
/// carrying the machine evidence in its note, each awaiting the owner's
/// merge. NOTHING checks itself silently: the board is unchanged until
/// the merges land. Human-only items are never proposed (the proposal
/// edge refuses them structurally).
#[tauri::command]
pub async fn precheck_submission_items(
    db: State<'_, Db>,
    mission_id: String,
) -> Result<Vec<crate::domain::proposals::Proposal>, String> {
    let id = parse_mission(&mission_id)?;
    let c = db.0.lock().await;
    precheck_inner(&c, id)
}

/// Complete the submission mission (AD-12): every item must be checked —
/// the success criterion satisfied — else the typed refusal says what
/// rests. The terminal event carries its timestamp and reason.
#[tauri::command]
pub async fn complete_submission_mission(
    db: State<'_, Db>,
    mission_id: String,
) -> Result<SubmissionView, String> {
    let id = parse_mission(&mission_id)?;
    let c = db.0.lock().await;
    let submission = require_submission(&c, id)?;
    if submission.status != MissionStatus::Active {
        return Err(format!(
            "not_active: the submission mission is `{}` — a decided mission never decides \
             again (AD-12)",
            submission.status.as_str()
        ));
    }
    let unchecked = submission.items.iter().filter(|i| i.checked.is_none()).count();
    if unchecked > 0 {
        return Err(format!(
            "items_pending: {unchecked} checklist items unchecked — a submission mission \
             completes when every item is checked (AD-12)"
        ));
    }
    EventStore::new(&c)
        .append(
            NewEvent::new(
                crate::domain::missions::MISSION_COMPLETED,
                crate::eventstore::Actor::User,
                serde_json::json!({ "reason": "submission-ready", "mission_id": id.to_string() }),
            )
            .map_err(err)?
            .with_causes(vec![id]),
        )
        .map_err(err)?;
    submission_view_inner(&c, id)
}

/// Stop the submission mission (AD-12's other terminal): the owner
/// abandons the venue with a reason — no checklist rests without one.
#[tauri::command]
pub async fn stop_submission_mission(
    db: State<'_, Db>,
    mission_id: String,
    reason: String,
) -> Result<SubmissionView, String> {
    let id = parse_mission(&mission_id)?;
    if reason.trim().is_empty() {
        return Err(
            "stop_reason_required: stopping a mission records its reason (AD-12)".into(),
        );
    }
    let c = db.0.lock().await;
    let submission = require_submission(&c, id)?;
    if submission.status != MissionStatus::Active {
        return Err(format!(
            "not_active: the submission mission is `{}` — a decided mission never decides \
             again (AD-12)",
            submission.status.as_str()
        ));
    }
    EventStore::new(&c)
        .append(
            NewEvent::new(
                crate::domain::missions::MISSION_STOPPED,
                crate::eventstore::Actor::User,
                serde_json::json!({ "reason": reason.trim(), "mission_id": id.to_string() }),
            )
            .map_err(err)?
            .with_causes(vec![id]),
        )
        .map_err(err)?;
    submission_view_inner(&c, id)
}

/// The submission mission of an id, or the typed not-found.
fn require_submission(conn: &rusqlite::Connection, id: Uuid) -> Result<SubmissionMission, String> {
    let events = EventStore::new(conn).events_all().map_err(err)?;
    SubmissionProjection::for_mission(&events, id)
        .map_err(err)?
        .ok_or_else(|| format!("not_found: no submission mission with id `{id}`"))
}

/// The full view: the folded submission + the tier-2 "ready to submit?"
/// verdict — the gate scoped to the submission's source mission, the
/// human confirmations taken from THIS checklist's user-checked human
/// items (a venue's human criterion is confirmed when its owner checked
/// it here).
fn submission_view_inner(
    conn: &rusqlite::Connection,
    id: Uuid,
) -> Result<SubmissionView, String> {
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let submission = SubmissionProjection::for_mission(&events, id)
        .map_err(err)?
        .ok_or_else(|| format!("not_found: no submission mission with id `{id}`"))?;
    let readiness = submission_readiness_inner(conn, &events, &submission)?;
    let all_checked = submission.all_checked();
    Ok(SubmissionView {
        submission,
        readiness,
        all_checked,
    })
}

/// The tier-2 verdict of one submission (the mission card's "ready to
/// submit?" line): the same pure fold the readiness drawer renders,
/// scoped to the submission's source mission, confirmed by this
/// checklist's human items.
pub(crate) fn submission_readiness_inner(
    conn: &rusqlite::Connection,
    events: &[crate::eventstore::StoredEvent],
    submission: &SubmissionMission,
) -> Result<TierTwoReport, String> {
    let Some(venue) = journals::venue(&submission.venue_id) else {
        return Err(format!(
            "unknown_venue: `{}` — the bundled dataset carries no such venue",
            submission.venue_id
        ));
    };
    let scope = submission.source_mission_id;
    let manuscripts: Vec<crate::domain::manuscript::Manuscript> =
        ManuscriptsProjection::fold(events)
            .map_err(err)?
            .into_iter()
            .filter(|m| scope.is_none_or(|id| m.mission_id == id))
            .collect();
    let scans: Vec<_> = manuscripts.iter().map(build_scan).collect();
    let stats: Vec<_> = manuscripts.iter().map(journals::build_venue_stats).collect();
    let refs = crate::library_commands::list_refs_inner(conn, None, Some("active"))
        .map_err(err)?;
    tier_two_report(
        events,
        scope,
        venue,
        &scans,
        &stats,
        &refs,
        &submission.human_confirmed(),
    )
    .map_err(err)
}

/// The agent pre-check inner (testable without Tauri state): compute the
/// tier-2 machine verdicts of the submission's venue+scope, then propose
/// the check for every PASSING machine item not yet checked — one
/// proposal per item, each with its machine evidence note, each
/// cause-linked to a run the receipts drill-down can find. The run is
/// evented (run.started/run.finished) so the pre-check has a receipt
/// like every agent move.
fn precheck_inner(
    conn: &rusqlite::Connection,
    id: Uuid,
) -> Result<Vec<crate::domain::proposals::Proposal>, String> {
    let store = EventStore::new(conn);
    let events = store.events_all().map_err(err)?;
    let submission = SubmissionProjection::for_mission(&events, id)
        .map_err(err)?
        .ok_or_else(|| format!("not_found: no submission mission with id `{id}`"))?;
    if submission.status != MissionStatus::Active {
        return Err(format!(
            "not_active: the submission mission is `{}` — a decided mission's checklist \
             never changes (AD-12)",
            submission.status.as_str()
        ));
    }
    let report = submission_readiness_inner(conn, &events, &submission)?;
    let venue = journals::venue(&submission.venue_id)
        .ok_or_else(|| format!("unknown_venue: `{}`", submission.venue_id))?;

    // The run anchor (receipts): one pre-check run, evented.
    let run_id = format!("submission-precheck-{}", Uuid::new_v4().simple());
    store
        .append(
            NewEvent::run_started(&run_id, id, "manual", "submission-precheck").map_err(err)?,
        )
        .map_err(err)?;

    let mut proposed = 0usize;
    for item in report.items.iter().filter(|i| i.kind != "human_only") {
        if item.status != crate::domain::readiness::TierTwoStatus::Pass {
            continue; // only passing machine checks are proposed — an
                      // honest pre-check never proposes a failure
        }
        if submission
            .items
            .iter()
            .any(|x| x.item_id == item.criterion_id && x.checked.is_some())
        {
            continue; // already checked — never re-proposed
        }
        let intended = NewEvent::submission_item_checked(
            id,
            &submission.venue_id,
            &item.criterion_id,
            Some(&format!(
                "machine check passed: {}{}",
                item.kind,
                item.detail.as_deref().map(|d| format!(" ({d})")).unwrap_or_default()
            )),
        )
        .map_err(err)?;
        let basis = store.events_all().map_err(err)?.last().map(|e| e.seq).unwrap_or(1);
        let cause = store
            .events_all()
            .map_err(err)?
            .iter()
            .rev()
            .find(|e| e.kind == RUN_STARTED)
            .map(|e| e.id)
            .ok_or_else(|| "run_started: the pre-check run anchor is missing".to_string())?;
        store
            .append(
                NewEvent::proposal_created(&run_id, &intended, id, basis, cause)
                    .map_err(err)?
                    .with_causes(vec![id]),
            )
            .map_err(err)?;
        proposed += 1;
    }

    store
        .append(
            NewEvent::run_finished(
                &run_id,
                id,
                &format!("pre-check: {proposed} proposals"),
                proposed as u32,
            )
            .map_err(err)?,
        )
        .map_err(err)?;

    let events = store.events_all().map_err(err)?;
    let proposals = ProposalsProjection::fold(&events).map_err(err)?;
    Ok(proposals
        .into_iter()
        .filter(|p| p.run_id == run_id && p.status == crate::domain::proposals::ProposalStatus::Pending)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::missions::{Autonomy, MissionCreatedPayload};

    fn test_db() -> (Db, std::path::PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("rc-submissions-cmd-test-{}.sqlite", uuid::Uuid::new_v4()));
        let db = Db::open(&path).unwrap();
        (db, path)
    }

    fn cleanup(path: &std::path::PathBuf) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    }

    fn seed(conn: &rusqlite::Connection) -> (uuid::Uuid, uuid::Uuid) {
        let store = EventStore::new(conn);
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
        let submission = create_submission_inner(conn, Some(mission), "siam-jsc").unwrap();
        (mission, submission)
    }

    fn create_submission_inner(
        conn: &rusqlite::Connection,
        mission: Option<uuid::Uuid>,
        venue_id: &str,
    ) -> Result<uuid::Uuid, String> {
        let venue = journals::venue(venue_id).ok_or_else(|| {
            format!("unknown_venue: `{venue_id}` — the bundled dataset carries no such venue")
        })?;
        let event = NewEvent::submission_created(SubmissionCreatedPayload {
            question: format!("Envío a {}", venue.name),
            stop_condition: "submission-ready".into(),
            success_criterion: "todos los elementos del checklist marcados".into(),
            venue_id: venue.id.to_string(),
            source_mission_id: mission,
        })
        .map_err(err)?;
        Ok(EventStore::new(conn).append(event).map_err(err)?.id)
    }

    /// The full loop, end to end: spawn → human checks → agent pre-check
    /// proposes only passing machine items → merging applies with agent
    /// attribution → completing refuses while items rest → the last
    /// checks complete the mission (AD-12).
    #[tokio::test]
    async fn the_submission_loop_runs_end_to_end() {
        let (db, path) = test_db();
        {
            let c = db.0.lock().await;
            let (_mission, submission) = seed(&c);
            // completing refuses: items rest unchecked
            let e = complete_submission_inner(&c, submission).await.unwrap_err();
            assert!(e.starts_with("items_pending:"), "{e}");

            // the agent pre-check: no manuscript registered — the
            // manuscript-dependent checks all fail honestly (never a
            // proposed failure), and the one check that passes over an
            // empty board (load-bearing support, 0/0 claims) is proposed
            // with its evidence, awaiting the owner's merge (AD-3).
            let proposals = precheck_inner(&c, submission).unwrap();
            assert_eq!(proposals.len(), 1, "exactly the vacuously-passing support check");
            let proposed = &proposals[0];
            assert_eq!(
                proposed.proposed_payload["item_id"].as_str(),
                Some("load_bearing_support")
            );
            assert!(
                proposed.proposed_payload["note"]
                    .as_str()
                    .map(|n| n.contains("0/0 supported"))
                    .unwrap_or(false),
                "the machine evidence rides the proposal note"
            );

            // reject the pre-check (a human verdict on a machine proposal
            // is still the human's to give), then the owner checks every
            // item by hand — human items included
            EventStore::new(&c)
                .append(
                    NewEvent::merge_rejected(proposals[0].id, None).unwrap(),
                )
                .unwrap();
            let events = EventStore::new(&c).events_all().unwrap();
            let folded = SubmissionProjection::for_mission(&events, submission)
                .unwrap()
                .unwrap();
            for item in &folded.items {
                let event = NewEvent::submission_item_checked(
                    submission,
                    &folded.venue_id,
                    &item.item_id,
                    None,
                )
                .unwrap();
                EventStore::new(&c).append(event).unwrap();
            }
            let view = submission_view_inner(&c, submission).unwrap();
            assert!(view.all_checked);
            // every item is attributed to the user
            assert!(view
                .submission
                .items
                .iter()
                .all(|i| i.checked.as_ref().map(|s| s.actor.as_str()) == Some("user")));
            // the human items confirmed → the tier-2 verdict's human
            // items render confirmed
            assert!(view
                .readiness
                .items
                .iter()
                .filter(|i| i.kind == "human_only")
                .all(|i| i.status
                    == crate::domain::readiness::TierTwoStatus::HumanConfirmed));

            // completing now lands — terminal with reason
            let view = complete_submission_inner(&c, submission).await.unwrap();
            assert_eq!(view.submission.status, MissionStatus::Completed);
            // a second decision is refused
            let e = complete_submission_inner(&c, submission).await.unwrap_err();
            assert!(e.starts_with("not_active:"), "{e}");
        }
        cleanup(&path);
    }

    /// The agent pre-check proposes ONLY PASSING machine items — and the
    /// merge applies the agent's attribution. With no manuscript, every
    /// manuscript-dependent check fails honestly (never proposed), and
    /// the one passing check (load-bearing support over an empty board)
    /// IS proposed with its evidence.
    #[tokio::test]
    async fn the_precheck_proposes_only_passing_machine_items() {
        let (db, path) = test_db();
        {
            let c = db.0.lock().await;
            let (_mission, submission) = seed(&c);
            // hand-check the human items (the only things an agent can
            // never do)
            let events = EventStore::new(&c).events_all().unwrap();
            let folded = SubmissionProjection::for_mission(&events, submission)
                .unwrap()
                .unwrap();
            for item in folded.items.iter().filter(|i| i.human) {
                EventStore::new(&c)
                    .append(
                        NewEvent::submission_item_checked(
                            submission,
                            &folded.venue_id,
                            &item.item_id,
                            None,
                        )
                        .unwrap(),
                    )
                    .unwrap();
            }
            let proposals = precheck_inner(&c, submission).unwrap();
            assert_eq!(proposals.len(), 1, "only the passing check is proposed");
            assert!(proposals.iter().all(|p| {
                p.proposed_payload["item_id"].as_str() == Some("load_bearing_support")
            }));
            // the merge applies the agent's attribution
            EventStore::new(&c)
                .append(NewEvent::merge_approved(proposals[0].id, false, false, None).unwrap())
                .unwrap();
            let events = EventStore::new(&c).events_all().unwrap();
            let folded = SubmissionProjection::for_mission(&events, submission)
                .unwrap()
                .unwrap();
            let stamp = folded
                .items
                .iter()
                .find(|i| i.item_id == "load_bearing_support")
                .unwrap()
                .checked
                .clone()
                .expect("applied at merge");
            assert!(stamp.actor.starts_with("agent:"), "agent attribution: {}", stamp.actor);
            assert!(stamp.note.as_deref().unwrap().contains("0/0 supported"));
        }
        cleanup(&path);
    }

    /// An unknown venue and an unknown mission are typed errors.
    #[tokio::test]
    async fn typed_errors_at_the_edges() {
        let (db, path) = test_db();
        {
            let c = db.0.lock().await;
            let e = create_submission_inner(&c, None, "no-such-venue").unwrap_err();
            assert!(e.starts_with("unknown_venue:"), "{e}");
            let e = submission_view_inner(&c, uuid::Uuid::new_v4()).unwrap_err();
            assert!(e.starts_with("not_found:"), "{e}");
            let e = check_inner(&c, uuid::Uuid::new_v4(), "compiled_pdf").unwrap_err();
            assert!(e.starts_with("not_found:"), "{e}");
        }
        cleanup(&path);
    }

    fn check_inner(
        conn: &rusqlite::Connection,
        id: uuid::Uuid,
        item_id: &str,
    ) -> Result<(), String> {
        let submission = require_submission(conn, id)?;
        let event = NewEvent::submission_item_checked(
            id,
            &submission.venue_id,
            item_id,
            None,
        )
        .map_err(err)?;
        EventStore::new(conn).append(event).map_err(err)?;
        Ok(())
    }

    async fn complete_submission_inner(
        conn: &rusqlite::Connection,
        id: uuid::Uuid,
    ) -> Result<SubmissionView, String> {
        let submission = require_submission(conn, id)?;
        if submission.status != MissionStatus::Active {
            return Err(format!(
                "not_active: the submission mission is `{}`",
                submission.status.as_str()
            ));
        }
        let unchecked = submission.items.iter().filter(|i| i.checked.is_none()).count();
        if unchecked > 0 {
            return Err(format!("items_pending: {unchecked} items unchecked"));
        }
        EventStore::new(conn)
            .append(
                NewEvent::new(
                    crate::domain::missions::MISSION_COMPLETED,
                    crate::eventstore::Actor::User,
                    serde_json::json!({ "reason": "submission-ready", "mission_id": id.to_string() }),
                )
                .map_err(err)?
                .with_causes(vec![id]),
            )
            .map_err(err)?;
        submission_view_inner(conn, id)
    }
}
