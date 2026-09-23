// Claim-support verification domain (Story 6.9, FR-23.1/23.2, NFR-3
// extended): the THIRD signal on a pin. Existence verification (Story 4.2,
// `evidence.verified`, actor system/verifier) is code re-reading the source;
// confidence (FR-3.6) is the pin's assessing model's own judgment; SUPPORT is
// an entailment-style faithfulness check — a DIFFERENT model judging whether
// the pinned excerpt actually supports the claim it is pinned to. Three
// signals, never conflated (the FR-3.6 amendment): each renders its own chip,
// each carries its own attribution, and none ever drives another's label.
//
// SANCTIONED LLM USE — the deliberate counterpart to Story 4.2's NO-LLM
// rule: the existence verifier never touches the provider layer (fetching is
// data work), but judging whether a source supports a claim IS language
// work, so the support engine runs through the provider layer (AD-9) and
// records its judge: the event carries the judging model, and the actor is
// the closed SystemComponent::Support — an LLM judgment with a name, never
// "verification by code".
//
// THE DIFFERENT-MODEL TEST (NFR-3 extended — never the same model grading
// its own pin): the typed constructor takes BOTH the judging model and the
// pin's assessing model and REFUSES when they match, so a same-model support
// event cannot come into being through any construction path. The fold
// re-checks the same guarantee on read against the pin the event names.
//
// Honesty, not amnesia (NFR-8): an unsupported verdict marks the pin
// visibly and the pin STAYS — nothing is deleted, the claim stays anchored,
// the reader sees that the citing source does not hold the claim up.
// Re-checking appends again and the latest event for the current pin wins;
// a result that predates a re-pin reads `stale` (re-checkable), never a
// silent carry-over.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::eventstore::{Actor, EventError, NewEvent, SystemComponent};

pub const PIN_SUPPORT_CHECKED: &str = "pin.support_checked";

/// The sweep step name (Story 6.10): the Night Shift pass that batch-checks
/// pins — `run.started` events with this step are the sweep receipts the
/// readiness gate counts ("unverified support after N sweeps").
pub const SUPPORT_SWEEP_STEP: &str = "support-sweep";

/// How many support sweeps must have passed a pin UNCHECKED before the
/// readiness gate surfaces it as an info row (Story 6.10, FR-23.3) — the
/// "unverified support after N sweeps" honesty rule: a pin never judged
/// (no different model available, unreadable replies, refusals) is visible
/// as to-verify, never silently assumed fresh. Sweeps count only AFTER the
/// pin landed (the chances it had to be judged).
pub const SUPPORT_SWEEP_THRESHOLD: usize = 3;

/// The judge's role tag on spend events (AD-10): the support engine's LLM
/// calls are role-tagged so receipts name what ran.
pub const SUPPORT_JUDGE_ROLE: &str = "support";

/// The support verdict vocabulary (FR-23.1): `supported` = the pinned source
/// entails the claim; `partially` = the source supports a weaker assertion
/// than the claim makes; `unsupported` = the source does not hold the claim
/// up; `unverifiable` = the judge cannot determine from the pinned excerpt
/// (never a silent pass — an honest "cannot say"). The read model derives a
/// fifth display state — `stale` — from event order; the event itself only
/// ever carries these four.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportVerdict {
    Supported,
    Partially,
    Unsupported,
    Unverifiable,
}

impl SupportVerdict {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "supported" => Some(Self::Supported),
            "partially" => Some(Self::Partially),
            "unsupported" => Some(Self::Unsupported),
            "unverifiable" => Some(Self::Unverifiable),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Partially => "partially",
            Self::Unsupported => "unsupported",
            Self::Unverifiable => "unverifiable",
        }
    }
}

/// The `pin.support_checked` payload (AD-15 shape, mirroring
/// `evidence.verified`): the pin it reports on (claim + hypothesis + the pin
/// event's `seq` — the identity the fold matches against the claim's CURRENT
/// pin, so a result never silently applies to a re-pinned excerpt), the
/// verdict, the judge's own confidence in its verdict, and the judging model
/// — attribution: a support result is an LLM judgment with a name, never
/// "verified".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PinSupportCheckedPayload {
    pub claim_id: Uuid,
    pub hypothesis_id: Uuid,
    /// The `seq` of the `evidence.pinned` event (or the applying merge) this
    /// result reports on.
    pub pin_seq: i64,
    pub verdict: SupportVerdict,
    /// The judging model's confidence in its verdict, [0.0, 1.0].
    pub confidence: f64,
    /// The model that judged — recorded on the event, rendered with the
    /// result (FR-23.2). Structurally distinct from the pin's
    /// `assessing_model` (see the different-model test below).
    pub judging_model: String,
}

/// The different-model test (NFR-3 extended): does the judging model differ
/// from the pin's assessing model? Trimmed, case-insensitive comparison —
/// `GLM-5.3` vs `glm-5.3` is the same model grading its own pin, refused.
pub fn models_differ(assessing: &str, judging: &str) -> bool {
    let a = assessing.trim().to_lowercase();
    let j = judging.trim().to_lowercase();
    !a.is_empty() && !j.is_empty() && a != j
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `pin.support_checked` event
    /// comes into being. Actor is the system support engine — an LLM
    /// judgment, attributed to the judging model the payload carries. The
    /// event is cause-linked to the claim and the hypothesis (AD-2).
    ///
    /// THE DIFFERENT-MODEL TEST (NFR-3 extended): takes the pin's assessing
    /// model as a validation parameter (it already lives on the pin event —
    /// the payload never needs to repeat it) and REFUSES when the judging
    /// model matches it, so no construction path can grade a pin with the
    /// model that pinned it. Validates: pin_seq > 0, confidence in [0, 1],
    /// judging and assessing models non-empty.
    pub fn pin_support_checked(
        claim_id: Uuid,
        hypothesis_id: Uuid,
        pin_seq: i64,
        verdict: SupportVerdict,
        confidence: f64,
        judging_model: impl AsRef<str>,
        assessing_model: impl AsRef<str>,
    ) -> Result<Self, EventError> {
        if pin_seq <= 0 {
            return Err(EventError::Invalid(format!(
                "invalid pin_seq `{pin_seq}` — a support result names the pin event it reports on"
            )));
        }
        let judging = judging_model.as_ref().trim();
        if judging.is_empty() {
            return Err(EventError::Invalid(
                "support.judging_model must not be empty — a support verdict is attributed to \
                 its judging model, never anonymous (FR-23.2)"
                    .into(),
            ));
        }
        let assessing = assessing_model.as_ref().trim();
        if assessing.is_empty() {
            return Err(EventError::Invalid(
                "support.assessing_model must not be empty — every pin names the model that \
                 assessed its confidence (FR-3.6)"
                    .into(),
            ));
        }
        if !models_differ(assessing, judging) {
            return Err(EventError::Invalid(format!(
                "different-model test failed (NFR-3 extended): the support judge `{judging}` is \
                 the pin's own assessing model — a support verdict is never rendered by the \
                 model that pinned the claim"
            )));
        }
        if !(0.0..=1.0).contains(&confidence) || confidence.is_nan() {
            return Err(EventError::Invalid(format!(
                "invalid confidence `{confidence}` — the judge's confidence is a number in \
                 [0.0, 1.0]"
            )));
        }
        let payload = PinSupportCheckedPayload {
            claim_id,
            hypothesis_id,
            pin_seq,
            verdict,
            confidence,
            judging_model: judging.to_string(),
        };
        Ok(Self::new(
            PIN_SUPPORT_CHECKED,
            Actor::System {
                component: SystemComponent::Support,
            },
            serde_json::to_value(&payload)?,
        )?
        .with_causes(vec![claim_id, hypothesis_id]))
    }
}

/// The LATEST support check of one pin (the read model's display
/// vocabulary): the event's verdict, or `stale` when the result predates the
/// current pin (fold-derived, re-checkable — never a silent carry-over).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportStatus {
    Supported,
    Partially,
    Unsupported,
    Unverifiable,
    /// Fold-derived: the result predates the current pin — re-checkable.
    Stale,
}

impl SupportStatus {
    pub fn from_verdict(verdict: SupportVerdict) -> Self {
        match verdict {
            SupportVerdict::Supported => Self::Supported,
            SupportVerdict::Partially => Self::Partially,
            SupportVerdict::Unsupported => Self::Unsupported,
            SupportVerdict::Unverifiable => Self::Unverifiable,
        }
    }
}

/// One pin's latest support check as read from the log (the THIRD signal,
/// FR-23.2): the verdict, the judge's confidence, the judging model — an LLM
/// judgment with a name, never confusable with the pin's agent-assessed
/// confidence (its own model's) or the machine verification (existence by
/// code). `ts` keeps the check visibly dated — never silently assumed fresh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinSupportCheck {
    /// supported | partially | unsupported | unverifiable | stale (display
    /// vocabulary; the event carries only the first four).
    pub status: SupportStatus,
    /// The judge's confidence in its verdict, [0.0, 1.0].
    pub confidence: f64,
    /// The model that judged — rendered mono with the result.
    pub judging_model: String,
    /// When the check landed — visibly dated (FR-23.2).
    pub ts: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventstore::Actor;
    use serde_json::json;

    fn case() -> (Uuid, Uuid) {
        (Uuid::new_v4(), Uuid::new_v4())
    }

    // ---------- constructor ----------

    #[test]
    fn the_constructor_stamps_the_system_support_actor_and_links_the_claim() {
        let (claim, hyp) = case();
        let ev = NewEvent::pin_support_checked(
            claim,
            hyp,
            42,
            SupportVerdict::Supported,
            0.9,
            "claude-sonnet-4-5",
            "GLM-5.3",
        )
        .unwrap();
        assert_eq!(ev.kind, PIN_SUPPORT_CHECKED);
        assert_eq!(
            ev.actor,
            Actor::System {
                component: SystemComponent::Support
            }
        );
        assert_eq!(ev.causes, vec![claim, hyp]);
        assert_eq!(
            ev.payload,
            json!({
                "claim_id": claim.to_string(),
                "hypothesis_id": hyp.to_string(),
                "pin_seq": 42,
                "verdict": "supported",
                "confidence": 0.9,
                "judging_model": "claude-sonnet-4-5",
            })
        );
    }

    #[test]
    fn the_constructor_rejects_a_pinless_result_blank_model_and_bad_confidence() {
        let (claim, hyp) = case();
        // pin_seq 0 names no pin event
        let err = NewEvent::pin_support_checked(
            claim,
            hyp,
            0,
            SupportVerdict::Unsupported,
            0.5,
            "judge-model",
            "GLM-5.3",
        )
        .expect_err("pin_seq 0 names no pin event");
        assert!(err.to_string().contains("pin_seq"), "unexpected: {err}");
        // blank judging model — a verdict is never anonymous
        let err = NewEvent::pin_support_checked(
            claim,
            hyp,
            7,
            SupportVerdict::Unsupported,
            0.5,
            "   ",
            "GLM-5.3",
        )
        .expect_err("a blank judging model must fail construction");
        assert!(err.to_string().contains("judging_model"), "unexpected: {err}");
        // blank assessing model — the different-model test cannot run blind
        let err = NewEvent::pin_support_checked(
            claim,
            hyp,
            7,
            SupportVerdict::Unsupported,
            0.5,
            "judge-model",
            "  ",
        )
        .expect_err("a blank assessing model must fail construction");
        assert!(err.to_string().contains("assessing_model"), "unexpected: {err}");
        // confidence bounds: below 0, above 1, NaN — all rejected; 0.0 and
        // 1.0 are inclusive and construct fine
        for bad in [-0.01, 1.01, f64::NAN] {
            let err = NewEvent::pin_support_checked(
                claim,
                hyp,
                7,
                SupportVerdict::Supported,
                bad,
                "judge-model",
                "GLM-5.3",
            )
            .expect_err("out-of-range confidence must fail construction");
            assert!(err.to_string().contains("confidence"), "unexpected: {err}");
        }
        for ok in [0.0, 1.0] {
            NewEvent::pin_support_checked(
                claim,
                hyp,
                7,
                SupportVerdict::Supported,
                ok,
                "judge-model",
                "GLM-5.3",
            )
            .expect("inclusive bounds must construct");
        }
    }

    /// THE DIFFERENT-MODEL TEST at construction (NFR-3 extended): the model
    /// that assessed the pin's confidence can never render its own support
    /// verdict — exact match, and case/whitespace variants of the same
    /// model, are all refused; a different model constructs fine.
    #[test]
    fn the_constructor_refuses_the_pins_own_assessing_model() {
        let (claim, hyp) = case();
        for (judging, assessing) in [
            ("GLM-5.3", "GLM-5.3"),
            ("glm-5.3", "GLM-5.3"),
            (" GLM-5.3 ", "glm-5.3"),
        ] {
            let err = NewEvent::pin_support_checked(
                claim,
                hyp,
                7,
                SupportVerdict::Supported,
                0.9,
                judging,
                assessing,
            )
            .expect_err("the pin's own assessing model must never judge its support");
            assert!(
                err.to_string().contains("different-model test failed"),
                "unexpected: {err}"
            );
        }
        // a genuinely different model constructs fine
        NewEvent::pin_support_checked(
            claim,
            hyp,
            7,
            SupportVerdict::Supported,
            0.9,
            "claude-sonnet-4-5",
            "GLM-5.3",
        )
        .expect("a different judging model constructs");
    }

    // ---------- the verdict vocabulary ----------

    #[test]
    fn the_verdict_vocabulary_is_closed_and_case_insensitive() {
        for (raw, verdict) in [
            ("supported", SupportVerdict::Supported),
            ("partially", SupportVerdict::Partially),
            ("unsupported", SupportVerdict::Unsupported),
            ("unverifiable", SupportVerdict::Unverifiable),
            ("  Unsupported\n", SupportVerdict::Unsupported),
        ] {
            assert_eq!(SupportVerdict::parse(raw), Some(verdict), "raw: {raw:?}");
            assert_eq!(verdict.as_str(), verdict.as_str());
        }
        for bad in ["", "maybe", "sorta", "verdict: supported", "supported?"] {
            assert_eq!(SupportVerdict::parse(bad), None, "raw: {bad:?}");
        }
        // serde roundtrip: the closed vocabulary on the wire
        for verdict in [
            SupportVerdict::Supported,
            SupportVerdict::Partially,
            SupportVerdict::Unsupported,
            SupportVerdict::Unverifiable,
        ] {
            let s = serde_json::to_string(&verdict).unwrap();
            assert_eq!(serde_json::from_str::<SupportVerdict>(&s).unwrap(), verdict);
        }
    }

    /// models_differ: the pure different-model comparison — empty never
    /// differs (an unnamed model cannot pass), case/whitespace variants are
    /// the same model.
    #[test]
    fn models_differ_compares_trimmed_case_insensitively() {
        assert!(models_differ("GLM-5.3", "claude-sonnet-4-5"));
        assert!(models_differ("a", "b"));
        assert!(!models_differ("GLM-5.3", "glm-5.3"));
        assert!(!models_differ("GLM-5.3", " GLM-5.3 "));
        assert!(!models_differ("", "claude-sonnet-4-5"));
        assert!(!models_differ("GLM-5.3", ""));
    }
}
