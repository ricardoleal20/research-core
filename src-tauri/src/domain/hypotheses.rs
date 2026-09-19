// Hypotheses domain (FR-2, Story 1.5): hypotheses are first-class board
// objects with a machine-enforced lifecycle. Every status change is a
// `hypothesis.status_changed` event carrying actor, timestamp, and a
// cause basis (audit stamp, FR-2.2); the legality of a transition is
// enforced at the typed-constructor level — an illegal transition can
// never even be constructed, so it can never be appended. Typed relations
// between two hypotheses are `hypothesis.related` events, cause-linked to
// both endpoint creation events (AD-2); they render as chips, never as a
// graph canvas (FR-2.3).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::domain::proposals::{
    MergeApprovedPayload, ProposalCreatedPayload, MERGE_APPROVED, PROPOSAL_CREATED,
};
use crate::eventstore::{Actor, EventError, NewEvent, StoredEvent};

pub const HYPOTHESIS_CREATED: &str = "hypothesis.created";
pub const HYPOTHESIS_STATUS_CHANGED: &str = "hypothesis.status_changed";
pub const HYPOTHESIS_RELATED: &str = "hypothesis.related";

/// A hypothesis's lifecycle status (FR-2.2). `Proposed` is the only initial
/// state — stamped by the constructor, never chosen by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisStatus {
    Proposed,
    Testing,
    Supported,
    Refuted,
    Revised,
}

impl HypothesisStatus {
    /// Parse the wire form used by shell commands
    /// (`proposed | testing | supported | refuted | revised`).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "proposed" => Some(Self::Proposed),
            "testing" => Some(Self::Testing),
            "supported" => Some(Self::Supported),
            "refuted" => Some(Self::Refuted),
            "revised" => Some(Self::Revised),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Testing => "testing",
            Self::Supported => "supported",
            Self::Refuted => "refuted",
            Self::Revised => "revised",
        }
    }

    /// The statuses this status may legally move to (FR-2.2). Drives the
    /// read model; illegal transitions never even render as buttons.
    pub fn next(self) -> &'static [Self] {
        use HypothesisStatus::*;
        match self {
            Self::Proposed => &[Testing],
            Self::Testing => &[Supported, Refuted],
            Self::Supported => &[Revised],
            Self::Refuted => &[Revised],
            Self::Revised => &[Testing],
        }
    }
}

impl std::fmt::Display for HypothesisStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// FR-2.2 transition table, the single source of truth:
/// proposed→testing, testing→supported, testing→refuted,
/// supported→revised, refuted→revised, revised→testing.
/// Everything else — e.g. proposed→supported, supported→refuted,
/// refuted→supported — is illegal and rejected at construction.
pub fn transition_allowed(from: HypothesisStatus, to: HypothesisStatus) -> bool {
    from.next().contains(&to)
}

/// The `hypothesis.created` payload — the statement is non-empty and
/// validated at the domain edge; `mission_id` links the hypothesis to its
/// mission (also set as the event's cause); `status` is always `proposed`,
/// stamped by the constructor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HypothesisCreatedPayload {
    pub statement: String,
    pub mission_id: Uuid,
    pub status: HypothesisStatus,
}

/// The `hypothesis.status_changed` payload — the audit stamp's basis
/// (FR-2.2): from, to, and a non-empty human-given basis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HypothesisStatusChangedPayload {
    pub from: HypothesisStatus,
    pub to: HypothesisStatus,
    pub basis: String,
}

/// The typed-relation vocabulary (FR-2.3): contradicts | extends |
/// specializes | supports_the_same_claim — the PRD chip vocabulary
/// (contradicted-by / extended-by / supports).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Contradicts,
    Extends,
    Specializes,
    SupportsTheSameClaim,
}

impl RelationKind {
    /// Parse the wire form used by shell commands.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "contradicts" => Some(Self::Contradicts),
            "extends" => Some(Self::Extends),
            "specializes" => Some(Self::Specializes),
            "supports_the_same_claim" => Some(Self::SupportsTheSameClaim),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contradicts => "contradicts",
            Self::Extends => "extends",
            Self::Specializes => "specializes",
            Self::SupportsTheSameClaim => "supports_the_same_claim",
        }
    }
}

/// The `hypothesis.related` payload — a typed relation between two
/// hypotheses. The event is cause-linked to both endpoint creation events
/// by the constructor (AD-2): `causes: [from_hypothesis_id,
/// to_hypothesis_id]` — the ids *are* the creation event ids.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HypothesisRelatedPayload {
    pub from_hypothesis_id: Uuid,
    pub to_hypothesis_id: Uuid,
    pub relation_kind: RelationKind,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `hypothesis.created` event
    /// comes into being. Actor is always the user in this story — future
    /// agent proposals arrive through their own path (AD-3), never here.
    /// The event is cause-linked to the mission's creation event.
    pub fn hypothesis_created(
        statement: impl Into<String>,
        mission_id: Uuid,
    ) -> Result<Self, EventError> {
        let statement = statement.into();
        if statement.trim().is_empty() {
            return Err(EventError::Invalid(
                "hypothesis.statement must not be empty — a hypothesis says something falsifiable"
                    .into(),
            ));
        }
        let payload = HypothesisCreatedPayload {
            statement,
            mission_id,
            status: HypothesisStatus::Proposed,
        };
        Ok(Self::new(
            HYPOTHESIS_CREATED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?
        .with_causes(vec![mission_id]))
    }

    /// Typed constructor (AD-15, FR-2.2): the one way a
    /// `hypothesis.status_changed` event comes into being. Rejects illegal
    /// transitions (the FR-2.2 table) and empty bases at the domain edge —
    /// an illegal event can never be constructed, so it can never be
    /// appended. The shell cause-links the event to the hypothesis.
    pub fn hypothesis_status_changed(
        from: HypothesisStatus,
        to: HypothesisStatus,
        basis: impl AsRef<str>,
    ) -> Result<Self, EventError> {
        if !transition_allowed(from, to) {
            return Err(EventError::Invalid(format!(
                "illegal_transition: {from} → {to} is not a legal hypothesis lifecycle \
                 transition (FR-2.2) — legal transitions: proposed→testing, testing→supported, \
                 testing→refuted, supported→revised, refuted→revised, revised→testing"
            )));
        }
        let basis = basis.as_ref().trim();
        if basis.is_empty() {
            return Err(EventError::Invalid(
                "hypothesis.basis must not be empty — every transition names its basis (FR-2.2)"
                    .into(),
            ));
        }
        let payload = HypothesisStatusChangedPayload {
            from,
            to,
            basis: basis.to_string(),
        };
        Self::new(
            HYPOTHESIS_STATUS_CHANGED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )
    }

    /// Typed constructor (AD-15, FR-2.3): the one way a typed relation
    /// comes into being — an event, cause-linked to both endpoint creation
    /// events. Self-relations are rejected at the domain edge.
    pub fn hypothesis_related(payload: HypothesisRelatedPayload) -> Result<Self, EventError> {
        if payload.from_hypothesis_id == payload.to_hypothesis_id {
            return Err(EventError::Invalid(
                "hypothesis.related: a hypothesis cannot relate to itself".into(),
            ));
        }
        let causes = vec![payload.from_hypothesis_id, payload.to_hypothesis_id];
        Ok(Self::new(
            HYPOTHESIS_RELATED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?
        .with_causes(causes))
    }
}

/// One chip on a hypothesis card (FR-2.3): a typed relation, from this
/// card's perspective. `outgoing` reads "⟶ contradicts H-7"; `incoming`
/// reads "⟵ contradicted-by H-3". Chips only — never a graph canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationDirection {
    Outgoing,
    Incoming,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationChip {
    /// The `seq` of the relation event (latest wins per endpoint pair).
    pub seq: i64,
    pub kind: RelationKind,
    pub direction: RelationDirection,
    /// The other endpoint's id (its creation event id).
    pub other_id: Uuid,
    /// The other endpoint's creation `seq` — the `H-n` chip label.
    pub other_seq: i64,
    pub other_statement: String,
}

/// The audit stamp of the last lifecycle event (FR-2.2): actor, ts, and
/// basis — never silent, never anonymous.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditStamp {
    pub seq: i64,
    pub ts: DateTime<Utc>,
    /// Short actor label: `user` | `agent` | `system:<component>`.
    pub actor: String,
    /// For a transition, the human-given basis; on creation, the event kind.
    pub basis: String,
}

/// A hypothesis as read from the log — the read model the board renders
/// (AD-8). Field names are camelCase on the wire (Tauri 2 convention).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hypothesis {
    /// The `hypothesis.created` event's id — the hypothesis's identity.
    pub id: Uuid,
    /// The `seq` of the creation event — the `H-n` label derives from it.
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub statement: String,
    pub mission_id: Uuid,
    /// Derived: current lifecycle status from `hypothesis.status_changed`
    /// events referencing this hypothesis — last transition in `seq` order.
    pub status: HypothesisStatus,
    /// Derived: typed relations, folded onto both endpoint hypotheses.
    pub relations: Vec<RelationChip>,
    /// Derived: the audit stamp of the last lifecycle event.
    pub audit: AuditStamp,
}

fn actor_label(actor: &Actor) -> String {
    match actor {
        Actor::User => "user".into(),
        Actor::Agent { .. } => "agent".into(),
        Actor::System { component } => {
            format!("system:{}", format!("{component:?}").to_lowercase())
        }
    }
}

/// Pure fold of the log into hypothesis read models (AD-1: current state
/// is exclusively a projection over the log). Events fold in `seq` order;
/// corrupt payloads fail loudly. Transition events reference their
/// hypothesis via `causes`; a `from` that mismatches the folded status is
/// corrupt and fails the fold. Relations fold onto both endpoint
/// hypotheses; the latest relation event per endpoint pair wins (relations
/// are edited by appending).
///
/// Quarantine (AD-3, AD-13): `proposal.created` events record intent only
/// — the proposed change is EXCLUDED from this fold (and every projection)
/// until a `merge.approved` lands. The fold applies each merged proposal's
/// intended transition when it meets the approval event, so approval seq
/// order IS the application order (approval-order-by-seq, AD-13). A merged
/// transition skips the `from` corruption check: its basis was validated at
/// approve time, and a forced merge past a stale basis is the recorded
/// human override — the fold applies the intended `to` deterministically.
pub struct HypothesesProjection;

impl HypothesesProjection {
    pub fn fold(events: &[StoredEvent]) -> Result<Vec<Hypothesis>, EventError> {
        let mut hyps: Vec<Hypothesis> = Vec::new();
        let mut index: HashMap<Uuid, usize> = HashMap::new();
        // Quarantine index (AD-3): hypothesis-targeting proposals by event
        // id, so a merge.approved can resolve and apply the intended change.
        let mut proposals: HashMap<Uuid, (ProposalCreatedPayload, String)> = HashMap::new();
        for event in events {
            match event.kind.as_str() {
                HYPOTHESIS_CREATED => {
                    let payload: HypothesisCreatedPayload = serde_json::from_value(
                        event.payload.clone(),
                    )
                    .map_err(|e| {
                        EventError::Invalid(format!(
                            "corrupt {HYPOTHESIS_CREATED} payload at seq {}: {e}",
                            event.seq
                        ))
                    })?;
                    // Machine-enforced initial state (FR-2.2): only the
                    // constructor writes this event, and it always stamps
                    // `proposed` — anything else in the log is corrupt.
                    if payload.status != HypothesisStatus::Proposed {
                        return Err(EventError::Invalid(format!(
                            "corrupt {HYPOTHESIS_CREATED} payload at seq {}: initial status must \
                             be `proposed`, found `{}`",
                            event.seq, payload.status
                        )));
                    }
                    index.insert(event.id, hyps.len());
                    hyps.push(Hypothesis {
                        id: event.id,
                        seq: event.seq,
                        ts: event.ts,
                        statement: payload.statement,
                        mission_id: payload.mission_id,
                        status: HypothesisStatus::Proposed,
                        relations: Vec::new(),
                        audit: AuditStamp {
                            seq: event.seq,
                            ts: event.ts,
                            actor: actor_label(&event.actor),
                            basis: HYPOTHESIS_CREATED.into(),
                        },
                    });
                }
                HYPOTHESIS_STATUS_CHANGED => {
                    let Some(&i) = event.causes.iter().find_map(|c| index.get(c)) else {
                        continue; // references no known hypothesis — skipped
                    };
                    let payload: HypothesisStatusChangedPayload = serde_json::from_value(
                        event.payload.clone(),
                    )
                    .map_err(|e| {
                        EventError::Invalid(format!(
                            "corrupt {HYPOTHESIS_STATUS_CHANGED} payload at seq {}: {e}",
                            event.seq
                        ))
                    })?;
                    let hyp = &mut hyps[i];
                    if payload.from != hyp.status {
                        return Err(EventError::Invalid(format!(
                            "corrupt {HYPOTHESIS_STATUS_CHANGED} event at seq {}: claims `from` \
                             {} but the hypothesis is {} — the log contradicts itself",
                            event.seq, payload.from, hyp.status
                        )));
                    }
                    hyp.status = payload.to;
                    hyp.audit = AuditStamp {
                        seq: event.seq,
                        ts: event.ts,
                        actor: actor_label(&event.actor),
                        basis: payload.basis,
                    };
                }
                HYPOTHESIS_RELATED => {
                    let payload: HypothesisRelatedPayload = serde_json::from_value(
                        event.payload.clone(),
                    )
                    .map_err(|e| {
                        EventError::Invalid(format!(
                            "corrupt {HYPOTHESIS_RELATED} payload at seq {}: {e}",
                            event.seq
                        ))
                    })?;
                    let (Some(fi), Some(ti)) = (
                        index.get(&payload.from_hypothesis_id).copied(),
                        index.get(&payload.to_hypothesis_id).copied(),
                    ) else {
                        continue; // an endpoint is unknown — skipped, not fatal
                    };
                    let (from_seq, from_statement) =
                        (hyps[fi].seq, hyps[fi].statement.clone());
                    let (to_seq, to_statement) = (hyps[ti].seq, hyps[ti].statement.clone());
                    // Fold onto both endpoint hypotheses (FR-2.3): outgoing
                    // chip on `from`, incoming chip on `to`; the latest event
                    // per (direction, other) pair wins — editing appends.
                    upsert_relation(
                        &mut hyps[fi].relations,
                        event.seq,
                        payload.relation_kind,
                        RelationDirection::Outgoing,
                        payload.to_hypothesis_id,
                        to_seq,
                        to_statement,
                    );
                    upsert_relation(
                        &mut hyps[ti].relations,
                        event.seq,
                        payload.relation_kind,
                        RelationDirection::Incoming,
                        payload.from_hypothesis_id,
                        from_seq,
                        from_statement,
                    );
                }
                PROPOSAL_CREATED => {
                    // Quarantine (AD-3): a proposal records intent only —
                    // the proposed change stays EXCLUDED from the fold until
                    // a merge.approved lands. Indexed here so the merge
                    // application below can resolve it.
                    let payload: ProposalCreatedPayload = serde_json::from_value(
                        event.payload.clone(),
                    )
                    .map_err(|e| {
                        EventError::Invalid(format!(
                            "corrupt {PROPOSAL_CREATED} payload at seq {}: {e}",
                            event.seq
                        ))
                    })?;
                    if payload.proposed_kind == HYPOTHESIS_STATUS_CHANGED {
                        proposals.insert(event.id, (payload, actor_label(&event.actor)));
                    }
                }
                MERGE_APPROVED => {
                    // The application point (AD-13): the approval applies the
                    // proposal's intended transition to its target — in this
                    // event's seq order, i.e. by approval order, never by
                    // proposal order. The audit stamp carries the merge seq,
                    // the proposing agent's actor label, and the agent's basis.
                    let payload: MergeApprovedPayload = serde_json::from_value(
                        event.payload.clone(),
                    )
                    .map_err(|e| {
                        EventError::Invalid(format!(
                            "corrupt {MERGE_APPROVED} payload at seq {}: {e}",
                            event.seq
                        ))
                    })?;
                    let Some((proposal, actor)) = proposals.get(&payload.proposal_id) else {
                        continue; // references no known proposal — skipped
                    };
                    let Some(&i) = index.get(&proposal.target_entity) else {
                        continue; // the target never appeared — skipped
                    };
                    let intended: HypothesisStatusChangedPayload =
                        serde_json::from_value(proposal.proposed_payload.clone()).map_err(
                            |e| {
                                EventError::Invalid(format!(
                                    "corrupt proposed payload of proposal {} applied at seq \
                                     {}: {e}",
                                    payload.proposal_id, event.seq
                                ))
                            },
                        )?;
                    let hyp = &mut hyps[i];
                    hyp.status = intended.to;
                    hyp.audit = AuditStamp {
                        seq: event.seq,
                        ts: event.ts,
                        actor: actor.clone(),
                        basis: intended.basis,
                    };
                }
                _ => {}
            }
        }
        Ok(hyps)
    }

    /// The hypotheses of one mission, in creation `seq` order.
    pub fn fold_for(
        events: &[StoredEvent],
        mission_id: Uuid,
    ) -> Result<Vec<Hypothesis>, EventError> {
        Ok(Self::fold(events)?
            .into_iter()
            .filter(|h| h.mission_id == mission_id)
            .collect())
    }
}

fn upsert_relation(
    relations: &mut Vec<RelationChip>,
    seq: i64,
    kind: RelationKind,
    direction: RelationDirection,
    other_id: Uuid,
    other_seq: i64,
    other_statement: String,
) {
    if let Some(chip) = relations
        .iter_mut()
        .find(|c| c.direction == direction && c.other_id == other_id)
    {
        chip.seq = seq;
        chip.kind = kind;
    } else {
        relations.push(RelationChip {
            seq,
            kind,
            direction,
            other_id,
            other_seq,
            other_statement,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::missions::{Autonomy, MissionCreatedPayload, MISSION_STOPPED};
    use crate::eventstore::{EventStore, NewEvent as RawNewEvent};
    use rusqlite::Connection;
    use serde_json::json;
    use HypothesisStatus::{Proposed, Refuted, Revised, Supported, Testing};

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

    /// A store holding one mission; returns (conn-backed store in a new
    /// scope is impossible, so callers use `seed()` inline).
    fn seed_mission(store: &EventStore) -> uuid::Uuid {
        store.append(RawNewEvent::mission_created(mission()).unwrap()).unwrap().id
    }

    // ---------- constructors ----------

    #[test]
    fn created_constructor_rejects_a_blank_statement() {
        let err = NewEvent::hypothesis_created("   ", Uuid::new_v4())
            .expect_err("a blank statement must fail construction");
        assert!(err.to_string().contains("statement"), "unexpected: {err}");
    }

    #[test]
    fn created_constructor_stamps_proposed_user_actor_and_mission_cause() {
        let mission_id = Uuid::new_v4();
        let ev = NewEvent::hypothesis_created("Adaptive timesteps drift on stiff systems.", mission_id)
            .unwrap();
        assert_eq!(ev.kind, "hypothesis.created");
        assert_eq!(ev.actor, Actor::User);
        assert_eq!(ev.causes, vec![mission_id]);
        assert_eq!(
            ev.payload,
            json!({
                "statement": "Adaptive timesteps drift on stiff systems.",
                "mission_id": mission_id.to_string(),
                "status": "proposed",
            })
        );
    }

    /// The FR-2.2 transition table, parametrized over every (from, to)
    /// pair: exactly the six legal transitions pass, all others are
    /// rejected at the typed-constructor level — before any append.
    #[test]
    fn the_transition_table_is_enforced_at_construction() {
        let legal = [
            (Proposed, Testing),
            (Testing, Supported),
            (Testing, Refuted),
            (Supported, Revised),
            (Refuted, Revised),
            (Revised, Testing),
        ];
        for from in [Proposed, Testing, Supported, Refuted, Revised] {
            for to in [Proposed, Testing, Supported, Refuted, Revised] {
                let pair = (from, to);
                let should = legal.contains(&pair);
                assert_eq!(
                    transition_allowed(from, to),
                    should,
                    "transition_allowed({from} → {to}) should be {should}"
                );
                let result = NewEvent::hypothesis_status_changed(from, to, "run 42");
                if should {
                    let ev = result.expect("legal transition must construct");
                    assert_eq!(ev.kind, "hypothesis.status_changed");
                    assert_eq!(ev.actor, Actor::User);
                    assert_eq!(
                        ev.payload,
                        json!({ "from": from.as_str(), "to": to.as_str(), "basis": "run 42" })
                    );
                } else {
                    let err = result.expect_err("illegal transition must fail construction");
                    assert!(
                        err.to_string().contains("illegal_transition"),
                        "error should carry the illegal_transition code: {err}"
                    );
                    assert!(
                        err.to_string().contains(from.as_str())
                            && err.to_string().contains(to.as_str()),
                        "error should name both statuses: {err}"
                    );
                }
            }
        }
    }

    /// Explicit spot-checks of the named illegal transitions from the
    /// story: proposed→supported, supported→refuted, refuted→supported.
    #[test]
    fn the_named_illegal_transitions_are_rejected() {
        for (from, to) in [(Proposed, Supported), (Supported, Refuted), (Refuted, Supported)] {
            let err = NewEvent::hypothesis_status_changed(from, to, "basis")
                .expect_err("must be rejected");
            assert!(err.to_string().contains("illegal_transition"), "unexpected: {err}");
        }
    }

    #[test]
    fn status_changed_requires_a_non_empty_basis() {
        for basis in ["", "   "] {
            let err = NewEvent::hypothesis_status_changed(Proposed, Testing, basis)
                .expect_err("a transition without a basis must fail construction");
            assert!(err.to_string().contains("basis"), "unexpected: {err}");
        }
    }

    #[test]
    fn relation_constructor_cause_links_both_endpoints_and_validates() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let ev = NewEvent::hypothesis_related(HypothesisRelatedPayload {
            from_hypothesis_id: a,
            to_hypothesis_id: b,
            relation_kind: RelationKind::Contradicts,
        })
        .unwrap();
        assert_eq!(ev.kind, "hypothesis.related");
        assert_eq!(ev.actor, Actor::User);
        assert_eq!(ev.causes, vec![a, b]);
        assert_eq!(
            ev.payload,
            json!({
                "from_hypothesis_id": a.to_string(),
                "to_hypothesis_id": b.to_string(),
                "relation_kind": "contradicts",
            })
        );
        // self-relation is rejected at the domain edge
        let err = NewEvent::hypothesis_related(HypothesisRelatedPayload {
            from_hypothesis_id: a,
            to_hypothesis_id: a,
            relation_kind: RelationKind::Extends,
        })
        .expect_err("a hypothesis cannot relate to itself");
        assert!(err.to_string().contains("itself"), "unexpected: {err}");
    }

    #[test]
    fn relation_kind_parses_only_the_typed_vocabulary() {
        for k in [
            RelationKind::Contradicts,
            RelationKind::Extends,
            RelationKind::Specializes,
            RelationKind::SupportsTheSameClaim,
        ] {
            assert_eq!(RelationKind::parse(k.as_str()), Some(k));
            let back: RelationKind = serde_json::from_value(json!(k.as_str())).unwrap();
            assert_eq!(back, k);
        }
        for bad in ["", "contradicted-by", "support", "EXTENDS"] {
            assert_eq!(RelationKind::parse(bad), None, "should reject {bad:?}");
        }
    }

    // ---------- fold ----------

    #[test]
    fn fold_reads_hypotheses_as_proposed_with_a_creation_stamp() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m1 = seed_mission(&store);
        let m2 = seed_mission(&store);
        let h1 = store
            .append(NewEvent::hypothesis_created("H one.", m1).unwrap())
            .unwrap();
        let h2 = store
            .append(NewEvent::hypothesis_created("H two.", m2).unwrap())
            .unwrap();

        let hyps = HypothesesProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(hyps.len(), 2);
        assert_eq!(hyps[0].id, h1.id);
        assert_eq!(hyps[0].status, Proposed);
        assert_eq!(hyps[0].statement, "H one.");
        assert_eq!(hyps[0].mission_id, m1);
        assert_eq!(hyps[0].relations, vec![]);
        assert_eq!(hyps[0].audit.seq, h1.seq);
        assert_eq!(hyps[0].audit.actor, "user");
        assert_eq!(hyps[0].audit.basis, "hypothesis.created");
        assert_eq!(hyps[1].id, h2.id);

        // mission scoping: fold_for returns only that mission's hypotheses
        let scoped = HypothesesProjection::fold_for(&store.events_all().unwrap(), m1).unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].id, h1.id);
        assert!(HypothesesProjection::fold_for(&store.events_all().unwrap(), Uuid::new_v4())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn fold_applies_legal_transitions_and_stamps_the_last() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = store
            .append(NewEvent::hypothesis_created("Adaptive timesteps drift.", m).unwrap())
            .unwrap();
        // proposed → testing → supported → revised → testing: the full legal loop
        let t1 = store
            .append(
                NewEvent::hypothesis_status_changed(Proposed, Testing, "night shift 1 begun")
                    .unwrap()
                    .with_causes(vec![h.id]),
            )
            .unwrap();
        let t2 = store
            .append(
                NewEvent::hypothesis_status_changed(Testing, Supported, "2 pins hold")
                    .unwrap()
                    .with_causes(vec![h.id]),
            )
            .unwrap();
        let _t3 = store
            .append(
                NewEvent::hypothesis_status_changed(Supported, Revised, "scope narrowed")
                    .unwrap()
                    .with_causes(vec![h.id]),
            )
            .unwrap();
        let t4 = store
            .append(
                NewEvent::hypothesis_status_changed(Revised, Testing, "retest on suite v2")
                    .unwrap()
                    .with_causes(vec![h.id]),
            )
            .unwrap();

        let [hyp] = HypothesesProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one hypothesis");
        assert_eq!(hyp.status, Testing); // last transition in seq order wins
        assert_eq!(hyp.audit.seq, t4.seq);
        assert_eq!(hyp.audit.ts, t4.ts);
        assert_eq!(hyp.audit.actor, "user");
        assert_eq!(hyp.audit.basis, "retest on suite v2");
        // intermediate stamps existed and were superseded (seq monotonic)
        assert!(t1.seq < t2.seq && t2.seq < t4.seq);
    }

    #[test]
    fn fold_fails_loudly_when_the_log_contradicts_itself() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = store
            .append(NewEvent::hypothesis_created("H.", m).unwrap())
            .unwrap();
        // a transition event whose `from` mismatches the folded status
        store
            .append(
                NewEvent::hypothesis_status_changed(Testing, Supported, "basis")
                    .unwrap()
                    .with_causes(vec![h.id]),
            )
            .unwrap();
        let err = HypothesesProjection::fold(&store.events_all().unwrap())
            .expect_err("a from-mismatch is corrupt and must fail the fold");
        assert!(err.to_string().contains("contradicts"), "unexpected: {err}");
    }

    #[test]
    fn fold_fails_loudly_on_corrupt_payloads() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        store
            .append(
                RawNewEvent::new(HYPOTHESIS_CREATED, Actor::User, json!({"statement": "orphan"}))
                    .unwrap(),
            )
            .unwrap();
        let err = HypothesesProjection::fold(&store.events_all().unwrap())
            .expect_err("a payload missing fields must fail the fold");
        assert!(err.to_string().contains("corrupt"), "unexpected: {err}");
    }

    #[test]
    fn fold_of_an_empty_log_is_empty() {
        let conn = mem_conn();
        assert!(HypothesesProjection::fold(&EventStore::new(&conn).events_all().unwrap())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn relations_fold_onto_both_endpoint_hypotheses() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let a = store
            .append(NewEvent::hypothesis_created("H A.", m).unwrap())
            .unwrap();
        let b = store
            .append(NewEvent::hypothesis_created("H B.", m).unwrap())
            .unwrap();
        let rel = store
            .append(NewEvent::hypothesis_related(HypothesisRelatedPayload {
                from_hypothesis_id: a.id,
                to_hypothesis_id: b.id,
                relation_kind: RelationKind::Contradicts,
            })
            .unwrap())
            .unwrap();

        let hyps = HypothesesProjection::fold(&store.events_all().unwrap()).unwrap();
        let ha = hyps.iter().find(|h| h.id == a.id).unwrap();
        let hb = hyps.iter().find(|h| h.id == b.id).unwrap();
        // outgoing chip on A: "⟶ contradicts H-B"
        assert_eq!(
            ha.relations,
            vec![RelationChip {
                seq: rel.seq,
                kind: RelationKind::Contradicts,
                direction: RelationDirection::Outgoing,
                other_id: b.id,
                other_seq: b.seq,
                other_statement: "H B.".into(),
            }]
        );
        // incoming chip on B: "⟵ contradicted-by H-A"
        assert_eq!(
            hb.relations,
            vec![RelationChip {
                seq: rel.seq,
                kind: RelationKind::Contradicts,
                direction: RelationDirection::Incoming,
                other_id: a.id,
                other_seq: a.seq,
                other_statement: "H A.".into(),
            }]
        );
    }

    /// Relations are edited as events: appending a new relation event for
    /// the same endpoint pair replaces the chip (latest wins).
    #[test]
    fn editing_a_relation_appends_and_the_latest_event_wins() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let a = store
            .append(NewEvent::hypothesis_created("H A.", m).unwrap())
            .unwrap();
        let b = store
            .append(NewEvent::hypothesis_created("H B.", m).unwrap())
            .unwrap();
        store
            .append(NewEvent::hypothesis_related(HypothesisRelatedPayload {
                from_hypothesis_id: a.id,
                to_hypothesis_id: b.id,
                relation_kind: RelationKind::Contradicts,
            })
            .unwrap())
            .unwrap();
        let edited = store
            .append(NewEvent::hypothesis_related(HypothesisRelatedPayload {
                from_hypothesis_id: a.id,
                to_hypothesis_id: b.id,
                relation_kind: RelationKind::Extends,
            })
            .unwrap())
            .unwrap();

        let hyps = HypothesesProjection::fold(&store.events_all().unwrap()).unwrap();
        let ha = hyps.iter().find(|h| h.id == a.id).unwrap();
        assert_eq!(ha.relations.len(), 1, "one chip per endpoint pair, not two");
        assert_eq!(ha.relations[0].kind, RelationKind::Extends);
        assert_eq!(ha.relations[0].seq, edited.seq);
    }

    #[test]
    fn relations_to_unknown_endpoints_are_skipped_not_fatal() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let a = store
            .append(NewEvent::hypothesis_created("H A.", m).unwrap())
            .unwrap();
        // a relation whose `to` endpoint never appeared in the log
        store
            .append(NewEvent::hypothesis_related(HypothesisRelatedPayload {
                from_hypothesis_id: a.id,
                to_hypothesis_id: Uuid::new_v4(),
                relation_kind: RelationKind::Extends,
            })
            .unwrap())
            .unwrap();
        let hyps = HypothesesProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(hyps.len(), 1);
        assert_eq!(hyps[0].relations, vec![], "unknown endpoint → no chip, no error");
    }

    /// Non-hypothesis events in the log never disturb the fold — and
    /// hypothesis events reference missions without breaking their fold.
    #[test]
    fn the_two_folds_coexist_over_one_log() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = store
            .append(NewEvent::hypothesis_created("H.", m).unwrap())
            .unwrap();
        store
            .append(
                NewEvent::hypothesis_status_changed(Proposed, Testing, "first test")
                    .unwrap()
                    .with_causes(vec![h.id]),
            )
            .unwrap();
        // an unrelated mission transition
        store
            .append(
                RawNewEvent::new(MISSION_STOPPED, Actor::User, json!({"reason": "done"}))
                    .unwrap()
                    .with_causes(vec![m]),
            )
            .unwrap();

        let missions = crate::domain::missions::MissionsProjection::fold(
            &store.events_all().unwrap(),
        )
        .unwrap();
        assert_eq!(missions.len(), 1);
        assert_eq!(missions[0].status, crate::domain::missions::MissionStatus::Stopped);
        // the hypothesis.created event appears in the mission's run list
        // (payload mission_id) — the mission drill-down sees the board grow.
        let runs = crate::domain::missions::MissionsProjection::runs_for(
            &store.events_all().unwrap(),
            m,
        );
        assert!(runs.iter().any(|r| r.kind == HYPOTHESIS_CREATED));

        let hyps = HypothesesProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(hyps.len(), 1);
        assert_eq!(hyps[0].status, Testing);
        assert_eq!(hyps[0].audit.basis, "first test");
    }

    /// An illegal transition cannot be constructed, hence cannot be
    /// appended: after a rejected attempt the log — and the folded state —
    /// are unchanged. This is the dry-run guarantee of FR-2.2.
    #[test]
    fn a_rejected_illegal_transition_leaves_the_log_unchanged() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let m = seed_mission(&store);
        let h = store
            .append(NewEvent::hypothesis_created("H.", m).unwrap())
            .unwrap();
        let head = store.head_seq().unwrap();
        let _ = h;

        let err = NewEvent::hypothesis_status_changed(Proposed, Supported, "try to skip testing");
        assert!(err.is_err());

        assert_eq!(store.head_seq().unwrap(), head, "nothing was appended");
        let [hyp] = HypothesesProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one hypothesis");
        assert_eq!(hyp.status, Proposed, "state unchanged");
        assert_eq!(hyp.audit.basis, HYPOTHESIS_CREATED, "stamp unchanged");
    }

    /// A status_changed event referencing no known hypothesis is skipped —
    /// the missions fold's treatment of unknown references, mirrored.
    #[test]
    fn a_status_event_referencing_no_hypothesis_is_skipped() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        store
            .append(
                NewEvent::hypothesis_status_changed(Proposed, Testing, "orphan basis")
                    .unwrap()
                    .with_causes(vec![Uuid::new_v4()]),
            )
            .unwrap();
        assert!(HypothesesProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .is_empty());
    }
}
