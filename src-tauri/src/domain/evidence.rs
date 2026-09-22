// Evidence pin domain (FR-3, Stories 1.7–1.8): an AI-generated claim is
// a first-class log object (`claim.registered`, cause-linked to its
// hypothesis) that can carry an evidence pin. A citation pin
// (`evidence.pinned`) references a library ref, quotes the specific
// excerpt it rests on, and carries a sha-256 digest of that excerpt
// computed at construction (AD-5) — callers never supply a digest, so a
// pin can never claim to anchor text it does not quote. A numerical pin
// (FR-3.3, Story 1.8) anchors the claim to a numerical artifact
// (file/figure/table) by `artifact_ref` + the sha-256 digest of the
// pinned content — the same AD-5 rule, the researcher's own results
// instead of a library passage. Confidence is agent-assessed and
// attributed: the pin stores the assessing model (FR-3.6) — a pin is
// never "verified", it is pinned with a stated confidence by a named
// model. Claims without a pin read as UNPINNED so the UI can flag them
// (FR-3.4).
//
// Claim model choice: claims are registered as their own events rather
// than inferred from assistant message fragments — inference would need a
// message-parsing projection the board does not have, while a
// `claim.registered` event makes every claim (pinned or not) a durable,
// flaggable object in the log with a stable id pins can reference.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

use crate::domain::proposals::{
    MergeApprovedPayload, ProposalCreatedPayload, MERGE_APPROVED, PROPOSAL_CREATED,
};
use crate::domain::verifier::{EvidenceVerifiedPayload, EVIDENCE_VERIFIED};
use crate::eventstore::{Actor, EventError, NewEvent, StoredEvent};

pub const CLAIM_REGISTERED: &str = "claim.registered";
pub const EVIDENCE_PINNED: &str = "evidence.pinned";

/// The pin vocabulary (AD-5): `citation` pins quote a library ref; the
/// `numerical` kind (data/results figures) arrives with its own story —
/// the enum is closed, unknown kinds fail the fold loudly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinKind {
    Citation,
    Numerical,
}

impl PinKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "citation" => Some(Self::Citation),
            "numerical" => Some(Self::Numerical),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Citation => "citation",
            Self::Numerical => "numerical",
        }
    }
}

/// The sha-256 hex digest of a pinned excerpt (AD-5). The single digest
/// implementation: the typed constructor uses it to compute, the fold uses
/// it to verify — a tampered excerpt (payload digest ≠ its own excerpt)
/// fails the fold loudly.
pub fn excerpt_digest(excerpt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(excerpt.as_bytes());
    let bytes = hasher.finalize();
    let mut hex = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

/// The `claim.registered` payload — an AI-generated claim attached to a
/// hypothesis. `source_message_id` is optional provenance (the assistant
/// message the fragment came from, when known).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaimRegisteredPayload {
    pub hypothesis_id: Uuid,
    pub text: String,
    pub source_message_id: Option<String>,
}

/// The `evidence.pinned` payload (AD-5 shape): the pinned claim, the pin
/// kind, the source it anchors to (a library `ref_id` for citations, an
/// `artifact_ref` for numerical pins), the pinned content (`excerpt` —
/// the quoted passage or the values the claim rests on), its sha-256
/// digest, and the agent-assessed confidence attributed to the assessing
/// model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidencePinnedPayload {
    pub claim_id: Uuid,
    pub hypothesis_id: Uuid,
    pub kind: PinKind,
    /// The library ref a citation pin cites — must exist in the refs table
    /// (enforced by the shell; the domain validates presence for the
    /// kind). None for numerical pins.
    #[serde(default)]
    pub ref_id: Option<String>,
    /// The artifact (file/figure/table) a numerical pin anchors to
    /// (FR-3.3) — a path/id in the workspace or fetched from a run. None
    /// for citation pins.
    #[serde(default)]
    pub artifact_ref: Option<String>,
    /// The pinned content — the quoted passage (citation) or the values
    /// (numerical) the claim rests on. Non-empty; the digest binds to
    /// exactly this.
    pub excerpt: String,
    /// sha-256 of the excerpt — computed at construction, never trusted
    /// from callers (AD-5).
    pub digest: String,
    /// Agent-assessed confidence in [0.0, 1.0] (FR-3.6).
    pub confidence: f64,
    /// The model that assessed the confidence — attribution, never "verified".
    pub assessing_model: String,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `claim.registered` event
    /// comes into being. The event is cause-linked to the hypothesis
    /// (AD-2). Actor is the user — the shell registers the claim on the
    /// board; agent-originated claims arrive through their own path (AD-3).
    pub fn claim_registered(
        text: impl Into<String>,
        hypothesis_id: Uuid,
        source_message_id: Option<String>,
    ) -> Result<Self, EventError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(EventError::Invalid(
                "claim.text must not be empty — a claim says something".into(),
            ));
        }
        let payload = ClaimRegisteredPayload {
            hypothesis_id,
            text,
            source_message_id,
        };
        Ok(Self::new(
            CLAIM_REGISTERED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?
        .with_causes(vec![hypothesis_id]))
    }

    /// Typed constructor (AD-15, AD-5): the one way a citation
    /// `evidence.pinned` event comes into being. The digest is COMPUTED
    /// here from the excerpt — it is not a parameter, so no caller can pin
    /// an excerpt with a digest of different text. Validates: ref_id
    /// non-empty (existence in the library is the shell's check), excerpt
    /// non-empty, confidence in [0, 1], assessing_model non-empty. The
    /// event is cause-linked to the claim and the hypothesis (AD-2).
    pub fn evidence_pinned_citation(
        claim_id: Uuid,
        hypothesis_id: Uuid,
        ref_id: impl AsRef<str>,
        excerpt: impl AsRef<str>,
        confidence: f64,
        assessing_model: impl AsRef<str>,
    ) -> Result<Self, EventError> {
        let ref_id = ref_id.as_ref().trim();
        if ref_id.is_empty() {
            return Err(EventError::Invalid(
                "evidence.ref_id must not be empty — a citation pin names its library ref".into(),
            ));
        }
        let excerpt = excerpt.as_ref();
        if excerpt.trim().is_empty() {
            return Err(EventError::Invalid(
                "evidence.excerpt must not be empty — a pin quotes the passage it rests on".into(),
            ));
        }
        let assessing_model = assessing_model.as_ref().trim();
        Self::pin_payload(
            claim_id,
            hypothesis_id,
            PinKind::Citation,
            Some(ref_id.to_string()),
            None,
            excerpt,
            confidence,
            assessing_model,
        )
    }

    /// Typed constructor (AD-15, AD-5, FR-3.3): the one way a numerical
    /// `evidence.pinned` event comes into being. The claim is anchored to
    /// a numerical artifact (`artifact_ref` — a path/id of a file, figure,
    /// or table) by the sha-256 digest of the pinned content, COMPUTED
    /// here from `content` — never a parameter. Validates: artifact_ref
    /// non-empty, content non-empty, confidence in [0, 1],
    /// assessing_model non-empty. The event is cause-linked to the claim
    /// and the hypothesis (AD-2).
    pub fn evidence_pinned_numerical(
        claim_id: Uuid,
        hypothesis_id: Uuid,
        artifact_ref: impl AsRef<str>,
        content: impl AsRef<str>,
        confidence: f64,
        assessing_model: impl AsRef<str>,
    ) -> Result<Self, EventError> {
        let artifact_ref = artifact_ref.as_ref().trim();
        if artifact_ref.is_empty() {
            return Err(EventError::Invalid(
                "evidence.artifact_ref must not be empty — a numerical pin names the artifact it \
                 anchors to (FR-3.3)"
                    .into(),
            ));
        }
        let content = content.as_ref();
        if content.trim().is_empty() {
            return Err(EventError::Invalid(
                "evidence.excerpt must not be empty — a pin anchors the content it rests on".into(),
            ));
        }
        let assessing_model = assessing_model.as_ref().trim();
        Self::pin_payload(
            claim_id,
            hypothesis_id,
            PinKind::Numerical,
            None,
            Some(artifact_ref.to_string()),
            content,
            confidence,
            assessing_model,
        )
    }

    /// The shared pin-construction core: confidence bounds (FR-3.6),
    /// attribution, digest computed from the pinned content (AD-5), and
    /// the cause-linked event (AD-2). Kind-specific source presence is the
    /// callers' check; kind/source consistency is re-checked on read.
    fn pin_payload(
        claim_id: Uuid,
        hypothesis_id: Uuid,
        kind: PinKind,
        ref_id: Option<String>,
        artifact_ref: Option<String>,
        content: &str,
        confidence: f64,
        assessing_model: &str,
    ) -> Result<Self, EventError> {
        if !(0.0..=1.0).contains(&confidence) || confidence.is_nan() {
            return Err(EventError::Invalid(format!(
                "invalid confidence `{confidence}` — agent-assessed confidence is a number in \
                 [0.0, 1.0] (FR-3.6)"
            )));
        }
        if assessing_model.is_empty() {
            return Err(EventError::Invalid(
                "evidence.assessing_model must not be empty — confidence is attributed to the \
                 assessing model, never anonymous (FR-3.6)"
                    .into(),
            ));
        }
        let payload = EvidencePinnedPayload {
            claim_id,
            hypothesis_id,
            kind,
            ref_id,
            artifact_ref,
            excerpt: content.to_string(),
            digest: excerpt_digest(content),
            confidence,
            assessing_model: assessing_model.to_string(),
        };
        Ok(Self::new(
            EVIDENCE_PINNED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?
        .with_causes(vec![claim_id, hypothesis_id]))
    }
}

/// One evidence pin as read from the log (AD-5 shape, AD-8 read model).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidencePin {
    /// The `seq` of the pin event (latest per claim wins — re-pinning appends).
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub claim_id: Uuid,
    pub hypothesis_id: Uuid,
    pub kind: PinKind,
    /// The library ref a citation pin cites; None for numerical pins.
    pub ref_id: Option<String>,
    /// The artifact a numerical pin anchors to (FR-3.3); None for
    /// citation pins.
    pub artifact_ref: Option<String>,
    pub excerpt: String,
    pub digest: String,
    pub confidence: f64,
    pub assessing_model: String,
    /// Author-year label enriched by the shell from the refs table
    /// (e.g. "Vaswani et al. 2017") — the fold leaves it None; refs live
    /// in SQL, not in the log. Citation pins only.
    pub ref_label: Option<String>,
    /// The LATEST machine verification of this pin (Story 4.2) — None
    /// until the first `evidence.verified` event for THIS pin lands
    /// (unverified). Never confusable with `confidence`: verification is
    /// existence by code; confidence is a named model's judgment.
    pub verification: Option<PinVerification>,
}

/// The LATEST verification status of one pin (Story 4.2, AD-15): a
/// projection from `evidence.verified` events — the latest result for the
/// claim's CURRENT pin wins. `status` is the display vocabulary:
/// `verified` / `failed` come straight from the event's outcome; `stale` is
/// fold-derived — a result that predates the current pin (the claim was
/// re-pinned after the verification ran), which renders the re-verify
/// affordance, never a silent carry-over. A pin with NO verification event
/// reads `verification: None` — UNVERIFIED, a state entirely distinct from
/// the agent-assessed confidence (FR-3.6: confidence is a named model's
/// judgment; verification is existence by code).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinVerification {
    /// verified | failed | stale (the read model's display vocabulary; the
    /// event itself only ever carries verified | failed).
    pub status: VerificationStatus,
    /// Machine detail code from the event: excerpt_matched / digest_ok /
    /// not_found / artifact_changed / artifact_missing / fetch_error /
    /// no_source.
    pub detail: String,
    /// What was consulted: `arxiv:<id>`, a URL, or an artifact ref.
    pub source: String,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Verified,
    Failed,
    /// Fold-derived: the result predates the current pin — re-verifiable.
    Stale,
}

/// A claim as read from the log — the read model the board renders.
/// `pinned` is the FR-3.4 flag: claims without an `evidence.pinned` event
/// read as unpinned so the UI can flag them; the flag flips the moment a
/// pin event lands in the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Claim {
    /// The `claim.registered` event's id — the claim's identity.
    pub id: Uuid,
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub hypothesis_id: Uuid,
    pub text: String,
    pub source_message_id: Option<String>,
    /// Derived (FR-3.4): true iff an `evidence.pinned` event references
    /// this claim — the unpinned amber chip renders when false.
    pub pinned: bool,
    /// Derived: the latest pin on this claim, if any.
    pub pin: Option<EvidencePin>,
}

/// Apply one pin payload to its claim (shared by the direct
/// `evidence.pinned` arm and the merged-proposal application, Story 3.4):
/// AD-5 on read — the digest must be the sha-256 of the excerpt the pin
/// itself quotes; kind-specific source presence and FR-3.6 attribution and
/// bounds are re-checked (the constructor's guarantees, re-checked on
/// read). A pin referencing no known claim pins nothing (skipped).
fn apply_pin(
    claims: &mut [Claim],
    index: &HashMap<Uuid, usize>,
    payload: EvidencePinnedPayload,
    seq: i64,
    ts: chrono::DateTime<Utc>,
) -> Result<(), EventError> {
    // AD-5 on read: the digest must be the sha-256 of the
    // excerpt the pin itself quotes — a tampered excerpt
    // (or a pasted-in digest of other text) is corrupt.
    if payload.digest != excerpt_digest(&payload.excerpt) {
        return Err(EventError::Invalid(format!(
            "corrupt {EVIDENCE_PINNED} event at seq {seq}: digest does not match \
             its own excerpt — the pin was tampered with"
        )));
    }
    // AD-5 on read, kind-specific source: a citation pin
    // names its library ref, a numerical pin names its
    // artifact — the constructors' guarantees, re-checked.
    let (ref_id, artifact_ref) = match payload.kind {
        PinKind::Citation => {
            let Some(ref_id) = payload
                .ref_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                return Err(EventError::Invalid(format!(
                    "corrupt {EVIDENCE_PINNED} event at seq {seq}: a citation pin \
                     without its library ref"
                )));
            };
            (Some(ref_id.to_string()), None)
        }
        PinKind::Numerical => {
            let Some(artifact_ref) = payload
                .artifact_ref
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                return Err(EventError::Invalid(format!(
                    "corrupt {EVIDENCE_PINNED} event at seq {seq}: a numerical pin \
                     without its artifact_ref (FR-3.3)"
                )));
            };
            (None, Some(artifact_ref.to_string()))
        }
    };
    // FR-3.6 on read: attribution and bounds are structural
    // — every pin names its assessing model, and confidence
    // stays in [0, 1].
    if payload.assessing_model.trim().is_empty() {
        return Err(EventError::Invalid(format!(
            "corrupt {EVIDENCE_PINNED} event at seq {seq}: assessing_model is empty \
             — confidence is never anonymous (FR-3.6)"
        )));
    }
    if !(0.0..=1.0).contains(&payload.confidence) || payload.confidence.is_nan() {
        return Err(EventError::Invalid(format!(
            "corrupt {EVIDENCE_PINNED} event at seq {seq}: confidence {} is outside \
             [0.0, 1.0]",
            payload.confidence
        )));
    }
    let Some(&i) = index.get(&payload.claim_id) else {
        return Ok(()); // references no known claim — skipped
    };
    let claim = &mut claims[i];
    claim.pinned = true;
    // Story 4.2 on re-pin: a previous pin's verification result NEVER
    // silently carries over to the new excerpt — but it stays VISIBLE as
    // `stale` (the result predates the current pin), so the re-verify
    // affordance renders. Honesty, not amnesia — and never a silent
    // "verified" for text that was never checked.
    let carried_stale = claim.pin.as_ref().and_then(|old| {
        old.verification.as_ref().map(|v| PinVerification {
            status: VerificationStatus::Stale,
            detail: v.detail.clone(),
            source: v.source.clone(),
            ts: v.ts,
        })
    });
    claim.pin = Some(EvidencePin {
        seq,
        ts,
        claim_id: payload.claim_id,
        hypothesis_id: payload.hypothesis_id,
        kind: payload.kind,
        ref_id,
        artifact_ref,
        excerpt: payload.excerpt,
        digest: payload.digest,
        confidence: payload.confidence,
        assessing_model: payload.assessing_model,
        ref_label: None,
        verification: carried_stale,
    });
    Ok(())
}

/// Pure fold of the log into claim read models (AD-1). Events fold in
/// `seq` order; corrupt payloads fail loudly. Pin events verify their own
/// digest against their own excerpt (AD-5): a tampered excerpt — payload
/// digest ≠ sha-256(payload excerpt) — is corrupt and fails the fold, and
/// so is a pin missing its attribution or carrying an out-of-range
/// confidence (the constructor's guarantees, re-checked on read).
/// Story 3.4 (AD-3/AD-13): a MERGED result-pin proposal applies its
/// intended numerical pin at the approval event — approval order is the
/// application order; the intended event itself never lands in the log.
pub struct EvidenceProjection;

impl EvidenceProjection {
    pub fn fold(events: &[StoredEvent]) -> Result<Vec<Claim>, EventError> {
        // The shared fold cursor (AD-1, Story 2.6): fold the live events —
        // a rolled-back pin or claim never happened for the read model.
        let cursor = crate::domain::checkpoints::FoldCursor::over(events);
        let events = &cursor.live_owned(events);
        let mut claims: Vec<Claim> = Vec::new();
        let mut index: HashMap<Uuid, usize> = HashMap::new();
        // Quarantine index (AD-3, Story 3.4): result-pin proposals by event
        // id, so a merge.approved can resolve and apply the intended pin.
        let mut pin_proposals: HashMap<Uuid, ProposalCreatedPayload> = HashMap::new();
        for event in events {
            match event.kind.as_str() {
                CLAIM_REGISTERED => {
                    let payload: ClaimRegisteredPayload = serde_json::from_value(
                        event.payload.clone(),
                    )
                    .map_err(|e| {
                        EventError::Invalid(format!(
                            "corrupt {CLAIM_REGISTERED} payload at seq {}: {e}",
                            event.seq
                        ))
                    })?;
                    index.insert(event.id, claims.len());
                    claims.push(Claim {
                        id: event.id,
                        seq: event.seq,
                        ts: event.ts,
                        hypothesis_id: payload.hypothesis_id,
                        text: payload.text,
                        source_message_id: payload.source_message_id,
                        pinned: false,
                        pin: None,
                    });
                }
                EVIDENCE_PINNED => {
                    let payload: EvidencePinnedPayload = serde_json::from_value(
                        event.payload.clone(),
                    )
                    .map_err(|e| {
                        EventError::Invalid(format!(
                            "corrupt {EVIDENCE_PINNED} payload at seq {}: {e}",
                            event.seq
                        ))
                    })?;
                    apply_pin(&mut claims, &index, payload, event.seq, event.ts)?;
                }
                EVIDENCE_VERIFIED => {
                    // Story 4.2 (AD-15): a machine verification result —
                    // actor system/verifier, never an LLM. The result
                    // applies to the claim's CURRENT pin only when its
                    // `pin_seq` matches (a result never silently applies to
                    // a re-pinned excerpt); a result for an older pin marks
                    // the current one `stale` when nothing newer holds. The
                    // LATEST event for the current pin wins — re-verification
                    // flips failed→verified and never the other way without
                    // a new event. The constructor's guarantees (detail and
                    // source non-empty, closed outcome vocabulary) are
                    // re-checked on read.
                    let payload: EvidenceVerifiedPayload =
                        serde_json::from_value(event.payload.clone()).map_err(|e| {
                            EventError::Invalid(format!(
                                "corrupt {EVIDENCE_VERIFIED} payload at seq {}: {e}",
                                event.seq
                            ))
                        })?;
                    if payload.detail.trim().is_empty() || payload.source.trim().is_empty() {
                        return Err(EventError::Invalid(format!(
                            "corrupt {EVIDENCE_VERIFIED} event at seq {}: detail and source are \
                             never empty — a result says why and names what it consulted",
                            event.seq
                        )));
                    }
                    let Some(&i) = index.get(&payload.claim_id) else {
                        continue; // references no known claim — skipped
                    };
                    let claim = &mut claims[i];
                    let Some(pin) = claim.pin.as_mut() else {
                        continue; // the claim has no pin to verify — skipped
                    };
                    if pin.seq == payload.pin_seq {
                        pin.verification = Some(PinVerification {
                            status: match payload.outcome {
                                crate::domain::verifier::VerificationOutcome::Verified => {
                                    VerificationStatus::Verified
                                }
                                crate::domain::verifier::VerificationOutcome::Failed => {
                                    VerificationStatus::Failed
                                }
                            },
                            detail: payload.detail,
                            source: payload.source,
                            ts: event.ts,
                        });
                    } else if pin.verification.as_ref().is_some_and(|v| {
                        matches!(v.status, VerificationStatus::Stale)
                    }) || pin.verification.is_none() {
                        // A result for an older version of this pin —
                        // visible as stale (re-verifiable), never carried as
                        // verified/failed for text it did not check.
                        pin.verification = Some(PinVerification {
                            status: VerificationStatus::Stale,
                            detail: payload.detail,
                            source: payload.source,
                            ts: event.ts,
                        });
                    }
                }
                PROPOSAL_CREATED => {
                    // Quarantine (AD-3, Story 3.4): a result-pin proposal
                    // records intent only — the intended pin stays EXCLUDED
                    // from this fold (and every projection) until a
                    // merge.approved lands. Indexed here so the merge
                    // application below can resolve it.
                    if let Ok(payload) =
                        serde_json::from_value::<ProposalCreatedPayload>(event.payload.clone())
                    {
                        if payload.proposed_kind == EVIDENCE_PINNED {
                            pin_proposals.insert(event.id, payload);
                        }
                    }
                }
                MERGE_APPROVED => {
                    // The application point (AD-13, Story 3.4): the approval
                    // applies the proposal's intended numerical pin to its
                    // claim — in this event's seq order, i.e. by approval
                    // order. The merged pin carries the MERGE's seq/ts (the
                    // human's act, never a silent agent append — the
                    // intended event itself never lands in the log). AD-5 on
                    // read: the intended payload's digest is re-verified
                    // here; the approve() path refuses a tampered candidate
                    // first (digest_mismatch:).
                    let payload: MergeApprovedPayload = serde_json::from_value(
                        event.payload.clone(),
                    )
                    .map_err(|e| {
                        EventError::Invalid(format!(
                            "corrupt {MERGE_APPROVED} payload at seq {}: {e}",
                            event.seq
                        ))
                    })?;
                    let Some(proposal) = pin_proposals.get(&payload.proposal_id) else {
                        continue; // references no known pin proposal — skipped
                    };
                    let intended: EvidencePinnedPayload =
                        serde_json::from_value(proposal.proposed_payload.clone()).map_err(
                            |e| {
                                EventError::Invalid(format!(
                                    "corrupt proposed payload of proposal {} applied at seq \
                                     {}: {e}",
                                    payload.proposal_id, event.seq
                                ))
                            },
                        )?;
                    apply_pin(&mut claims, &index, intended, event.seq, event.ts)?;
                }
                _ => {}
            }
        }
        Ok(claims)
    }

    /// The claims of one hypothesis, in registration `seq` order.
    pub fn fold_for(
        events: &[StoredEvent],
        hypothesis_id: Uuid,
    ) -> Result<Vec<Claim>, EventError> {
        Ok(Self::fold(events)?
            .into_iter()
            .filter(|c| c.hypothesis_id == hypothesis_id)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::hypotheses::HypothesesProjection;
    use crate::eventstore::EventStore;
    use rusqlite::Connection;
    use serde_json::json;
    use uuid::Uuid;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    /// A store holding one mission + one hypothesis; returns the
    /// hypothesis's creation event id.
    fn seed_hypothesis(store: &EventStore) -> Uuid {
        let mission = store
            .append(
                NewEvent::mission_created(crate::domain::missions::MissionCreatedPayload {
                    question: "Does X hold up?".into(),
                    stop_condition: "Stop after $5.".into(),
                    success_criterion: "A blind rater agrees.".into(),
                    autonomy: crate::domain::missions::Autonomy::Suggest,
                    spend_ceiling_cents: 500,
                    schedule: "daily-03:00".into(),
                roles: vec![],
                })
                .unwrap(),
            )
            .unwrap();
        store
            .append(NewEvent::hypothesis_created("X holds under stiff systems.", mission.id).unwrap())
            .unwrap()
            .id
    }

    // ---------- constructors ----------

    #[test]
    fn claim_constructor_rejects_a_blank_text_and_links_the_hypothesis() {
        let h = Uuid::new_v4();
        let err = NewEvent::claim_registered("   ", h, None)
            .expect_err("a blank claim must fail construction");
        assert!(err.to_string().contains("claim.text"), "unexpected: {err}");

        let ev = NewEvent::claim_registered("Self-attention replaces recurrence.", h, None).unwrap();
        assert_eq!(ev.kind, "claim.registered");
        assert_eq!(ev.actor, Actor::User);
        assert_eq!(ev.causes, vec![h]);
        assert_eq!(
            ev.payload,
            json!({
                "hypothesis_id": h.to_string(),
                "text": "Self-attention replaces recurrence.",
                "source_message_id": null,
            })
        );
    }

    #[test]
    fn claim_constructor_keeps_the_optional_message_provenance() {
        let h = Uuid::new_v4();
        let ev = NewEvent::claim_registered(
            "Claim.",
            h,
            Some("msg-42".into()),
        )
        .unwrap();
        assert_eq!(
            ev.payload,
            json!({
                "hypothesis_id": h.to_string(),
                "text": "Claim.",
                "source_message_id": "msg-42",
            })
        );
    }

    #[test]
    fn pin_constructor_computes_the_digest_and_never_trusts_one() {
        let (claim, h) = (Uuid::new_v4(), Uuid::new_v4());
        let excerpt = "Attention mechanisms, as introduced by Vaswani et al., dispense with recurrence entirely.";
        let ev =
            NewEvent::evidence_pinned_citation(claim, h, "ref-1", excerpt, 0.82, "GLM-5.3").unwrap();
        assert_eq!(ev.kind, "evidence.pinned");
        assert_eq!(ev.actor, Actor::User);
        assert_eq!(ev.causes, vec![claim, h]);
        let payload: EvidencePinnedPayload = serde_json::from_value(ev.payload.clone()).unwrap();
        // The digest is the sha-256 of the excerpt — computed at
        // construction, and there is no parameter through which a caller
        // could have supplied it.
        assert_eq!(payload.digest, excerpt_digest(excerpt));
        assert_eq!(payload.digest.len(), 64);
        assert_eq!(payload.kind, PinKind::Citation);
        assert_eq!(payload.confidence, 0.82);
        assert_eq!(payload.assessing_model, "GLM-5.3");
        // A different excerpt yields a different digest — the digest binds
        // the pin to exactly the text it quotes.
        let other =
            NewEvent::evidence_pinned_citation(claim, h, "ref-1", "A different passage.", 0.82, "GLM-5.3")
                .unwrap();
        let other_payload: EvidencePinnedPayload =
            serde_json::from_value(other.payload.clone()).unwrap();
        assert_ne!(other_payload.digest, payload.digest);
    }

    #[test]
    fn pin_constructor_rejects_invalid_ref_excerpt_confidence_and_model() {
        let (claim, h) = (Uuid::new_v4(), Uuid::new_v4());
        // empty ref_id
        let err = NewEvent::evidence_pinned_citation(claim, h, "  ", "excerpt", 0.5, "GLM-5.3")
            .expect_err("an empty ref_id must fail construction");
        assert!(err.to_string().contains("ref_id"), "unexpected: {err}");
        // empty excerpt
        let err = NewEvent::evidence_pinned_citation(claim, h, "ref-1", "   ", 0.5, "GLM-5.3")
            .expect_err("an empty excerpt must fail construction");
        assert!(err.to_string().contains("excerpt"), "unexpected: {err}");
        // confidence bounds: below 0, above 1, and NaN are all rejected
        for bad in [-0.01, 1.01, f64::NAN] {
            let err = NewEvent::evidence_pinned_citation(claim, h, "ref-1", "excerpt", bad, "GLM-5.3")
                .expect_err("out-of-range confidence must fail construction");
            assert!(err.to_string().contains("confidence"), "unexpected: {err}");
        }
        // the bounds themselves are inclusive — 0.0 and 1.0 construct fine
        for ok in [0.0, 1.0] {
            NewEvent::evidence_pinned_citation(claim, h, "ref-1", "excerpt", ok, "GLM-5.3")
                .expect("inclusive bounds must construct");
        }
        // empty assessing model — confidence is never anonymous
        let err = NewEvent::evidence_pinned_citation(claim, h, "ref-1", "excerpt", 0.5, "  ")
            .expect_err("an empty assessing model must fail construction");
        assert!(err.to_string().contains("assessing_model"), "unexpected: {err}");
    }

    // ---------- numerical pins (FR-3.3, Story 1.8) ----------

    #[test]
    fn numerical_pin_constructor_computes_the_digest_from_the_content() {
        let (claim, h) = (Uuid::new_v4(), Uuid::new_v4());
        let content = "benchmark: 42.7 mean, ±0.4, n=12, table 3 of run 7";
        let ev = NewEvent::evidence_pinned_numerical(
            claim,
            h,
            "runs/007/table-3.csv",
            content,
            0.9,
            "GLM-5.3",
        )
        .unwrap();
        assert_eq!(ev.kind, "evidence.pinned");
        assert_eq!(ev.actor, Actor::User);
        assert_eq!(ev.causes, vec![claim, h]);
        let payload: EvidencePinnedPayload = serde_json::from_value(ev.payload.clone()).unwrap();
        // FR-3.3 / AD-5: the numerical pin anchors by artifact_ref + the
        // sha-256 of the pinned content — computed here, never a parameter.
        assert_eq!(payload.kind, PinKind::Numerical);
        assert_eq!(payload.artifact_ref.as_deref(), Some("runs/007/table-3.csv"));
        assert_eq!(payload.ref_id, None);
        assert_eq!(payload.digest, excerpt_digest(content));
        assert_eq!(payload.digest.len(), 64);
        assert_eq!(payload.confidence, 0.9);
        assert_eq!(payload.assessing_model, "GLM-5.3");
        // Different content yields a different digest — the digest binds
        // the pin to exactly the values it anchors.
        let other = NewEvent::evidence_pinned_numerical(
            claim,
            h,
            "runs/007/table-3.csv",
            "benchmark: 43.1 mean",
            0.9,
            "GLM-5.3",
        )
        .unwrap();
        let other_payload: EvidencePinnedPayload =
            serde_json::from_value(other.payload.clone()).unwrap();
        assert_ne!(other_payload.digest, payload.digest);
    }

    #[test]
    fn numerical_pin_constructor_rejects_invalid_artifact_content_confidence_and_model() {
        let (claim, h) = (Uuid::new_v4(), Uuid::new_v4());
        // empty artifact_ref
        let err = NewEvent::evidence_pinned_numerical(claim, h, "  ", "values", 0.5, "GLM-5.3")
            .expect_err("an empty artifact_ref must fail construction");
        assert!(err.to_string().contains("artifact_ref"), "unexpected: {err}");
        // empty content
        let err = NewEvent::evidence_pinned_numerical(claim, h, "runs/7/t.csv", "   ", 0.5, "GLM-5.3")
            .expect_err("empty content must fail construction");
        assert!(err.to_string().contains("excerpt"), "unexpected: {err}");
        // confidence bounds
        for bad in [-0.01, 1.01, f64::NAN] {
            let err = NewEvent::evidence_pinned_numerical(claim, h, "runs/7/t.csv", "values", bad, "GLM-5.3")
                .expect_err("out-of-range confidence must fail construction");
            assert!(err.to_string().contains("confidence"), "unexpected: {err}");
        }
        // empty assessing model
        let err = NewEvent::evidence_pinned_numerical(claim, h, "runs/7/t.csv", "values", 0.5, "  ")
            .expect_err("an empty assessing model must fail construction");
        assert!(err.to_string().contains("assessing_model"), "unexpected: {err}");
    }

    #[test]
    fn a_numerical_pin_folds_with_its_artifact_and_digest() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let h = seed_hypothesis(&store);
        let claim = store
            .append(NewEvent::claim_registered("Table 3 shows a 12% gain.", h, None).unwrap())
            .unwrap();
        let content = "gain: 12.3%, p<0.01, n=48";
        store
            .append(NewEvent::evidence_pinned_numerical(
                claim.id,
                h,
                "runs/007/table-3.csv",
                content,
                0.91,
                "GLM-5.3",
            )
            .unwrap())
            .unwrap();
        let [pinned] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        assert!(pinned.pinned);
        let pin = pinned.pin.as_ref().expect("the pin is present");
        assert_eq!(pin.kind, PinKind::Numerical);
        assert_eq!(pin.artifact_ref.as_deref(), Some("runs/007/table-3.csv"));
        assert_eq!(pin.ref_id, None);
        assert_eq!(pin.digest, excerpt_digest(content));
        assert_eq!(pin.confidence, 0.91);
        assert_eq!(pin.assessing_model, "GLM-5.3");
        assert_eq!(pin.ref_label, None, "no library label on a numerical pin");
    }

    #[test]
    fn a_tampered_numerical_content_fails_the_fold_loudly() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let h = seed_hypothesis(&store);
        let claim = store
            .append(NewEvent::claim_registered("Claim.", h, None).unwrap())
            .unwrap();
        // A hand-built numerical pin whose digest matches DIFFERENT content
        // than the values it quotes — the AD-5 tamper case, numerical kind.
        let digest_of_other_content = excerpt_digest("entirely different numbers");
        let payload = json!({
            "claim_id": claim.id.to_string(),
            "hypothesis_id": h.to_string(),
            "kind": "numerical",
            "artifact_ref": "runs/007/table-3.csv",
            "excerpt": "gain: 12.3%, n=48",
            "digest": digest_of_other_content,
            "confidence": 0.9,
            "assessing_model": "GLM-5.3",
        });
        store
            .append(
                NewEvent::new(EVIDENCE_PINNED, Actor::User, payload)
                    .unwrap()
                    .with_causes(vec![claim.id, h]),
            )
            .unwrap();
        let err = EvidenceProjection::fold(&store.events_all().unwrap())
            .expect_err("a digest that does not match its own content must fail the fold");
        assert!(err.to_string().contains("tampered"), "unexpected: {err}");
    }

    #[test]
    fn a_pin_missing_its_kind_source_fails_the_fold() {
        for (kind, body) in [
            (
                "citation",
                json!({
                    "kind": "citation",
                    "artifact_ref": "runs/7/t.csv",
                    "excerpt": "The quoted excerpt.",
                    "digest": excerpt_digest("The quoted excerpt."),
                    "confidence": 0.9,
                    "assessing_model": "GLM-5.3",
                }),
            ),
            (
                "numerical",
                json!({
                    "kind": "numerical",
                    "ref_id": "ref-1",
                    "excerpt": "gain: 12.3%, n=48",
                    "digest": excerpt_digest("gain: 12.3%, n=48"),
                    "confidence": 0.9,
                    "assessing_model": "GLM-5.3",
                }),
            ),
        ] {
            let conn = mem_conn();
            let store = EventStore::new(&conn);
            let h = seed_hypothesis(&store);
            let claim = store
                .append(NewEvent::claim_registered("Claim.", h, None).unwrap())
                .unwrap();
            let mut payload = body;
            payload["claim_id"] = json!(claim.id.to_string());
            payload["hypothesis_id"] = json!(h.to_string());
            store
                .append(
                    NewEvent::new(EVIDENCE_PINNED, Actor::User, payload)
                        .unwrap()
                        .with_causes(vec![claim.id, h]),
                )
                .unwrap();
            let err = EvidenceProjection::fold(&store.events_all().unwrap())
                .expect_err("a pin missing its kind's source must fail the fold");
            assert!(
                err.to_string().contains(kind),
                "the error names the kind: {err}"
            );
        }
    }

    /// FR-3.3 + FR-3.1 in one fold: citation and numerical pins coexist on
    /// the same hypothesis — each claim keeps its own kind's anatomy.
    #[test]
    fn both_pin_kinds_coexist_in_one_fold() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let h = seed_hypothesis(&store);
        let cited = store
            .append(NewEvent::claim_registered("Attention drops recurrence.", h, None).unwrap())
            .unwrap();
        let measured = store
            .append(NewEvent::claim_registered("Table 3 shows a 12% gain.", h, None).unwrap())
            .unwrap();
        store
            .append(
                NewEvent::evidence_pinned_citation(
                    cited.id,
                    h,
                    "ref-1",
                    "Attention dispenses with recurrence entirely.",
                    0.82,
                    "GLM-5.3",
                )
                .unwrap(),
            )
            .unwrap();
        store
            .append(NewEvent::evidence_pinned_numerical(
                measured.id,
                h,
                "runs/007/table-3.csv",
                "gain: 12.3%, n=48",
                0.91,
                "GLM-5.3",
            )
            .unwrap())
            .unwrap();
        let claims = EvidenceProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(claims.len(), 2);
        assert!(claims.iter().all(|c| c.pinned));
        let citation = claims.iter().find(|c| c.id == cited.id).unwrap();
        assert_eq!(citation.pin.as_ref().unwrap().kind, PinKind::Citation);
        assert_eq!(citation.pin.as_ref().unwrap().ref_id.as_deref(), Some("ref-1"));
        assert_eq!(citation.pin.as_ref().unwrap().artifact_ref, None);
        let numerical = claims.iter().find(|c| c.id == measured.id).unwrap();
        let pin = numerical.pin.as_ref().unwrap();
        assert_eq!(pin.kind, PinKind::Numerical);
        assert_eq!(pin.artifact_ref.as_deref(), Some("runs/007/table-3.csv"));
        assert_eq!(pin.ref_id, None);
        assert_eq!(pin.digest, excerpt_digest("gain: 12.3%, n=48"));
    }

    // ---------- fold ----------

    #[test]
    fn an_unpinned_claim_folds_as_unpinned_then_flips_when_pinned() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let h = seed_hypothesis(&store);
        let claim = store
            .append(NewEvent::claim_registered("Self-attention replaces recurrence.", h, None).unwrap())
            .unwrap();

        // FR-3.4: with no pin event, the claim reads as UNPINNED.
        let [unpinned] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        assert!(!unpinned.pinned, "a claim without a pin reads as unpinned");
        assert_eq!(unpinned.pin, None);
        assert_eq!(unpinned.hypothesis_id, h);

        // Pin it; the same fold now reads pinned=true with the full AD-5
        // pin shape — the exact flip the UI renders.
        store
            .append(
                NewEvent::evidence_pinned_citation(
                    claim.id,
                    h,
                    "ref-1",
                    "Attention dispenses with recurrence entirely.",
                    0.82,
                    "GLM-5.3",
                )
                .unwrap(),
            )
            .unwrap();
        let [pinned] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        assert!(pinned.pinned);
        let pin = pinned.pin.as_ref().expect("the pin is present");
        assert_eq!(pin.claim_id, claim.id);
        assert_eq!(pin.kind, PinKind::Citation);
        assert_eq!(pin.ref_id.as_deref(), Some("ref-1"));
        assert_eq!(pin.artifact_ref, None);
        assert_eq!(pin.digest, excerpt_digest("Attention dispenses with recurrence entirely."));
        assert_eq!(pin.confidence, 0.82);
        // FR-3.6: attribution is present on the pin — the assessing model,
        // never a "verified" label.
        assert_eq!(pin.assessing_model, "GLM-5.3");
        assert_eq!(pin.ref_label, None, "the fold leaves enrichment to the shell");

        // Hypothesis scoping: fold_for returns only that hypothesis's claims.
        assert_eq!(
            EvidenceProjection::fold_for(&store.events_all().unwrap(), h).unwrap().len(),
            1
        );
        assert!(EvidenceProjection::fold_for(&store.events_all().unwrap(), Uuid::new_v4())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_tampered_excerpt_fails_the_fold_loudly() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let h = seed_hypothesis(&store);
        let claim = store
            .append(NewEvent::claim_registered("Claim.", h, None).unwrap())
            .unwrap();
        // A hand-built event whose digest matches DIFFERENT text than the
        // excerpt it quotes — the AD-5 tamper case. The constructor cannot
        // produce this; only a raw append can, and the fold catches it.
        let digest_of_other_text = excerpt_digest("entirely different passage");
        let payload = json!({
            "claim_id": claim.id.to_string(),
            "hypothesis_id": h.to_string(),
            "kind": "citation",
            "ref_id": "ref-1",
            "excerpt": "The quoted excerpt.",
            "digest": digest_of_other_text,
            "confidence": 0.9,
            "assessing_model": "GLM-5.3",
        });
        store
            .append(
                NewEvent::new(EVIDENCE_PINNED, Actor::User, payload)
                    .unwrap()
                    .with_causes(vec![claim.id, h]),
            )
            .unwrap();
        let err = EvidenceProjection::fold(&store.events_all().unwrap())
            .expect_err("a digest that does not match its own excerpt must fail the fold");
        assert!(err.to_string().contains("tampered"), "unexpected: {err}");
    }

    #[test]
    fn a_pin_without_attribution_or_with_bad_confidence_fails_the_fold() {
        for (confidence, model) in [(0.5, "  "), (1.5, "GLM-5.3")] {
            let conn = mem_conn();
            let store = EventStore::new(&conn);
            let h = seed_hypothesis(&store);
            let claim = store
                .append(NewEvent::claim_registered("Claim.", h, None).unwrap())
                .unwrap();
            let payload = json!({
                "claim_id": claim.id.to_string(),
                "hypothesis_id": h.to_string(),
                "kind": "citation",
                "ref_id": "ref-1",
                "excerpt": "The quoted excerpt.",
                "digest": excerpt_digest("The quoted excerpt."),
                "confidence": confidence,
                "assessing_model": model,
            });
            store
                .append(
                    NewEvent::new(EVIDENCE_PINNED, Actor::User, payload)
                        .unwrap()
                        .with_causes(vec![claim.id, h]),
                )
                .unwrap();
            let err = EvidenceProjection::fold(&store.events_all().unwrap())
                .expect_err("a pin missing its guarantees must fail the fold");
            assert!(
                err.to_string().contains("assessing_model")
                    || err.to_string().contains("confidence"),
                "unexpected: {err}"
            );
        }
    }

    #[test]
    fn a_pin_referencing_an_unknown_claim_is_skipped_not_fatal() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let h = seed_hypothesis(&store);
        store
            .append(NewEvent::claim_registered("Claim.", h, None).unwrap())
            .unwrap();
        store
            .append(
                NewEvent::evidence_pinned_citation(
                    Uuid::new_v4(), // no such claim
                    h,
                    "ref-1",
                    "Excerpt.",
                    0.5,
                    "GLM-5.3",
                )
                .unwrap(),
            )
            .unwrap();
        let claims = EvidenceProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(claims.len(), 1);
        assert!(!claims[0].pinned, "a pin to a ghost claim pins nothing");
    }

    /// Re-pinning a claim appends: the latest `evidence.pinned` event for
    /// the claim wins — editing a pin appends, never mutates.
    #[test]
    fn repinning_appends_and_the_latest_pin_wins() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let h = seed_hypothesis(&store);
        let claim = store
            .append(NewEvent::claim_registered("Claim.", h, None).unwrap())
            .unwrap();
        store
            .append(NewEvent::evidence_pinned_citation(claim.id, h, "ref-1", "First excerpt.", 0.5, "GLM-5.3").unwrap())
            .unwrap();
        let second = store
            .append(NewEvent::evidence_pinned_citation(claim.id, h, "ref-2", "Second excerpt.", 0.9, "GLM-5.3").unwrap())
            .unwrap();
        let [claim_read] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        let pin = claim_read.pin.as_ref().unwrap();
        assert_eq!(pin.seq, second.seq);
        assert_eq!(pin.ref_id.as_deref(), Some("ref-2"));
        assert_eq!(pin.digest, excerpt_digest("Second excerpt."));
    }

    /// The evidence fold coexists with the hypothesis fold over one log —
    /// pin events never disturb the board, and hypothesis events never
    /// disturb the claims.
    #[test]
    fn the_evidence_and_hypothesis_folds_coexist_over_one_log() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let h = seed_hypothesis(&store);
        let claim = store
            .append(NewEvent::claim_registered("Claim.", h, None).unwrap())
            .unwrap();
        store
            .append(NewEvent::evidence_pinned_citation(claim.id, h, "ref-1", "Excerpt.", 0.82, "GLM-5.3").unwrap())
            .unwrap();
        let hyps = HypothesesProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(hyps.len(), 1);
        assert_eq!(hyps[0].relations, vec![]);
        let claims = EvidenceProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(claims.len(), 1);
        assert!(claims[0].pinned);
    }

    #[test]
    fn an_empty_log_folds_to_no_claims() {
        let conn = mem_conn();
        assert!(EvidenceProjection::fold(&EventStore::new(&conn).events_all().unwrap())
            .unwrap()
            .is_empty());
    }

    // ---------- machine verification (Story 4.2, AD-15) ----------

    /// A pinned claim reads UNVERIFIED until the first `evidence.verified`
    /// event for ITS pin lands — and the verification is a separate axis
    /// from the agent-assessed confidence (FR-3.6): both render, neither
    /// drives the other.
    #[test]
    fn a_pin_reads_unverified_until_the_first_event_and_keeps_both_axes() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let h = seed_hypothesis(&store);
        let claim = store
            .append(NewEvent::claim_registered("Attention drops recurrence.", h, None).unwrap())
            .unwrap();
        let pin = store
            .append(
                NewEvent::evidence_pinned_citation(
                    claim.id,
                    h,
                    "ref-1",
                    "Attention dispenses with recurrence entirely.",
                    0.82,
                    "GLM-5.3",
                )
                .unwrap(),
            )
            .unwrap();
        // No verification event yet → unverified (None), while the
        // confidence + attribution stay on the pin.
        let [c] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        let read = c.pin.as_ref().expect("the pin is present");
        assert_eq!(read.verification, None, "unverified until the first event");
        assert_eq!(read.confidence, 0.82);
        assert_eq!(read.assessing_model, "GLM-5.3");

        // The first event lands → verified, with the detail + source; the
        // confidence axis is untouched (never replaced by "verified").
        store
            .append(
                NewEvent::evidence_verified(
                    claim.id,
                    h,
                    pin.seq,
                    crate::domain::verifier::VerificationOutcome::Verified,
                    crate::domain::verifier::DETAIL_EXCERPT_MATCHED,
                    "arxiv:1706.03762",
                )
                .unwrap(),
            )
            .unwrap();
        let [c] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        let read = c.pin.as_ref().expect("the pin is present");
        let v = read.verification.as_ref().expect("verified");
        assert_eq!(v.status, VerificationStatus::Verified);
        assert_eq!(v.detail, "excerpt_matched");
        assert_eq!(v.source, "arxiv:1706.03762");
        // FR-3.6 separation: the agent-assessed confidence and its
        // attribution remain exactly what they were — "verified" is the
        // machine axis, never the confidence label.
        assert_eq!(read.confidence, 0.82);
        assert_eq!(read.assessing_model, "GLM-5.3");
    }

    /// A FAILED verification marks the pin visibly and the pin STAYS — the
    /// claim remains pinned-but-flagged (honesty, not amnesia). The latest
    /// event wins: a later success flips failed→verified.
    #[test]
    fn a_failed_verification_marks_the_pin_and_reverification_flips_it() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let h = seed_hypothesis(&store);
        let claim = store
            .append(NewEvent::claim_registered("Claim.", h, None).unwrap())
            .unwrap();
        let pin = store
            .append(
                NewEvent::evidence_pinned_citation(claim.id, h, "ref-1", "Excerpt.", 0.5, "GLM-5.3")
                    .unwrap(),
            )
            .unwrap();
        // A failed run (fetch error) — the pin stays in place, flagged.
        store
            .append(
                NewEvent::evidence_verified(
                    claim.id,
                    h,
                    pin.seq,
                    crate::domain::verifier::VerificationOutcome::Failed,
                    crate::domain::verifier::DETAIL_FETCH_ERROR,
                    "https://doi.org/10.1000/ghost",
                )
                .unwrap(),
            )
            .unwrap();
        let [c] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        assert!(c.pinned, "a failed verification never deletes the pin");
        let read = c.pin.as_ref().expect("the pin is present");
        let v = read.verification.as_ref().expect("flagged");
        assert_eq!(v.status, VerificationStatus::Failed);
        assert_eq!(v.detail, "fetch_error");

        // Re-verification succeeds → the latest event wins: failed→verified.
        store
            .append(
                NewEvent::evidence_verified(
                    claim.id,
                    h,
                    pin.seq,
                    crate::domain::verifier::VerificationOutcome::Verified,
                    crate::domain::verifier::DETAIL_EXCERPT_MATCHED,
                    "https://doi.org/10.1000/ghost",
                )
                .unwrap(),
            )
            .unwrap();
        let [c] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        let v = c.pin.as_ref().unwrap().verification.as_ref().unwrap();
        assert_eq!(v.status, VerificationStatus::Verified);
    }

    /// A result for an OLDER pin never applies to a re-pinned excerpt: a
    /// re-pin carries the old result forward as STALE (re-verifiable), and
    /// a late event for the old pin reads stale too — never a silent
    /// "verified" for text that was never checked.
    #[test]
    fn a_result_for_an_older_pin_reads_stale_never_verified() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let h = seed_hypothesis(&store);
        let claim = store
            .append(NewEvent::claim_registered("Claim.", h, None).unwrap())
            .unwrap();
        let first = store
            .append(
                NewEvent::evidence_pinned_citation(claim.id, h, "ref-1", "First excerpt.", 0.5, "GLM-5.3")
                    .unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::evidence_verified(
                    claim.id,
                    h,
                    first.seq,
                    crate::domain::verifier::VerificationOutcome::Verified,
                    crate::domain::verifier::DETAIL_EXCERPT_MATCHED,
                    "arxiv:1706.03762",
                )
                .unwrap(),
            )
            .unwrap();
        // Re-pin with a NEW excerpt: the old verified result must not apply.
        store
            .append(
                NewEvent::evidence_pinned_citation(claim.id, h, "ref-1", "Second excerpt.", 0.9, "GLM-5.3")
                    .unwrap(),
            )
            .unwrap();
        let [c] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        let read = c.pin.as_ref().expect("the pin is present");
        assert_eq!(read.excerpt, "Second excerpt.");
        let v = read.verification.as_ref().expect("the old result stays visible");
        assert_eq!(v.status, VerificationStatus::Stale, "stale, never verified");

        // A late event for the OLD pin_seq also reads stale (the current
        // pin was never checked by it).
        store
            .append(
                NewEvent::evidence_verified(
                    claim.id,
                    h,
                    first.seq,
                    crate::domain::verifier::VerificationOutcome::Verified,
                    crate::domain::verifier::DETAIL_EXCERPT_MATCHED,
                    "arxiv:1706.03762",
                )
                .unwrap(),
            )
            .unwrap();
        let [c] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        let v = c.pin.as_ref().unwrap().verification.as_ref().unwrap();
        assert_eq!(v.status, VerificationStatus::Stale);

        // A verification of the CURRENT pin wins over the stale marker.
        let second_seq = c.pin.as_ref().unwrap().seq;
        store
            .append(
                NewEvent::evidence_verified(
                    claim.id,
                    h,
                    second_seq,
                    crate::domain::verifier::VerificationOutcome::Failed,
                    crate::domain::verifier::DETAIL_NOT_FOUND,
                    "arxiv:1706.03762",
                )
                .unwrap(),
            )
            .unwrap();
        let [c] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        let v = c.pin.as_ref().unwrap().verification.as_ref().unwrap();
        assert_eq!(v.status, VerificationStatus::Failed);
    }

    /// A verification event for a claim with no pin, or an unknown claim,
    /// pins nothing and fails nothing — skipped, not fatal.
    #[test]
    fn a_verification_without_its_pin_or_claim_is_skipped() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let h = seed_hypothesis(&store);
        let unpinned = store
            .append(NewEvent::claim_registered("Unpinned claim.", h, None).unwrap())
            .unwrap();
        store
            .append(
                NewEvent::evidence_verified(
                    unpinned.id,
                    h,
                    42,
                    crate::domain::verifier::VerificationOutcome::Verified,
                    crate::domain::verifier::DETAIL_EXCERPT_MATCHED,
                    "arxiv:1706.03762",
                )
                .unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::evidence_verified(
                    Uuid::new_v4(), // no such claim
                    h,
                    7,
                    crate::domain::verifier::VerificationOutcome::Failed,
                    crate::domain::verifier::DETAIL_NOT_FOUND,
                    "arxiv:1706.03762",
                )
                .unwrap(),
            )
            .unwrap();
        let claims = EvidenceProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(claims.len(), 1);
        assert!(!claims[0].pinned);
        assert_eq!(claims[0].pin, None);
    }

    /// A corrupt verification event (blank detail, unknown outcome) fails
    /// the fold loudly — the constructor's guarantees, re-checked on read.
    #[test]
    fn a_corrupt_verification_event_fails_the_fold_loudly() {
        for payload in [
            serde_json::json!({
                "outcome": "verified",
                "detail": "   ",
                "source": "arxiv:1706.03762",
            }),
            serde_json::json!({
                "outcome": "maybe",
                "detail": "excerpt_matched",
                "source": "arxiv:1706.03762",
            }),
        ] {
            let conn = mem_conn();
            let store = EventStore::new(&conn);
            let h = seed_hypothesis(&store);
            let claim = store
                .append(NewEvent::claim_registered("Claim.", h, None).unwrap())
                .unwrap();
            let pin = store
                .append(
                    NewEvent::evidence_pinned_citation(claim.id, h, "ref-1", "Excerpt.", 0.5, "GLM-5.3")
                        .unwrap(),
                )
                .unwrap();
            let mut payload = payload;
            payload["claim_id"] = serde_json::json!(claim.id.to_string());
            payload["hypothesis_id"] = serde_json::json!(h.to_string());
            payload["pin_seq"] = serde_json::json!(pin.seq);
            store
                .append(
                    NewEvent::new(EVIDENCE_VERIFIED, Actor::User, payload)
                        .unwrap()
                        .with_causes(vec![claim.id, h]),
                )
                .unwrap();
            let err = EvidenceProjection::fold(&store.events_all().unwrap())
                .expect_err("a corrupt verification event must fail the fold");
            assert!(
                err.to_string().contains("evidence.verified"),
                "unexpected: {err}"
            );
        }
    }



    /// A merged pin proposal applies its intended numerical pin AT the
    /// approval event — approval order is the application order, and the
    /// pin carries the MERGE's seq/ts (the human's act). The intended
    /// evidence.pinned event never lands in the log as its own event.
    #[test]
    fn a_merged_pin_proposal_applies_at_the_approval_event() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let h = seed_hypothesis(&store);
        let claim = store
            .append(NewEvent::claim_registered("Result artifact `jobs/7/stdout`.", h, None).unwrap())
            .unwrap();
        let content = "accuracy: 0.912, ±0.006, n=5 seeds";
        let intended = NewEvent::evidence_pinned_numerical(
            claim.id,
            h,
            "jobs/7/stdout",
            content,
            0.5,
            "GLM-5.3",
        )
        .unwrap();
        let proposal = store
            .append(
                NewEvent::proposal_created("fetch-7", &intended, h, claim.seq + 1, claim.id)
                    .unwrap(),
            )
            .unwrap();
        // pending: nothing is pinned (AD-3)
        let [pending] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        assert!(!pending.pinned);
        // the merge (the only construction site is crate-private — the
        // command path calls approve(); the fold contract is the same)
        let merge = store
            .append(NewEvent::merge_approved(proposal.id, false, false).unwrap())
            .unwrap();
        let [pinned] = EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .try_into()
            .ok()
            .expect("one claim");
        assert!(pinned.pinned, "the merge applied the intended pin");
        let pin = pinned.pin.as_ref().unwrap();
        assert_eq!(pin.kind, PinKind::Numerical);
        assert_eq!(pin.artifact_ref.as_deref(), Some("jobs/7/stdout"));
        assert_eq!(pin.digest, excerpt_digest(content));
        assert_eq!(pin.confidence, 0.5);
        assert_eq!(pin.assessing_model, "GLM-5.3");
        assert_eq!(pin.seq, merge.seq, "the merged pin carries the merge's seq");
        assert_eq!(pin.ts, merge.ts);
        // the intended event itself never landed — only the proposal + the
        // approval exist in the log
        assert!(
            !store
                .events_all()
                .unwrap()
                .iter()
                .any(|e| e.kind == EVIDENCE_PINNED),
            "no evidence.pinned event was appended — the fold applied the intent"
        );
    }

    /// AD-5 on read for merged intents: a hand-merge of a TAMPERED intended
    /// payload (digest ≠ its own excerpt) fails the fold loudly — the same
    /// guarantee the direct pin events carry, extended to the proposal path
    /// (the approve() path refuses it first with digest_mismatch:).
    #[test]
    fn a_merged_tampered_pin_intent_fails_the_fold_loudly() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let h = seed_hypothesis(&store);
        let claim = store
            .append(NewEvent::claim_registered("Claim.", h, None).unwrap())
            .unwrap();
        let digest_of_other_content = excerpt_digest("entirely different numbers");
        let tampered = json!({
            "claim_id": claim.id.to_string(),
            "hypothesis_id": h.to_string(),
            "kind": "numerical",
            "artifact_ref": "jobs/7/stdout",
            "excerpt": "accuracy: 0.912, n=5",
            "digest": digest_of_other_content,
            "confidence": 0.5,
            "assessing_model": "GLM-5.3",
        });
        let intended = NewEvent::new(EVIDENCE_PINNED, Actor::User, tampered).unwrap();
        let proposal = store
            .append(
                NewEvent::proposal_created("fetch-7", &intended, h, claim.seq + 1, claim.id)
                    .unwrap(),
            )
            .unwrap();
        store
            .append(NewEvent::merge_approved(proposal.id, true, false).unwrap())
            .unwrap();
        let err = EvidenceProjection::fold(&store.events_all().unwrap())
            .expect_err("a tampered merged intent must fail the fold");
        assert!(err.to_string().contains("tampered"), "unexpected: {err}");
    }

    /// A pin proposal whose claim never registered (a ghost anchor) pins
    /// nothing when merged — skipped, not fatal, mirroring direct pins.
    #[test]
    fn a_merged_pin_intent_for_a_ghost_claim_pins_nothing() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let h = seed_hypothesis(&store);
        let intended = NewEvent::evidence_pinned_numerical(
            Uuid::new_v4(), // no such claim
            h,
            "jobs/7/stdout",
            "values",
            0.5,
            "GLM-5.3",
        )
        .unwrap();
        let proposal = store
            .append(
                NewEvent::proposal_created("fetch-7", &intended, h, 2, h).unwrap(),
            )
            .unwrap();
        store
            .append(NewEvent::merge_approved(proposal.id, false, false).unwrap())
            .unwrap();
        assert!(EvidenceProjection::fold(&store.events_all().unwrap())
            .unwrap()
            .is_empty());
    }
}
