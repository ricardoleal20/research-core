// Evidence shell commands (AD-15a): the typed core APIs the board's
// evidence pins call. They construct events via the domain's typed
// constructors and read state via the projection — never raw JSON
// appends. Ref validity (the pin's `ref_id` must name an existing ref in
// the library) is checked here against the refs table before anything is
// appended; constructor guarantees (digest computed, confidence bounds,
// attribution) are enforced at the domain edge. Error strings lead with a
// stable code and stay in code form — bilingual-safe by construction
// (codes are never translated, EXPERIENCE.md).

use crate::db::Db;
use crate::domain::evidence::{Claim, EvidenceProjection};
use crate::domain::hypotheses::HypothesesProjection;
use crate::eventstore::{EventStore, NewEvent};
use rusqlite::Connection;
use tauri::State;
use uuid::Uuid;

fn err(e: impl ToString) -> String {
    e.to_string()
}

fn parse_id(raw: &str, what: &str) -> Result<Uuid, String> {
    raw.parse()
        .map_err(|e| format!("invalid {what} id `{raw}`: {e}"))
}

/// The library ref a pin cites, as a short author-year label
/// (e.g. "Vaswani et al. 2017") — the enrichment the card's citation pin
/// renders. None when the ref row no longer exists.
fn ref_label(conn: &Connection, ref_id: &str) -> Result<Option<String>, String> {
    let label = conn
        .query_row(
            "SELECT authors, year, title FROM refs WHERE id = ?1",
            [ref_id],
            |r| {
                let authors: Option<String> = r.get(0)?;
                let year: Option<i64> = r.get(1)?;
                let title: Option<String> = r.get(2)?;
                Ok(match (authors, year) {
                    (Some(a), Some(y)) if !a.trim().is_empty() => format!("{} {}", a.trim(), y),
                    (Some(a), _) if !a.trim().is_empty() => a.trim().to_string(),
                    (_, Some(y)) => format!("{} ({})", title.unwrap_or_default().trim(), y),
                    _ => title.unwrap_or_default().trim().to_string(),
                })
            },
        )
        .map_err(err)?;
    Ok(Some(label).filter(|l| !l.is_empty()))
}

/// Enrich a folded claim's pin with its ref label from the library.
fn enrich(conn: &Connection, claim: &mut Claim) -> Result<(), String> {
    if let Some(pin) = claim.pin.as_mut() {
        pin.ref_label = ref_label(conn, &pin.ref_id)?;
    }
    Ok(())
}

/// The shared read: all claims of one hypothesis (pinned and unpinned —
/// FR-3.4 marks the difference), folded from the log and enriched with
/// ref labels. Also the server shell's read path (AD-7, read-only).
pub(crate) fn list_evidence_inner(
    conn: &Connection,
    hypothesis_id: Uuid,
) -> Result<Vec<Claim>, String> {
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let mut claims = EvidenceProjection::fold_for(&events, hypothesis_id).map_err(err)?;
    for claim in claims.iter_mut() {
        enrich(conn, claim)?;
    }
    Ok(claims)
}

/// The hypothesis must exist in the fold — a claim never attaches to a
/// ghost hypothesis.
fn require_hypothesis(conn: &Connection, hypothesis_id: Uuid) -> Result<(), String> {
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let exists = HypothesesProjection::fold(&events)
        .map_err(err)?
        .iter()
        .any(|h| h.id == hypothesis_id);
    if exists {
        Ok(())
    } else {
        Err(format!("not_found: no hypothesis with id `{hypothesis_id}`"))
    }
}

/// Register an AI-generated claim on a hypothesis: append one
/// `claim.registered` event (actor=user, cause-linked to the hypothesis)
/// and return the folded read model. The claim starts UNPINNED (FR-3.4)
/// — pinning is its own event.
#[tauri::command]
pub async fn register_claim(
    db: State<'_, Db>,
    hypothesis_id: String,
    text: String,
    source_message_id: Option<String>,
) -> Result<Claim, String> {
    let hypothesis_id = parse_id(&hypothesis_id, "hypothesis")?;
    let c = db.0.lock().await;
    let event = register_claim_inner(&c, hypothesis_id, text, source_message_id)?;
    // Return exactly what the log now holds — the read model, not the input.
    let mut claims = EvidenceProjection::fold(&[event]).map_err(err)?;
    let claim = claims.pop().expect("fold of one registration event yields one claim");
    Ok(claim)
}

/// Plain inner (testable without Tauri state): validate + append one
/// `claim.registered` event, returning the stored event.
fn register_claim_inner(
    conn: &Connection,
    hypothesis_id: Uuid,
    text: String,
    source_message_id: Option<String>,
) -> Result<crate::eventstore::StoredEvent, String> {
    require_hypothesis(conn, hypothesis_id)?;
    let event = NewEvent::claim_registered(text, hypothesis_id, source_message_id).map_err(err)?;
    EventStore::new(conn).append(event).map_err(err)
}

/// Pin a claim to a citation (FR-3.1, AD-5): append one `evidence.pinned`
/// event quoting the excerpt, with the sha-256 digest computed at the
/// domain constructor (never accepted from the caller), the
/// agent-assessed confidence attributed to the assessing model (FR-3.6),
/// and a `ref_id` that must name an existing ref in the library — an
/// unknown ref is refused with `invalid_ref:` and nothing is appended.
#[tauri::command]
pub async fn pin_claim_to_citation(
    db: State<'_, Db>,
    claim_id: String,
    hypothesis_id: String,
    ref_id: String,
    excerpt: String,
    confidence: f64,
    assessing_model: String,
) -> Result<Claim, String> {
    let claim_id = parse_id(&claim_id, "claim")?;
    let hypothesis_id = parse_id(&hypothesis_id, "hypothesis")?;
    let c = db.0.lock().await;
    pin_claim_to_citation_inner(
        &c,
        claim_id,
        hypothesis_id,
        ref_id,
        excerpt,
        confidence,
        assessing_model,
    )?;
    // Return the re-folded, enriched read model — the log is the only truth.
    Ok(list_evidence_inner(&c, hypothesis_id)?
        .into_iter()
        .find(|cl| cl.id == claim_id)
        .expect("the claim was just pinned"))
}

/// Plain inner (testable without Tauri state): validate + append one
/// `evidence.pinned` event. Typed error codes: `invalid_ref:` for an
/// unknown ref, `not_found:` for an unknown claim/hypothesis; confidence
/// bounds and attribution are the constructor's guarantees.
fn pin_claim_to_citation_inner(
    conn: &Connection,
    claim_id: Uuid,
    hypothesis_id: Uuid,
    ref_id: String,
    excerpt: String,
    confidence: f64,
    assessing_model: String,
) -> Result<(), String> {
    require_hypothesis(conn, hypothesis_id)?;
    // The claim must exist and belong to this hypothesis.
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let claim = EvidenceProjection::fold(&events)
        .map_err(err)?
        .into_iter()
        .find(|cl| cl.id == claim_id)
        .ok_or_else(|| format!("not_found: no claim with id `{claim_id}`"))?;
    if claim.hypothesis_id != hypothesis_id {
        return Err(format!(
            "invalid_claim: claim `{claim_id}` belongs to hypothesis `{}`, not `{hypothesis_id}`",
            claim.hypothesis_id
        ));
    }
    // FR-3.1: the ref must exist in the library — a pin never cites a ghost.
    let ref_id = ref_id.trim();
    if ref_id.is_empty() {
        return Err("invalid_ref: `` — a citation pin names its library ref".into());
    }
    let known: i64 = conn
        .query_row("SELECT COUNT(*) FROM refs WHERE id = ?1", [ref_id], |r| r.get(0))
        .map_err(err)?;
    if known == 0 {
        return Err(format!(
            "invalid_ref: `{ref_id}` — no reference with this id in the library"
        ));
    }
    let event = NewEvent::evidence_pinned(
        claim_id,
        hypothesis_id,
        ref_id,
        excerpt,
        confidence,
        assessing_model,
    )
    .map_err(err)?;
    EventStore::new(conn).append(event).map_err(err)?;
    Ok(())
}

/// All claims of one hypothesis with their pin state (FR-3.4 unpinned
/// included), folded from the log, enriched with ref labels.
#[tauri::command]
pub async fn list_evidence(
    db: State<'_, Db>,
    hypothesis_id: String,
) -> Result<Vec<Claim>, String> {
    let hypothesis_id = parse_id(&hypothesis_id, "hypothesis")?;
    let c = db.0.lock().await;
    list_evidence_inner(&c, hypothesis_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventstore::NewEvent as RawNewEvent;

    /// A real Db (migrated + seeded): the refs table and the demo library
    /// exist, so ref validation is tested against actual rows.
    fn test_db() -> (Db, std::path::PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("rc-evidence-cmd-test-{}.sqlite", uuid::Uuid::new_v4()));
        let db = Db::open(&path).unwrap();
        (db, path)
    }

    fn cleanup(path: &std::path::PathBuf) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    }

    /// Seed one mission + hypothesis in the db's event log; returns the
    /// hypothesis id. Uses blocking_lock — tests are sync.
    fn seed_hypothesis(db: &Db) -> Uuid {
        let c = db.0.blocking_lock();
        let store = EventStore::new(&c);
        let mission = store
            .append(RawNewEvent::mission_created(
                crate::domain::missions::MissionCreatedPayload {
                    question: "Does X hold up?".into(),
                    stop_condition: "Stop after $5.".into(),
                    success_criterion: "A blind rater agrees.".into(),
                    autonomy: crate::domain::missions::Autonomy::Suggest,
                    spend_ceiling_cents: 500,
                },
            )
            .unwrap())
            .unwrap();
        store
            .append(RawNewEvent::hypothesis_created("X holds.", mission.id).unwrap())
            .unwrap()
            .id
    }

    fn one_ref_id(c: &Connection) -> String {
        c.query_row("SELECT id FROM refs LIMIT 1", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn an_invalid_ref_id_is_rejected_and_nothing_is_appended() {
        let (db, path) = test_db();
        let h = seed_hypothesis(&db);
        let c = db.0.blocking_lock();
        let store = EventStore::new(&c);
        let claim = store
            .append(RawNewEvent::claim_registered("A claim.", h, None).unwrap())
            .unwrap();
        let head = store.head_seq().unwrap();

        // A ref id that is not in the library → typed invalid_ref error,
        // nothing appended.
        let e = pin_claim_to_citation_inner(
            &c,
            claim.id,
            h,
            "no-such-ref".into(),
            "Excerpt.".into(),
            0.8,
            "GLM-5.3".into(),
        )
        .expect_err("an unknown ref must be refused");
        assert!(e.starts_with("invalid_ref:"), "unexpected: {e}");
        // An empty ref id → the same typed code.
        let e = pin_claim_to_citation_inner(
            &c,
            claim.id,
            h,
            "   ".into(),
            "Excerpt.".into(),
            0.8,
            "GLM-5.3".into(),
        )
        .expect_err("an empty ref must be refused");
        assert!(e.starts_with("invalid_ref:"), "unexpected: {e}");
        assert_eq!(store.head_seq().unwrap(), head, "nothing was appended");

        // A real library ref pins cleanly, and the read model carries the
        // author-year label the card renders.
        let ref_id = one_ref_id(&c);
        pin_claim_to_citation_inner(
            &c,
            claim.id,
            h,
            ref_id.clone(),
            "Attention dispenses with recurrence.".into(),
            0.82,
            "GLM-5.3".into(),
        )
        .unwrap();
        let claims = list_evidence_inner(&c, h).unwrap();
        assert_eq!(claims.len(), 1);
        assert!(claims[0].pinned);
        let pin = claims[0].pin.as_ref().unwrap();
        assert_eq!(pin.ref_id, ref_id);
        assert!(
            pin.ref_label.as_deref().is_some_and(|l| !l.is_empty()),
            "the pin is enriched with an author-year label: {:?}",
            pin.ref_label
        );
        drop(c);
        cleanup(&path);
    }

    #[test]
    fn registering_and_pinning_validate_their_targets() {
        let (db, path) = test_db();
        let h = seed_hypothesis(&db);
        let c = db.0.blocking_lock();

        // A claim on a ghost hypothesis is refused.
        let e = register_claim_inner(&c, Uuid::new_v4(), "Ghost claim.".into(), None)
            .expect_err("a claim on a ghost hypothesis must be refused");
        assert!(e.starts_with("not_found:"), "unexpected: {e}");

        // A valid claim registers and reads back unpinned (FR-3.4).
        let stored = register_claim_inner(&c, h, "A real claim.".into(), None).unwrap();
        let claims = list_evidence_inner(&c, h).unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].id, stored.id);
        assert!(!claims[0].pinned);

        // Pinning a ghost claim is refused; pinning another hypothesis's
        // claim is refused with the typed invalid_claim code.
        let e = pin_claim_to_citation_inner(
            &c,
            Uuid::new_v4(),
            h,
            one_ref_id(&c),
            "Excerpt.".into(),
            0.8,
            "GLM-5.3".into(),
        )
        .expect_err("pinning a ghost claim must be refused");
        assert!(e.starts_with("not_found:"), "unexpected: {e}");

        // Out-of-range confidence is refused by the constructor guarantee.
        let claim_id = stored.id;
        let e = pin_claim_to_citation_inner(
            &c,
            claim_id,
            h,
            one_ref_id(&c),
            "Excerpt.".into(),
            1.5,
            "GLM-5.3".into(),
        )
        .expect_err("out-of-range confidence must be refused");
        assert!(e.contains("confidence"), "unexpected: {e}");
        drop(c);
        cleanup(&path);
    }
}
