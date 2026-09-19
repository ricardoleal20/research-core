// Checkpoints domain (FR-10.1, AD-1, AD-11, Story 2.6): named seq positions
// and the rollback that NEVER rewrites history. Creating a checkpoint
// appends `checkpoint.created` carrying the log head at creation; rolling
// back appends `checkpoint.rolled_back` carrying the target checkpoint's
// seq. The log is never truncated and there is no mutable head pointer —
// instead every domain projection folds through the shared `FoldCursor`
// below: fold the prefix (seq <= target), SKIP the orphaned suffix (the
// events after the checkpoint up to the rollback event), then continue
// with the events appended after the rollback action.
//
// Orphaned events are excluded from projections but never hidden
// (EXPERIENCE.md): the rollback plan/outcome lists every orphaned event —
// and every orphaned PROPOSAL by name — so the confirmation and the
// superseded-history views can render them.
//
// AD-11 (export consistency): an export renders at a single named seq cut.
// A rollback appended AFTER the cut changes what that cut now means, so
// the export must be flagged stale — `export_is_stale` is the signal
// Story 3.1's exporter consumes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::proposals::{ProposalCreatedPayload, PROPOSAL_CREATED};
use crate::eventstore::{Actor, EventError, EventStore, NewEvent, StoredEvent};

pub const CHECKPOINT_CREATED: &str = "checkpoint.created";
pub const CHECKPOINT_ROLLED_BACK: &str = "checkpoint.rolled_back";

// ---------------------------------------------------------------------------
// Typed constructors (AD-15)
// ---------------------------------------------------------------------------

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `checkpoint.created` event
    /// comes into being — the user naming the current log head as a restore
    /// point (FR-10.1). `seq` is the log head AT CREATION (the command
    /// captures it; 0 = the empty log's beginning). Actor is always the
    /// user: a checkpoint is a human decision, never telemetry.
    pub fn checkpoint_created(name: &str, seq: i64) -> Result<Self, EventError> {
        let payload = CheckpointCreatedPayload { name: name.trim().to_string(), seq };
        if payload.name.is_empty() {
            return Err(EventError::Invalid(
                "checkpoint.created requires a name — a restore point is named (FR-10.1)".into(),
            ));
        }
        if payload.seq < 0 {
            return Err(EventError::Invalid(
                "checkpoint.created requires the log head at creation — a seq position (AD-2)"
                    .into(),
            ));
        }
        Self::new(CHECKPOINT_CREATED, Actor::User, serde_json::to_value(&payload)?)
    }

    /// Typed constructor (AD-15): the one way a `checkpoint.rolled_back`
    /// event comes into being — the user rolling the read model back to a
    /// named checkpoint (AD-1). The payload carries the target checkpoint's
    /// id, name and seq, plus how many events the rollback orphaned (the
    /// honest count the confirmation surfaces). Cause-linked to the target
    /// checkpoint so the audit trail reads end to end.
    pub fn checkpoint_rolled_back(
        checkpoint_id: Uuid,
        name: &str,
        target_seq: i64,
        orphaned_count: u64,
    ) -> Result<Self, EventError> {
        let payload = CheckpointRolledBackPayload {
            checkpoint_id,
            name: name.trim().to_string(),
            target_seq,
            orphaned_count,
        };
        if payload.name.is_empty() {
            return Err(EventError::Invalid(
                "checkpoint.rolled_back requires the target checkpoint's name — the rollback \
                 names where it returns to"
                    .into(),
            ));
        }
        if payload.target_seq < 0 {
            return Err(EventError::Invalid(
                "checkpoint.rolled_back requires the target's seq position (AD-2)".into(),
            ));
        }
        if payload.checkpoint_id.is_nil() {
            return Err(EventError::Invalid(
                "checkpoint.rolled_back requires the target checkpoint's id — a rollback names \
                 the restore point it returns to"
                    .into(),
            ));
        }
        Ok(Self::new(
            CHECKPOINT_ROLLED_BACK,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?
        .with_causes(vec![checkpoint_id]))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointCreatedPayload {
    pub name: String,
    pub seq: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRolledBackPayload {
    pub checkpoint_id: Uuid,
    pub name: String,
    pub target_seq: i64,
    pub orphaned_count: u64,
}

// ---------------------------------------------------------------------------
// The shared fold cursor (AD-1 — the rollback fold rule)
// ---------------------------------------------------------------------------

/// The shared fold cursor every domain projection folds through (AD-1):
/// which events are LIVE for the read model. Computed from the raw log's
/// `checkpoint.rolled_back` events — each rollback to target seq T appends
/// at seq R and orphans the open-closed interval (T, R]: the prefix
/// (seq <= T) folds, the orphaned suffix is skipped, and events appended
/// after the rollback action (seq > R) continue folding. A second rollback
/// extends the orphaned set the same way — orphaned ranges only grow, so
/// folding is order-independent and idempotent.
///
/// Projections call `FoldCursor::over(events)` on the RAW slice, then fold
/// `cursor.live(events)` — never the raw slice. The cursor itself always
/// reads the raw log (a filtered slice contains no rollback events).
pub struct FoldCursor {
    /// Orphaned intervals `(target_seq, rollback_seq]`, in log order.
    orphaned: Vec<(i64, i64)>,
}

impl FoldCursor {
    /// Compute the cursor over the RAW log. Pure.
    pub fn over(events: &[StoredEvent]) -> Self {
        let mut orphaned = Vec::new();
        for event in events {
            if event.kind != CHECKPOINT_ROLLED_BACK {
                continue;
            }
            if let Some(target) = event.payload.get("target_seq").and_then(|v| v.as_i64()) {
                orphaned.push((target, event.seq));
            }
        }
        Self { orphaned }
    }

    /// Is this event live for the read model? A domain event is orphaned
    /// when any rollback's interval `(target, rollback_seq]` contains it.
    /// Checkpoint bookkeeping (`checkpoint.created` /
    /// `checkpoint.rolled_back`) is NEVER orphaned: checkpoints are named
    /// positions in an append-only history, not domain state — a rollback
    /// returns the read model to a position without erasing the positions
    /// themselves (and a rollback to the newest checkpoint orphans nothing
    /// of substance).
    pub fn is_live(&self, event: &StoredEvent) -> bool {
        if event.kind == CHECKPOINT_CREATED || event.kind == CHECKPOINT_ROLLED_BACK {
            return true;
        }
        !self
            .orphaned
            .iter()
            .any(|&(target, rollback)| event.seq > target && event.seq <= rollback)
    }

    /// Is the seq POSITION `seq` live? (Positions, unlike bookkeeping
    /// events, orphan with the domain events around them — a checkpoint
    /// whose recorded head sits inside a rolled-back range cannot be
    /// restored to.)
    pub fn position_is_live(&self, seq: i64) -> bool {
        !self.orphaned.iter().any(|&(target, rollback)| seq > target && seq <= rollback)
    }

    /// The live events, in seq order — the slice every projection folds.
    pub fn live<'a>(
        &self,
        events: &'a [StoredEvent],
    ) -> impl Iterator<Item = &'a StoredEvent> + use<'a, '_> {
        events.iter().filter(move |e| self.is_live(e))
    }

    /// The live events as an owned slice (for passing to sub-folds that
    /// take `&[StoredEvent]` — idempotent: a cursor over a filtered slice
    /// finds no rollbacks and changes nothing).
    pub fn live_owned(&self, events: &[StoredEvent]) -> Vec<StoredEvent> {
        self.live(events).cloned().collect()
    }

    /// The orphaned events — the SUPERSEDED HISTORY (EXPERIENCE.md):
    /// excluded from every projection, but listable, never hidden. Checkpoint
    /// bookkeeping never appears here (it is never orphaned).
    pub fn orphaned<'a>(
        &self,
        events: &'a [StoredEvent],
    ) -> impl Iterator<Item = &'a StoredEvent> + use<'a, '_> {
        events.iter().filter(move |e| !self.is_live(e))
    }

    /// The seq of the rollback event that orphaned the event at `seq`, when
    /// it is orphaned — the "superseded by rollback e-{seq}" stamp.
    pub fn rolled_back_at(&self, seq: i64) -> Option<i64> {
        self.orphaned
            .iter()
            .find(|&&(target, rollback)| seq > target && seq <= rollback)
            .map(|&(_, rollback)| rollback)
    }
}

// ---------------------------------------------------------------------------
// Read models (AD-8)
// ---------------------------------------------------------------------------

/// One restore point as read from the log (FR-10.1). Field names are
/// camelCase on the wire (Tauri 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    /// The `checkpoint.created` event's id — the checkpoint's identity.
    pub id: Uuid,
    /// The log head at creation — the seq the read model returns to.
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub name: String,
}

/// One rollback in the log — the history the checkpoint control lists
/// under its restore points (never hidden, EXPERIENCE.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackRecord {
    /// The `checkpoint.rolled_back` event's seq.
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub checkpoint_id: Uuid,
    pub name: String,
    pub target_seq: i64,
    pub orphaned_count: u64,
}

/// Everything the checkpoint control renders (FR-10.1): the current head,
/// the restore points, and the rollback history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointsView {
    pub head_seq: i64,
    pub checkpoints: Vec<Checkpoint>,
    pub rollbacks: Vec<RollbackRecord>,
}

/// One orphaned event, as the rollback confirmation lists it — the
/// superseded history (EXPERIENCE.md: never hidden, never summarized away).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanedEvent {
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub kind: String,
    /// Short actor label: `user` | `agent` | `system:<component>`.
    pub actor: String,
}

/// One orphaned PROPOSAL, by name — the confirmation's non-negotiable
/// content (EXPERIENCE.md: the rollback confirmation names every orphaned
/// proposal, never a summary).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanedProposal {
    pub proposal_id: Uuid,
    pub seq: i64,
    /// The target hypothesis's statement — the proposal's NAME.
    pub target_label: Option<String>,
    /// The target hypothesis's creation seq — the H-n label.
    pub target_seq: Option<i64>,
    /// The transition the proposal intended (e.g. `testing`).
    pub proposed_to: Option<String>,
    /// The agent's basis for the change — one line, why it proposed it.
    pub basis: Option<String>,
}

/// The plan of a rollback BEFORE it happens — what the confirmation dialog
/// renders (every orphaned proposal by name, the full orphaned event list).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackPlan {
    pub checkpoint: Checkpoint,
    pub orphaned_events: Vec<OrphanedEvent>,
    pub orphaned_proposals: Vec<OrphanedProposal>,
}

/// The outcome of an executed rollback — the record plus exactly what it
/// orphaned (the post-rollback superseded history).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackOutcome {
    pub rollback: RollbackRecord,
    pub orphaned_events: Vec<OrphanedEvent>,
    pub orphaned_proposals: Vec<OrphanedProposal>,
}

/// Everything that can go wrong checkpointing or rolling back — typed, with
/// stable codes the UI never translates (EXPERIENCE.md).
#[derive(Debug, thiserror::Error)]
pub enum CheckpointError {
    #[error("not_found: no checkpoint with id `{0}`")]
    NotFound(Uuid),
    #[error("orphaned_checkpoint: checkpoint `{0}` sits inside a rolled-back range — restore to a live checkpoint")]
    OrphanedCheckpoint(Uuid),
    #[error(transparent)]
    Store(#[from] EventError),
}

/// Pure fold of the log into the checkpoints view (FR-10.1): restore points
/// from `checkpoint.created` events (a checkpoint inside a rolled-back
/// range stays listed — it is history — but cannot be rolled back to), the
/// rollback history from `checkpoint.rolled_back` events. Corrupt payloads
/// fail loudly (the projections' contract).
pub fn fold_checkpoints(events: &[StoredEvent]) -> Result<CheckpointsView, EventError> {
    let mut head_seq = 0;
    let mut checkpoints = Vec::new();
    let mut rollbacks = Vec::new();
    for event in events {
        head_seq = head_seq.max(event.seq);
        match event.kind.as_str() {
            CHECKPOINT_CREATED => {
                let payload: CheckpointCreatedPayload = serde_json::from_value(
                    event.payload.clone(),
                )
                .map_err(|e| {
                    EventError::Invalid(format!(
                        "corrupt {CHECKPOINT_CREATED} payload at seq {}: {e}",
                        event.seq
                    ))
                })?;
                checkpoints.push(Checkpoint {
                    id: event.id,
                    seq: payload.seq,
                    ts: event.ts,
                    name: payload.name,
                });
            }
            CHECKPOINT_ROLLED_BACK => {
                let payload: CheckpointRolledBackPayload = serde_json::from_value(
                    event.payload.clone(),
                )
                .map_err(|e| {
                    EventError::Invalid(format!(
                        "corrupt {CHECKPOINT_ROLLED_BACK} payload at seq {}: {e}",
                        event.seq
                    ))
                })?;
                rollbacks.push(RollbackRecord {
                    seq: event.seq,
                    ts: event.ts,
                    checkpoint_id: payload.checkpoint_id,
                    name: payload.name,
                    target_seq: payload.target_seq,
                    orphaned_count: payload.orphaned_count,
                });
            }
            _ => {}
        }
    }
    Ok(CheckpointsView { head_seq, checkpoints, rollbacks })
}

// ---------------------------------------------------------------------------
// The rollback plan and the rollback itself (AD-1)
// ---------------------------------------------------------------------------

fn actor_label(actor: &Actor) -> String {
    match actor {
        Actor::User => "user".into(),
        Actor::Agent { .. } => "agent".into(),
        Actor::System { component } => {
            format!("system:{}", format!("{component:?}").to_lowercase())
        }
    }
}

/// The enrichment map for orphaned-proposal labels: hypothesis id →
/// (seq, statement), from the RAW log (an orphaned proposal may target a
/// hypothesis whose own creation was orphaned — the name still resolves).
fn hypothesis_labels(events: &[StoredEvent]) -> std::collections::HashMap<Uuid, (i64, String)> {
    let mut labels = std::collections::HashMap::new();
    for event in events {
        if event.kind != crate::domain::hypotheses::HYPOTHESIS_CREATED {
            continue;
        }
        if let Ok(payload) = serde_json::from_value::<
            crate::domain::hypotheses::HypothesisCreatedPayload,
        >(event.payload.clone())
        {
            labels.insert(event.id, (event.seq, payload.statement));
        }
    }
    labels
}

/// Compute what rolling back to `checkpoint_id` WOULD orphan right now: the
/// currently-LIVE events after the checkpoint's seq. Events already
/// orphaned by an earlier rollback are not re-listed (they are already
/// superseded history). Every orphaned proposal is listed BY NAME. Pure.
pub fn rollback_plan(
    events: &[StoredEvent],
    checkpoint_id: Uuid,
) -> Result<RollbackPlan, CheckpointError> {
    let view = fold_checkpoints(events)?;
    let Some(checkpoint) = view.checkpoints.iter().find(|c| c.id == checkpoint_id) else {
        return Err(CheckpointError::NotFound(checkpoint_id));
    };
    let cursor = FoldCursor::over(events);
    if !cursor.position_is_live(checkpoint.seq) {
        return Err(CheckpointError::OrphanedCheckpoint(checkpoint_id));
    }
    let labels = hypothesis_labels(events);
    let mut orphaned_events = Vec::new();
    let mut orphaned_proposals = Vec::new();
    for event in cursor.live(events) {
        if event.seq <= checkpoint.seq {
            continue; // the prefix — the state the rollback returns to
        }
        if event.kind == CHECKPOINT_CREATED || event.kind == CHECKPOINT_ROLLED_BACK {
            continue; // bookkeeping: positions in history, never domain state
        }
        orphaned_events.push(OrphanedEvent {
            seq: event.seq,
            ts: event.ts,
            kind: event.kind.clone(),
            actor: actor_label(&event.actor),
        });
        if event.kind == PROPOSAL_CREATED {
            if let Ok(payload) =
                serde_json::from_value::<ProposalCreatedPayload>(event.payload.clone())
            {
                orphaned_proposals.push(OrphanedProposal {
                    proposal_id: event.id,
                    seq: event.seq,
                    target_label: labels.get(&payload.target_entity).map(|(_, s)| s.clone()),
                    target_seq: labels.get(&payload.target_entity).map(|(s, _)| *s),
                    proposed_to: payload
                        .proposed_payload
                        .get("to")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    basis: payload
                        .proposed_payload
                        .get("basis")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                });
            }
        }
    }
    Ok(RollbackPlan { checkpoint: checkpoint.clone(), orphaned_events, orphaned_proposals })
}

/// Execute the rollback (AD-1): validate the checkpoint is live, compute
/// the plan, and append ONE `checkpoint.rolled_back` event carrying the
/// target and the orphaned count. History is never rewritten — the
/// projections' fold cursor does the returning. Returns the outcome with
/// every orphaned event and proposal listed (the superseded history).
pub fn rollback(
    store: &EventStore<'_>,
    checkpoint_id: Uuid,
) -> Result<RollbackOutcome, CheckpointError> {
    let events = store.events_all()?;
    let plan = rollback_plan(&events, checkpoint_id)?;
    let orphaned_count = plan.orphaned_events.len() as u64;
    let rollback_event = store.append(NewEvent::checkpoint_rolled_back(
        checkpoint_id,
        &plan.checkpoint.name,
        plan.checkpoint.seq,
        orphaned_count,
    )?)?;
    Ok(RollbackOutcome {
        rollback: RollbackRecord {
            seq: rollback_event.seq,
            ts: rollback_event.ts,
            checkpoint_id,
            name: plan.checkpoint.name.clone(),
            target_seq: plan.checkpoint.seq,
            orphaned_count,
        },
        orphaned_events: plan.orphaned_events,
        orphaned_proposals: plan.orphaned_proposals,
    })
}

// ---------------------------------------------------------------------------
// AD-11: export staleness
// ---------------------------------------------------------------------------

/// Is an export taken at `export_seq_cut` stale? (AD-11: an export renders
/// at a single named seq cut.) A `checkpoint.rolled_back` appended AFTER
/// the cut changes what that cut means — the read model no longer folds the
/// way the export rendered it — so the export must be re-rendered or
/// marked stale. Rollbacks at or before the cut were already applied when
/// the export rendered; newer non-rollback events never stale a named cut.
pub fn export_is_stale(events: &[StoredEvent], export_seq_cut: i64) -> bool {
    events
        .iter()
        .any(|e| e.kind == CHECKPOINT_ROLLED_BACK && e.seq > export_seq_cut)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::hypotheses::{HypothesisStatus, HypothesesProjection};
    use crate::domain::missions::{MissionStatus, MissionsProjection};
    use crate::domain::proposals::{ProposalStatus, ProposalsProjection};
    use crate::eventstore::EventStore;
    use rusqlite::Connection;
    use serde_json::json;
    use uuid::Uuid;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    fn seed_mission(store: &EventStore<'_>) -> StoredEvent {
        store
            .append(
                NewEvent::mission_created(crate::domain::missions::MissionCreatedPayload {
                    question: "Does X hold up?".into(),
                    stop_condition: "Stop after $5.".into(),
                    success_criterion: "A rater agrees.".into(),
                    autonomy: crate::domain::missions::Autonomy::Suggest,
                    spend_ceiling_cents: 500,
                    roles: vec![],
                    schedule: "off".into(),
                })
                .unwrap(),
            )
            .unwrap()
    }

    #[test]
    fn checkpoint_constructors_carry_the_user_actor() {
        let ev = NewEvent::checkpoint_created("pre-trial-1", 42).unwrap();
        assert_eq!(ev.kind, CHECKPOINT_CREATED);
        assert_eq!(ev.actor, Actor::User);
        assert_eq!(ev.payload, json!({ "name": "pre-trial-1", "seq": 42 }));

        let id = Uuid::new_v4();
        let rb = NewEvent::checkpoint_rolled_back(id, "pre-trial-1", 42, 3).unwrap();
        assert_eq!(rb.kind, CHECKPOINT_ROLLED_BACK);
        assert_eq!(rb.actor, Actor::User);
        assert_eq!(rb.causes, vec![id]);
        assert_eq!(rb.payload["target_seq"], json!(42));
        assert_eq!(rb.payload["orphaned_count"], json!(3));

        // empty names, nil ids, negative seqs never become events
        assert!(NewEvent::checkpoint_created("  ", 1).is_err());
        assert!(NewEvent::checkpoint_created("ok", -1).is_err());
        assert!(NewEvent::checkpoint_rolled_back(Uuid::nil(), "n", 1, 0).is_err());
        assert!(NewEvent::checkpoint_rolled_back(id, "", 1, 0).is_err());
    }

    #[test]
    fn the_fold_cursor_skips_orphaned_suffixes_and_continues() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let mission = seed_mission(&store);
        // events 2-3: the work the checkpoint will return to
        let hyp = store
            .append(NewEvent::hypothesis_created("X holds.", mission.id).unwrap())
            .unwrap();
        store
            .append(
                NewEvent::hypothesis_status_changed(
                    HypothesisStatus::Proposed,
                    HypothesisStatus::Testing,
                    "checkpoint-era work",
                )
                .unwrap()
                .with_causes(vec![hyp.id]),
            )
            .unwrap();
        // the checkpoint: seq 4 is the head
        let checkpoint = store.append(NewEvent::checkpoint_created("pre-trial", 4).unwrap()).unwrap();
        // the orphaned suffix: post-checkpoint work (seq 5-7)
        store
            .append(
                NewEvent::hypothesis_status_changed(
                    HypothesisStatus::Testing,
                    HypothesisStatus::Supported,
                    "work the rollback orphans",
                )
                .unwrap()
                .with_causes(vec![hyp.id]),
            )
            .unwrap();
        store
            .append(NewEvent::checkpoint_created("later", 5).unwrap())
            .unwrap();
        let other = seed_mission(&store); // seq 7
        // the rollback: seq 8, target seq 4
        store
            .append(NewEvent::checkpoint_rolled_back(checkpoint.id, "pre-trial", 4, 3).unwrap())
            .unwrap();
        // post-rollback events continue folding (seq 9+)
        let post = store
            .append(NewEvent::hypothesis_created("Post-rollback hypothesis.", other.id).unwrap())
            .unwrap();

        let events = store.events_all().unwrap();
        let cursor = FoldCursor::over(&events);
        // the orphaned interval is (4, 8]: positions 5-8 are orphaned...
        assert!(!cursor.position_is_live(5));
        assert!(!cursor.position_is_live(7));
        assert!(!cursor.position_is_live(8), "the rollback's own position is not domain state");
        // ...but checkpoint bookkeeping (the "later" checkpoint at seq 6,
        // the rollback event at seq 8) is never orphaned — positions in an
        // append-only history, not domain state
        // the prefix and the post-rollback events fold
        assert!(cursor.position_is_live(4));
        assert!(cursor.position_is_live(9));
        let live: Vec<i64> = cursor.live(&events).map(|e| e.seq).collect();
        assert_eq!(live, vec![1, 2, 3, 4, 6, 8, 9]);

        // the whole read model agrees (AD-1): the board reflects the
        // checkpoint state — the hypothesis is back to `testing`, the
        // post-rollback hypothesis folds in
        let board = HypothesesProjection::fold(&events).unwrap();
        assert_eq!(board.len(), 2);
        let rolled_back = board.iter().find(|h| h.id == hyp.id).unwrap();
        assert_eq!(rolled_back.status, HypothesisStatus::Testing, "the supported transition was orphaned");
        assert!(board.iter().any(|h| h.id == post.id));
        // the mission created in the orphaned zone is gone from the fold
        let missions = MissionsProjection::fold(&events).unwrap();
        assert_eq!(missions.len(), 1, "the orphaned mission.created is excluded");
        assert_eq!(missions[0].id, mission.id);
        assert_eq!(missions[0].status, MissionStatus::Active);
    }

    #[test]
    fn a_second_rollback_extends_the_orphaned_set() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let a = seed_mission(&store); // seq 1
        let cp1 = store.append(NewEvent::checkpoint_created("one", 1).unwrap()).unwrap(); // seq 2
        store
            .append(NewEvent::hypothesis_created("H1.", a.id).unwrap())
            .unwrap(); // seq 3 — will be orphaned by rollback 1
        store
            .append(NewEvent::checkpoint_rolled_back(cp1.id, "one", 1, 1).unwrap())
            .unwrap(); // rollback 1 at seq 4
        // post-rollback work (seq 5-6)
        let cp2 = store.append(NewEvent::checkpoint_created("two", 5).unwrap()).unwrap(); // seq 5
        store
            .append(NewEvent::hypothesis_created("H2.", a.id).unwrap())
            .unwrap(); // seq 6 — live until rollback 2
        // rollback 2 to cp2 (target seq 5): orphans (5, 7] = seqs 6, 7
        store
            .append(NewEvent::checkpoint_rolled_back(cp2.id, "two", 5, 1).unwrap())
            .unwrap(); // seq 7

        let events = store.events_all().unwrap();
        let cursor = FoldCursor::over(&events);
        let live: Vec<i64> = cursor.live(&events).map(|e| e.seq).collect();
        // rollback 1 (seq 4) orphaned (1, 4]: H1 (seq 3) and cp1 (seq 2 —
        // bookkeeping, exempt). Rollback 2 (seq 7) orphaned (5, 7]: H2
        // (seq 6). Domain events live: the mission alone; checkpoint
        // bookkeeping (2, 4, 5, 7) stays.
        assert_eq!(live, vec![1, 2, 4, 5, 7]);
        // the board: both hypotheses orphaned
        let board = HypothesesProjection::fold(&events).unwrap();
        assert!(board.is_empty());
    }

    #[test]
    fn rollback_lists_orphaned_proposals_by_name_and_refuses_orphaned_checkpoints() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let mission = seed_mission(&store);
        let hyp = store
            .append(NewEvent::hypothesis_created("Sparse attention holds at 32k.", mission.id).unwrap())
            .unwrap();
        let checkpoint = store.append(NewEvent::checkpoint_created("pre-merge", 2).unwrap()).unwrap();
        // an orphaned-to-be proposal
        crate::domain::proposals::propose_transition(
            &store,
            "ns-1",
            hyp.id,
            HypothesisStatus::Testing,
            "night scan suggests testing",
        )
        .unwrap();
        let events = store.events_all().unwrap();

        // the plan names the orphaned proposal by its target's statement
        let plan = rollback_plan(&events, checkpoint.id).unwrap();
        assert_eq!(plan.orphaned_proposals.len(), 1);
        assert_eq!(
            plan.orphaned_proposals[0].target_label.as_deref(),
            Some("Sparse attention holds at 32k.")
        );
        assert_eq!(plan.orphaned_proposals[0].proposed_to.as_deref(), Some("testing"));
        assert_eq!(plan.orphaned_events.len(), 1);
        assert_eq!(plan.orphaned_events[0].kind, PROPOSAL_CREATED);
        assert_eq!(plan.orphaned_events[0].actor, "agent");

        // execute: the outcome carries the same lists + the record
        let outcome = rollback(&store, checkpoint.id).unwrap();
        assert_eq!(outcome.rollback.orphaned_count, 1);
        assert_eq!(outcome.rollback.target_seq, 2);
        assert_eq!(outcome.orphaned_proposals.len(), 1);

        // the executed rollback folds into the view's history
        let view = fold_checkpoints(&store.events_all().unwrap()).unwrap();
        assert_eq!(view.rollbacks.len(), 1);
        assert_eq!(view.rollbacks[0].name, "pre-merge");

        // the orphaned proposal reads as SUPERSEDED history in the
        // quarantine fold — never hidden, never mergeable
        let proposals = ProposalsProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].status, ProposalStatus::Superseded);
        assert!(proposals[0].orphaned_by_rollback);
        assert!(proposals[0].rolled_back_seq.is_some());

        // a second rollback to the now-orphaned "later" checkpoint is refused
        let conn2 = mem_conn();
        let store2 = EventStore::new(&conn2);
        let m = seed_mission(&store2);
        let cp = store2.append(NewEvent::checkpoint_created("target", 1).unwrap()).unwrap();
        let inside = store2.append(NewEvent::checkpoint_created("inside", 2).unwrap()).unwrap();
        store2
            .append(NewEvent::checkpoint_rolled_back(cp.id, "target", 1, 1).unwrap())
            .unwrap();
        let err = rollback(&store2, inside.id).unwrap_err();
        assert!(err.to_string().contains("orphaned_checkpoint"), "unexpected: {err}");
        let _ = m;
    }

    #[test]
    fn rollback_with_no_orphaned_events_is_an_honest_no_op_record() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        seed_mission(&store);
        // a checkpoint at the CURRENT head — nothing after it to orphan
        let head = store.head_seq().unwrap();
        let checkpoint = store.append(NewEvent::checkpoint_created("now", head).unwrap()).unwrap();
        let outcome = rollback(&store, checkpoint.id).unwrap();
        assert_eq!(outcome.rollback.orphaned_count, 0);
        assert!(outcome.orphaned_events.is_empty());
        assert!(outcome.orphaned_proposals.is_empty());
        // the read model is unchanged — the mission still folds
        let missions = MissionsProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(missions.len(), 1);
    }

    #[test]
    fn an_export_cut_before_a_rollback_is_stale_and_a_re_render_is_not() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        seed_mission(&store);
        let export_cut = store.head_seq().unwrap();
        // no rollback yet: the export is fresh
        assert!(!export_is_stale(&store.events_all().unwrap(), export_cut));
        let cp = store.append(NewEvent::checkpoint_created("cp", export_cut).unwrap()).unwrap();
        store
            .append(NewEvent::hypothesis_created("X.", Uuid::new_v4()).unwrap())
            .unwrap();
        // still fresh: newer events never stale a named cut (AD-11)
        assert!(!export_is_stale(&store.events_all().unwrap(), export_cut));
        rollback(&store, cp.id).unwrap();
        // the rollback predates nothing — the export's cut now renders a
        // read model that no longer folds: STALE
        assert!(export_is_stale(&store.events_all().unwrap(), export_cut));
        // a re-render at the new head is fresh again
        let fresh_cut = store.head_seq().unwrap();
        assert!(!export_is_stale(&store.events_all().unwrap(), fresh_cut));
    }

    #[test]
    fn fold_checkpoints_fails_loudly_on_corrupt_payloads() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        store
            .append(
                NewEvent::new(CHECKPOINT_CREATED, Actor::User, json!({ "name": "broken" }))
                    .unwrap(),
            )
            .unwrap();
        let err = fold_checkpoints(&store.events_all().unwrap());
        assert!(err.is_err(), "a payload missing its seq must fail the fold");
    }
}
