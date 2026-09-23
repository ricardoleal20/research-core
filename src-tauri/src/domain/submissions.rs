// The submission checklist as a mission (Story 6.13, FR-19.4): choosing a
// venue spawns a SUBMISSION MISSION — stop condition "submission-ready",
// success criterion "all items checked" (AD-12) — whose items come from
// the venue template (data, Story 6.11). Item state is EVENTED with audit
// stamps (FR-2.2 discipline): a human check appends `submission.item_checked`
// (actor=user); an agent pre-check NEVER appends — it lands as a proposal
// (AD-3, AD-15d) whose merge applies the intended check with the agent's
// attribution, exactly the hypothesis-transition pattern. Human-only
// items are never agent-checkable: the proposal validation refuses an
// intended check of a human criterion at the edge.
//
// Terminal state (AD-12): a submission mission completes only when every
// item is checked (`mission.completed`, reason `submission-ready`) and
// can be stopped with a reason — no checklist rests without one.
//
// The model reuses missions: `submission.created` IS a mission birth (the
// MissionsProjection folds it — the checklist appears on the missions
// home and dashboard like any mission) bound to its venue. No new store
// exists: the checklist is a projection over the log + the venue
// template (FR-13.2's spirit — the template is data, the state is
// events).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::checkpoints::FoldCursor;
use crate::domain::journals;
use crate::domain::missions::MissionStatus;
use crate::domain::proposals::{
    MergeApprovedPayload, ProposalCreatedPayload, MERGE_APPROVED, PROPOSAL_CREATED,
};
use crate::domain::missions::{MISSION_COMPLETED, MISSION_FAILED, MISSION_STOPPED};
use crate::eventstore::{Actor, EventError, NewEvent, StoredEvent};

pub const SUBMISSION_CREATED: &str = "submission.created";
pub const SUBMISSION_ITEM_CHECKED: &str = "submission.item_checked";
pub const SUBMISSION_ITEM_UNCHECKED: &str = "submission.item_unchecked";

/// The `submission.created` payload: a mission birth bound to a venue.
/// The terminator fields are non-nullable and validated at construction
/// (AD-12 — a checklist that cannot end cannot exist); `source_mission_id`
/// links the research mission the submission derives from (None = a
/// workspace-level checklist).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubmissionCreatedPayload {
    pub question: String,
    pub stop_condition: String,
    pub success_criterion: String,
    /// The venue template this checklist runs (a dataset id, FR-19.2).
    pub venue_id: String,
    #[serde(default)]
    pub source_mission_id: Option<Uuid>,
}

/// The `submission.item_checked` / `submission.item_unchecked` payload:
/// the submission mission, its venue, the checklist item (a criterion id
/// of the venue template), and an optional evidence note (an agent
/// pre-check's note carries its machine evidence; a human check may
/// carry none).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubmissionItemPayload {
    pub mission_id: Uuid,
    pub venue_id: String,
    pub item_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `submission.created` event
    /// comes into being. Actor is always the user on the direct path — a
    /// fit-finder proposal carries this event as its intent and the merge
    /// applies it with the agent's attribution (the hypothesis-transition
    /// pattern). The venue must exist in the bundled dataset; the
    /// terminator fields must be non-empty (AD-12).
    pub fn submission_created(payload: SubmissionCreatedPayload) -> Result<Self, EventError> {
        for (field, value) in [
            ("question", &payload.question),
            ("stop_condition", &payload.stop_condition),
            ("success_criterion", &payload.success_criterion),
        ] {
            if value.trim().is_empty() {
                return Err(EventError::Invalid(format!(
                    "submission.{field} must not be empty — a mission that cannot end cannot \
                     exist (AD-12)"
                )));
            }
        }
        if journals::venue(&payload.venue_id).is_none() {
            return Err(EventError::Invalid(format!(
                "unknown_venue: `{}` — the bundled dataset carries no such venue",
                payload.venue_id
            )));
        }
        let mut causes = Vec::new();
        if let Some(source) = payload.source_mission_id {
            causes.push(source);
        }
        Ok(Self::new(
            SUBMISSION_CREATED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?
        .with_causes(causes))
    }

    /// Typed constructor (AD-15): the one way a `submission.item_checked`
    /// event comes into being — the human path appends it directly; the
    /// agent path never does (an agent pre-check is a proposal whose
    /// merge applies this payload with the agent's attribution). The item
    /// must be a criterion of the venue's template.
    pub fn submission_item_checked(
        mission_id: Uuid,
        venue_id: &str,
        item_id: &str,
        note: Option<&str>,
    ) -> Result<Self, EventError> {
        validate_item(venue_id, item_id)?;
        let payload = SubmissionItemPayload {
            mission_id,
            venue_id: venue_id.trim().to_string(),
            item_id: item_id.trim().to_string(),
            note: note.map(str::trim).filter(|n| !n.is_empty()).map(str::to_string),
        };
        Ok(Self::new(
            SUBMISSION_ITEM_CHECKED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?
        .with_causes(vec![mission_id]))
    }

    /// Typed constructor (AD-15): the one way an item's check is undone —
    /// a human correction, actor=user (an agent never unchecks).
    pub fn submission_item_unchecked(
        mission_id: Uuid,
        venue_id: &str,
        item_id: &str,
        note: Option<&str>,
    ) -> Result<Self, EventError> {
        validate_item(venue_id, item_id)?;
        let payload = SubmissionItemPayload {
            mission_id,
            venue_id: venue_id.trim().to_string(),
            item_id: item_id.trim().to_string(),
            note: note.map(str::trim).filter(|n| !n.is_empty()).map(str::to_string),
        };
        Ok(Self::new(
            SUBMISSION_ITEM_UNCHECKED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?
        .with_causes(vec![mission_id]))
    }
}

/// The item must be a criterion of the venue's template — a checklist
/// item is template data, never freeform.
fn validate_item(venue_id: &str, item_id: &str) -> Result<(), EventError> {
    let Some(venue) = journals::venue(venue_id) else {
        return Err(EventError::Invalid(format!(
            "unknown_venue: `{venue_id}` — the bundled dataset carries no such venue"
        )));
    };
    if venue.criteria.iter().all(|c| c.id != item_id.trim()) {
        return Err(EventError::Invalid(format!(
            "unknown_item: `{}` is not a criterion of venue `{}` — checklist items come \
             from the venue template (FR-19.2)",
            item_id.trim(),
            venue_id.trim()
        )));
    }
    Ok(())
}

/// Is the criterion human-only? (The proposal edge refuses an agent
/// pre-check of a human item — never agent-checkable, FR-19.4.)
pub fn item_is_human(venue_id: &str, item_id: &str) -> bool {
    journals::venue(venue_id)
        .and_then(|v| v.criteria.iter().find(|c| c.id == item_id.trim()))
        .map(|c| c.human)
        .unwrap_or(false)
}

/// The audit stamp of one checked item (FR-2.2 discipline): the deciding
/// event's seq + ts and WHO checked it — "user" on the direct human
/// path, "agent:<run>" when a merged agent pre-check applied it. Agent
/// vs human is always visibly attributed, never conflated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckStamp {
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub actor: String,
    /// The evidence note (an agent pre-check's machine evidence, or a
    /// human note) — rendered with the stamp, never silent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One checklist item as read from the log: the venue criterion it
/// answers, its human-only flag, and its check stamp (None = unchecked).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionItem {
    pub item_id: String,
    pub human: bool,
    /// The machine check's code, or `human_only` — the UI renders labels
    /// from it + the venue's data (the tier-2 idiom).
    pub kind: String,
    pub checked: Option<CheckStamp>,
}

/// A submission mission as read from the log (camelCase on the wire,
/// AD-8): the venue-bound mission whose checklist derives from the venue
/// template + the item events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionMission {
    /// The `submission.created` event's id — the mission's identity.
    pub id: Uuid,
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub venue_id: String,
    pub venue_name: String,
    pub question: String,
    pub stop_condition: String,
    pub success_criterion: String,
    #[serde(default)]
    pub source_mission_id: Option<Uuid>,
    pub status: MissionStatus,
    pub items: Vec<SubmissionItem>,
}

impl SubmissionMission {
    /// AD-12's success criterion, derived: every item checked.
    pub fn all_checked(&self) -> bool {
        self.items.iter().all(|i| i.checked.is_some())
    }

    /// The human criterion ids this mission's owner confirmed (checked by
    /// the user — the only path a human item ever checks) — the tier-2
    /// gate's confirmation input (Story 6.11's `human_confirmed`).
    pub fn human_confirmed(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|i| i.human && i.checked.as_ref().map(|s| s.actor.as_str()) == Some("user"))
            .map(|i| i.item_id.clone())
            .collect()
    }
}

/// Pure fold of the log into the submission read model. The checklist
/// derives from the venue template (data) + the item events; merged agent
/// pre-checks apply at their `merge.approved` seq with the proposing
/// agent's attribution (approval-order-by-seq, AD-13). The fold respects
/// the shared FoldCursor (AD-1): a rollback that orphaned a submission
/// means it never happened.
pub struct SubmissionProjection;

impl SubmissionProjection {
    pub fn fold(events: &[StoredEvent]) -> Result<Vec<SubmissionMission>, EventError> {
        let cursor = FoldCursor::over(events);
        let events = &cursor.live_owned(events);
        let mut submissions: Vec<SubmissionMission> = Vec::new();
        let mut index: std::collections::HashMap<Uuid, usize> =
            std::collections::HashMap::new();
        // Quarantine index (AD-3): item-check proposals by event id, so a
        // merge.approved can resolve and apply the intended check.
        let mut proposals: std::collections::HashMap<
            Uuid,
            (ProposalCreatedPayload, String),
        > = std::collections::HashMap::new();
        for event in events {
            match event.kind.as_str() {
                SUBMISSION_CREATED => {
                    let payload: SubmissionCreatedPayload =
                        serde_json::from_value(event.payload.clone()).map_err(|e| {
                            EventError::Invalid(format!(
                                "corrupt {SUBMISSION_CREATED} payload at seq {}: {e}",
                                event.seq
                            ))
                        })?;
                    let Some(venue) = journals::venue(&payload.venue_id) else {
                        return Err(EventError::Invalid(format!(
                            "corrupt {SUBMISSION_CREATED} payload at seq {}: unknown venue `{}`",
                            event.seq, payload.venue_id
                        )));
                    };
                    let items = venue
                        .criteria
                        .iter()
                        .map(|c| SubmissionItem {
                            item_id: c.id.clone(),
                            human: c.human,
                            kind: c.check.clone().unwrap_or_else(|| "human_only".into()),
                            checked: None,
                        })
                        .collect();
                    index.insert(event.id, submissions.len());
                    submissions.push(SubmissionMission {
                        id: event.id,
                        seq: event.seq,
                        ts: event.ts,
                        venue_id: venue.id.to_string(),
                        venue_name: venue.name.clone(),
                        question: payload.question,
                        stop_condition: payload.stop_condition,
                        success_criterion: payload.success_criterion,
                        source_mission_id: payload.source_mission_id,
                        status: MissionStatus::Active,
                        items,
                    });
                }
                PROPOSAL_CREATED => {
                    // Quarantine (AD-3): a proposal records intent only —
                    // the intended item check stays EXCLUDED until a
                    // merge.approved lands.
                    let payload: ProposalCreatedPayload =
                        serde_json::from_value(event.payload.clone()).map_err(|e| {
                            EventError::Invalid(format!(
                                "corrupt {PROPOSAL_CREATED} payload at seq {}: {e}",
                                event.seq
                            ))
                        })?;
                    if payload.proposed_kind == SUBMISSION_ITEM_CHECKED {
                        proposals.insert(event.id, (payload, actor_label(&event.actor)));
                    }
                }
                MERGE_APPROVED => {
                    // The application point (AD-13): the approval applies
                    // the intended item check — attributed to the
                    // proposing agent, stamped with the merge seq.
                    let payload: MergeApprovedPayload =
                        serde_json::from_value(event.payload.clone()).map_err(|e| {
                            EventError::Invalid(format!(
                                "corrupt {MERGE_APPROVED} payload at seq {}: {e}",
                                event.seq
                            ))
                        })?;
                    let Some((proposal, actor)) = proposals.get(&payload.proposal_id) else {
                        continue; // references no known proposal — skipped
                    };
                    let Some(&i) = index.get(&proposal.target_entity) else {
                        continue; // the submission never appeared — skipped
                    };
                    let intended: SubmissionItemPayload =
                        serde_json::from_value(proposal.proposed_payload.clone()).map_err(
                            |e| {
                                EventError::Invalid(format!(
                                    "corrupt proposed payload of proposal {} applied at seq \
                                     {}: {e}",
                                    payload.proposal_id, event.seq
                                ))
                            },
                        )?;
                    apply_check(&mut submissions[i], event, intended, actor.clone());
                }
                SUBMISSION_ITEM_CHECKED => {
                    // The direct human path (the agent path only ever
                    // applies through a merge above).
                    let payload: SubmissionItemPayload =
                        serde_json::from_value(event.payload.clone()).map_err(|e| {
                            EventError::Invalid(format!(
                                "corrupt {SUBMISSION_ITEM_CHECKED} payload at seq {}: {e}",
                                event.seq
                            ))
                        })?;
                    let Some(&i) = index.get(&payload.mission_id) else {
                        continue; // references no known submission — skipped
                    };
                    apply_check(&mut submissions[i], event, payload, actor_label(&event.actor));
                }
                SUBMISSION_ITEM_UNCHECKED => {
                    let payload: SubmissionItemPayload =
                        serde_json::from_value(event.payload.clone()).map_err(|e| {
                            EventError::Invalid(format!(
                                "corrupt {SUBMISSION_ITEM_UNCHECKED} payload at seq {}: {e}",
                                event.seq
                            ))
                        })?;
                    let Some(&i) = index.get(&payload.mission_id) else {
                        continue;
                    };
                    if let Some(item) =
                        submissions[i].items.iter_mut().find(|x| x.item_id == payload.item_id)
                    {
                        item.checked = None;
                    }
                }
                MISSION_COMPLETED => transition(&mut submissions, &index, event, MissionStatus::Completed),
                MISSION_STOPPED => transition(&mut submissions, &index, event, MissionStatus::Stopped),
                MISSION_FAILED => transition(&mut submissions, &index, event, MissionStatus::Failed),
                _ => {}
            }
        }
        Ok(submissions)
    }

    /// The submission mission with this id (its creation event id).
    pub fn for_mission(
        events: &[StoredEvent],
        mission_id: Uuid,
    ) -> Result<Option<SubmissionMission>, EventError> {
        Ok(Self::fold(events)?.into_iter().find(|s| s.id == mission_id))
    }
}

/// Apply one item check with its audit stamp — the shared body of the
/// direct human path and the merged agent pre-check.
fn apply_check(
    submission: &mut SubmissionMission,
    event: &StoredEvent,
    payload: SubmissionItemPayload,
    actor: String,
) {
    if let Some(item) = submission
        .items
        .iter_mut()
        .find(|x| x.item_id == payload.item_id)
    {
        item.checked = Some(CheckStamp {
            seq: event.seq,
            ts: event.ts,
            actor,
            note: payload.note,
        });
    }
}

/// A lifecycle transition on a submission mission (the event references
/// it in its causes or payload).
fn transition(
    submissions: &mut [SubmissionMission],
    index: &std::collections::HashMap<Uuid, usize>,
    event: &StoredEvent,
    to: MissionStatus,
) {
    let Some(&i) = index
        .get(&event.causes.first().copied().unwrap_or(Uuid::nil()))
        .or_else(|| {
            event
                .payload
                .get("mission_id")
                .and_then(serde_json::Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
                .and_then(|id| index.get(&id))
        })
    else {
        return;
    };
    submissions[i].status = to;
}

fn actor_label(actor: &Actor) -> String {
    match actor {
        Actor::User => "user".into(),
        Actor::Agent { run_id } => format!("agent:{run_id}"),
        Actor::System { component } => {
            format!("system:{}", format!("{component:?}").to_lowercase())
        }
    }
}

/// The human criterion ids confirmed for a venue in a scope (Story 6.11's
/// gate input): every submission mission of the venue whose source is the
/// scoped research mission (None = every submission of the venue),
/// carrying its user-checked human items. An agent can never check a
/// human item, so every confirmed human item is a human confirmation.
pub fn human_confirmed_for_venue(
    events: &[StoredEvent],
    scope: Option<Uuid>,
    venue_id: &str,
) -> Result<Vec<String>, EventError> {
    Ok(SubmissionProjection::fold(events)?
        .into_iter()
        .filter(|s| {
            s.venue_id == venue_id
                && scope.is_none_or(|m| s.source_mission_id == Some(m))
        })
        .flat_map(|s| s.human_confirmed())
        .collect())
}

// ---------------------------------------------------------------------------
// Tests (NFR-8)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::missions::{Autonomy, MissionCreatedPayload};
    use crate::eventstore::EventStore;
    use rusqlite::Connection;

    fn with_store(f: impl FnOnce(&EventStore<'_>)) {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        let store = EventStore::new(&conn);
        f(&store);
    }

    fn seed_mission(store: &EventStore<'_>) -> Uuid {
        store
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
            .id
    }

    fn seed_submission(store: &EventStore<'_>, mission: Uuid) -> SubmissionMission {
        let event = store
            .append(
                NewEvent::submission_created(SubmissionCreatedPayload {
                    question: "Envío a SIAM JSC".into(),
                    stop_condition: "submission-ready".into(),
                    success_criterion: "todos los elementos marcados".into(),
                    venue_id: "siam-jsc".into(),
                    source_mission_id: Some(mission),
                })
                .unwrap(),
            )
            .unwrap();
        SubmissionProjection::for_mission(&store.events_all().unwrap(), event.id)
            .unwrap()
            .expect("just created")
    }

    /// Choosing a venue spawns a mission whose items come from the venue
    /// template — machine and human, in template order (FR-19.4).
    #[test]
    fn a_submission_mission_derives_its_checklist_from_the_template() {
        with_store(|store| {
            let mission = seed_mission(store);
            let submission = seed_submission(store, mission);
            assert_eq!(submission.venue_id, "siam-jsc");
            assert_eq!(submission.status, MissionStatus::Active);
            assert_eq!(submission.stop_condition, "submission-ready");
            let venue = journals::venue("siam-jsc").unwrap();
            assert_eq!(submission.items.len(), venue.criteria.len());
            assert_eq!(
                submission.items.iter().filter(|i| i.human).count() as u32,
                venue.human_count()
            );
            assert!(submission.items.iter().all(|i| i.checked.is_none()));
            assert!(!submission.all_checked());
            assert!(submission.human_confirmed().is_empty());
        });
    }

    /// The terminator fields are non-nullable (AD-12) and the venue must
    /// be a real dataset id — stable coded errors.
    #[test]
    fn construction_validates_the_edges() {
        let err = NewEvent::submission_created(SubmissionCreatedPayload {
            question: "  ".into(),
            stop_condition: "s".into(),
            success_criterion: "s".into(),
            venue_id: "siam-jsc".into(),
            source_mission_id: None,
        })
        .unwrap_err()
        .to_string();
        assert!(err.contains("must not be empty"), "{err}");

        let err = NewEvent::submission_created(SubmissionCreatedPayload {
            question: "q".into(),
            stop_condition: "s".into(),
            success_criterion: "s".into(),
            venue_id: "no-such".into(),
            source_mission_id: None,
        })
        .unwrap_err()
        .to_string();
        assert!(err.contains("unknown_venue:"), "{err}");

        let err = NewEvent::submission_item_checked(
            Uuid::new_v4(),
            "siam-jsc",
            "not-an-item",
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("unknown_item:"), "{err}");
    }

    /// Item state is evented with audit stamps: a human check stamps
    /// actor=user; an uncheck clears it; re-checking re-stamps (the last
    /// event wins — append-only, nothing disappears).
    #[test]
    fn item_state_is_evented_with_audit_stamps() {
        with_store(|store| {
            let mission = seed_mission(store);
            let submission = seed_submission(store, mission);
            let item = "compiled_pdf";
            store
                .append(
                    NewEvent::submission_item_checked(
                        submission.id,
                        "siam-jsc",
                        item,
                        Some("pdf compilado, e-42"),
                    )
                    .unwrap(),
                )
                .unwrap();
            let folded =
                SubmissionProjection::for_mission(&store.events_all().unwrap(), submission.id)
                    .unwrap()
                    .unwrap();
            let checked = folded
                .items
                .iter()
                .find(|i| i.item_id == item)
                .unwrap()
                .checked
                .clone()
                .expect("checked");
            assert_eq!(checked.actor, "user");
            assert_eq!(checked.note.as_deref(), Some("pdf compilado, e-42"));

            store
                .append(
                    NewEvent::submission_item_unchecked(submission.id, "siam-jsc", item, None)
                        .unwrap(),
                )
                .unwrap();
            let folded =
                SubmissionProjection::for_mission(&store.events_all().unwrap(), submission.id)
                    .unwrap()
                    .unwrap();
            assert!(folded
                .items
                .iter()
                .find(|i| i.item_id == item)
                .unwrap()
                .checked
                .is_none());
        });
    }

    /// An agent pre-check NEVER appends — it lands as a proposal (AD-3)
    /// whose merge applies the check with the agent's attribution; the
    /// agent's evidence note rides the stamp. A direct user check of the
    /// same item later re-stamps it user (the human word is final).
    #[test]
    fn an_agent_precheck_rides_the_quarantine_and_attributes_the_agent() {
        with_store(|store| {
            let mission = seed_mission(store);
            let submission = seed_submission(store, mission);
            let item = "compiled_pdf";
            // the agent's intended check, through the typed constructor
            let intended = NewEvent::submission_item_checked(
                submission.id,
                "siam-jsc",
                item,
                Some("machine check passed: compile ok (e-7)"),
            )
            .unwrap();
            let basis = store.events_all().unwrap().last().unwrap().seq;
            let cause = store.events_all().unwrap().last().unwrap().id;
            let proposal = store
                .append(
                    NewEvent::proposal_created(
                        "precheck-run-1",
                        &intended,
                        submission.id,
                        basis,
                        cause,
                    )
                    .unwrap(),
                )
                .unwrap();
            // quarantined: nothing applied yet (AD-3)
            let folded =
                SubmissionProjection::for_mission(&store.events_all().unwrap(), submission.id)
                    .unwrap()
                    .unwrap();
            assert!(folded
                .items
                .iter()
                .find(|i| i.item_id == item)
                .unwrap()
                .checked
                .is_none());
            // the merge applies it, attributed to the agent
            store
                .append(NewEvent::merge_approved(proposal.id, false, false, None).unwrap())
                .unwrap();
            let folded =
                SubmissionProjection::for_mission(&store.events_all().unwrap(), submission.id)
                    .unwrap()
                    .unwrap();
            let stamp = folded
                .items
                .iter()
                .find(|i| i.item_id == item)
                .unwrap()
                .checked
                .clone()
                .expect("applied at merge");
            assert_eq!(stamp.actor, "agent:precheck-run-1");
            assert!(stamp.note.as_deref().unwrap().contains("e-7"));
        });
    }

    /// Human-only items are never agent-checkable: the proposal edge
    /// refuses an intended check of a human criterion.
    #[test]
    fn a_human_item_is_never_agent_checkable() {
        assert!(item_is_human("siam-jsc", "co_author_signoffs"));
        assert!(!item_is_human("siam-jsc", "compiled_pdf"));
        with_store(|store| {
            let mission = seed_mission(store);
            let submission = seed_submission(store, mission);
            let intended = NewEvent::submission_item_checked(
                submission.id,
                "siam-jsc",
                "co_author_signoffs",
                None,
            )
            .unwrap();
            let basis = store.events_all().unwrap().last().unwrap().seq;
            let cause = store.events_all().unwrap().last().unwrap().id;
            let err = NewEvent::proposal_created(
                "precheck-run-1",
                &intended,
                submission.id,
                basis,
                cause,
            )
            .unwrap_err()
            .to_string();
            assert!(
                err.contains("never agent-checkable"),
                "the edge refuses the agent pre-check of a human item: {err}"
            );
        });
    }

    /// The gate's confirmation input: user-checked human items of the
    /// venue's submissions in scope confirm the venue's human criteria.
    #[test]
    fn human_confirmations_feed_the_tier_two_gate() {
        with_store(|store| {
            let mission = seed_mission(store);
            let submission = seed_submission(store, mission);
            let confirmed =
                human_confirmed_for_venue(&store.events_all().unwrap(), Some(mission), "siam-jsc")
                    .unwrap();
            assert!(confirmed.is_empty());
            store
                .append(
                    NewEvent::submission_item_checked(
                        submission.id,
                        "siam-jsc",
                        "co_author_signoffs",
                        None,
                    )
                    .unwrap(),
                )
                .unwrap();
            let confirmed =
                human_confirmed_for_venue(&store.events_all().unwrap(), Some(mission), "siam-jsc")
                    .unwrap();
            assert_eq!(confirmed, ["co_author_signoffs".to_string()]);
            // a different mission's scope sees nothing
            let other =
                human_confirmed_for_venue(&store.events_all().unwrap(), Some(Uuid::new_v4()), "siam-jsc")
                    .unwrap();
            assert!(other.is_empty());
        });
    }
}
