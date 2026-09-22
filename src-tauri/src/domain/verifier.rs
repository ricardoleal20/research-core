// Non-LLM citation verifier (Epic 4, Story 4.2, FR-14.1, AD-5, AD-15):
// "pinned means real, verified by code, not by a model's word". The verifier
// re-fetches each pinned source (DOI/arXiv/URL for citations; the local
// artifact or the job stdout ref for numerical pins) with NO LLM call —
// fetching is data work and lives here, outside the provider layer (AD-9
// governs LLM calls only). A citation pin verifies when its pinned excerpt
// appears in the fetched text (whitespace-normalized containment); a
// numerical pin verifies when the sha-256 digest recomputes over the
// re-read artifact (FR-14.1, AD-5).
//
// Outcomes land as `evidence.verified` events (AD-15, actor =
// system/verifier — the closed SystemComponent::Verifier). FAILURES ARE
// VISIBLE, NEVER DELETING: a failed verification marks the pin and the pin
// stays in place — honesty, not amnesia. Re-verification appends again and
// the latest event wins (a transient fetch error that later succeeds flips
// failed→verified; a verified pin is never auto-unverified — only a new
// event, or a re-pin that changes the pinned content, can change what
// renders).
//
// Verification is a SEPARATE axis from the agent-assessed confidence
// (FR-3.6): confidence is a named model's judgment of support; verification
// is existence by code. "verified" never drives the confidence label.

use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::eventstore::{Actor, EventError, NewEvent, SystemComponent};

pub const EVIDENCE_VERIFIED: &str = "evidence.verified";

/// The verification outcome vocabulary (AD-15): `verified` = the code
/// confirmed the pinned content against the re-fetched source; `failed` =
// it could not (excerpt absent, digest mismatch, fetch error, no source).
/// The read model derives a third display state — stale — from event order;
/// the event itself only ever carries these two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationOutcome {
    Verified,
    Failed,
}

impl VerificationOutcome {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "verified" => Some(Self::Verified),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Failed => "failed",
        }
    }
}

// Machine detail codes — code form, never translated (bilingual-safe,
// EXPERIENCE.md). The UI renders the code plus its own localized gloss.
pub const DETAIL_EXCERPT_MATCHED: &str = "excerpt_matched";
pub const DETAIL_DIGEST_OK: &str = "digest_ok";
pub const DETAIL_NOT_FOUND: &str = "not_found";
pub const DETAIL_ARTIFACT_CHANGED: &str = "artifact_changed";
pub const DETAIL_ARTIFACT_MISSING: &str = "artifact_missing";
pub const DETAIL_FETCH_ERROR: &str = "fetch_error";
pub const DETAIL_NO_SOURCE: &str = "no_source";

/// The `evidence.verified` payload (AD-15 shape): the pin it reports on
/// (claim + hypothesis + the pin event's `seq` — the identity the fold
/// matches against the claim's CURRENT pin, so a result never silently
/// applies to a re-pinned excerpt), the outcome, a machine detail code
/// naming WHY, and the source that was consulted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceVerifiedPayload {
    pub claim_id: Uuid,
    pub hypothesis_id: Uuid,
    /// The `seq` of the `evidence.pinned` event (or the applying merge) this
    /// result reports on.
    pub pin_seq: i64,
    pub outcome: VerificationOutcome,
    /// Machine detail code: excerpt_matched / digest_ok / not_found /
    /// artifact_changed / artifact_missing / fetch_error / no_source.
    pub detail: String,
    /// What was consulted: `arxiv:<id>`, a URL, or an artifact ref.
    pub source: String,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way an `evidence.verified` event
    /// comes into being. Actor is the system verifier — NEVER an LLM call
    /// (the whole point of Story 4.2): the verifier is code, and its
    /// attribution is the component, not a model. The event is cause-linked
    /// to the claim and the hypothesis (AD-2). Validates: pin_seq > 0,
    /// detail and source non-empty, outcome from the closed vocabulary.
    pub fn evidence_verified(
        claim_id: Uuid,
        hypothesis_id: Uuid,
        pin_seq: i64,
        outcome: VerificationOutcome,
        detail: impl AsRef<str>,
        source: impl AsRef<str>,
    ) -> Result<Self, EventError> {
        if pin_seq <= 0 {
            return Err(EventError::Invalid(format!(
                "invalid pin_seq `{pin_seq}` — a verification result names the pin event it \
                 reports on"
            )));
        }
        let detail = detail.as_ref().trim();
        if detail.is_empty() {
            return Err(EventError::Invalid(
                "evidence.detail must not be empty — a verification result says why".into(),
            ));
        }
        let source = source.as_ref().trim();
        if source.is_empty() {
            return Err(EventError::Invalid(
                "evidence.source must not be empty — a verification result names what it \
                 consulted"
                    .into(),
            ));
        }
        let payload = EvidenceVerifiedPayload {
            claim_id,
            hypothesis_id,
            pin_seq,
            outcome,
            detail: detail.to_string(),
            source: source.to_string(),
        };
        Ok(Self::new(
            EVIDENCE_VERIFIED,
            Actor::System {
                component: SystemComponent::Verifier,
            },
            serde_json::to_value(&payload)?,
        )?
        .with_causes(vec![claim_id, hypothesis_id]))
    }
}

/// Where a pin's content can be re-read from — resolved by the shell (the
/// refs table for citations, the `artifact_ref` grammar for numerical pins)
/// and consumed by the fetcher. The closed v1 vocabulary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PinSource {
    /// An arXiv paper by id — the fetched content is the title + abstract
    /// the public export API returns (the same adapter onboarding uses).
    Arxiv { id: String },
    /// A plain URL — the ref's own URL, or `https://doi.org/<doi>` for a
    /// DOI-only ref (redirects followed).
    Url { url: String },
    /// A local file path (numerical artifacts: files/figures/tables).
    File { path: String },
    /// A compute job's stdout — the `jobs/<id>/stdout` artifact ref; the
    /// verifier re-fetches the job's results through the target adapter.
    JobStdout { job_id: Uuid },
}

/// The future a source fetch returns (the `ProviderClient` seam's shape —
/// Send, so the async shell can await it inside a Tauri command).
pub type FetchResult = Result<String, String>;
pub type FetchFuture<'a> = Pin<Box<dyn Future<Output = FetchResult> + Send + 'a>>;

/// The fetch seam: how the verifier re-reads a pin's source. The real
/// implementation is HTTP/fs/adapter work in the shell; tests seed a
/// fetchable corpus. This seam is deliberately NOT the provider layer —
/// there is no path from here to an LLM call, and the NO-LLM invariant test
/// armed-proves it.
pub trait PinSourceFetcher: Send + Sync {
    /// Re-read one source. The future borrows both the fetcher and the
    /// source (the `ProviderClient` seam's shape, extended to a borrowed
    /// source).
    fn fetch<'a>(&'a self, source: &'a PinSource) -> FetchFuture<'a>;
}

/// Whitespace-normalized matching, the documented citation check: both the
/// pinned excerpt and the fetched text are collapsed to single-space words
/// before containment, so a line-wrapped abstract or a reflowed paragraph
/// still matches its excerpt. Everything else must match exactly.
pub fn excerpt_appears(excerpt: &str, fetched: &str) -> bool {
    normalize_ws(fetched).contains(&normalize_ws(excerpt))
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The citation check (FR-14.1): the pinned excerpt must appear in the
/// fetched source text. `Ok` = the source fetched; `Err` = the fetch itself
/// failed (network error, unreachable DOI, vanished paper) — an honest
/// `fetch_error`, never a silent pass.
pub fn verify_citation(excerpt: &str, fetched: &FetchResult) -> (VerificationOutcome, &'static str) {
    match fetched {
        Ok(text) if excerpt_appears(excerpt, text) => {
            (VerificationOutcome::Verified, DETAIL_EXCERPT_MATCHED)
        }
        Ok(_) => (VerificationOutcome::Failed, DETAIL_NOT_FOUND),
        Err(_) => (VerificationOutcome::Failed, DETAIL_FETCH_ERROR),
    }
}

/// The numerical check (FR-14.1, AD-5): re-read the artifact and recompute
/// the sha-256 digest — `digest_ok` when the artifact's whole content still
/// hashes to the pin's digest (the artifact IS the pinned content, e.g. a
/// job's stdout), `excerpt_matched` when the pinned values still appear in
/// a larger artifact. A readable artifact whose content no longer matches
/// reads `artifact_changed`; an unreadable/absent one reads
/// `artifact_missing`.
pub fn verify_numerical(
    excerpt: &str,
    digest: &str,
    artifact: &FetchResult,
) -> (VerificationOutcome, &'static str) {
    match artifact {
        Ok(content) => {
            if crate::domain::evidence::excerpt_digest(content) == digest {
                (VerificationOutcome::Verified, DETAIL_DIGEST_OK)
            } else if excerpt_appears(excerpt, content) {
                (VerificationOutcome::Verified, DETAIL_EXCERPT_MATCHED)
            } else {
                (VerificationOutcome::Failed, DETAIL_ARTIFACT_CHANGED)
            }
        }
        Err(_) => (VerificationOutcome::Failed, DETAIL_ARTIFACT_MISSING),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::evidence::excerpt_digest;
    use serde_json::json;

    // ---------- constructor ----------

    #[test]
    fn the_constructor_stamps_the_system_verifier_and_links_the_claim() {
        let (claim, hyp) = (Uuid::new_v4(), Uuid::new_v4());
        let ev = NewEvent::evidence_verified(
            claim,
            hyp,
            42,
            VerificationOutcome::Verified,
            DETAIL_EXCERPT_MATCHED,
            "arxiv:1706.03762",
        )
        .unwrap();
        assert_eq!(ev.kind, "evidence.verified");
        assert_eq!(
            ev.actor,
            Actor::System {
                component: SystemComponent::Verifier
            }
        );
        assert_eq!(ev.causes, vec![claim, hyp]);
        assert_eq!(
            ev.payload,
            json!({
                "claim_id": claim.to_string(),
                "hypothesis_id": hyp.to_string(),
                "pin_seq": 42,
                "outcome": "verified",
                "detail": "excerpt_matched",
                "source": "arxiv:1706.03762",
            })
        );
    }

    #[test]
    fn the_constructor_rejects_a_pinless_result_and_blank_why_or_source() {
        let (claim, hyp) = (Uuid::new_v4(), Uuid::new_v4());
        let err = NewEvent::evidence_verified(
            claim,
            hyp,
            0,
            VerificationOutcome::Failed,
            DETAIL_NOT_FOUND,
            "url",
        )
        .expect_err("pin_seq 0 names no pin event");
        assert!(err.to_string().contains("pin_seq"), "unexpected: {err}");
        for (detail, source) in [("  ", "url"), ("not_found", "  ")] {
            let err = NewEvent::evidence_verified(
                claim,
                hyp,
                7,
                VerificationOutcome::Failed,
                detail,
                source,
            )
            .expect_err("a blank detail or source must fail construction");
            assert!(
                err.to_string().contains("must not be empty"),
                "unexpected: {err}"
            );
        }
    }

    // ---------- the checks ----------

    #[test]
    fn a_citation_verifies_when_the_fetched_source_contains_the_excerpt() {
        let excerpt = "Attention dispenses with recurrence entirely.";
        let fetched = Ok(format!(
            "The dominant sequence transduction models are based on recurrent networks.\n\
             Attention dispenses with   recurrence entirely.\nEnd of abstract."
        ));
        // Whitespace-normalized: the line wraps and doubled spaces in the
        // fetched text still match the excerpt.
        let (outcome, detail) = verify_citation(excerpt, &fetched);
        assert_eq!(outcome, VerificationOutcome::Verified);
        assert_eq!(detail, DETAIL_EXCERPT_MATCHED);
        // The excerpt is absent → not_found, never a silent pass.
        let (outcome, detail) = verify_citation("A passage the paper never contained.", &fetched);
        assert_eq!(outcome, VerificationOutcome::Failed);
        assert_eq!(detail, DETAIL_NOT_FOUND);
        // The fetch itself failed → fetch_error.
        let (outcome, detail) = verify_citation(excerpt, &Err("timeout".into()));
        assert_eq!(outcome, VerificationOutcome::Failed);
        assert_eq!(detail, DETAIL_FETCH_ERROR);
    }

    #[test]
    fn a_numerical_pin_verifies_when_the_digest_recomputes_and_fails_when_changed() {
        let content = "accuracy: 0.912, ±0.006, n=5 seeds";
        let digest = excerpt_digest(content);
        // The artifact is exactly the pinned content → the digest
        // recomputes.
        let (outcome, detail) = verify_numerical(content, &digest, &Ok(content.to_string()));
        assert_eq!(outcome, VerificationOutcome::Verified);
        assert_eq!(detail, DETAIL_DIGEST_OK);
        // A larger artifact still containing the values → excerpt_matched.
        let larger = format!("header row\n{content}\nfooter row");
        let (outcome, detail) = verify_numerical(content, &digest, &Ok(larger));
        assert_eq!(outcome, VerificationOutcome::Verified);
        assert_eq!(detail, DETAIL_EXCERPT_MATCHED);
        // The artifact changed — the pinned values are gone → failed.
        let (outcome, detail) =
            verify_numerical(content, &digest, &Ok("accuracy: 0.901, n=3 seeds".into()));
        assert_eq!(outcome, VerificationOutcome::Failed);
        assert_eq!(detail, DETAIL_ARTIFACT_CHANGED);
        // The artifact is unreadable/absent → artifact_missing.
        let (outcome, detail) = verify_numerical(content, &digest, &Err("no such file".into()));
        assert_eq!(outcome, VerificationOutcome::Failed);
        assert_eq!(detail, DETAIL_ARTIFACT_MISSING);
    }
}
