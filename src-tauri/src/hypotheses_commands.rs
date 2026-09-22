// Hypothesis shell commands (AD-15a): the typed core APIs the board calls.
// They construct events via the domain's typed constructors and read state
// via the projection — never raw JSON appends, never direct writes. The
// FR-2.2 lifecycle is enforced at the domain edge: `transition_hypothesis`
// computes the current status from the fold and hands it to the
// constructor, so an illegal transition is refused with a typed error
// code (`illegal_transition: …`) before anything is appended. Error
// strings lead with a stable code and stay in code form — bilingual-safe
// by construction (codes are never translated, EXPERIENCE.md).

use crate::db::Db;
use crate::domain::hypotheses::{
    Hypothesis, HypothesesProjection, HypothesisRelatedPayload, HYPOTHESIS_CREATED, RelationKind,
};
use crate::domain::hypotheses::HypothesisStatus;
use crate::domain::missions::MISSION_CREATED;
use crate::eventstore::{EventStore, NewEvent, StoredEvent};
use tauri::State;
use uuid::Uuid;

fn err(e: impl ToString) -> String {
    e.to_string()
}

fn parse_id(raw: &str, what: &str) -> Result<Uuid, String> {
    raw.parse()
        .map_err(|e| format!("invalid {what} id `{raw}`: {e}"))
}

fn find<'a>(events: &'a [StoredEvent], hypothesis_id: Uuid) -> Result<&'a StoredEvent, String> {
    events
        .iter()
        .find(|e| e.id == hypothesis_id && e.kind == HYPOTHESIS_CREATED)
        .ok_or_else(|| format!("not_found: no hypothesis with id `{hypothesis_id}`"))
}

/// Create a hypothesis on a mission: append one `hypothesis.created` event
/// (actor=user, cause-linked to the mission) and return the folded read
/// model. Fails loudly when the statement is empty or the mission is
/// unknown.
#[tauri::command]
pub async fn create_hypothesis(
    db: State<'_, Db>,
    statement: String,
    mission_id: String,
) -> Result<Hypothesis, String> {
    let mission_id = parse_id(&mission_id, "mission")?;
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    let events = store.events_all().map_err(err)?;
    // The mission must exist: its creation event is in the log.
    if !events
        .iter()
        .any(|e| e.id == mission_id && e.kind == MISSION_CREATED)
    {
        return Err(format!("not_found: no mission with id `{mission_id}`"));
    }
    let event = NewEvent::hypothesis_created(statement, mission_id).map_err(err)?;
    let stored = store.append(event).map_err(err)?;
    // Return exactly what the log now holds — the read model, not the input.
    let hyps = HypothesesProjection::fold(&[stored]).map_err(err)?;
    Ok(hyps.into_iter().next().expect("fold of one creation event yields one hypothesis"))
}

/// All hypotheses of one mission, folded from the log in `seq` order.
#[tauri::command]
pub async fn list_hypotheses(
    db: State<'_, Db>,
    mission_id: String,
) -> Result<Vec<Hypothesis>, String> {
    let mission_id = parse_id(&mission_id, "mission")?;
    let c = db.0.lock().await;
    let events = EventStore::new(&c).events_all().map_err(err)?;
    HypothesesProjection::fold_for(&events, mission_id).map_err(err)
}

/// Transition a hypothesis's lifecycle (FR-2.2): append one
/// `hypothesis.status_changed` event carrying the audit basis, cause-linked
/// to the hypothesis. The transition's legality is checked against the
/// FR-2.2 table at the typed constructor — illegal transitions are refused
/// with the `illegal_transition` code and nothing is appended.
#[tauri::command]
pub async fn transition_hypothesis(
    db: State<'_, Db>,
    hypothesis_id: String,
    to: String,
    basis: String,
) -> Result<Hypothesis, String> {
    let hypothesis_id = parse_id(&hypothesis_id, "hypothesis")?;
    let to = HypothesisStatus::parse(&to).ok_or_else(|| {
        format!("unknown_status: `{to}` — expected proposed | testing | supported | refuted | revised")
    })?;
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    let events = store.events_all().map_err(err)?;
    // The current status comes from the fold — the caller never asserts it.
    let current = HypothesesProjection::fold(&events)
        .map_err(err)?
        .into_iter()
        .find(|h| h.id == hypothesis_id)
        .ok_or_else(|| format!("not_found: no hypothesis with id `{hypothesis_id}`"))?;
    let event = NewEvent::hypothesis_status_changed(current.status, to, basis)
        .map_err(err)?
        .with_causes(vec![hypothesis_id]);
    store.append(event).map_err(err)?;
    // Return the re-folded read model — the log is the only truth.
    let events = store.events_all().map_err(err)?;
    let updated = HypothesesProjection::fold(&events).map_err(err)?;
    Ok(updated
        .into_iter()
        .find(|h| h.id == hypothesis_id)
        .expect("the hypothesis was just transitioned"))
}

/// Add a typed relation between two hypotheses (FR-2.3): append one
/// `hypothesis.related` event, cause-linked to both endpoint creation
/// events (AD-2). Relations are created and edited as events — appending
/// for an existing pair replaces the chip (latest wins in the fold).
#[tauri::command]
pub async fn add_relation(
    db: State<'_, Db>,
    from_hypothesis_id: String,
    to_hypothesis_id: String,
    relation_kind: String,
) -> Result<Hypothesis, String> {
    let from_hypothesis_id = parse_id(&from_hypothesis_id, "hypothesis")?;
    let to_hypothesis_id = parse_id(&to_hypothesis_id, "hypothesis")?;
    let relation_kind = RelationKind::parse(&relation_kind).ok_or_else(|| {
        format!(
            "unknown_relation: `{relation_kind}` — expected contradicts | extends | specializes | supports_the_same_claim"
        )
    })?;
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    let events = store.events_all().map_err(err)?;
    // Both endpoints must exist — a relation never references a ghost.
    find(&events, from_hypothesis_id)?;
    find(&events, to_hypothesis_id)?;
    let event = NewEvent::hypothesis_related(HypothesisRelatedPayload {
        from_hypothesis_id,
        to_hypothesis_id,
        relation_kind,
    })
    .map_err(err)?;
    store.append(event).map_err(err)?;
    let events = store.events_all().map_err(err)?;
    let updated = HypothesesProjection::fold(&events).map_err(err)?;
    Ok(updated
        .into_iter()
        .find(|h| h.id == from_hypothesis_id)
        .expect("the from-hypothesis was just related"))
}
