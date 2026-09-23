// Proposals domain (AD-3, AD-13, Story 2.2): agent-produced changes land as
// proposals, EXCLUDED from projections until a human merges them. A proposal
// is any agent-actor event that intends a domain change — recorded as a
// `proposal.created` event carrying the intended event (kind + payload, built
// through its own typed constructor), the target entity, the `basis_seq` of
// the entity state it was derived from, and a cause event id (AD-2).
//
// The lifecycle (AD-13): pending → merged | rejected | superseded | voided.
// `merge.approved` exists only through `approve()` below: the approval is
// validated against the CURRENT projection first — if the target entity has
// advanced past the proposal's basis the merge is refused with a typed
// `basis_stale:` error unless force-approved, and a forced merge records a
// `basis_stale: true` marker the UI must surface. There is no path to append
// `merge.approved` that bypasses this validation (the constructor is
// crate-private; blind merges are non-compliant by construction).
//
// Fold rule (AD-13): when several merged proposals affect one entity, the
// fold applies them by APPROVAL order — the seq of their `merge.approved`
// events — never by proposal order. The hypotheses fold applies each merged
// proposal's intended transition when it meets the approval event, so seq
// order over approvals IS the application order.
//
// Conflicting pending siblings: two pending proposals derived from the SAME
// entity state (same target, same proposed kind, same basis) are true
// alternatives — merging one supersedes the rest (`merge.superseded`, cause-
// linked to both proposals), so nothing disappears and a later merge attempt
// on a superseded sibling fails loudly with `not_pending:`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::domain::evidence::{
    excerpt_digest, EvidencePinnedPayload, PinKind, EVIDENCE_PINNED,
};
use crate::domain::hypotheses::{
    HypothesisStatus, HypothesisStatusChangedPayload, HypothesesProjection,
    HYPOTHESIS_CREATED, HYPOTHESIS_RELATED, HYPOTHESIS_STATUS_CHANGED,
};
use crate::eventstore::{Actor, EventError, EventStore, NewEvent, StoredEvent};

pub const PROPOSAL_CREATED: &str = "proposal.created";
pub const MERGE_APPROVED: &str = "merge.approved";
pub const MERGE_REJECTED: &str = "merge.rejected";
pub const MERGE_SUPERSEDED: &str = "merge.superseded";
pub const PROPOSAL_VOIDED: &str = "proposal.voided";

/// The closed vocabulary of proposalable event kinds (AD-15): the domain
/// changes an agent may propose. v1: hypothesis lifecycle transitions, and
/// evidence pins (Story 3.4, FR-11.5) — a fetched job result proposes a
/// numerical pin; the merge applies it (auto-pinning is v0.2.0). The
/// proposal constructor rejects anything else at the edge.
pub const PROPOSAL_TARGETS: &[&str] = &[HYPOTHESIS_STATUS_CHANGED, EVIDENCE_PINNED];

/// A proposal's lifecycle status (AD-13). `Pending` is the only initial
/// state; every deciding event (`merge.approved`, `merge.rejected`,
/// `merge.superseded`, `proposal.voided`) moves it to its terminal state —
/// a decided proposal can never be decided again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Pending,
    Merged,
    Rejected,
    Superseded,
    Voided,
}

impl ProposalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Merged => "merged",
            Self::Rejected => "rejected",
            Self::Superseded => "superseded",
            Self::Voided => "voided",
        }
    }
}

impl std::fmt::Display for ProposalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The `proposal.created` payload (AD-13): the intended event (kind +
/// payload — built through its own typed constructor, so an illegal intended
/// change can never even be proposed), the entity it targets, the seq of the
/// entity state it was derived from (`basis_seq` — the merge validates
/// against it), and the cause event id this proposal builds on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposalCreatedPayload {
    pub proposed_kind: String,
    pub proposed_payload: serde_json::Value,
    pub target_entity: Uuid,
    pub basis_seq: i64,
    pub cause: Uuid,
}

/// The `merge.approved` payload: which proposal, whether the human forced
/// past a stale basis, the recorded `basis_stale` marker (true only on a
/// forced merge past a stale basis — the UI surfaces it), and the surface
/// the decision came from (`None` = the desktop review surface; a remote
/// one-tap carries `mobile` — Story 6.16, NFR-13: every remote decision is
/// evented with surface attribution visible in receipts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MergeApprovedPayload {
    pub proposal_id: Uuid,
    pub force: bool,
    pub basis_stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MergeRejectedPayload {
    pub proposal_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MergeSupersededPayload {
    pub proposal_id: Uuid,
    pub superseded_by: Uuid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposalVoidedPayload {
    pub proposal_id: Uuid,
}

impl NewEvent {
    /// Typed constructor (AD-15, AD-3): the one way a `proposal.created`
    /// event comes into being. The intended event arrives already built
    /// through ITS typed constructor (an illegal intended change — e.g. an
    /// illegal hypothesis transition — is refused there, before any proposal
    /// exists). This constructor closes the rest of the edges: the intended
    /// kind must be in the proposalable vocabulary, its payload must parse
    /// for that kind (a proposal the fold could never apply is refused), the
    /// basis must name a real seq (blind proposals are non-compliant), and
    /// the cause must be a real event id. Actor is always the proposing
    /// agent — the user path applies changes directly, never through here.
    pub fn proposal_created(
        run_id: impl Into<String>,
        proposed: &NewEvent,
        target_entity: Uuid,
        basis_seq: i64,
        cause: Uuid,
    ) -> Result<Self, EventError> {
        let run_id = run_id.into();
        if run_id.trim().is_empty() {
            return Err(EventError::Invalid(
                "proposal.run_id must not be empty — a proposal names its proposing run".into(),
            ));
        }
        if !PROPOSAL_TARGETS.contains(&proposed.kind.as_str()) {
            return Err(EventError::Invalid(format!(
                "proposal.target: `{}` is not a proposalable event kind — vocabulary: {} (AD-3)",
                proposed.kind,
                PROPOSAL_TARGETS.join(" | ")
            )));
        }
        validate_proposed_payload(&proposed.kind, &proposed.payload)?;
        if target_entity.is_nil() {
            return Err(EventError::Invalid(
                "proposal.target_entity must not be nil — a proposal names the entity it \
                 intends to change"
                    .into(),
            ));
        }
        if basis_seq < 1 {
            return Err(EventError::Invalid(
                "proposal.basis_seq must name the seq of the entity state the proposal was \
                 derived from — blind proposals are non-compliant (AD-13)"
                    .into(),
            ));
        }
        if cause.is_nil() {
            return Err(EventError::Invalid(
                "proposal.cause must not be nil — a proposal builds on a real event (AD-2)"
                    .into(),
            ));
        }
        let payload = ProposalCreatedPayload {
            proposed_kind: proposed.kind.clone(),
            proposed_payload: proposed.payload.clone(),
            target_entity,
            basis_seq,
            cause,
        };
        Ok(Self::new(
            PROPOSAL_CREATED,
            Actor::Agent { run_id },
            serde_json::to_value(&payload)?,
        )?
        .with_causes(vec![cause, target_entity]))
    }

    /// The ONLY construction site for `merge.approved` is `approve()` below —
    /// crate-private by design (AD-13): there is no path to append a merge
    /// that bypassed the basis validation.
    pub(crate) fn merge_approved(
        proposal_id: Uuid,
        force: bool,
        basis_stale: bool,
        surface: Option<&str>,
    ) -> Result<Self, EventError> {
        let payload = MergeApprovedPayload {
            proposal_id,
            force,
            basis_stale,
            surface: surface.map(str::to_string),
        };
        Ok(Self::new(MERGE_APPROVED, Actor::User, serde_json::to_value(&payload)?)?
            .with_causes(vec![proposal_id]))
    }

    pub(crate) fn merge_rejected(
        proposal_id: Uuid,
        surface: Option<&str>,
    ) -> Result<Self, EventError> {
        let payload = MergeRejectedPayload {
            proposal_id,
            surface: surface.map(str::to_string),
        };
        Ok(Self::new(MERGE_REJECTED, Actor::User, serde_json::to_value(&payload)?)?
            .with_causes(vec![proposal_id]))
    }

    pub(crate) fn merge_superseded(
        proposal_id: Uuid,
        superseded_by: Uuid,
    ) -> Result<Self, EventError> {
        let payload = MergeSupersededPayload {
            proposal_id,
            superseded_by,
        };
        Ok(Self::new(
            MERGE_SUPERSEDED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?
        .with_causes(vec![proposal_id, superseded_by]))
    }

    pub(crate) fn proposal_voided(proposal_id: Uuid) -> Result<Self, EventError> {
        let payload = ProposalVoidedPayload { proposal_id };
        Ok(Self::new(PROPOSAL_VOIDED, Actor::User, serde_json::to_value(&payload)?)?
            .with_causes(vec![proposal_id]))
    }
}

/// The intended payload must parse for its kind — a proposal carrying a
/// payload the fold could never apply is refused at the edge.
fn validate_proposed_payload(
    kind: &str,
    payload: &serde_json::Value,
) -> Result<(), EventError> {
    match kind {
        HYPOTHESIS_STATUS_CHANGED => {
            serde_json::from_value::<HypothesisStatusChangedPayload>(payload.clone())
                .map(|_| ())
                .map_err(|e| {
                    EventError::Invalid(format!(
                        "proposal.proposed_payload does not parse as a `{kind}` payload: {e}"
                    ))
                })
        }
        EVIDENCE_PINNED => {
            serde_json::from_value::<EvidencePinnedPayload>(payload.clone())
                .map(|_| ())
                .map_err(|e| {
                    EventError::Invalid(format!(
                        "proposal.proposed_payload does not parse as a `{kind}` payload: {e}"
                    ))
                })
        }
        _ => Err(EventError::Invalid(format!(
            "proposal.target: `{kind}` is not a proposalable event kind (AD-3)"
        ))),
    }
}

/// The receipt stamp of the event that decided a proposal (merged, rejected,
/// superseded, or voided): seq, ts, actor — never silent, never anonymous —
/// and, for remote decisions, the surface they came from (`mobile`,
/// NFR-13/Story 6.16: visible in receipts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionStamp {
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub actor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
}

/// A proposal as read from the log (AD-8) — the quarantine read model the
/// review surface renders. Field names are camelCase on the wire (Tauri 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    /// The `proposal.created` event's id — the proposal's identity.
    pub id: Uuid,
    pub seq: i64,
    pub ts: DateTime<Utc>,
    /// The proposing agent run (the actor of the proposal.created event).
    pub run_id: String,
    /// The target hypothesis's mission — enriched from the log; null when the
    /// target never appeared (a corrupt proposal the merge will refuse).
    pub mission_id: Option<Uuid>,
    /// The entity the proposal intends to change (its creation event id).
    pub target_entity: Uuid,
    /// The target hypothesis's statement — the review card's label; null for
    /// unknown targets.
    pub target_label: Option<String>,
    /// The target hypothesis's creation seq — the `H-n` label; null for
    /// unknown targets.
    pub target_seq: Option<i64>,
    pub proposed_kind: String,
    pub proposed_payload: serde_json::Value,
    /// The seq of the entity state the proposal was derived from (AD-13).
    pub basis_seq: i64,
    /// Pending: derived — true once the entity advanced past `basis_seq` (the
    /// basis-stale warning variant). Merged: the recorded force marker. The
    /// UI must surface both.
    pub basis_stale: bool,
    pub status: ProposalStatus,
    /// The stamp of the deciding event; None while pending.
    pub decided: Option<DecisionStamp>,
    /// The proposal whose merge superseded this one.
    pub superseded_by: Option<Uuid>,
    /// True when a rollback orphaned this proposal's creation (AD-1, Story
    /// 2.6): it renders as SUPERSEDED history in the quarantine view —
    /// excluded from every projection, never hidden (EXPERIENCE.md) — and
    /// can never be merged.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub orphaned_by_rollback: bool,
    /// The seq of the `checkpoint.rolled_back` event that orphaned it — the
    /// "superseded by rollback e-{seq}" stamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rolled_back_seq: Option<i64>,
}

/// Everything that can go wrong proposing, approving, or rejecting — typed,
/// with stable codes the UI never translates (EXPERIENCE.md).
#[derive(Debug, thiserror::Error)]
pub enum ProposalError {
    #[error("not_found: no proposal with id `{0}`")]
    NotFound(Uuid),
    #[error("not_found: proposal `{0}` targets no known entity in the log")]
    TargetNotFound(Uuid),
    #[error("not_pending: proposal `{proposal_id}` is `{status}`, not pending — a decided proposal can never be merged again (AD-13)")]
    NotPending {
        proposal_id: Uuid,
        status: ProposalStatus,
    },
    #[error("basis_stale: proposal `{proposal_id}` was derived from seq {basis_seq} but the entity has advanced to seq {current_seq} — force-approve (force: true) to merge past it; the basis-stale marker will be recorded and surfaced")]
    BasisStale {
        proposal_id: Uuid,
        basis_seq: i64,
        current_seq: i64,
    },
    #[error("digest_mismatch: proposal `{0}` carries an intended evidence.pinned whose digest does not match its own excerpt — the pin candidate was tampered with; it can never merge (AD-5)")]
    DigestMismatch(Uuid),
    #[error(transparent)]
    Store(#[from] EventError),
}

/// The seq of the last event that mutated the target entity's projected
/// state — the entity's "current seq" a merge validates the basis against
/// (AD-13). Mutations: the entity's own creation, domain events referencing
/// it in their causes (status changes, relations), and `merge.approved`
/// events whose proposal targets it (approvals apply at fold time). Pending
/// proposals NEVER advance the basis — they change nothing (AD-3).
pub(crate) fn entity_current_seq(events: &[StoredEvent], target: Uuid) -> Option<i64> {
    // proposal id → target, for resolving merge approvals
    let mut proposal_targets: HashMap<Uuid, Uuid> = HashMap::new();
    for event in events {
        if event.kind == PROPOSAL_CREATED {
            if let Ok(payload) =
                serde_json::from_value::<ProposalCreatedPayload>(event.payload.clone())
            {
                proposal_targets.insert(event.id, payload.target_entity);
            }
        }
    }
    let mut max: Option<i64> = None;
    for event in events {
        let hits = match event.kind.as_str() {
            HYPOTHESIS_CREATED => event.id == target,
            HYPOTHESIS_STATUS_CHANGED | HYPOTHESIS_RELATED => {
                event.causes.iter().any(|c| *c == target)
            }
            MERGE_APPROVED => event
                .causes
                .first()
                .and_then(|pid| proposal_targets.get(pid))
                .map(|t| *t == target)
                .unwrap_or(false),
            _ => false,
        };
        if hits {
            max = Some(event.seq);
        }
    }
    max
}

/// Pure fold of the log into the quarantine read model (AD-8). Events fold
/// in `seq` order; corrupt payloads fail loudly. A deciding event on an
/// already-decided proposal is a log that contradicts itself — the fold
/// fails loudly (the command path refuses it first with `not_pending:`).
/// Deciding events referencing unknown proposals are skipped, mirroring the
/// hypotheses fold's treatment of unknown references.
pub struct ProposalsProjection;

impl ProposalsProjection {
    pub fn fold(events: &[StoredEvent]) -> Result<Vec<Proposal>, EventError> {
        // The shared fold cursor (AD-1, Story 2.6): fold the live events —
        // the prefix up to each rollback's target, skipping the orphaned
        // suffix, continuing after the rollback action. Orphaned proposals
        // are re-listed below as superseded history, never hidden.
        let raw = events;
        let cursor = crate::domain::checkpoints::FoldCursor::over(raw);
        let events = &cursor.live_owned(raw);
        let mut proposals: Vec<Proposal> = Vec::new();
        let mut index: HashMap<Uuid, usize> = HashMap::new();
        // target enrichment: hypothesis creation events by id
        let mut hyp_mission: HashMap<Uuid, Uuid> = HashMap::new();
        let mut hyp_label: HashMap<Uuid, (i64, String)> = HashMap::new();
        for event in events {
            match event.kind.as_str() {
                HYPOTHESIS_CREATED => {
                    if let Ok(payload) = serde_json::from_value::<
                        crate::domain::hypotheses::HypothesisCreatedPayload,
                    >(event.payload.clone())
                    {
                        hyp_mission.insert(event.id, payload.mission_id);
                        hyp_label.insert(event.id, (event.seq, payload.statement));
                    }
                }
                PROPOSAL_CREATED => {
                    let payload: ProposalCreatedPayload = serde_json::from_value(
                        event.payload.clone(),
                    )
                    .map_err(|e| {
                        EventError::Invalid(format!(
                            "corrupt {PROPOSAL_CREATED} payload at seq {}: {e}",
                            event.seq
                        ))
                    })?;
                    // A proposal is an agent-actor event by definition (AD-3)
                    // — anything else in the log is corrupt.
                    let Actor::Agent { run_id } = &event.actor else {
                        return Err(EventError::Invalid(format!(
                            "corrupt {PROPOSAL_CREATED} at seq {}: a proposal is an agent-actor \
                             event — the user path applies changes directly, never through here",
                            event.seq
                        )));
                    };
                    index.insert(event.id, proposals.len());
                    proposals.push(Proposal {
                        id: event.id,
                        seq: event.seq,
                        ts: event.ts,
                        run_id: run_id.clone(),
                        mission_id: hyp_mission.get(&payload.target_entity).copied(),
                        target_label: hyp_label
                            .get(&payload.target_entity)
                            .map(|(_, s)| s.clone()),
                        target_seq: hyp_label.get(&payload.target_entity).map(|(s, _)| *s),
                        target_entity: payload.target_entity,
                        proposed_kind: payload.proposed_kind,
                        proposed_payload: payload.proposed_payload,
                        basis_seq: payload.basis_seq,
                        basis_stale: false, // derived below for pending proposals
                        status: ProposalStatus::Pending,
                        decided: None,
                        superseded_by: None,
                        orphaned_by_rollback: false,
                        rolled_back_seq: None,
                    });
                }
                MERGE_APPROVED => {
                    let payload: MergeApprovedPayload = serde_json::from_value(
                        event.payload.clone(),
                    )
                    .map_err(|e| {
                        EventError::Invalid(format!(
                            "corrupt {MERGE_APPROVED} payload at seq {}: {e}",
                            event.seq
                        ))
                    })?;
                    let Some(&i) = index.get(&payload.proposal_id) else {
                        continue; // references no known proposal — skipped
                    };
                    let was = proposals[i].status;
                    decide(
                        &mut proposals[i],
                        ProposalStatus::Merged,
                        event,
                        format!(
                            "proposal `{}` is `{}`, not pending — a second merge attempt \
                             contradicts the log",
                            payload.proposal_id, was
                        ),
                    )?;
                    proposals[i].basis_stale = payload.basis_stale;
                }
                MERGE_REJECTED => {
                    let payload: MergeRejectedPayload = serde_json::from_value(
                        event.payload.clone(),
                    )
                    .map_err(|e| {
                        EventError::Invalid(format!(
                            "corrupt {MERGE_REJECTED} payload at seq {}: {e}",
                            event.seq
                        ))
                    })?;
                    let Some(&i) = index.get(&payload.proposal_id) else {
                        continue;
                    };
                    let was = proposals[i].status;
                    decide(
                        &mut proposals[i],
                        ProposalStatus::Rejected,
                        event,
                        format!("proposal `{}` is `{}`, not pending", payload.proposal_id, was),
                    )?;
                }
                MERGE_SUPERSEDED => {
                    let payload: MergeSupersededPayload = serde_json::from_value(
                        event.payload.clone(),
                    )
                    .map_err(|e| {
                        EventError::Invalid(format!(
                            "corrupt {MERGE_SUPERSEDED} payload at seq {}: {e}",
                            event.seq
                        ))
                    })?;
                    let Some(&i) = index.get(&payload.proposal_id) else {
                        continue;
                    };
                    let was = proposals[i].status;
                    decide(
                        &mut proposals[i],
                        ProposalStatus::Superseded,
                        event,
                        format!("proposal `{}` is `{}`, not pending", payload.proposal_id, was),
                    )?;
                    proposals[i].superseded_by = Some(payload.superseded_by);
                }
                PROPOSAL_VOIDED => {
                    let payload: ProposalVoidedPayload = serde_json::from_value(
                        event.payload.clone(),
                    )
                    .map_err(|e| {
                        EventError::Invalid(format!(
                            "corrupt {PROPOSAL_VOIDED} payload at seq {}: {e}",
                            event.seq
                        ))
                    })?;
                    let Some(&i) = index.get(&payload.proposal_id) else {
                        continue;
                    };
                    let was = proposals[i].status;
                    decide(
                        &mut proposals[i],
                        ProposalStatus::Voided,
                        event,
                        format!("proposal `{}` is `{}`, not pending", payload.proposal_id, was),
                    )?;
                }
                _ => {}
            }
        }
        // Pending basis-staleness is derived against the entity's CURRENT
        // seq — the warning variant the review card renders before any
        // approve attempt.
        if proposals
            .iter()
            .any(|p| p.status == ProposalStatus::Pending)
        {
            for p in proposals.iter_mut().filter(|p| p.status == ProposalStatus::Pending) {
                if let Some(current) = entity_current_seq(events, p.target_entity) {
                    p.basis_stale = current > p.basis_seq;
                }
            }
        }
        // Orphaned proposals (AD-1, Story 2.6): a rollback excluded their
        // creation from the live fold — they return here as SUPERSEDED
        // history, listable in the quarantine view, never hidden
        // (EXPERIENCE.md), and never mergeable (no live merge.approved can
        // reference them: the approve path folds this same read model).
        if cursor.orphaned(raw).any(|e| e.kind == PROPOSAL_CREATED) {
            // labels from the RAW log: an orphaned proposal may target a
            // hypothesis whose own creation was orphaned — the name still
            // resolves for the history view.
            let mut raw_mission: HashMap<Uuid, Uuid> = HashMap::new();
            let mut raw_labels: HashMap<Uuid, (i64, String)> = HashMap::new();
            for event in raw.iter() {
                if event.kind == crate::domain::hypotheses::HYPOTHESIS_CREATED {
                    if let Ok(payload) = serde_json::from_value::<
                        crate::domain::hypotheses::HypothesisCreatedPayload,
                    >(event.payload.clone())
                    {
                        raw_mission.insert(event.id, payload.mission_id);
                        raw_labels.insert(event.id, (event.seq, payload.statement));
                    }
                }
            }
            for event in cursor.orphaned(raw) {
                if event.kind != PROPOSAL_CREATED {
                    continue;
                }
                let payload: ProposalCreatedPayload = serde_json::from_value(
                    event.payload.clone(),
                )
                .map_err(|e| {
                    EventError::Invalid(format!(
                        "corrupt {PROPOSAL_CREATED} payload at seq {}: {e}",
                        event.seq
                    ))
                })?;
                let Actor::Agent { run_id } = &event.actor else {
                    return Err(EventError::Invalid(format!(
                        "corrupt {PROPOSAL_CREATED} at seq {}: a proposal is an agent-actor \
                         event — the user path applies changes directly, never through here",
                        event.seq
                    )));
                };
                proposals.push(Proposal {
                    id: event.id,
                    seq: event.seq,
                    ts: event.ts,
                    run_id: run_id.clone(),
                    mission_id: raw_mission.get(&payload.target_entity).copied(),
                    target_label: raw_labels.get(&payload.target_entity).map(|(_, s)| s.clone()),
                    target_seq: raw_labels.get(&payload.target_entity).map(|(s, _)| *s),
                    target_entity: payload.target_entity,
                    proposed_kind: payload.proposed_kind,
                    proposed_payload: payload.proposed_payload,
                    basis_seq: payload.basis_seq,
                    basis_stale: false,
                    status: ProposalStatus::Superseded,
                    decided: None,
                    superseded_by: None,
                    orphaned_by_rollback: true,
                    rolled_back_seq: cursor.rolled_back_at(event.seq),
                });
            }
            proposals.sort_by_key(|p| p.seq); // creation seq order, history included
        }
        Ok(proposals)
    }

    /// The proposals of one mission (by target), in creation `seq` order.
    pub fn fold_for(
        events: &[StoredEvent],
        mission_id: Uuid,
    ) -> Result<Vec<Proposal>, EventError> {
        Ok(Self::fold(events)?
            .into_iter()
            .filter(|p| p.mission_id == Some(mission_id))
            .collect())
    }

    /// The LIVE result proposals of one job (Story 3.4, FR-11.5): proposals
    /// whose `proposal.created` event is cause-linked to the job — the
    /// fetch-results read (and the idempotency check's basis). A rollback
    /// that orphaned a job's proposals un-blocks a re-fetch (the orphaned
    /// ones never happened, AD-1).
    pub fn live_for_job(
        events: &[StoredEvent],
        job_id: Uuid,
    ) -> Result<Vec<Proposal>, EventError> {
        let cursor = crate::domain::checkpoints::FoldCursor::over(events);
        let live = cursor.live_owned(events);
        let ids: std::collections::HashSet<Uuid> = live
            .iter()
            .filter(|e| e.kind == PROPOSAL_CREATED && e.causes.contains(&job_id))
            .map(|e| e.id)
            .collect();
        Ok(Self::fold(events)?
            .into_iter()
            .filter(|p| ids.contains(&p.id))
            .collect())
    }
}

/// Move a pending proposal to its decided state, or fail loudly — a deciding
/// event on an already-decided proposal means the log contradicts itself.
fn decide(
    proposal: &mut Proposal,
    to: ProposalStatus,
    event: &StoredEvent,
    corrupt: String,
) -> Result<(), EventError> {
    if proposal.status != ProposalStatus::Pending {
        return Err(EventError::Invalid(format!(
            "corrupt {} at seq {}: {}",
            event.kind, event.seq, corrupt
        )));
    }
    // Remote decisions carry their surface in the payload (NFR-13) — the
    // desktop's own decisions carry none.
    let surface = event
        .payload
        .get("surface")
        .and_then(serde_json::Value::as_str)
        .map(String::from);
    proposal.status = to;
    proposal.decided = Some(DecisionStamp {
        seq: event.seq,
        ts: event.ts,
        actor: match &event.actor {
            Actor::User => "user".into(),
            Actor::Agent { run_id } => format!("agent:{run_id}"),
            Actor::System { component } => {
                format!("system:{}", format!("{component:?}").to_lowercase())
            }
        },
        surface,
    });
    Ok(())
}

/// What a merge produced: the merged proposal's read model and the pending
/// siblings the merge superseded (surfaced so nothing disappears quietly).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveOutcome {
    pub proposal: Proposal,
    pub superseded: Vec<Proposal>,
}

/// Propose a hypothesis transition (the runtime's seam, AD-3): derive the
/// `from` status and the basis from the CURRENT fold — the proposal is never
/// blind — build the intended event through its own typed constructor (an
/// illegal transition is refused BEFORE any proposal exists), and append the
/// `proposal.created` event (actor = the proposing agent run). The board is
/// unchanged: the proposed change stays excluded until a merge.
pub fn propose_transition(
    store: &EventStore<'_>,
    run_id: &str,
    hypothesis_id: Uuid,
    to: HypothesisStatus,
    basis_note: &str,
) -> Result<Proposal, ProposalError> {
    let events = store.events_all()?;
    let hyps = HypothesesProjection::fold(&events)?;
    let hyp = hyps
        .iter()
        .find(|h| h.id == hypothesis_id)
        .ok_or(ProposalError::TargetNotFound(hypothesis_id))?;
    // The basis: the seq of the entity state the proposal derives from — the
    // last event that mutated the target (its current projection state).
    let basis_seq = entity_current_seq(&events, hypothesis_id)
        .ok_or(ProposalError::TargetNotFound(hypothesis_id))?;
    // The cause: the event at the basis — the state the proposal builds on.
    let cause = events
        .iter()
        .find(|e| e.seq == basis_seq)
        .map(|e| e.id)
        .ok_or(ProposalError::TargetNotFound(hypothesis_id))?;
    // The intended event, through its own typed constructor — illegal
    // transitions never even become proposals.
    let intended =
        NewEvent::hypothesis_status_changed(hyp.status, to, basis_note)?;
    let event = NewEvent::proposal_created(run_id, &intended, hypothesis_id, basis_seq, cause)?;
    let stored = store.append(event)?;
    let proposals = ProposalsProjection::fold(&store.events_all()?)?;
    Ok(proposals
        .into_iter()
        .find(|p| p.id == stored.id)
        .expect("the proposal was just appended"))
}

/// Propose a numerical evidence pin (Story 3.4, FR-11.5, AD-3/AD-5): the
/// fetch-results seam. The intended `evidence.pinned` event is built through
/// ITS typed constructor — the sha-256 digest of the content is COMPUTED
/// there, at proposal time, never a parameter — and the basis is derived
/// from the target hypothesis's CURRENT state (AD-13, never blind). The
/// proposal is cause-linked to the producing event (`cause` — the job's
/// terminal event) plus the passed links (the job and its mission), so the
/// receipts drill-down and the idempotency check can find it. Like every
/// proposal it lands EXCLUDED from projections until a human merges it —
/// fully automatic result pinning does not exist in v1 (auto-pinning is
/// v0.2.0).
pub fn propose_evidence_pin(
    store: &EventStore<'_>,
    run_id: &str,
    claim_id: Uuid,
    hypothesis_id: Uuid,
    artifact_ref: &str,
    content: &str,
    confidence: f64,
    assessing_model: &str,
    cause: Uuid,
    links: Vec<Uuid>,
) -> Result<Proposal, ProposalError> {
    let events = store.events_all()?;
    let hyps = HypothesesProjection::fold(&events)?;
    if !hyps.iter().any(|h| h.id == hypothesis_id) {
        return Err(ProposalError::TargetNotFound(hypothesis_id));
    }
    // The basis: the seq of the hypothesis state the pin proposal derives
    // from — its current projection state (AD-13).
    let basis_seq = entity_current_seq(&events, hypothesis_id)
        .ok_or(ProposalError::TargetNotFound(hypothesis_id))?;
    // The intended pin, through its own typed constructor (AD-5): the digest
    // is computed here, at proposal time, from the content — never trusted
    // from a caller.
    let intended = NewEvent::evidence_pinned_numerical(
        claim_id,
        hypothesis_id,
        artifact_ref,
        content,
        confidence,
        assessing_model,
    )?;
    let event = NewEvent::proposal_created(
        run_id,
        &intended,
        hypothesis_id,
        basis_seq,
        cause,
    )?
    // The proposal is cause-linked to the producing event, the job, and the
    // mission (AD-2) — the runs drill-down and the per-job idempotency check
    // read these causes.
    .with_causes({
        let mut causes = vec![cause];
        causes.extend(links);
        causes.push(hypothesis_id);
        causes
    });
    let stored = store.append(event)?;
    let proposals = ProposalsProjection::fold(&store.events_all()?)?;
    Ok(proposals
        .into_iter()
        .find(|p| p.id == stored.id)
        .expect("the proposal was just appended"))
}

/// Approve (merge) a proposal (AD-13) — the ONLY path to a `merge.approved`
/// event. Validates the proposal is pending and its basis against the
/// CURRENT entity state: if the entity advanced past `basis_seq` the merge
/// is refused with `basis_stale:` unless `force` — a forced merge records
/// the `basis_stale` marker the UI surfaces. Merging supersedes conflicting
/// pending siblings (same target, same proposed kind, same basis — true
/// alternatives), and the superseded list is returned so the surface can
/// show what was displaced. `surface` names the deciding surface when the
/// decision is remote (`Some("mobile")` through the bridge, NFR-13) — the
/// desktop review surface passes `None`.
pub fn approve(
    store: &EventStore<'_>,
    proposal_id: Uuid,
    force: bool,
    surface: Option<&str>,
) -> Result<ApproveOutcome, ProposalError> {
    let events = store.events_all()?;
    let proposals = ProposalsProjection::fold(&events)?;
    let proposal = proposals
        .iter()
        .find(|p| p.id == proposal_id)
        .ok_or(ProposalError::NotFound(proposal_id))?
        .clone();
    if proposal.status != ProposalStatus::Pending {
        return Err(ProposalError::NotPending {
            proposal_id,
            status: proposal.status,
        });
    }
    // AD-5 at merge-check time (Story 3.4): an intended evidence.pinned must
    // carry the sha-256 of its own excerpt — the constructor computes it, so
    // only a hand-tampered proposal can mismatch. Corrupt candidates never
    // merge (force does not help — this is not a stale basis, it is a pin
    // that would anchor content it does not quote).
    if proposal.proposed_kind == EVIDENCE_PINNED {
        let Ok(intended) = serde_json::from_value::<EvidencePinnedPayload>(
            proposal.proposed_payload.clone(),
        ) else {
            // cannot happen through the constructor (it validates the parse);
            // a raw-appended proposal that no longer parses is tampered
            return Err(ProposalError::DigestMismatch(proposal_id));
        };
        if intended.digest != excerpt_digest(&intended.excerpt)
            || intended.kind != PinKind::Numerical
            || intended
                .artifact_ref
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
        {
            return Err(ProposalError::DigestMismatch(proposal_id));
        }
    }
    let current = entity_current_seq(&events, proposal.target_entity)
        .ok_or(ProposalError::TargetNotFound(proposal.target_entity))?;
    let stale = current > proposal.basis_seq;
    if stale && !force {
        return Err(ProposalError::BasisStale {
            proposal_id,
            basis_seq: proposal.basis_seq,
            current_seq: current,
        });
    }
    // The approval — the only construction site (crate-private ctor above).
    store.append(NewEvent::merge_approved(proposal_id, force, stale, surface)?)?;
    // Conflicting pending siblings: same target, same kind, same basis —
    // alternatives derived from the same state. The merge supersedes them.
    let sibling_ids: Vec<Uuid> = proposals
        .iter()
        .filter(|p| {
            p.id != proposal_id
                && p.status == ProposalStatus::Pending
                && p.target_entity == proposal.target_entity
                && p.proposed_kind == proposal.proposed_kind
                && p.basis_seq == proposal.basis_seq
        })
        .map(|p| p.id)
        .collect();
    for sibling_id in &sibling_ids {
        store.append(NewEvent::merge_superseded(*sibling_id, proposal_id)?)?;
    }
    // The read model after the merge — the log is the only truth.
    let after = ProposalsProjection::fold(&store.events_all()?)?;
    let merged = after
        .iter()
        .find(|p| p.id == proposal_id)
        .expect("the proposal was just merged")
        .clone();
    let superseded = after
        .iter()
        .filter(|p| sibling_ids.contains(&p.id))
        .cloned()
        .collect();
    Ok(ApproveOutcome {
        proposal: merged,
        superseded,
    })
}

/// Reject a pending proposal — the change never applies, the proposal stays
/// in the log with its rejection receipt. A decided proposal can never be
/// rejected (or merged) again. `surface` names the deciding surface when the
/// rejection is remote (`Some("mobile")` through the bridge, NFR-13).
pub fn reject(
    store: &EventStore<'_>,
    proposal_id: Uuid,
    surface: Option<&str>,
) -> Result<Proposal, ProposalError> {
    let events = store.events_all()?;
    let proposals = ProposalsProjection::fold(&events)?;
    let proposal = proposals
        .iter()
        .find(|p| p.id == proposal_id)
        .ok_or(ProposalError::NotFound(proposal_id))?;
    if proposal.status != ProposalStatus::Pending {
        return Err(ProposalError::NotPending {
            proposal_id,
            status: proposal.status,
        });
    }
    store.append(NewEvent::merge_rejected(proposal_id, surface)?)?;
    let after = ProposalsProjection::fold(&store.events_all()?)?;
    Ok(after
        .into_iter()
        .find(|p| p.id == proposal_id)
        .expect("the proposal was just rejected"))
}

/// Void a pending proposal (withdrawn before any decision). A decided
/// proposal can never be voided — nothing disappears after a decision.
pub fn void(store: &EventStore<'_>, proposal_id: Uuid) -> Result<Proposal, ProposalError> {
    let events = store.events_all()?;
    let proposals = ProposalsProjection::fold(&events)?;
    let proposal = proposals
        .iter()
        .find(|p| p.id == proposal_id)
        .ok_or(ProposalError::NotFound(proposal_id))?;
    if proposal.status != ProposalStatus::Pending {
        return Err(ProposalError::NotPending {
            proposal_id,
            status: proposal.status,
        });
    }
    store.append(NewEvent::proposal_voided(proposal_id)?)?;
    let after = ProposalsProjection::fold(&store.events_all()?)?;
    Ok(after
        .into_iter()
        .find(|p| p.id == proposal_id)
        .expect("the proposal was just voided"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::hypotheses::HypothesisStatus::{
        Proposed, Supported, Testing,
    };
    use crate::domain::missions::{Autonomy, MissionCreatedPayload};
    use crate::eventstore::EventStore;
    use rusqlite::Connection;
    use serde_json::json;

    fn mission() -> MissionCreatedPayload {
        MissionCreatedPayload {
            question: "Does retrieval-augmented drafting reduce hallucinated citations?".into(),
            stop_condition: "Stop after 3 rounds or $5.00 spent.".into(),
            success_criterion: "A blind rater finds zero fabricated citations.".into(),
            autonomy: Autonomy::Suggest,
            spend_ceiling_cents: 500,
            schedule: "daily-03:00".into(),
            roles: vec![],
        }
    }

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    fn seed_mission(store: &EventStore) -> uuid::Uuid {
        store.append(NewEvent::mission_created(mission()).unwrap()).unwrap().id
    }

    fn seed_hypothesis(store: &EventStore, mission_id: Uuid) -> StoredEvent {
        store
            .append(NewEvent::hypothesis_created("Adaptive timesteps drift on stiff systems.", mission_id).unwrap())
            .unwrap()
    }

    fn user_transitions(
        store: &EventStore,
        hypothesis_id: Uuid,
        from: HypothesisStatus,
        to: HypothesisStatus,
        basis: &str,
    ) -> StoredEvent {
        store
            .append(
                NewEvent::hypothesis_status_changed(from, to, basis)
                    .unwrap()
                    .with_causes(vec![hypothesis_id]),
            )
            .unwrap()
    }

    fn board(store: &EventStore) -> Vec<crate::domain::hypotheses::Hypothesis> {
        HypothesesProjection::fold(&store.events_all().unwrap()).unwrap()
    }

    fn board_status(store: &EventStore, hypothesis_id: Uuid) -> HypothesisStatus {
        board(store)
            .into_iter()
            .find(|h| h.id == hypothesis_id)
            .unwrap()
            .status
    }

    // ---------- constructors ----------

    #[test]
    fn proposal_constructor_validates_the_edges() {
        let target = Uuid::new_v4();
        let cause = Uuid::new_v4();
        let intended =
            NewEvent::hypothesis_status_changed(Proposed, Testing, "run 7 notes").unwrap();
        // the happy path: agent actor, cause-linked to cause + target
        let ev =
            NewEvent::proposal_created("run-7", &intended, target, 5, cause).unwrap();
        assert_eq!(ev.kind, "proposal.created");
        assert_eq!(
            ev.actor,
            Actor::Agent { run_id: "run-7".into() }
        );
        assert_eq!(ev.causes, vec![cause, target]);
        assert_eq!(
            ev.payload,
            json!({
                "proposed_kind": "hypothesis.status_changed",
                "proposed_payload": { "from": "proposed", "to": "testing", "basis": "run 7 notes" },
                "target_entity": target.to_string(),
                "basis_seq": 5,
                "cause": cause.to_string(),
            })
        );
        // empty run id
        assert!(NewEvent::proposal_created("  ", &intended, target, 5, cause).is_err());
        // non-vocabulary intended kind
        let off_targets =
            NewEvent::new("mission.created", Actor::User, json!({"question": "q"})).unwrap();
        let err = NewEvent::proposal_created("run-7", &off_targets, target, 5, cause)
            .expect_err("only vocabulary kinds are proposalable");
        assert!(err.to_string().contains("proposal.target"), "unexpected: {err}");
        // blind basis (seq 0)
        let err = NewEvent::proposal_created("run-7", &intended, target, 0, cause)
            .expect_err("a proposal must name its basis");
        assert!(err.to_string().contains("basis_seq"), "unexpected: {err}");
        // nil cause
        assert!(NewEvent::proposal_created("run-7", &intended, target, 5, Uuid::nil()).is_err());
        // an intended payload that does not parse for its kind
        let garbage = NewEvent::new(
            HYPOTHESIS_STATUS_CHANGED,
            Actor::Agent { run_id: "run-7".into() },
            json!({"from": "proposed"}),
        )
        .unwrap();
        let err = NewEvent::proposal_created("run-7", &garbage, target, 5, cause)
            .expect_err("the proposed payload must parse for its kind");
        assert!(err.to_string().contains("proposed_payload"), "unexpected: {err}");
    }

    #[test]
    fn propose_transition_derives_from_basis_and_cause_from_the_fold() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        let proposal =
            propose_transition(&store, "run-7", h.id, Testing, "evidence pins hold").unwrap();
        // from/basis derived from the fold — never asserted by the caller
        assert_eq!(proposal.status, ProposalStatus::Pending);
        assert_eq!(proposal.run_id, "run-7");
        assert_eq!(proposal.target_entity, h.id);
        assert_eq!(proposal.target_label.as_deref(), Some("Adaptive timesteps drift on stiff systems."));
        assert_eq!(proposal.mission_id, Some(m));
        assert_eq!(proposal.basis_seq, h.seq, "basis = the hypothesis's current state seq");
        assert_eq!(
            proposal.proposed_payload,
            json!({ "from": "proposed", "to": "testing", "basis": "evidence pins hold" })
        );
        // the cause event is the basis event (the state the proposal builds on)
        let created: ProposalCreatedPayload =
            serde_json::from_value(
                store.events_all().unwrap()
                    .iter().find(|e| e.id == proposal.id).unwrap().payload.clone(),
            )
            .unwrap();
        assert_eq!(created.cause, h.id);
        // pending proposals never advance the entity's basis
        assert_eq!(entity_current_seq(&store.events_all().unwrap(), h.id), Some(h.seq));
    }

    #[test]
    fn propose_transition_refuses_illegal_transitions_and_ghost_targets() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        let head = store.head_seq().unwrap();
        // illegal intended transition: refused by the hypothesis constructor
        // BEFORE any proposal exists — nothing appended
        let err = propose_transition(&store, "run-7", h.id, Supported, "skip testing").unwrap_err();
        assert!(err.to_string().contains("illegal_transition"), "unexpected: {err}");
        // ghost target
        let ghost = Uuid::new_v4();
        let err = propose_transition(&store, "run-7", ghost, Testing, "basis").unwrap_err();
        assert!(err.to_string().contains("not_found"), "unexpected: {err}");
        assert_eq!(store.head_seq().unwrap(), head, "nothing was appended");
    }

    // ---------- AD-3: exclusion until merged ----------

    #[test]
    fn a_pending_proposal_is_excluded_from_projections_until_approved() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        let p = propose_transition(&store, "run-7", h.id, Testing, "pins hold").unwrap();
        // excluded: the board still reads the pre-proposal state
        assert_eq!(board_status(&store, h.id), Proposed);
        // approve → the fold applies the intended transition at the approval
        let outcome = approve(&store, p.id, false, None).unwrap();
        assert_eq!(outcome.proposal.status, ProposalStatus::Merged);
        assert!(outcome.superseded.is_empty());
        assert_eq!(board_status(&store, h.id), Testing);
        // the audit stamp: the merge event's seq, the agent's actor and basis
        let hyp = board(&store).into_iter().find(|x| x.id == h.id).unwrap();
        let merge = store
            .events_all().unwrap()
            .into_iter()
            .find(|e| e.kind == MERGE_APPROVED)
            .unwrap();
        assert_eq!(hyp.audit.seq, merge.seq);
        assert_eq!(hyp.audit.actor, "agent");
        assert_eq!(hyp.audit.basis, "pins hold");
        // and the merge event is cause-linked to the proposal (AD-2)
        assert_eq!(merge.causes, vec![p.id]);
        assert_eq!(merge.actor, Actor::User, "the human merges — never the agent");
    }

    // ---------- AD-13: basis validation ----------

    #[test]
    fn a_stale_merge_is_refused_with_the_typed_basis_stale_error() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        let p = propose_transition(&store, "run-7", h.id, Testing, "pins hold").unwrap();
        // the entity advances past the proposal's basis (a user transition)
        let t = user_transitions(&store, h.id, Proposed, Testing, "user ran the suite first");
        assert!(t.seq > p.basis_seq);
        let head = store.head_seq().unwrap();
        // the merge is refused with the typed error carrying both seqs
        let err = approve(&store, p.id, false, None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("basis_stale"), "unexpected: {msg}");
        assert!(msg.contains(&format!("{}", p.basis_seq)), "carries the basis seq: {msg}");
        assert!(msg.contains(&format!("{}", t.seq)), "carries the current seq: {msg}");
        // nothing was appended — the refusal is a dry run
        assert_eq!(store.head_seq().unwrap(), head);
        assert_eq!(board_status(&store, h.id), Testing, "the user's transition stands");
        // the pending read model now carries the derived stale flag
        let pending = ProposalsProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .into_iter()
            .find(|x| x.id == p.id)
            .unwrap();
        assert!(pending.basis_stale, "the pending card renders the warning variant");
    }

    #[test]
    fn a_forced_merge_carries_the_basis_stale_marker() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        let p = propose_transition(&store, "run-7", h.id, Testing, "pins hold").unwrap();
        user_transitions(&store, h.id, Proposed, Testing, "user ran the suite first");
        // force past the stale basis: the approval records the marker
        let outcome = approve(&store, p.id, true, None).unwrap();
        assert_eq!(outcome.proposal.status, ProposalStatus::Merged);
        assert!(outcome.proposal.basis_stale, "the marker the UI must surface");
        let merge = store
            .events_all().unwrap()
            .into_iter()
            .find(|e| e.kind == MERGE_APPROVED)
            .unwrap();
        let payload: MergeApprovedPayload = serde_json::from_value(merge.payload.clone()).unwrap();
        assert!(payload.force);
        assert!(payload.basis_stale);
        // and the change still applied
        assert_eq!(board_status(&store, h.id), Testing);
    }

    #[test]
    fn a_second_merge_attempt_on_the_same_proposal_fails_loudly() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        let p = propose_transition(&store, "run-7", h.id, Testing, "pins hold").unwrap();
        approve(&store, p.id, false, None).unwrap();
        let head = store.head_seq().unwrap();
        let err = approve(&store, p.id, false, None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("not_pending"), "unexpected: {msg}");
        assert!(msg.contains("merged"), "names the current status: {msg}");
        assert_eq!(store.head_seq().unwrap(), head, "nothing was appended");
        // forcing does not help — a decided proposal is decided
        let err = approve(&store, p.id, true, None).unwrap_err();
        assert!(err.to_string().contains("not_pending"), "unexpected: {err}");
    }

    // ---------- AD-13: approval-order-by-seq fold ----------

    /// Two proposals on one entity, approved in the OPPOSITE order of their
    /// creation: the fold applies them by approval order (merge.approved
    /// seq) — the last APPROVED proposal's `to` wins. A proposal-order fold
    /// would end on the last CREATED proposal's `to` instead: this is the
    /// divergence the rule exists to prevent.
    #[test]
    fn the_fold_applies_merged_proposals_by_approval_order_not_proposal_order() {
        // Store A: P1 created first (proposed→testing), the user advances the
        // entity, P2 created second (testing→supported); approvals run in the
        // OPPOSITE order: P2 first (clean), then P1 (forced past the stale
        // basis). Approval order says P1's `to` lands last.
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        let p1 = propose_transition(&store, "run-7", h.id, Testing, "first: move to testing").unwrap();
        user_transitions(&store, h.id, Proposed, Testing, "user advanced the entity");
        let p2 = propose_transition(&store, "run-8", h.id, Supported, "second: evidence holds").unwrap();
        assert!(p1.seq < p2.seq, "proposal order: P1 before P2");
        assert_ne!(p1.basis_seq, p2.basis_seq, "different bases — not conflicting siblings");
        // approvals in opposite order: P2 (clean), then P1 (forced)
        approve(&store, p2.id, false, None).unwrap();
        let out1 = approve(&store, p1.id, true, None).unwrap();
        assert!(out1.proposal.basis_stale, "P1 merged past its stale basis");
        // the fold applied by approval order: P1 (last approved) wins
        assert_eq!(board_status(&store, h.id), Testing, "approval order: P1's to wins");
        let hyp = board(&store).into_iter().find(|x| x.id == h.id).unwrap();
        assert_eq!(hyp.audit.basis, "first: move to testing");

        // The counterfactual, in the mirror store: the SAME two proposals
        // approved in creation order (P1 first — forced past the user's
        // transition — then P2, forced past P1's merge). There the fold ends
        // on P2's `to`. Different approval orders, different deterministic
        // outcomes — the fold tracks APPROVALS, never proposal order.
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        let p1 = propose_transition(&store, "run-7", h.id, Testing, "first: move to testing").unwrap();
        user_transitions(&store, h.id, Proposed, Testing, "user advanced the entity");
        let p2 = propose_transition(&store, "run-8", h.id, Supported, "second: evidence holds").unwrap();
        approve(&store, p1.id, true, None).unwrap();
        approve(&store, p2.id, true, None).unwrap();
        assert_eq!(board_status(&store, h.id), Supported, "creation-order approvals end on P2's to");
        // the divergence: the two orders fold to different states, each
        // deterministic — a proposal-order fold would always end on P2's
        // `to` (the later proposal), which is exactly what store A does NOT do.
    }

    // ---------- conflicting pending siblings ----------

    #[test]
    fn a_conflicting_pending_sibling_is_superseded_by_the_merge() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        // two alternatives derived from the SAME state (same basis)
        let p1 = propose_transition(&store, "run-7", h.id, Testing, "alternative A").unwrap();
        let p2 = propose_transition(&store, "run-8", h.id, Testing, "alternative B").unwrap();
        assert_eq!(p1.basis_seq, p2.basis_seq, "same basis — true alternatives");
        // merging P2 supersedes the pending sibling P1
        let outcome = approve(&store, p2.id, false, None).unwrap();
        assert_eq!(outcome.superseded.len(), 1);
        assert_eq!(outcome.superseded[0].id, p1.id);
        assert_eq!(outcome.superseded[0].status, ProposalStatus::Superseded);
        assert_eq!(outcome.superseded[0].superseded_by, Some(p2.id));
        // P1 stays in the read model — nothing disappears
        let p1_after = ProposalsProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .into_iter()
            .find(|p| p.id == p1.id)
            .unwrap();
        assert_eq!(p1_after.status, ProposalStatus::Superseded);
        assert!(p1_after.decided.is_some(), "the supersession carries a receipt stamp");
        // a later merge attempt on the superseded sibling fails loudly
        let head = store.head_seq().unwrap();
        let err = approve(&store, p1.id, false, None).unwrap_err();
        assert!(err.to_string().contains("not_pending"), "unexpected: {err}");
        assert!(err.to_string().contains("superseded"), "unexpected: {err}");
        assert_eq!(store.head_seq().unwrap(), head, "nothing was appended");
        // the superseded alternative never touched the board
        assert_eq!(board_status(&store, h.id), Testing, "P2's merge applied, P1's did not");
    }

    #[test]
    fn non_conflicting_siblings_stay_pending_until_decided() {
        // different bases (derived from different states) are sequential
        // intent, not alternatives — a merge does not supersede them
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        let p1 = propose_transition(&store, "run-7", h.id, Testing, "first").unwrap();
        user_transitions(&store, h.id, Proposed, Testing, "user advanced");
        let p2 = propose_transition(&store, "run-8", h.id, Supported, "second").unwrap();
        let outcome = approve(&store, p2.id, false, None).unwrap();
        assert!(outcome.superseded.is_empty(), "different bases — no supersession");
        let p1_after = ProposalsProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .into_iter()
            .find(|p| p.id == p1.id)
            .unwrap();
        assert_eq!(p1_after.status, ProposalStatus::Pending);
        assert!(p1_after.basis_stale, "but its basis is now stale — surfaced");
    }

    // ---------- rejected / voided ----------

    #[test]
    fn rejected_and_voided_proposals_can_never_merge() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        // rejected
        let p1 = propose_transition(&store, "run-7", h.id, Testing, "will be rejected").unwrap();
        let rejected = reject(&store, p1.id, None).unwrap();
        assert_eq!(rejected.status, ProposalStatus::Rejected);
        assert!(rejected.decided.is_some());
        let err = approve(&store, p1.id, false, None).unwrap_err();
        assert!(err.to_string().contains("not_pending"), "unexpected: {err}");
        assert!(err.to_string().contains("rejected"), "unexpected: {err}");
        assert_eq!(board_status(&store, h.id), Proposed, "a rejection never touches the board");
        // voided
        let p2 = propose_transition(&store, "run-8", h.id, Testing, "will be voided").unwrap();
        let voided = void(&store, p2.id).unwrap();
        assert_eq!(voided.status, ProposalStatus::Voided);
        let err = approve(&store, p2.id, true, None).unwrap_err();
        assert!(err.to_string().contains("not_pending"), "unexpected: {err}");
        assert!(err.to_string().contains("voided"), "unexpected: {err}");
        // a decided proposal can also never be re-rejected
        let err = reject(&store, p1.id, None).unwrap_err();
        assert!(err.to_string().contains("not_pending"), "unexpected: {err}");
    }

    #[test]
    fn reject_and_void_refuse_unknown_proposals_without_appending() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        assert!(approve(&store, Uuid::new_v4(), false, None).unwrap_err().to_string().contains("not_found"));
        assert!(reject(&store, Uuid::new_v4(), None).unwrap_err().to_string().contains("not_found"));
        assert!(void(&store, Uuid::new_v4()).unwrap_err().to_string().contains("not_found"));
        assert_eq!(store.head_seq().unwrap(), 0, "nothing was appended");
    }

    // ---------- fold invariants ----------

    #[test]
    fn the_fold_fails_loudly_on_a_double_decide() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        let p = propose_transition(&store, "run-7", h.id, Testing, "pins hold").unwrap();
        approve(&store, p.id, false, None).unwrap();
        // a second merge.approved appended by hand (the command path refuses
        // it first) — the fold must fail loudly, not silently re-decide
        store.append(NewEvent::merge_approved(p.id, false, false, None).unwrap()).unwrap();
        let err = ProposalsProjection::fold(&store.events_all().unwrap())
            .expect_err("a double decide contradicts the log");
        assert!(err.to_string().contains("not pending"), "unexpected: {err}");
    }

    #[test]
    fn the_fold_skips_decide_events_referencing_unknown_proposals() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let ghost = Uuid::new_v4();
        store.append(NewEvent::merge_approved(ghost, false, false, None).unwrap()).unwrap();
        store.append(NewEvent::merge_rejected(ghost, None).unwrap()).unwrap();
        assert!(ProposalsProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn the_fold_fails_loudly_on_a_non_agent_proposal() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let payload = ProposalCreatedPayload {
            proposed_kind: HYPOTHESIS_STATUS_CHANGED.into(),
            proposed_payload: json!({ "from": "proposed", "to": "testing", "basis": "x" }),
            target_entity: Uuid::new_v4(),
            basis_seq: 3,
            cause: Uuid::new_v4(),
        };
        store
            .append(
                NewEvent::new(PROPOSAL_CREATED, Actor::User, serde_json::to_value(&payload).unwrap())
                    .unwrap(),
            )
            .unwrap();
        let err = ProposalsProjection::fold(&store.events_all().unwrap())
            .expect_err("a user-actor proposal contradicts AD-3");
        assert!(err.to_string().contains("agent-actor"), "unexpected: {err}");
    }

    #[test]
    fn fold_for_scopes_by_the_targets_mission() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m1 = seed_mission(&store);
        let m2 = seed_mission(&store);
        let h1 = seed_hypothesis(&store, m1);
        let h2 = seed_hypothesis(&store, m2);
        propose_transition(&store, "run-7", h1.id, Testing, "a").unwrap();
        propose_transition(&store, "run-8", h2.id, Testing, "b").unwrap();
        let scoped = ProposalsProjection::fold_for(&store.events_all().unwrap(), m1).unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].target_entity, h1.id);
        assert!(ProposalsProjection::fold_for(&store.events_all().unwrap(), Uuid::new_v4())
            .unwrap()
            .is_empty());
    }

    // ---------- evidence-pin proposals (Story 3.4, FR-11.5) ----------

    use crate::domain::evidence::{excerpt_digest, EvidenceProjection, EVIDENCE_PINNED};

    /// The fetch-results seam: an anchor claim + one proposal whose intended
    /// payload is a numerical pin with the digest computed at proposal time,
    /// basis derived from the hypothesis's current state, cause-linked to
    /// the producing event + the job + the mission.
    #[test]
    fn propose_evidence_pin_derives_the_basis_and_computes_the_digest() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        let claim = store
            .append(NewEvent::claim_registered("Result artifact.", h.id, None).unwrap())
            .unwrap();
        let content = "benchmark: 42.7 mean, ±0.4, n=12";
        let p = propose_evidence_pin(
            &store,
            "fetch-1",
            claim.id,
            h.id,
            "jobs/abc/stdout",
            content,
            0.5,
            "GLM-5.3",
            h.id, // the producing event (a job terminal in the real flow)
            vec![Uuid::new_v4(), m], // the job + the mission
        )
        .unwrap();
        assert_eq!(p.status, ProposalStatus::Pending);
        assert_eq!(p.run_id, "fetch-1");
        assert_eq!(p.target_entity, h.id);
        assert_eq!(p.proposed_kind, EVIDENCE_PINNED);
        assert_eq!(p.basis_seq, h.seq, "basis = the hypothesis's current state seq");
        // the intended pin: kind numerical, digest computed at proposal time
        assert_eq!(
            p.proposed_payload["kind"],
            json!("numerical"),
            "the intended payload: {p:?}"
        );
        assert_eq!(p.proposed_payload["artifact_ref"], json!("jobs/abc/stdout"));
        assert_eq!(p.proposed_payload["excerpt"], json!(content));
        assert_eq!(p.proposed_payload["digest"], json!(excerpt_digest(content)));
        assert_eq!(p.proposed_payload["assessing_model"], json!("GLM-5.3"));
        // cause-linked: the producing event, the job, the mission, the target
        let created = store
            .events_all()
            .unwrap()
            .into_iter()
            .find(|e| e.id == p.id)
            .unwrap();
        assert!(created.causes.contains(&m), "cause-linked to the mission");
        assert_eq!(created.actor, Actor::Agent { run_id: "fetch-1".into() });
        // excluded until merged: the claim still reads unpinned
        let [c] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        assert!(!c.pinned, "AD-3: a pending pin proposal pins nothing");
        // a ghost target hypothesis is refused
        let err = propose_evidence_pin(
            &store,
            "fetch-1",
            claim.id,
            Uuid::new_v4(),
            "jobs/abc/stdout",
            content,
            0.5,
            "GLM-5.3",
            h.id,
            vec![],
        )
        .unwrap_err();
        assert!(err.to_string().contains("not_found"), "unexpected: {err}");
    }

    /// Merging the pin proposal applies the numerical pin — through the
    /// fold, at the approval event; the intended event NEVER lands in the
    /// log as its own event (auto-pinning is structurally absent, v0.2.0).
    #[test]
    fn merging_a_pin_proposal_pins_the_claim_without_appending_the_pin() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        let claim = store
            .append(NewEvent::claim_registered("Result artifact.", h.id, None).unwrap())
            .unwrap();
        let content = "gain: 12.3%, p<0.01, n=48";
        let p = propose_evidence_pin(
            &store,
            "fetch-1",
            claim.id,
            h.id,
            "jobs/abc/stdout",
            content,
            0.5,
            "GLM-5.3",
            h.id,
            vec![m],
        )
        .unwrap();
        // the structural guarantee, part 1: no evidence.pinned event exists
        // in the log — the pin proposal carries its intended payload only
        assert!(
            !store
                .events_all()
                .unwrap()
                .iter()
                .any(|e| e.kind == EVIDENCE_PINNED),
            "the intended pin never lands as its own event"
        );
        let merge = approve(&store, p.id, false, None).unwrap();
        assert_eq!(merge.proposal.status, ProposalStatus::Merged);
        // part 2: still no evidence.pinned event — the fold applies the
        // intended pin AT the merge (AD-13), and no actor=agent path ever
        // appends one
        let all = store.events_all().unwrap();
        assert!(
            !all.iter().any(|e| e.kind == EVIDENCE_PINNED),
            "the merge applies the pin in-fold — the log holds the proposal + the approval only"
        );
        // the claim reads pinned with the full AD-5 numerical anatomy
        let [c] = EvidenceProjection::fold(&all)
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        assert!(c.pinned);
        let pin = c.pin.as_ref().expect("the merged pin");
        assert_eq!(pin.kind, crate::domain::evidence::PinKind::Numerical);
        assert_eq!(pin.artifact_ref.as_deref(), Some("jobs/abc/stdout"));
        assert_eq!(pin.digest, excerpt_digest(content));
        assert_eq!(pin.confidence, 0.5);
        assert_eq!(pin.assessing_model, "GLM-5.3");
        let merge_event = all
            .iter()
            .find(|e| e.kind == MERGE_APPROVED)
            .unwrap();
        assert_eq!(pin.seq, merge_event.seq, "the merged pin carries the merge's seq");
        // and the hypothesis fold is undisturbed — a pin proposal never
        // transitions anything
        let hyps = HypothesesProjection::fold(&all).unwrap();
        assert_eq!(hyps[0].status, crate::domain::hypotheses::HypothesisStatus::Proposed);
    }

    /// AD-13 on pin proposals: a hypothesis that advanced past the basis
    /// refuses the merge with `basis_stale:` unless forced — and a forced
    /// merge records the marker, exactly like a transition proposal.
    #[test]
    fn a_stale_pin_proposal_merges_only_past_the_recorded_marker() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        let claim = store
            .append(NewEvent::claim_registered("Result artifact.", h.id, None).unwrap())
            .unwrap();
        let p = propose_evidence_pin(
            &store,
            "fetch-1",
            claim.id,
            h.id,
            "jobs/abc/stdout",
            "values",
            0.5,
            "GLM-5.3",
            h.id,
            vec![m],
        )
        .unwrap();
        // the hypothesis advances past the pin proposal's basis
        user_transitions(&store, h.id, Proposed, Testing, "user ran the suite first");
        let head = store.head_seq().unwrap();
        let err = approve(&store, p.id, false, None).unwrap_err();
        assert!(err.to_string().contains("basis_stale"), "unexpected: {err}");
        assert_eq!(store.head_seq().unwrap(), head, "the refusal is a dry run");
        // the pending read model carries the derived stale flag
        let pending = ProposalsProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .into_iter()
            .find(|x| x.id == p.id)
            .unwrap();
        assert!(pending.basis_stale, "the pending card renders the warning variant");
        // forced: the marker is recorded, and the pin still applies
        let outcome = approve(&store, p.id, true, None).unwrap();
        assert!(outcome.proposal.basis_stale, "the marker the UI must surface");
        let [c] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        assert!(c.pinned, "the forced merge still pins");
    }

    /// AD-5 at merge-check time: a hand-tampered intended payload — a
    /// digest that does not match its own excerpt — is refused with the
    /// typed `digest_mismatch:` error and can NEVER merge (force is for
    /// stale bases, not corrupt pins).
    #[test]
    fn a_tampered_pin_candidate_is_refused_at_merge_check_time() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        let claim = store
            .append(NewEvent::claim_registered("Result artifact.", h.id, None).unwrap())
            .unwrap();
        // a raw-appended proposal whose intended digest matches DIFFERENT
        // content than the excerpt it quotes — the constructor cannot
        // produce this; only a hand-built payload can
        let tampered = json!({
            "claim_id": claim.id.to_string(),
            "hypothesis_id": h.id.to_string(),
            "kind": "numerical",
            "artifact_ref": "jobs/abc/stdout",
            "excerpt": "gain: 12.3%, n=48",
            "digest": excerpt_digest("entirely different numbers"),
            "confidence": 0.5,
            "assessing_model": "GLM-5.3",
        });
        let intended = NewEvent::new(
            EVIDENCE_PINNED,
            Actor::User,
            tampered,
        )
        .unwrap();
        let p = store
            .append(
                NewEvent::proposal_created("fetch-1", &intended, h.id, h.seq, h.id)
                    .unwrap(),
            )
            .unwrap();
        let head = store.head_seq().unwrap();
        let err = approve(&store, p.id, false, None).unwrap_err();
        assert!(err.to_string().contains("digest_mismatch"), "unexpected: {err}");
        // force does not help — a corrupt candidate never merges
        let err = approve(&store, p.id, true, None).unwrap_err();
        assert!(err.to_string().contains("digest_mismatch"), "unexpected: {err}");
        assert_eq!(store.head_seq().unwrap(), head, "nothing was appended");
        // and nothing was pinned
        let [c] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        assert!(!c.pinned);
    }

    /// The vocabulary is closed but now carries both kinds: a transition
    /// proposal and a pin proposal coexist in one quarantine view.
    #[test]
    fn transition_and_pin_proposals_coexist_in_quarantine() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = seed_hypothesis(&store, m);
        let claim = store
            .append(NewEvent::claim_registered("Result artifact.", h.id, None).unwrap())
            .unwrap();
        let t = propose_transition(&store, "run-7", h.id, Testing, "the scan suggests testing")
            .unwrap();
        let p = propose_evidence_pin(
            &store,
            "fetch-1",
            claim.id,
            h.id,
            "jobs/abc/stdout",
            "values",
            0.5,
            "GLM-5.3",
            h.id,
            vec![m],
        )
        .unwrap();
        let all = ProposalsProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|x| x.id == t.id && x.proposed_kind == HYPOTHESIS_STATUS_CHANGED));
        assert!(all.iter().any(|x| x.id == p.id && x.proposed_kind == EVIDENCE_PINNED));
        // live_for_job scopes to the job's proposals (the idempotency read)
        assert_eq!(
            ProposalsProjection::live_for_job(&store.events_all().unwrap(), m).unwrap().len(),
            1,
            "the mission id was passed as the job link in this fixture"
        );
        assert!(ProposalsProjection::live_for_job(&store.events_all().unwrap(), Uuid::new_v4())
            .unwrap()
            .is_empty());
    }
}
