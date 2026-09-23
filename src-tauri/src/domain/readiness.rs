// Readiness gate (FR-13, Story 4.3 — the FINAL story of the PRD): the
// preprint-tier gate, DERIVED from board state (FR-13.1) with NO new data
// model anywhere (FR-13.2, AD-1) — this module adds not one event, not one
// table; it reads the SAME folds the board renders and answers one question:
// what, specifically, blocks this board from being preprint-ready?
//
// No scores, no percentages (FR-13.1, EXPERIENCE.md Flow 2): every blocking
// item references the SPECIFIC board object that blocks it — the claim
// (CLAIMS-{seq}), the hypothesis (H-{seq}, with its lifecycle status and the
// ties that make it load-bearing), the null-result search (search log #{seq}).
// A clean board reports `ready` — displayed "preprint-ready" — with the
// evidence trail that justifies it: what was checked, the counts, and the
// objects that satisfied each check.
//
// Blocker categories (each derived, each referencing its object):
//
// 1. UNPINNED CLAIMS — every claim with no `evidence.pinned` event (the
//    1.7 fold's `pinned: false`): the preprint would cite an assertion with
//    nothing behind it.
//
// 2. LOAD-BEARING UNRESOLVED / REFUTED HYPOTHESES — a hypothesis that things
//    rest on (claims registered to it, or relations naming it an endpoint)
//    and that is still `proposed`/`testing` (unresolved — the things resting
//    on it rest on an open question), or `refuted` (the things resting on it
//    rest on a dead limb). A supported hypothesis never blocks; an unresolved
//    hypothesis with NOTHING resting on it never blocks (nothing in the
//    preprint depends on it).
//
// 3. UNRECKONED NULL RESULTS — v1's search protocol makes a null result
//    DISCLOSED the moment it happens: the `search.run` event IS the
//    disclosure, the log cannot be skipped (FR-12.1), and the 4.1 fold emits
//    one row per search, nulls visibly marked. So nothing can be "hidden"
//    — the honesty gap the gate CAN derive is the UNRECKONED null: a
//    null-result search (search log #{seq}) that ran for a mission whose
//    board still carries unresolved hypotheses. The empty result is on the
//    record, but the research it searched for hasn't answered it — the null
//    must drive a resolution (refute on the strength of it, or support by
//    other means) before the board defends a preprint. The blocker cites the
//    search log row (the disclosure) AND clears the moment the mission's
//    hypotheses resolve. Workspace-wide null searches (no mission) never
//    block: no board object rests on them.
//
// Info rows (advisory, NEVER blockers — decided and documented):
//
// - PIN VERIFICATION FAILED (4.2): a PINNED claim whose latest machine
//   verification failed does NOT block readiness — the pin exists, the claim
//   is anchored, and the failed check is already visible on the pin (honesty,
//   not amnesia). The report flags it as info ("pinned but unsupported by
//   verified evidence", citing the claim) so the "verified" distinction stays
//   honest: verification is existence by code, and a failed check is a fact
//   to see, not a gate to smuggle in through the back door.
// - MERGE QUEUE PENDING: pending quarantine proposals are NOT board state
//   (AD-3 — excluded from every projection until merged), so they cannot
//   block a report derived FROM board state; the report surfaces the count
//   as info so a non-empty queue is never a surprise.
//
// The three-state presentation (not ready / near / ready) derives on the
// read side: `not_ready` = ≥1 blocker; `near` = 0 blockers but ≥1 info row;
// `ready` = neither — states, never scores.
//
// The gate respects the shared FoldCursor (AD-1, Story 2.6): readiness after
// a rollback reads the same live view the board renders — an orphaned pin or
// claim never happened for this report.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::checkpoints::FoldCursor;
use crate::domain::evidence::{Claim, EvidenceProjection, VerificationStatus};
use crate::domain::hypotheses::{
    Hypothesis, HypothesesProjection, HypothesisStatus, RelationChip, RelationDirection,
    RelationKind,
};
use crate::domain::manuscript::{self, ManuscriptFlag, ManuscriptFlagKind, ManuscriptScan};
use crate::domain::nightshift::RUN_STARTED;
use crate::domain::proposals::{ProposalStatus, ProposalsProjection};
use crate::domain::search::search_disclosure;
use crate::domain::support::{
    SupportStatus, SUPPORT_SWEEP_STEP, SUPPORT_SWEEP_THRESHOLD,
};
use crate::eventstore::{EventError, StoredEvent};

/// The gate's verdict (FR-13.1): `ready` renders "preprint-ready"; anything
/// else is `not_ready` — derived, never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessVerdict {
    Ready,
    NotReady,
}

/// One blocking or advisory item. Every item references EXACTLY ONE specific
/// board object (FR-13.1): the claim, the hypothesis, or the search log row
/// — the fields below are Option'd by kind, exactly one reference family is
/// set per kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessItemKind {
    /// A claim with no evidence.pinned event (1.7's `pinned: false`).
    UnpinnedClaim,
    /// A hypothesis things rest on, still proposed/testing.
    LoadBearingUnresolved,
    /// A hypothesis things rest on, refuted.
    LoadBearingRefuted,
    /// A null-result search whose mission still carries unresolved
    /// hypotheses — on the record, not yet answered (info: the search log
    /// row IS the disclosure; the reckoning is the board's).
    UnreckonedNullResult,
    /// INFO: a pinned claim whose latest machine verification failed —
    /// never a blocker (see the module doc).
    PinVerificationFailed,
    /// INFO (Story 6.10, FR-23.3): a pinned claim whose SUPPORT check
    /// returned `unsupported` — pinned but not held up by what it cites.
    /// Never a blocker: the pin exists, the claim is anchored; the verdict
    /// is already visible on the pin (honesty, not amnesia).
    PinUnsupported,
    /// INFO (Story 6.10, FR-23.3): a support verdict of `partially` — the
    /// claim asserts more than its citing source supports.
    PinPartiallySupported,
    /// INFO (Story 6.10, FR-23.3): support still unverified after N sweeps —
    /// a pinned claim the sweeps never judged (no different model
    /// available, unreadable replies, refusals), visible as to-verify,
    /// never silently assumed fresh.
    SupportUnchecked,
    /// INFO: pending quarantine proposals — not board state (AD-3), so not a
    /// blocker; surfaced so a non-empty queue is never a surprise.
    MergeQueuePending,
    // The manuscript scope (Story 6.8, FR-20.4/20.5): the paper cannot
    // quietly outrun the evidence — each flag references the specific
    // hypothesis card / claim AND the manuscript location (FR-13.1).
    /// A `\hyp`/`\claim` marker citing a hypothesis still proposed/testing.
    ManuscriptHypothesisUnresolved,
    /// A marker citing a refuted hypothesis.
    ManuscriptHypothesisRefuted,
    /// A `\claim` marker citing a board claim with no evidence pin.
    ManuscriptClaimUnpinned,
    /// A marker that resolves to no board object — an unlinked claim.
    ManuscriptClaimUnlinked,
    /// INFO: a registered manuscript whose directory could not be read —
    /// the consistency check could not run, and that is surfaced, never
    /// silently skipped.
    ManuscriptUnreadable,
}

/// One typed relation tie on a load-blocking hypothesis: the relation kind,
/// the other endpoint's `H-{n}` seq, and whether it reads incoming on this
/// hypothesis ("contradicted-by H-3") or outgoing ("contradicts H-3").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessRelationTie {
    pub kind: RelationKind,
    pub other_seq: i64,
    pub incoming: bool,
}

/// One blocking or advisory item (camelCase on the wire, AD-8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessItem {
    pub kind: ReadinessItemKind,
    /// The claim the item references (unpinned / verification-failed).
    pub claim_id: Option<Uuid>,
    /// The claim's registration seq — the CLAIMS-{n} chip.
    pub claim_seq: Option<i64>,
    /// The hypothesis the item references (load-bearing).
    pub hypothesis_id: Option<Uuid>,
    /// The hypothesis's creation seq — the H-{n} chip.
    pub hypothesis_seq: Option<i64>,
    pub hypothesis_status: Option<HypothesisStatus>,
    /// The search log row the item references (unreckoned null) — the
    /// search.run event's seq, the "#{n}" chip.
    pub search_seq: Option<i64>,
    pub mission_id: Option<Uuid>,
    /// What rests on a load-bearing hypothesis: the CLAIMS-{n} seqs of the
    /// claims registered to it.
    pub claim_ties: Vec<i64>,
    /// The typed relations naming it an endpoint.
    pub relation_ties: Vec<ReadinessRelationTie>,
    /// The merge-queue info row's pending count.
    pub pending_count: u32,
    // The manuscript scope (Story 6.8): the flag's manuscript location —
    // the repo-relative file, the 1-based line, and the raw marker.
    pub manuscript_file: Option<String>,
    pub manuscript_line: Option<u32>,
    pub marker: Option<String>,
}

impl ReadinessItem {
    fn unpinned_claim(claim: &Claim, hypothesis_seq: Option<i64>) -> Self {
        Self {
            kind: ReadinessItemKind::UnpinnedClaim,
            claim_id: Some(claim.id),
            claim_seq: Some(claim.seq),
            hypothesis_id: Some(claim.hypothesis_id),
            hypothesis_seq,
            hypothesis_status: None,
            search_seq: None,
            mission_id: None,
            claim_ties: Vec::new(),
            relation_ties: Vec::new(),
            pending_count: 0,
            manuscript_file: None,
            manuscript_line: None,
            marker: None,
        }
    }

    fn load_bearing(
        kind: ReadinessItemKind,
        hyp: &Hypothesis,
        claim_ties: Vec<i64>,
        relation_ties: Vec<ReadinessRelationTie>,
    ) -> Self {
        Self {
            kind,
            claim_id: None,
            claim_seq: None,
            hypothesis_id: Some(hyp.id),
            hypothesis_seq: Some(hyp.seq),
            hypothesis_status: Some(hyp.status),
            search_seq: None,
            mission_id: Some(hyp.mission_id),
            claim_ties,
            relation_ties,
            pending_count: 0,
            manuscript_file: None,
            manuscript_line: None,
            marker: None,
        }
    }

    fn unreckoned_null(search_seq: i64, mission_id: Option<Uuid>) -> Self {
        Self {
            kind: ReadinessItemKind::UnreckonedNullResult,
            claim_id: None,
            claim_seq: None,
            hypothesis_id: None,
            hypothesis_seq: None,
            hypothesis_status: None,
            search_seq: Some(search_seq),
            mission_id,
            claim_ties: Vec::new(),
            relation_ties: Vec::new(),
            pending_count: 0,
            manuscript_file: None,
            manuscript_line: None,
            marker: None,
        }
    }

    fn verification_failed(claim: &Claim, hypothesis_seq: Option<i64>) -> Self {
        Self {
            kind: ReadinessItemKind::PinVerificationFailed,
            claim_id: Some(claim.id),
            claim_seq: Some(claim.seq),
            hypothesis_id: Some(claim.hypothesis_id),
            hypothesis_seq,
            hypothesis_status: None,
            search_seq: None,
            mission_id: None,
            claim_ties: Vec::new(),
            relation_ties: Vec::new(),
            pending_count: 0,
            manuscript_file: None,
            manuscript_line: None,
            marker: None,
        }
    }

    /// The support-info constructor (Story 6.10): one pinned claim, one
    /// support-derived kind — every item references its CLAIMS-{n} chip.
    fn support_info(
        kind: ReadinessItemKind,
        claim: &Claim,
        hypothesis_seq: Option<i64>,
    ) -> Self {
        Self {
            kind,
            claim_id: Some(claim.id),
            claim_seq: Some(claim.seq),
            hypothesis_id: Some(claim.hypothesis_id),
            hypothesis_seq,
            hypothesis_status: None,
            search_seq: None,
            mission_id: None,
            claim_ties: Vec::new(),
            relation_ties: Vec::new(),
            pending_count: 0,
        }
    }

    fn merge_queue(pending_count: u32, mission_id: Option<Uuid>) -> Self {
        Self {
            kind: ReadinessItemKind::MergeQueuePending,
            claim_id: None,
            claim_seq: None,
            hypothesis_id: None,
            hypothesis_seq: None,
            hypothesis_status: None,
            search_seq: None,
            mission_id,
            claim_ties: Vec::new(),
            relation_ties: Vec::new(),
            pending_count,
            manuscript_file: None,
            manuscript_line: None,
            marker: None,
        }
    }

    /// One manuscript consistency flag (Story 6.8, FR-20.5): a blocker
    /// referencing the board object AND the manuscript location.
    fn manuscript(flag: &ManuscriptFlag) -> Self {
        Self {
            kind: match flag.kind {
                ManuscriptFlagKind::HypothesisUnresolved => {
                    ReadinessItemKind::ManuscriptHypothesisUnresolved
                }
                ManuscriptFlagKind::HypothesisRefuted => {
                    ReadinessItemKind::ManuscriptHypothesisRefuted
                }
                ManuscriptFlagKind::ClaimUnpinned => ReadinessItemKind::ManuscriptClaimUnpinned,
                ManuscriptFlagKind::ClaimUnlinked => ReadinessItemKind::ManuscriptClaimUnlinked,
            },
            claim_id: flag.claim_id,
            claim_seq: flag.claim_seq,
            hypothesis_id: flag.hypothesis_id,
            hypothesis_seq: flag.hypothesis_seq,
            hypothesis_status: flag.hypothesis_status,
            search_seq: None,
            mission_id: Some(flag.mission_id),
            claim_ties: Vec::new(),
            relation_ties: Vec::new(),
            pending_count: 0,
            manuscript_file: Some(flag.file.clone()),
            manuscript_line: Some(flag.line),
            marker: Some(flag.marker.clone()),
        }
    }

    /// INFO: a registered manuscript whose dir could not be read — the
    /// consistency check could not run; surfaced, never silently skipped.
    fn manuscript_unreadable(scan: &ManuscriptScan) -> Self {
        Self {
            kind: ReadinessItemKind::ManuscriptUnreadable,
            claim_id: None,
            claim_seq: None,
            hypothesis_id: None,
            hypothesis_seq: None,
            hypothesis_status: None,
            search_seq: None,
            mission_id: Some(scan.mission_id),
            claim_ties: Vec::new(),
            relation_ties: Vec::new(),
            pending_count: 0,
            manuscript_file: Some(scan.dir.clone()),
            manuscript_line: None,
            marker: None,
        }
    }
}

/// One evidence-trail row (the clean board's justification): what was
/// checked, the counts, and the specific objects that satisfied it — the
/// checklist of facts the researcher can defend line by line (EXPERIENCE.md
/// Flow 2). Four rows, matching the readiness frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessTrailKind {
    /// N/N claims pinned (and how many are machine-verified).
    ClaimsPinned,
    /// N/N hypotheses resolved (supported or refuted — none left open).
    HypothesesResolved,
    /// N/N null-result searches reckoned with by the board.
    NullsDisclosed,
    /// The merge queue: pending count (0 = empty), citing the last decided
    /// proposal when clean.
    MergeQueue,
    /// The manuscript scope (Story 6.8, FR-20.5): N/N board markers linked
    /// clean. Present ONLY when a manuscript is registered (progressive
    /// disclosure — the row never renders uninvited).
    ManuscriptConsistency,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessTrailRow {
    pub kind: ReadinessTrailKind,
    /// What the check covered: claims in scope, hypotheses in scope,
    /// null-result searches in scope. 0 for the merge-queue row.
    pub total: u32,
    /// What passed: pinned claims, resolved hypotheses, reckoned nulls.
    /// 0 for the merge-queue row.
    pub clean: u32,
    /// Claims row only: how many pins carry a verified machine check.
    pub verified: u32,
    /// Merge-queue row only: pending proposals in scope.
    pub pending: u32,
    /// The specific objects the row cites (mono chips, already labeled):
    /// "CLAIMS-2", "H-4", "#12", "pr-1042".
    pub refs: Vec<String>,
}

/// The readiness report of one scope (FR-13.1/13.2): a PURE derived view —
/// blockers (the specific objects), infos (advisory rows, never blockers),
/// and the trail (what a clean board did right). `scope` is the mission the
/// report ran for; None = the whole workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessReport {
    pub scope: Option<Uuid>,
    pub verdict: ReadinessVerdict,
    pub blockers: Vec<ReadinessItem>,
    pub infos: Vec<ReadinessItem>,
    pub trail: Vec<ReadinessTrailRow>,
}

/// The relations of one hypothesis as load-bearing ties.
fn relation_ties(relations: &[RelationChip]) -> Vec<ReadinessRelationTie> {
    relations
        .iter()
        .map(|r| ReadinessRelationTie {
            kind: r.kind,
            other_seq: r.other_seq,
            incoming: r.direction == RelationDirection::Incoming,
        })
        .collect()
}

/// How many support sweeps ran for the mission AFTER `since_seq` — the
/// chances a pin that landed at `since_seq` had to be judged (Story 6.10).
/// A sweep is a `run.started` with the `support-sweep` step referencing the
/// mission; only sweeps AFTER the pin count (the pin could not have been
/// judged by a sweep that ran before it existed).
fn support_sweeps_since(events: &[StoredEvent], mission_id: Uuid, since_seq: i64) -> usize {
    let mid = mission_id.to_string();
    events
        .iter()
        .filter(|e| {
            e.seq > since_seq
                && e.kind == RUN_STARTED
                && e.payload.get("step").and_then(|s| s.as_str()) == Some(SUPPORT_SWEEP_STEP)
                && (e.causes.contains(&mission_id)
                    || e.payload.get("mission_id").and_then(|m| m.as_str()) == Some(mid.as_str()))
        })
        .count()
}

/// Is this hypothesis status resolved for the preprint tier? Supported and
/// refuted are answers; proposed, testing, and revised are still open
/// questions (revised is mid-rework by construction — revised→testing is its
/// only exit).
fn is_resolved(status: HypothesisStatus) -> bool {
    matches!(status, HypothesisStatus::Supported | HypothesisStatus::Refuted)
}

/// Compute the readiness report of a scope (FR-13.1/13.2): a pure fold over
/// the log — the SAME event slice the board renders, through the SAME folds
/// (evidence 1.7, hypotheses 1.5, search disclosure 4.1, proposals 2.2), all
/// under the shared FoldCursor (AD-1: post-rollback, the report reads the
/// live view — an orphaned pin never happened for this report). No readiness
/// state is written anywhere: asking again re-folds, replaying the same
/// events yields the same report (the FR-13.2 invariant the test below
/// proves). The board-only report: no manuscript registered, no manuscript
/// scope (progressive disclosure).
pub fn readiness_report(
    events: &[StoredEvent],
    scope: Option<Uuid>,
) -> Result<ReadinessReport, EventError> {
    readiness_report_with_manuscript(events, scope, &[])
}

/// The report with the manuscript scope extended (Story 6.8, FR-20.5): the
/// same pure board fold plus a scan of the registered manuscripts — the
/// consistency flags land as BLOCKING items referencing the board object
/// AND the manuscript location; an unreadable manuscript dir is an INFO
/// row (the check could not run — surfaced, never silently skipped); a
/// clean manuscript adds its trail row. The scan is computed by the caller
/// (the command seam reads the disk); the report itself stays a pure
/// function of the log + the scan (FR-13.2: same log + same files, same
/// report).
pub fn readiness_report_with_manuscript(
    events: &[StoredEvent],
    scope: Option<Uuid>,
    scans: &[ManuscriptScan],
) -> Result<ReadinessReport, EventError> {
    // The shared fold cursor (AD-1, Story 2.6) applied ONCE here: the live
    // slice feeds every sub-fold. The evidence/hypotheses/proposals folds
    // re-apply their own cursor over it (idempotent — the rollback events
    // survive filtering, the orphaned domain events do not); the search
    // disclosure fold takes the live slice directly so a rolled-back search
    // never blocks a report (it never happened for the board).
    let cursor = FoldCursor::over(events);
    let live = cursor.live_owned(events);

    let hyps = HypothesesProjection::fold(&live)?;
    let claims = EvidenceProjection::fold(&live)?;
    let disclosure = search_disclosure(&live, scope)?;
    let proposals = ProposalsProjection::fold(&live)?;

    // Scope: the mission's hypotheses (and everything hanging off them).
    let hyps_in_scope: Vec<&Hypothesis> = hyps
        .iter()
        .filter(|h| scope.is_none_or(|m| h.mission_id == m))
        .collect();
    let hyp_seq: std::collections::HashMap<Uuid, i64> =
        hyps.iter().map(|h| (h.id, h.seq)).collect();
    let hyp_mission: std::collections::HashMap<Uuid, Uuid> =
        hyps.iter().map(|h| (h.id, h.mission_id)).collect();
    let claims_in_scope: Vec<&Claim> = claims
        .iter()
        .filter(|c| {
            hyp_mission
                .get(&c.hypothesis_id)
                .is_some_and(|m| scope.is_none_or(|s| *m == s))
        })
        .collect();

    // --- Blocker 1: unpinned claims (FR-3.4's flag, one item per claim) ---
    let mut blockers: Vec<ReadinessItem> = claims_in_scope
        .iter()
        .filter(|c| !c.pinned)
        .map(|c| ReadinessItem::unpinned_claim(c, hyp_seq.get(&c.hypothesis_id).copied()))
        .collect();

    // --- Blocker 2: load-bearing unresolved / refuted hypotheses ---
    for hyp in &hyps_in_scope {
        let claim_ties: Vec<i64> = claims_in_scope
            .iter()
            .filter(|c| c.hypothesis_id == hyp.id)
            .map(|c| c.seq)
            .collect();
        let relation_ties = relation_ties(&hyp.relations);
        let load_bearing = !claim_ties.is_empty() || !relation_ties.is_empty();
        if !load_bearing {
            continue; // nothing rests on it — the preprint doesn't depend on it
        }
        let kind = match hyp.status {
            HypothesisStatus::Proposed | HypothesisStatus::Testing => {
                Some(ReadinessItemKind::LoadBearingUnresolved)
            }
            HypothesisStatus::Refuted => Some(ReadinessItemKind::LoadBearingRefuted),
            // supported: an answer. revised: mid-rework, but nothing the
            // gate's rule rests on (see the module doc).
            HypothesisStatus::Supported | HypothesisStatus::Revised => None,
        };
        if let Some(kind) = kind {
            blockers.push(ReadinessItem::load_bearing(
                kind,
                hyp,
                claim_ties,
                relation_ties,
            ));
        }
    }

    // --- Blocker 3: unreckoned null results ---
    // A null-result search blocks while the mission it ran for still
    // carries unresolved hypotheses (the board hasn't answered the empty
    // result). Disclosure rows are already scope-filtered; missions without
    // hypotheses, and workspace-wide null searches (no mission), never
    // block — no board object rests on them.
    let mut unreckoned: Vec<i64> = Vec::new();
    for row in disclosure.rows.iter().filter(|r| r.null_result) {
        let unreckoned_for_mission = row
            .mission_id
            .is_some_and(|m| {
                hyps.iter().any(|h| {
                    h.mission_id == m && !is_resolved(h.status)
                })
            });
        if unreckoned_for_mission {
            unreckoned.push(row.seq);
            blockers.push(ReadinessItem::unreckoned_null(row.seq, row.mission_id));
        }
    }

    // --- Infos: verified-failed pins, support status, merge queue (never
    // blockers) ---
    let mut infos: Vec<ReadinessItem> = claims_in_scope
        .iter()
        .filter(|c| {
            c.pinned
                && c.pin
                    .as_ref()
                    .and_then(|p| p.verification.as_ref())
                    .is_some_and(|v| v.status == VerificationStatus::Failed)
        })
        .map(|c| ReadinessItem::verification_failed(c, hyp_seq.get(&c.hypothesis_id).copied()))
        .collect();
    // Support status (Story 6.10, FR-23.3): the gate CONSUMES the third
    // signal — `unsupported` and `partially` render as info rows (the pin
    // stays pinned; the verdict is a fact to see, not a gate to smuggle in
    // through the back door), and support still unverified after
    // SUPPORT_SWEEP_THRESHOLD sweeps since the pin landed surfaces as
    // "unchecked" — never silently assumed fresh.
    for c in claims_in_scope.iter().filter(|c| c.pinned) {
        let Some(pin) = c.pin.as_ref() else {
            continue;
        };
        let hyp_s = hyp_seq.get(&c.hypothesis_id).copied();
        match pin.support.as_ref().map(|s| s.status) {
            Some(SupportStatus::Unsupported) => infos.push(ReadinessItem::support_info(
                ReadinessItemKind::PinUnsupported,
                c,
                hyp_s,
            )),
            Some(SupportStatus::Partially) => infos.push(ReadinessItem::support_info(
                ReadinessItemKind::PinPartiallySupported,
                c,
                hyp_s,
            )),
            None | Some(SupportStatus::Stale) => {
                // Unverified support after N sweeps — an info, never a
                // blocker (the pin exists; the check simply never landed).
                let mission = hyp_mission.get(&c.hypothesis_id).copied();
                if mission.is_some_and(|m| {
                    support_sweeps_since(&live, m, pin.seq) >= SUPPORT_SWEEP_THRESHOLD
                }) {
                    infos.push(ReadinessItem::support_info(
                        ReadinessItemKind::SupportUnchecked,
                        c,
                        hyp_s,
                    ));
                }
            }
            // supported / unverifiable: the check landed — a fact on the
            // pin, not a gate concern at tier 1.
            Some(SupportStatus::Supported | SupportStatus::Unverifiable) => {}
        }
    }
    let pending_proposals: Vec<_> = proposals
        .iter()
        .filter(|p| {
            p.status == ProposalStatus::Pending
                && scope.is_none_or(|m| p.mission_id == Some(m))
        })
        .collect();
    if !pending_proposals.is_empty() {
        infos.push(ReadinessItem::merge_queue(
            pending_proposals.len() as u32,
            scope,
        ));
    }

    // --- The manuscript scope (Story 6.8, FR-20.5): the paper cannot
    // quietly outrun the evidence. The consistency flags land as BLOCKERS
    // referencing the board object AND the manuscript location; an
    // unreadable dir is INFO (the check could not run — surfaced, never
    // silently skipped); the trail row appears only when a manuscript was
    // actually checked (progressive disclosure). ---
    let mut ms_markers_total: u32 = 0;
    let mut ms_flagged_locations: u32 = 0;
    let mut ms_clean_refs: Vec<String> = Vec::new();
    let mut ms_checked = false;
    for scan in scans.iter().filter(|s| scope.is_none_or(|m| s.mission_id == m)) {
        if scan.error.is_some() {
            infos.push(ReadinessItem::manuscript_unreadable(scan));
            continue;
        }
        ms_checked = true;
        let flags = manuscript::manuscript_consistency(events, scan)?;
        let total: u32 = scan.files.iter().map(|f| f.markers.len() as u32).sum();
        let flagged: std::collections::HashSet<(String, u32)> =
            flags.iter().map(|f| (f.file.clone(), f.line)).collect();
        ms_markers_total += total;
        ms_flagged_locations += flagged.len() as u32;
        for file in &scan.files {
            for marker in &file.markers {
                if !flagged.contains(&(file.path.clone(), marker.line)) {
                    ms_clean_refs.push(format!("{}:{}", file.path, marker.line));
                }
            }
        }
        for flag in &flags {
            blockers.push(ReadinessItem::manuscript(flag));
        }
    }

    // --- The trail: what was checked, the counts, the objects (always
    // honest — a not-ready board still sees its own counts). ---
    let pinned_claims: Vec<&Claim> =
        claims_in_scope.iter().copied().filter(|c| c.pinned).collect();
    let verified_pins = pinned_claims
        .iter()
        .filter(|c| {
            c.pin
                .as_ref()
                .and_then(|p| p.verification.as_ref())
                .is_some_and(|v| v.status == VerificationStatus::Verified)
        })
        .count() as u32;
    let resolved_hyps: Vec<&Hypothesis> =
        hyps_in_scope.iter().copied().filter(|h| is_resolved(h.status)).collect();
    let null_rows: Vec<i64> = disclosure
        .rows
        .iter()
        .filter(|r| r.null_result)
        .map(|r| r.seq)
        .collect();

    let mut trail = vec![
        ReadinessTrailRow {
            kind: ReadinessTrailKind::ClaimsPinned,
            total: claims_in_scope.len() as u32,
            clean: pinned_claims.len() as u32,
            verified: verified_pins,
            pending: 0,
            refs: pinned_claims
                .iter()
                .map(|c| format!("CLAIMS-{}", c.seq))
                .collect(),
        },
        ReadinessTrailRow {
            kind: ReadinessTrailKind::HypothesesResolved,
            total: hyps_in_scope.len() as u32,
            clean: resolved_hyps.len() as u32,
            verified: 0,
            pending: 0,
            refs: resolved_hyps.iter().map(|h| format!("H-{}", h.seq)).collect(),
        },
        ReadinessTrailRow {
            kind: ReadinessTrailKind::NullsDisclosed,
            total: null_rows.len() as u32,
            clean: (null_rows.len() - unreckoned.len()) as u32,
            verified: 0,
            pending: 0,
            // #{seq} — the search log row IS the disclosure (FR-12.1)
            refs: null_rows.iter().map(|seq| format!("#{seq}")).collect(),
        },
        ReadinessTrailRow {
            kind: ReadinessTrailKind::MergeQueue,
            total: 0,
            clean: 0,
            verified: 0,
            pending: pending_proposals.len() as u32,
            // Clean queue: cite the last decided proposal (the most recent
            // board-changing decision); non-empty: cite what waits.
            refs: if pending_proposals.is_empty() {
                proposals
                    .iter()
                    .filter(|p| p.decided.is_some())
                    .max_by_key(|p| p.decided.as_ref().map(|d| d.seq).unwrap_or(p.seq))
                    .map(|p| format!("pr-{}", p.seq))
                    .into_iter()
                    .collect()
            } else {
                pending_proposals
                    .iter()
                    .map(|p| format!("pr-{}", p.seq))
                    .collect()
            },
        },
    ];
    // The manuscript trail row (Story 6.8): N/N markers linked clean —
    // present only when a manuscript was actually checked.
    if ms_checked {
        trail.push(ReadinessTrailRow {
            kind: ReadinessTrailKind::ManuscriptConsistency,
            total: ms_markers_total,
            clean: ms_markers_total - ms_flagged_locations,
            verified: 0,
            pending: 0,
            refs: ms_clean_refs,
        });
    }

    Ok(ReadinessReport {
        scope,
        verdict: if blockers.is_empty() {
            ReadinessVerdict::Ready
        } else {
            ReadinessVerdict::NotReady
        },
        blockers,
        infos,
        trail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::evidence::excerpt_digest;
    use crate::domain::missions::{Autonomy, MissionCreatedPayload};
    use crate::domain::search::{SearchParams, DATABASE_ARXIV};
    use crate::domain::verifier::{VerificationOutcome, DETAIL_EXCERPT_MATCHED, DETAIL_NOT_FOUND};
    use crate::eventstore::{EventStore, NewEvent, StoredEvent};
    use rusqlite::Connection;
    use std::collections::BTreeMap;

    fn with_store(f: impl FnOnce(&EventStore<'_>)) {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        let store = EventStore::new(&conn);
        f(&store);
    }

    fn seed_mission(store: &EventStore<'_>) -> uuid::Uuid {
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

    fn seed_hypothesis(store: &EventStore<'_>, mission_id: uuid::Uuid, statement: &str) -> uuid::Uuid {
        store
            .append(NewEvent::hypothesis_created(statement, mission_id).unwrap())
            .unwrap()
            .id
    }

    fn seed_claim(store: &EventStore<'_>, hypothesis_id: uuid::Uuid, text: &str) -> StoredEvent {
        store
            .append(NewEvent::claim_registered(text, hypothesis_id, None).unwrap())
            .unwrap()
    }

    fn seed_pin(store: &EventStore<'_>, claim: &StoredEvent, hypothesis_id: uuid::Uuid, excerpt: &str) -> StoredEvent {
        store
            .append(
                NewEvent::evidence_pinned_citation(
                    claim.id,
                    hypothesis_id,
                    "ref-any",
                    excerpt,
                    0.8,
                    "GLM-5.3",
                )
                .unwrap(),
            )
            .unwrap()
    }

    fn seed_null_search(store: &EventStore<'_>, mission_id: uuid::Uuid) -> i64 {
        crate::domain::search::run_search(
            store,
            &SearchParams {
                query: "qqqq zzzz wwww".into(),
                database: DATABASE_ARXIV.into(),
                filters: BTreeMap::new(),
                order: None,
                first_page: true,
                mission_id: Some(mission_id),
                run_id: Some("nightshift-readiness".into()),
            },
        )
        .unwrap()
        .row
        .seq
    }

    fn seed_hit_search(store: &EventStore<'_>, mission_id: uuid::Uuid) {
        crate::domain::search::run_search(
            store,
            &SearchParams {
                query: "sparse attention".into(),
                database: DATABASE_ARXIV.into(),
                filters: BTreeMap::new(),
                order: None,
                first_page: true,
                mission_id: Some(mission_id),
                run_id: Some("nightshift-readiness".into()),
            },
        )
        .unwrap();
    }

    /// Blocker categories each produce exactly-referenced items (FR-13.1):
    /// the claim id + its hypothesis id (CLAIMS-{n}), the hypothesis id +
    /// status + its ties (H-{n}), and the search log seq (#{n}) are all IN
    /// THE PAYLOAD — and a verified-failed pin is an INFO row, never a
    /// blocker (the decided separation).
    #[test]
    fn every_blocker_references_its_specific_board_object() {
        with_store(|store| {
            let mission = seed_mission(store);
            let h = seed_hypothesis(store, mission, "X holds under load.");
            // H is contradicted by a second hypothesis — a relation tie.
            let other = seed_hypothesis(store, mission, "X fails under load.");
            store
                .append(
                    NewEvent::hypothesis_related(crate::domain::hypotheses::HypothesisRelatedPayload {
                        from_hypothesis_id: other,
                        to_hypothesis_id: h,
                        relation_kind: crate::domain::hypotheses::RelationKind::Contradicts,
                    })
                    .unwrap(),
                )
                .unwrap();
            // Three claims: unpinned, pinned+verified, pinned+failed.
            let unpinned = seed_claim(store, h, "X holds at 32k.");
            let verified = seed_claim(store, h, "X holds at 64k.");
            let failed = seed_claim(store, h, "X holds at 128k.");
            let pin_v = seed_pin(store, &verified, h, "X holds at 64k, stated.");
            seed_pin(store, &failed, h, "X holds at 128k, stated.");
            store
                .append(
                    NewEvent::evidence_verified(
                        verified.id,
                        h,
                        pin_v.seq,
                        VerificationOutcome::Verified,
                        DETAIL_EXCERPT_MATCHED,
                        "ref-any",
                    )
                    .unwrap(),
                )
                .unwrap();
            let pin_f = store
                .events_all()
                .unwrap()
                .into_iter()
                .find(|e| {
                    e.kind == "evidence.pinned"
                        && e.payload["claim_id"] == serde_json::json!(failed.id)
                })
                .unwrap();
            store
                .append(
                    NewEvent::evidence_verified(
                        failed.id,
                        h,
                        pin_f.seq,
                        VerificationOutcome::Failed,
                        DETAIL_NOT_FOUND,
                        "ref-any",
                    )
                    .unwrap(),
                )
                .unwrap();
            // The null-result search for the mission (H still testing).
            let null_search = seed_null_search(store, mission);

            let report =
                readiness_report(&store.events_all().unwrap(), None).unwrap();
            assert_eq!(report.verdict, ReadinessVerdict::NotReady);

            // Unpinned claim: the claim id + seq AND its hypothesis, in the payload.
            let [unpinned_item] = report
                .blockers
                .iter()
                .filter(|b| b.kind == ReadinessItemKind::UnpinnedClaim)
                .map(|b| b.clone())
                .collect::<Vec<_>>()
                .try_into()
                .ok()
                .expect("exactly one unpinned-claim blocker");
            assert_eq!(unpinned_item.claim_id, Some(unpinned.id));
            assert_eq!(unpinned_item.claim_seq, Some(unpinned.seq));
            assert_eq!(unpinned_item.hypothesis_id, Some(h));

            // Load-bearing unresolved: the hypothesis id + status + BOTH tie
            // families (claims resting on it, the contradicting relation).
            // The relation chips onto BOTH endpoints — the contradictor
            // (`other`) is load-bearing too (an unresolved contradiction
            // cuts both ways).
            let load_blockers: Vec<&ReadinessItem> = report
                .blockers
                .iter()
                .filter(|b| b.kind == ReadinessItemKind::LoadBearingUnresolved)
                .collect();
            assert_eq!(load_blockers.len(), 2, "both relation endpoints are load-bearing");
            let load_item = load_blockers
                .iter()
                .find(|b| b.hypothesis_id == Some(h))
                .expect("h has a load-bearing blocker");
            assert_eq!(load_item.hypothesis_status, Some(HypothesisStatus::Proposed));
            assert_eq!(load_item.claim_ties, vec![unpinned.seq, verified.seq, failed.seq]);
            assert_eq!(
                load_item.relation_ties,
                vec![ReadinessRelationTie {
                    kind: crate::domain::hypotheses::RelationKind::Contradicts,
                    other_seq: other_seq_of(store, other),
                    incoming: true,
                }]
            );
            let other_item = load_blockers
                .iter()
                .find(|b| b.hypothesis_id == Some(other))
                .expect("the contradictor carries its outgoing tie");
            assert_eq!(
                other_item.relation_ties,
                vec![ReadinessRelationTie {
                    kind: crate::domain::hypotheses::RelationKind::Contradicts,
                    other_seq: other_seq_of(store, h),
                    incoming: false,
                }]
            );

            // Unreckoned null: the search log seq in the payload.
            let [null_item] = report
                .blockers
                .iter()
                .filter(|b| b.kind == ReadinessItemKind::UnreckonedNullResult)
                .map(|b| b.clone())
                .collect::<Vec<_>>()
                .try_into()
                .ok()
                .expect("exactly one unreckoned-null blocker");
            assert_eq!(null_item.search_seq, Some(null_search));
            assert_eq!(null_item.mission_id, Some(mission));

            // Verified-failed pin: an INFO row, never a blocker — the
            // decided separation (module doc).
            let [failed_info] = report
                .infos
                .iter()
                .filter(|i| i.kind == ReadinessItemKind::PinVerificationFailed)
                .map(|i| i.clone())
                .collect::<Vec<_>>()
                .try_into()
                .ok()
                .expect("exactly one verification-failed info");
            assert_eq!(failed_info.claim_id, Some(failed.id));
            assert!(
                !report.blockers.iter().any(|b| b.claim_id == Some(failed.id)),
                "a verified-failed pin never blocks readiness"
            );
        });
    }

    fn other_seq_of(store: &EventStore<'_>, id: uuid::Uuid) -> i64 {
        store
            .events_all()
            .unwrap()
            .into_iter()
            .find(|e| e.id == id)
            .unwrap()
            .seq
    }

    /// A clean board reports ready with the non-empty evidence trail that
    /// justifies it (the story's third AC): four rows — claims pinned (+
    /// verified count), hypotheses resolved, nulls reckoned, queue empty.
    #[test]
    fn a_clean_board_reports_ready_with_the_evidence_trail() {
        with_store(|store| {
            let mission = seed_mission(store);
            let h = seed_hypothesis(store, mission, "X holds under load.");
            // One claim, pinned + machine-verified.
            let claim = seed_claim(store, h, "X holds at 32k.");
            let pin = seed_pin(store, &claim, h, "X holds at 32k, stated.");
            store
                .append(
                    NewEvent::evidence_verified(
                        claim.id,
                        h,
                        pin.seq,
                        VerificationOutcome::Verified,
                        DETAIL_EXCERPT_MATCHED,
                        "ref-any",
                    )
                    .unwrap(),
                )
                .unwrap();
            // The hypothesis resolves (an answer — even with claims resting
            // on it, supported is an answer, not a blocker).
            store
                .append(
                    NewEvent::hypothesis_status_changed(
                        HypothesisStatus::Proposed,
                        HypothesisStatus::Testing,
                        "Trial 1 ran.",
                    )
                    .unwrap()
                    .with_causes(vec![h]),
                )
                .unwrap();
            store
                .append(
                    NewEvent::hypothesis_status_changed(
                        HypothesisStatus::Testing,
                        HypothesisStatus::Supported,
                        "The pinned evidence held.",
                    )
                    .unwrap()
                    .with_causes(vec![h]),
                )
                .unwrap();
            // A null-result search, RECKONED: every hypothesis of the
            // mission is resolved, so the empty result is on the record and
            // answered — it must NOT block, and the trail counts it
            // disclosed.
            let null_search = seed_null_search(store, mission);

            let report = readiness_report(&store.events_all().unwrap(), None).unwrap();
            assert_eq!(report.verdict, ReadinessVerdict::Ready);
            assert!(report.blockers.is_empty(), "a clean board has no blockers");
            assert!(report.infos.is_empty());

            assert_eq!(report.trail.len(), 4, "the four-row trail");
            let claims_row = &report.trail[0];
            assert_eq!(claims_row.kind, ReadinessTrailKind::ClaimsPinned);
            assert_eq!((claims_row.total, claims_row.clean, claims_row.verified), (1, 1, 1));
            assert_eq!(claims_row.refs, vec![format!("CLAIMS-{}", claim.seq)]);
            let hyps_row = &report.trail[1];
            assert_eq!((hyps_row.total, hyps_row.clean), (1, 1));
            assert_eq!(hyps_row.refs, vec![format!("H-{}", other_seq_of(store, h))]);
            let nulls_row = &report.trail[2];
            assert_eq!((nulls_row.total, nulls_row.clean), (1, 1));
            assert_eq!(nulls_row.refs, vec![format!("#{null_search}")]);
            let queue_row = &report.trail[3];
            assert_eq!(queue_row.pending, 0);
        });
    }

    /// The FR-13.2 invariant (the story's second AC): the gate is a derived
    /// projection with no new data model — reaching the same board by
    /// REPLAYING the events into a fresh log yields the same report. The
    /// replay mints fresh event ids (the log's identity), so the assertion
    /// covers everything the report DERIVES — verdict, blocker kinds and
    /// order, every seq-based reference, every count — which is everything a
    /// derived view is. There is no readiness state anywhere to diverge: no
    /// table, no event, nothing but the fold.
    #[test]
    fn replaying_the_events_yields_the_same_report() {
        fn build(store: &EventStore<'_>) -> ReadinessReport {
            let mission = seed_mission(store);
            let h = seed_hypothesis(store, mission, "X holds under load.");
            let other = seed_hypothesis(store, mission, "X fails under load.");
            store
                .append(
                    NewEvent::hypothesis_related(crate::domain::hypotheses::HypothesisRelatedPayload {
                        from_hypothesis_id: other,
                        to_hypothesis_id: h,
                        relation_kind: crate::domain::hypotheses::RelationKind::Extends,
                    })
                    .unwrap(),
                )
                .unwrap();
            // one unpinned claim on h, one pinned+failed on other
            let unpinned = seed_claim(store, h, "X holds at 32k.");
            let pinned = seed_claim(store, other, "X fails at 32k.");
            let pin = seed_pin(store, &pinned, other, "X fails at 32k, stated.");
            store
                .append(
                    NewEvent::evidence_verified(
                        pinned.id,
                        other,
                        pin.seq,
                        VerificationOutcome::Failed,
                        DETAIL_NOT_FOUND,
                        "ref-any",
                    )
                    .unwrap(),
                )
                .unwrap();
            // resolve `other` so its pin's failed check stays info-only, and
            // h stays testing (load-bearing, unresolved)
            store
                .append(
                    NewEvent::hypothesis_status_changed(
                        HypothesisStatus::Proposed,
                        HypothesisStatus::Testing,
                        "Trial ran.",
                    )
                    .unwrap()
                    .with_causes(vec![other]),
                )
                .unwrap();
            store
                .append(
                    NewEvent::hypothesis_status_changed(
                        HypothesisStatus::Testing,
                        HypothesisStatus::Refuted,
                        "The null decided it.",
                    )
                    .unwrap()
                    .with_causes(vec![other]),
                )
                .unwrap();
            seed_null_search(store, mission);
            let _ = unpinned;
            readiness_report(&store.events_all().unwrap(), None).unwrap()
        }

        let conn_a = Connection::open_in_memory().unwrap();
        EventStore::init(&conn_a).unwrap();
        let store_a = EventStore::new(&conn_a);
        let report_a = build(&store_a);

        // The replay: the same events, the same order, a fresh log. Derived
        // state cannot survive this — only the fold can produce the report.
        let conn_b = Connection::open_in_memory().unwrap();
        EventStore::init(&conn_b).unwrap();
        let store_b = EventStore::new(&conn_b);
        let report_b = build(&store_b);

        assert_eq!(report_a.verdict, report_b.verdict);
        assert_eq!(report_a.blockers.len(), report_b.blockers.len());
        for (a, b) in report_a.blockers.iter().zip(report_b.blockers.iter()) {
            assert_eq!(a.kind, b.kind);
            // the seq-based references (CLAIMS-n, H-n, #n, ties) — identical
            assert_eq!(a.claim_seq, b.claim_seq);
            assert_eq!(a.hypothesis_seq, b.hypothesis_seq);
            assert_eq!(a.hypothesis_status, b.hypothesis_status);
            assert_eq!(a.search_seq, b.search_seq);
            assert_eq!(a.claim_ties, b.claim_ties);
            assert_eq!(a.relation_ties, b.relation_ties);
            // the raw ids differ (fresh log, fresh event ids) — everything
            // DERIVED is equal, which is the FR-13.2 point.
            if let (Some(a_id), Some(b_id)) = (a.claim_id, b.claim_id) {
                assert_ne!(a_id, b_id);
            }
        }
        assert_eq!(report_a.infos.len(), report_b.infos.len());
        assert_eq!(
            report_a.trail.iter().map(|r| (r.kind, r.total, r.clean, r.verified, r.pending, r.refs.clone())).collect::<Vec<_>>(),
            report_b.trail.iter().map(|r| (r.kind, r.total, r.clean, r.verified, r.pending, r.refs.clone())).collect::<Vec<_>>(),
        );
    }

    /// FoldCursor respect (Story 2.6): after a rollback the report reads the
    /// same live view the board renders — the fixing events (pin, status
    /// change) are orphaned, and the blockers they cleared return.
    #[test]
    fn a_rollback_returns_the_blockers_it_fixed() {
        with_store(|store| {
            let mission = seed_mission(store);
            let h = seed_hypothesis(store, mission, "X holds under load.");
            let claim = seed_claim(store, h, "X holds at 32k.");
            // Dirty: an unpinned claim + a load-bearing testing hypothesis.
            let dirty = readiness_report(&store.events_all().unwrap(), None).unwrap();
            assert_eq!(dirty.verdict, ReadinessVerdict::NotReady);
            assert_eq!(dirty.blockers.len(), 2);

            // Checkpoint the dirty state, then fix everything.
            let checkpoint = store
                .append(NewEvent::checkpoint_created("dirty", store.head_seq().unwrap()).unwrap())
                .unwrap();
            seed_pin(store, &claim, h, "X holds at 32k, stated.");
            for (from, to, basis) in [
                (HypothesisStatus::Proposed, HypothesisStatus::Testing, "Trial ran."),
                (HypothesisStatus::Testing, HypothesisStatus::Supported, "Evidence held."),
            ] {
                store
                    .append(
                        NewEvent::hypothesis_status_changed(from, to, basis)
                            .unwrap()
                            .with_causes(vec![h]),
                    )
                    .unwrap();
            }
            let fixed = readiness_report(&store.events_all().unwrap(), None).unwrap();
            assert_eq!(fixed.verdict, ReadinessVerdict::Ready, "the fixes took");

            // Roll back: the fixes are orphaned, the blockers return — same
            // kinds, same object seqs (the blocking objects predate the cut).
            store
                .append(
                    NewEvent::checkpoint_rolled_back(
                        checkpoint.id,
                        "dirty",
                        checkpoint.payload["seq"].as_i64().unwrap(),
                        3,
                    )
                    .unwrap(),
                )
                .unwrap();
            let rolled = readiness_report(&store.events_all().unwrap(), None).unwrap();
            assert_eq!(rolled.verdict, ReadinessVerdict::NotReady);
            assert_eq!(rolled.blockers.len(), 2);
            assert_eq!(
                rolled.blockers.iter().map(|b| b.kind).collect::<Vec<_>>(),
                dirty.blockers.iter().map(|b| b.kind).collect::<Vec<_>>()
            );
            assert_eq!(
                rolled.blockers.iter().filter(|b| b.claim_id.is_some()).map(|b| b.claim_seq).collect::<Vec<_>>(),
                dirty.blockers.iter().filter(|b| b.claim_id.is_some()).map(|b| b.claim_seq).collect::<Vec<_>>(),
            );
            // the checkpoint bookkeeping itself is never a blocker — the
            // report read 2 blockers, not 3.
        });
    }

    /// The decided info-vs-blocker separation, end to end: a board whose
    /// ONLY blemish is a failed pin verification reports READY with the
    /// failure as an info row — "verified" stays a distinct, honest axis.
    #[test]
    fn a_failed_verification_alone_is_info_not_a_blocker() {
        with_store(|store| {
            let mission = seed_mission(store);
            let h = seed_hypothesis(store, mission, "X holds under load.");
            let claim = seed_claim(store, h, "X holds at 32k.");
            let pin = seed_pin(store, &claim, h, "X holds at 32k, stated.");
            store
                .append(
                    NewEvent::evidence_verified(
                        claim.id,
                        h,
                        pin.seq,
                        VerificationOutcome::Failed,
                        DETAIL_NOT_FOUND,
                        "ref-any",
                    )
                    .unwrap(),
                )
                .unwrap();
            // h carries a claim but resolves — the failed pin is the only
            // blemish left.
            store
                .append(
                    NewEvent::hypothesis_status_changed(
                        HypothesisStatus::Proposed,
                        HypothesisStatus::Testing,
                        "Trial ran.",
                    )
                    .unwrap()
                    .with_causes(vec![h]),
                )
                .unwrap();
            store
                .append(
                    NewEvent::hypothesis_status_changed(
                        HypothesisStatus::Testing,
                        HypothesisStatus::Supported,
                        "Held.",
                    )
                    .unwrap()
                    .with_causes(vec![h]),
                )
                .unwrap();

            let report = readiness_report(&store.events_all().unwrap(), None).unwrap();
            assert_eq!(report.verdict, ReadinessVerdict::Ready);
            assert!(report.blockers.is_empty());
            assert_eq!(report.infos.len(), 1);
            assert_eq!(report.infos[0].kind, ReadinessItemKind::PinVerificationFailed);
            assert_eq!(report.infos[0].claim_id, Some(claim.id));
            // the trail stays honest: pinned 1/1, verified 0 (the failed
            // check is not a verified pin, and the counts say so)
            assert_eq!((report.trail[0].total, report.trail[0].clean, report.trail[0].verified), (1, 1, 0));
        });
    }

    /// Scoping: the workspace report aggregates every mission's blockers;
    /// one mission's scoped report sees only its own board — a dirty mission
    /// cannot hide inside a clean one, and a clean mission reports ready
    /// while the workspace does not. An unknown mission is an honest empty
    /// scope (the same contract as the disclosure read).
    #[test]
    fn the_scope_governs_what_blocks() {
        with_store(|store| {
            let dirty = seed_mission(store);
            let clean = seed_mission(store);
            // dirty: an unpinned claim on a testing hypothesis
            let h = seed_hypothesis(store, dirty, "X holds.");
            seed_claim(store, h, "X holds at 32k.");
            // clean: a pinned claim on a supported hypothesis
            let h2 = seed_hypothesis(store, clean, "Y holds.");
            let claim2 = seed_claim(store, h2, "Y holds at 32k.");
            seed_pin(store, &claim2, h2, "Y holds at 32k, stated.");
            for (from, to, basis) in [
                (HypothesisStatus::Proposed, HypothesisStatus::Testing, "Trial ran."),
                (HypothesisStatus::Testing, HypothesisStatus::Supported, "Held."),
            ] {
                store
                    .append(
                        NewEvent::hypothesis_status_changed(from, to, basis)
                            .unwrap()
                            .with_causes(vec![h2]),
                    )
                    .unwrap();
            }

            let workspace = readiness_report(&store.events_all().unwrap(), None).unwrap();
            assert_eq!(workspace.verdict, ReadinessVerdict::NotReady);
            assert!(workspace.blockers.iter().all(|b| b.claim_id.is_some() || b.hypothesis_id.is_some()));
            // the dirty mission's own report: its blockers
            let dirty_report = readiness_report(&store.events_all().unwrap(), Some(dirty)).unwrap();
            assert_eq!(dirty_report.verdict, ReadinessVerdict::NotReady);
            assert_eq!(dirty_report.scope, Some(dirty));
            assert!(dirty_report
                .blockers
                .iter()
                .all(|b| b.hypothesis_id != Some(h2)));
            // the clean mission's own report: ready, trail citing only its objects
            let clean_report = readiness_report(&store.events_all().unwrap(), Some(clean)).unwrap();
            assert_eq!(clean_report.verdict, ReadinessVerdict::Ready);
            assert_eq!(clean_report.trail[0].refs, vec![format!("CLAIMS-{}", claim2.seq)]);
            assert_eq!(clean_report.trail[1].total, 1);
            // an unknown mission is an honest empty scope
            let unknown = readiness_report(&store.events_all().unwrap(), Some(uuid::Uuid::new_v4()))
                .unwrap();
            assert_eq!(unknown.verdict, ReadinessVerdict::Ready);
            assert_eq!(unknown.trail[0].total, 0);
        });
    }

    /// A refuted load-bearing hypothesis blocks; a refuted hypothesis with
    /// nothing resting on it does not; a supported load-bearing hypothesis
    /// does not (an answer is an answer).
    #[test]
    fn load_bearing_rules_follow_the_lifecycle() {
        with_store(|store| {
            let mission = seed_mission(store);
            // refuted WITH a claim resting on it → blocks
            let h_refuted = seed_hypothesis(store, mission, "A fails.");
            seed_claim(store, h_refuted, "A fails at 32k.");
            for (from, to, basis) in [
                (HypothesisStatus::Proposed, HypothesisStatus::Testing, "Trial ran."),
                (HypothesisStatus::Testing, HypothesisStatus::Refuted, "It failed."),
            ] {
                store
                    .append(
                        NewEvent::hypothesis_status_changed(from, to, basis)
                            .unwrap()
                            .with_causes(vec![h_refuted]),
                    )
                    .unwrap();
            }
            // refuted with NOTHING resting on it → does not block
            let h_alone = seed_hypothesis(store, mission, "B fails.");
            for (from, to, basis) in [
                (HypothesisStatus::Proposed, HypothesisStatus::Testing, "Trial ran."),
                (HypothesisStatus::Testing, HypothesisStatus::Refuted, "It failed."),
            ] {
                store
                    .append(
                        NewEvent::hypothesis_status_changed(from, to, basis)
                            .unwrap()
                            .with_causes(vec![h_alone]),
                    )
                    .unwrap();
            }
            // supported WITH a claim resting on it → does not block
            let h_supported = seed_hypothesis(store, mission, "C holds.");
            let claim_c = seed_claim(store, h_supported, "C holds at 32k.");
            seed_pin(store, &claim_c, h_supported, "C holds at 32k, stated.");
            for (from, to, basis) in [
                (HypothesisStatus::Proposed, HypothesisStatus::Testing, "Trial ran."),
                (HypothesisStatus::Testing, HypothesisStatus::Supported, "Held."),
            ] {
                store
                    .append(
                        NewEvent::hypothesis_status_changed(from, to, basis)
                            .unwrap()
                            .with_causes(vec![h_supported]),
                    )
                    .unwrap();
            }
            // a hit search so no null blocks the mission
            seed_hit_search(store, mission);

            let report = readiness_report(&store.events_all().unwrap(), Some(mission)).unwrap();
            let load_blockers: Vec<_> = report
                .blockers
                .iter()
                .filter(|b| {
                    b.kind == ReadinessItemKind::LoadBearingUnresolved
                        || b.kind == ReadinessItemKind::LoadBearingRefuted
                })
                .collect();
            assert_eq!(load_blockers.len(), 1, "only the refuted-with-ties hypothesis blocks");
            assert_eq!(load_blockers[0].kind, ReadinessItemKind::LoadBearingRefuted);
            assert_eq!(load_blockers[0].hypothesis_id, Some(h_refuted));
        });
    }

    /// The digest of a pinned excerpt is verified on read (AD-5) — the
    /// readiness fold inherits every sub-fold's corruption guarantees: a
    /// tampered pin fails the report loudly, never silently passes a board.
    #[test]
    fn corrupt_events_fail_the_fold_loudly() {
        with_store(|store| {
            let mission = seed_mission(store);
            let h = seed_hypothesis(store, mission, "X holds.");
            let claim = seed_claim(store, h, "X holds at 32k.");
            seed_pin(store, &claim, h, "X holds at 32k, stated.");
            // tamper: rewrite the pin event's excerpt so the digest no
            // longer matches its own content (AD-5 on read)
            let conn_events = store.events_all().unwrap();
            let mut events = conn_events;
            let pin_idx = events
                .iter()
                .position(|e| e.kind == "evidence.pinned")
                .unwrap();
            // digest of DIFFERENT text than the excerpt the event quotes —
            // AD-5 on read fails this loudly
            events[pin_idx].payload["excerpt"] = serde_json::json!("tampered content");
            events[pin_idx].payload["digest"] =
                serde_json::json!(excerpt_digest("different content entirely"));
            let err = readiness_report(&events, None);
            assert!(err.is_err(), "a tampered pin never folds into a readiness report");
        });
    }

    // ---------- the manuscript scope (Story 6.8, FR-20.4/20.5) ----------

    use crate::domain::manuscript::{
        build_scan, ManuscriptScan, ScanFile, Marker, MarkerKind,
    };

    /// A temp .tex repo whose main.tex carries the marker convention.
    fn tex_repo(lines: &[&str]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rc-rd-ms-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut tex = String::from("\\documentclass{article}\n\\begin{document}\n");
        for line in lines {
            tex.push_str(line);
            tex.push('\n');
        }
        tex.push_str("\\end{document}\n");
        std::fs::write(dir.join("main.tex"), tex).unwrap();
        dir
    }

    /// The seeded board for the manuscript tests: h1 (testing) with an
    /// unpinned claim, h2 (supported) with a pinned claim — both creation
    /// seqs and claim seqs captured (the markers label real H-{seq} /
    /// CLAIMS-{seq} chips, which follow the log's seqs).
    struct MsBoard {
        mission: uuid::Uuid,
        h1: uuid::Uuid,
        h2: uuid::Uuid,
        h1_seq: i64,
        h2_seq: i64,
        unpinned: uuid::Uuid,
        pinned: uuid::Uuid,
        unpinned_seq: i64,
        pinned_seq: i64,
    }

    fn seed_manuscript_board(store: &EventStore<'_>) -> MsBoard {
        let mission = seed_mission(store);
        let h1 = seed_hypothesis(store, mission, "X holds under load.");
        let h2 = seed_hypothesis(store, mission, "Y holds under load.");
        let unpinned = seed_claim(store, h1, "X holds at 32k.");
        let pinned = seed_claim(store, h2, "Y holds at 32k.");
        seed_pin(store, &pinned, h2, "Y holds at 32k, stated.");
        for (h, from, to, basis) in [
            (h1, HypothesisStatus::Proposed, HypothesisStatus::Testing, "Trial running."),
            (h2, HypothesisStatus::Proposed, HypothesisStatus::Testing, "Trial ran."),
            (h2, HypothesisStatus::Testing, HypothesisStatus::Supported, "Evidence held."),
        ] {
            store
                .append(
                    NewEvent::hypothesis_status_changed(from, to, basis)
                        .unwrap()
                        .with_causes(vec![h]),
                )
                .unwrap();
        }
        let events = store.events_all().unwrap();
        let seq_of = |id: uuid::Uuid| {
            events.iter().find(|e| e.id == id).expect("seeded").seq
        };
        MsBoard {
            mission,
            h1,
            h2,
            h1_seq: seq_of(h1),
            h2_seq: seq_of(h2),
            unpinned: unpinned.id,
            pinned: pinned.id,
            unpinned_seq: seq_of(unpinned.id),
            pinned_seq: seq_of(pinned.id),
        }
    }

    fn register_and_scan(
        store: &EventStore<'_>,
        mission: uuid::Uuid,
        dir: &std::path::Path,
    ) -> ManuscriptScan {
        let ms = store
            .append(
                NewEvent::manuscript_registered(
                    mission,
                    &dir.display().to_string(),
                    "main.tex",
                )
                .unwrap(),
            )
            .unwrap();
        let events = store.events_all().unwrap();
        let registered = crate::domain::manuscript::ManuscriptsProjection::for_mission(
            &events,
            mission,
        )
        .unwrap()
        .expect("registered");
        assert_eq!(registered.seq, ms.seq);
        build_scan(&registered)
    }

    /// Every flag class blocks the gate, each referencing its board object
    /// AND its manuscript location (FR-20.4/20.5, FR-13.1 discipline). The
    /// markers carry the REAL H-{seq}/CLAIMS-{seq} labels — the log's
    /// creation seqs, exactly as the board renders them.
    #[test]
    fn manuscript_flags_block_the_gate_referencing_card_and_location() {
        with_store(|store| {
            let b = seed_manuscript_board(store);
            // the manuscript: 5 markers — 3 flag-bearing, 2 clean
            let dir = tex_repo(&[
                &format!("The gain holds \\hyp{{H-{}}}.", b.h1_seq), // testing → unresolved
                &format!("Secondary \\hyp{{H-{}}}.", b.h2_seq),      // supported → clean
                "A ghost \\hyp{H-99} claim.",                        // no object → unlinked
                &format!("We assert \\claim{{CLAIMS-{}}}.", b.unpinned_seq), // unpinned → flagged
                &format!("Cite \\claim{{CLAIMS-{}}}.", b.pinned_seq),        // pinned → clean
            ]);
            let scan = register_and_scan(store, b.mission, &dir);
            assert_eq!(scan.error, None);
            assert_eq!(scan.files.len(), 1);
            assert_eq!(scan.files[0].markers.len(), 5);

            let report =
                readiness_report_with_manuscript(&store.events_all().unwrap(), None, &[scan])
                    .unwrap();
            assert_eq!(report.verdict, ReadinessVerdict::NotReady);

            // \hyp{H-h1} on line 3: hypothesis unresolved, referenced by
            // seq + status + marker + file:line. A second unresolved flag
            // rides the unpinned claim's marker (its hypothesis is still
            // testing — the paper claim cites a testing hypothesis).
            let hyp_items: Vec<_> = report
                .blockers
                .iter()
                .filter(|b| b.kind == ReadinessItemKind::ManuscriptHypothesisUnresolved)
                .collect();
            assert_eq!(hyp_items.len(), 2, "the \\hyp marker + the \\claim's hypothesis");
            let hyp_item = hyp_items
                .iter()
                .find(|b| b.manuscript_line == Some(3))
                .expect("the \\hyp{{H-h1}} flag at line 3");
            assert_eq!(hyp_item.hypothesis_seq, Some(b.h1_seq));
            assert_eq!(hyp_item.hypothesis_status, Some(HypothesisStatus::Testing));
            assert_eq!(hyp_item.manuscript_file.as_deref(), Some("main.tex"));
            assert_eq!(
                hyp_item.marker.as_deref(),
                Some(format!("\\hyp{{H-{}}}", b.h1_seq).as_str())
            );

            // \hyp{H-99} on line 5: unlinked — the marker resolves to nothing
            let [unlinked] = report
                .blockers
                .iter()
                .filter(|b| b.kind == ReadinessItemKind::ManuscriptClaimUnlinked)
                .cloned()
                .collect::<Vec<_>>()
                .try_into()
                .ok()
                .expect("exactly one unlinked flag");
            assert_eq!(unlinked.marker.as_deref(), Some("\\hyp{H-99}"));
            assert_eq!(unlinked.manuscript_line, Some(5));

            // \claim{CLAIMS-{unpinned}} on line 6: the unpinned claim,
            // referenced by seq (the stable chip) + the manuscript location
            let [unpinned_item] = report
                .blockers
                .iter()
                .filter(|b| b.kind == ReadinessItemKind::ManuscriptClaimUnpinned)
                .cloned()
                .collect::<Vec<_>>()
                .try_into()
                .ok()
                .expect("exactly one unpinned-claim flag");
            assert_eq!(unpinned_item.claim_seq, Some(b.unpinned_seq));
            assert_eq!(unpinned_item.manuscript_line, Some(6));
            assert_eq!(
                unpinned_item.marker.as_deref(),
                Some(format!("\\claim{{CLAIMS-{}}}", b.unpinned_seq).as_str())
            );

            // the trail: five rows now, the manuscript row 5 markers / 2 clean
            assert_eq!(report.trail.len(), 5, "the board's four + the manuscript row");
            let ms_row = &report.trail[4];
            assert_eq!(ms_row.kind, ReadinessTrailKind::ManuscriptConsistency);
            assert_eq!((ms_row.total, ms_row.clean), (5, 2));
            assert!(ms_row.refs.contains(&"main.tex:4".to_string()));
            assert!(ms_row.refs.contains(&"main.tex:7".to_string()));
            let _ = std::fs::remove_dir_all(&dir);
        });
    }

    /// A clean manuscript reports ready with its trail row; an unreadable
    /// dir is an INFO row (the check could not run — surfaced, never
    /// silently skipped) and adds no trail row.
    #[test]
    fn a_clean_manuscript_extends_the_trail_and_an_unreadable_dir_is_info() {
        with_store(|store| {
            let b = seed_manuscript_board(store);
            // a CLEAN board: pin the unpinned claim, resolve h1
            let claim = store
                .events_all()
                .unwrap()
                .iter()
                .find(|e| e.id == b.unpinned)
                .unwrap()
                .clone();
            seed_pin(store, &claim, b.h1, "X holds at 32k, stated.");
            store
                .append(
                    NewEvent::hypothesis_status_changed(
                        HypothesisStatus::Testing,
                        HypothesisStatus::Supported,
                        "Held.",
                    )
                    .unwrap()
                    .with_causes(vec![b.h1]),
                )
                .unwrap();
            // the clean manuscript: every marker resolves to an answer
            let dir = tex_repo(&[
                &format!("The gain holds \\hyp{{H-{}}}.", b.h1_seq),
                &format!(
                    "Cite \\claim{{CLAIMS-{}}} plus \\claim{{CLAIMS-{}}}.",
                    b.unpinned_seq, b.pinned_seq
                ),
            ]);
            let scan = register_and_scan(store, b.mission, &dir);
            let report =
                readiness_report_with_manuscript(&store.events_all().unwrap(), None, &[scan])
                    .unwrap();
            assert_eq!(report.verdict, ReadinessVerdict::Ready, "board + manuscript clean");
            assert!(report.blockers.is_empty());
            assert_eq!(report.trail.len(), 5);
            let ms_row = &report.trail[4];
            assert_eq!((ms_row.total, ms_row.clean), (3, 3));

            // an unreadable dir: INFO, no trail row, never a silent skip
            let unreadable = ManuscriptScan {
                mission_id: b.mission,
                dir: "/nonexistent/manuscript".into(),
                files: Vec::new(),
                error: Some("invalid_dir: `/nonexistent/manuscript`".into()),
            };
            let report = readiness_report_with_manuscript(
                &store.events_all().unwrap(),
                None,
                &[unreadable],
            )
            .unwrap();
            assert_eq!(report.verdict, ReadinessVerdict::Ready, "info never blocks");
            assert_eq!(report.infos.len(), 1);
            assert_eq!(report.infos[0].kind, ReadinessItemKind::ManuscriptUnreadable);
            assert_eq!(report.trail.len(), 4, "no manuscript was checked — no row");
            let _ = std::fs::remove_dir_all(&dir);
        });
    }

    /// The scope governs the manuscript flags too: a mission-scoped report
    /// sees only its own manuscript's flags — a marker-rich manuscript of
    /// another mission never leaks in.
    #[test]
    fn the_scope_governs_the_manuscript_flags() {
        with_store(|store| {
            let b = seed_manuscript_board(store);
            let other = seed_mission(store);
            let dirty_dir = tex_repo(&["A ghost \\hyp{H-99} claim."]);
            let dirty = register_and_scan(store, b.mission, &dirty_dir);
            // the other mission's manuscript: ZERO markers — nothing to
            // check, no flags (a marker-free repo is clean for this check)
            let clean_dir = tex_repo(&["% no board links here yet."]);
            store
                .append(
                    NewEvent::manuscript_registered(
                        other,
                        &clean_dir.display().to_string(),
                        "main.tex",
                    )
                    .unwrap(),
                )
                .unwrap();
            let events = store.events_all().unwrap();
            let other_registered =
                crate::domain::manuscript::ManuscriptsProjection::for_mission(&events, other)
                    .unwrap()
                    .unwrap();
            let clean = build_scan(&other_registered);
            // workspace: the dirty manuscript's flag blocks
            let workspace =
                readiness_report_with_manuscript(&events, None, &[dirty.clone(), clean.clone()])
                    .unwrap();
            assert!(workspace
                .blockers
                .iter()
                .any(|b| b.kind == ReadinessItemKind::ManuscriptClaimUnlinked));
            // the dirty mission's scope: its flag
            let scoped =
                readiness_report_with_manuscript(&events, Some(b.mission), &[dirty, clean.clone()])
                    .unwrap();
            assert!(scoped
                .blockers
                .iter()
                .any(|b| b.kind == ReadinessItemKind::ManuscriptClaimUnlinked));
            // the clean mission's scope: no manuscript flag (its own scan
            // has none, and the dirty mission's scan is out of scope)
            let other_scoped = readiness_report_with_manuscript(
                &store.events_all().unwrap(),
                Some(other),
                &[clean],
            )
            .unwrap();
            assert!(other_scoped
                .blockers
                .iter()
                .all(|b| !matches!(
                    b.kind,
                    ReadinessItemKind::ManuscriptHypothesisUnresolved
                        | ReadinessItemKind::ManuscriptHypothesisRefuted
                        | ReadinessItemKind::ManuscriptClaimUnpinned
                        | ReadinessItemKind::ManuscriptClaimUnlinked
                )));
            let _ = std::fs::remove_dir_all(&dirty_dir);
            let _ = std::fs::remove_dir_all(&clean_dir);
        });
    }

    /// The FR-13.2 invariant for the manuscript scope: replaying the same
    /// events into a fresh log against the SAME files yields the same
    /// manuscript report — kinds, seq-based references, locations, counts.
    /// The replay mints fresh event ids; everything DERIVED is equal.
    #[test]
    fn replaying_events_and_files_yields_the_same_manuscript_report() {
        fn build(store: &EventStore<'_>, b: &MsBoard, dir: &std::path::Path) -> ReadinessReport {
            let scan = register_and_scan(store, b.mission, dir);
            readiness_report_with_manuscript(&store.events_all().unwrap(), None, &[scan])
                .unwrap()
        }
        let dir = tex_repo(&[
            "The gain holds \\hyp{H-1}.",
            "A ghost \\hyp{H-99} claim.",
            "We assert \\claim{CLAIMS-2}.",
        ]);
        // The labels are REAL chips of the log (H-{seq} / CLAIMS-{seq}):
        // both seeds are byte-identical, so one repo — written from the
        // first seed's seqs — names the same objects for both stores.
        fn write_repo(dir: &std::path::Path, b: &MsBoard) {
            std::fs::write(
                dir.join("main.tex"),
                format!(
                    "\\documentclass{{article}}\n\\begin{{document}}\n\
                     The gain holds \\hyp{{H-{}}}.\n\
                     A ghost \\hyp{{H-99}} claim.\n\
                     We assert \\claim{{CLAIMS-{}}}.\n\
                     \\end{{document}}\n",
                    b.h1_seq, b.unpinned_seq
                ),
            )
            .unwrap();
        }

        let conn_a = Connection::open_in_memory().unwrap();
        EventStore::init(&conn_a).unwrap();
        let store_a = EventStore::new(&conn_a);
        let b_a = seed_manuscript_board(&store_a);
        write_repo(&dir, &b_a);
        let report_a = build(&store_a, &b_a, &dir);

        let conn_b = Connection::open_in_memory().unwrap();
        EventStore::init(&conn_b).unwrap();
        let store_b = EventStore::new(&conn_b);
        let b_b = seed_manuscript_board(&store_b);
        let report_b = build(&store_b, &b_b, &dir);

        assert_eq!(report_a.verdict, report_b.verdict);
        assert_eq!(report_a.blockers.len(), report_b.blockers.len());
        for (a, b) in report_a.blockers.iter().zip(report_b.blockers.iter()) {
            assert_eq!(a.kind, b.kind);
            assert_eq!(a.claim_seq, b.claim_seq);
            assert_eq!(a.hypothesis_seq, b.hypothesis_seq);
            assert_eq!(a.hypothesis_status, b.hypothesis_status);
            assert_eq!(a.manuscript_file, b.manuscript_file);
            assert_eq!(a.manuscript_line, b.manuscript_line);
            assert_eq!(a.marker, b.marker);
            if let (Some(a_id), Some(b_id)) = (a.claim_id, b.claim_id) {
                assert_ne!(a_id, b_id, "fresh log, fresh ids — everything derived is equal");
            }
        }
        assert_eq!(
            report_a
                .trail
                .iter()
                .map(|r| (r.kind, r.total, r.clean, r.refs.clone()))
                .collect::<Vec<_>>(),
            report_b
                .trail
                .iter()
                .map(|r| (r.kind, r.total, r.clean, r.refs.clone()))
                .collect::<Vec<_>>(),
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The marker scanner (FR-20.4): macros and comments, several per line,
    /// 1-based lines, unterminated braces are not markers.
    #[test]
    fn scan_markers_reads_the_convention() {
        let tex = "\\documentclass{article}\n\
                   The gain holds \\hyp{H-1} and \\claim{CLAIMS-2}.\n\
                   % a comment link: \\hyp{H-3}\n\
                   broken \\hyp{H-4\n\
                   done\n";
        let markers = crate::domain::manuscript::scan_markers(tex);
        assert_eq!(markers.len(), 3, "unterminated braces are not markers");
        assert_eq!(
            markers[0],
            Marker { kind: MarkerKind::Hyp, reference: "H-1".into(), line: 2 }
        );
        assert_eq!(
            markers[1],
            Marker { kind: MarkerKind::Claim, reference: "CLAIMS-2".into(), line: 2 }
        );
        assert_eq!(
            markers[2],
            Marker { kind: MarkerKind::Hyp, reference: "H-3".into(), line: 3 }
        );
        let _ = ScanFile { path: "main.tex".into(), markers };
    }
}
