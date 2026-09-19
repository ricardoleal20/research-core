// Run Timeline Receipts (FR-6, Story 2.5): the replayable audit ledger of one
// agent run. A receipt is a PURE QUERY over the existing log (AD-2) — never a
// new log, never a parallel surface: repeating the query yields the identical
// ledger (replay = re-query). Every autonomous action shows — sources touched,
// claims extracted, tools used, cost (FR-6.1) — including the honest moment a
// hard ceiling refused a dispatch (`spend.refused`, never a silent no).
//
// Membership (the AD-2 fold, documented):
//
// - The anchor is the run's `run.started` (payload `run_id`). The terminal is
//   its `run.finished` | `run.failed`; a run without one is `open` — the
//   receipt still renders what the log holds.
// - Run-scoped atoms — `spend.recorded`, `spend.refused` — belong to the run
//   when they fall inside the run's seq window and reference the run's
//   mission (cause or payload `mission_id`). Provider calls run under their
//   own reservation token (AD-10 — the runtime reserves per dispatch), so the
//   token is not the run id; the seq window + mission cause is the honest
//   join. `spend.reserved` rows open those tokens; a `spend.released` settles
//   one — released rows join by the tokens the run opened.
// - `claim.registered` rows are the claims extracted on the mission's board
//   (cause-linked to a hypothesis of the mission, inside the window).
// - `proposal.created` rows join by the ACTOR run id (AD-2): the run's agent
//   actor created them, so they are the run's quarantined output.
// - `merge.approved` | `merge.rejected` | `merge.superseded` — the human's
//   decisions on the run's proposals — join by proposal id and are included
//   whenever they land (a human reviews after the run ends; the receipt
//   answers "what happened to what the run left").
// - The search row derives from the run's step: v1 Night Shift runs one
//   `literature-scan` step whose query is the mission's question (the scan
//   task is built from it — `nightshift::run_scan`). A dedicated search event
//   kind is FR-12's; until then the receipt says what actually ran, never
//   invents sources.
//
// Rows render in `seq` order with mono timestamps and `e-{seq}` refs — the
// OpenDesign receipts frame's ledger anatomy. The header reuses the missions
// fold for the spend-vs-ceiling line ("82¢ of 100¢") and the run's own call
// rows for its spend and models.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::evidence::CLAIM_REGISTERED;
use crate::domain::hypotheses::HypothesesProjection;
use crate::domain::missions::MissionsProjection;
use crate::domain::nightshift::{RUN_FAILED, RUN_FINISHED, RUN_STARTED, SCAN_STEP};
use crate::domain::proposals::{ProposalsProjection, MERGE_APPROVED, MERGE_REJECTED, MERGE_SUPERSEDED};
use crate::domain::spend::SPEND_RECORDED;
use crate::domain::trust::{SPEND_REFUSED, SPEND_RELEASED, SPEND_RESERVED};
use crate::eventstore::{EventError, StoredEvent};

/// The run's terminal state for the outcome chip (DESIGN.md receipt frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Finished,
    Failed,
    /// Started but not terminal (yet) — the ledger renders what the log holds.
    Open,
}

/// One ledger row: the event's audit seq (the `e-{seq}` ref) and timestamp,
/// plus the typed action the row renders. The `kind` tag discriminates the
/// wire form the UI composes its bilingual one-liners from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptRow {
    pub seq: i64,
    pub ts: DateTime<Utc>,
    #[serde(flatten)]
    pub action: ReceiptAction,
}

/// The typed action vocabulary of the ledger (the receipt frame's chips):
/// run start, search, provider call, claim, merge proposal (quarantined),
/// the human's decision on one, the honest ceiling refusal, a released
/// reservation, and the run's end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ReceiptAction {
    RunStart {
        /// The step the run opened (`literature-scan` in v1).
        step: String,
        schedule: String,
    },
    /// The search the run performed — v1: the scan step over the mission's
    /// question (the query the scan task was built from).
    Search {
        query: String,
    },
    /// One provider call: who was called, on which model, tokens, cost
    /// (FR-6.1 — from the call's `spend.recorded`).
    Call {
        provider: String,
        model: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        input_tokens: u64,
        output_tokens: u64,
        cost_cents: u64,
    },
    /// One claim extracted on the mission's board during the run.
    Claim {
        text: String,
    },
    /// One quarantined merge proposal the run left (`pr-{seq}` label).
    Proposal {
        proposal_id: Uuid,
        proposal_seq: i64,
        /// The transition the proposal intends (the `to` status).
        to: String,
        /// The proposal's current status — pending until a human decides.
        status: String,
    },
    /// The human's decision on one of the run's proposals — merged,
    /// rejected, or superseded (a sibling merge's side effect).
    Decision {
        proposal_id: Uuid,
        proposal_seq: i64,
        decision: String,
    },
    /// The honest refusal: a ceiling the dispatch would have crossed — the
    /// row the frame renders as the alert line ("cost ceiling hit — dispatch
    /// refused").
    Refused {
        scope: String,
        ceiling_cents: u64,
        would_be_cost_cents: u64,
    },
    /// A reservation the run opened settled without spending (canceled —
    /// provider error or skip).
    Released {
        reason: String,
    },
    /// The run's terminal: finished (with its one-line verdict) or failed
    /// (with its code-form reason).
    RunEnd {
        outcome: RunOutcome,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        verdict: Option<String>,
    },
}

impl ReceiptAction {
    /// The row's chip kind (the wire discriminant) — for tests and callers
    /// that match on the rendered vocabulary without re-serializing.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::RunStart { .. } => "run_start",
            Self::Search { .. } => "search",
            Self::Call { .. } => "call",
            Self::Claim { .. } => "claim",
            Self::Proposal { .. } => "proposal",
            Self::Decision { .. } => "decision",
            Self::Refused { .. } => "refused",
            Self::Released { .. } => "released",
            Self::RunEnd { .. } => "run_end",
        }
    }
}

/// The run receipt (FR-6.1): the run's meta header — outcome chip, duration,
/// spend vs ceiling, the models it called — plus the ordered ledger. A pure
/// projection: two folds of the same log yield the identical receipt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReceipt {
    pub run_id: String,
    pub mission_id: Uuid,
    /// The mission's creation seq — the `M-n` breadcrumb label.
    pub mission_seq: i64,
    pub outcome: RunOutcome,
    /// A ceiling refused one of the run's dispatches (a `spend.refused` row
    /// exists, or the run failed on `cost_ceiling_reached`).
    pub ceiling_hit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    pub started_ts: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_ts: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<i64>,
    /// The run's recorded spend (the sum of its call rows' costs) vs the
    /// mission's folded ceiling — the "82¢ of 100¢" line.
    pub spend_cents: u64,
    pub ceiling_cents: u64,
    /// The models the run called, in first-call order (the meta line).
    pub models: Vec<String>,
    /// The ordered ledger — `seq` order, the run's every logged atom.
    pub rows: Vec<ReceiptRow>,
}

/// Rows in construction order first, sorted by `(seq, order)` afterwards —
/// stable and identical on every replay (rows derived from one event keep
/// their construction order: run start before its search row).
#[derive(Default)]
struct RowBuilder {
    order: usize,
    rows: Vec<(i64, usize, ReceiptRow)>,
}

impl RowBuilder {
    fn push(&mut self, seq: i64, row: ReceiptRow) {
        self.rows.push((seq, self.order, row));
        self.order += 1;
    }

    fn finish(mut self) -> Vec<ReceiptRow> {
        self.rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        self.rows.into_iter().map(|(_, _, row)| row).collect()
    }
}

/// Does this event reference the given mission (payload id, then causes)?
fn references_mission(event: &StoredEvent, mission_id: Uuid) -> bool {
    event.causes.contains(&mission_id)
        || event
            .payload
            .get("mission_id")
            .and_then(serde_json::Value::as_str)
            .map(|s| s == mission_id.to_string())
            .unwrap_or(false)
}

/// The event's payload `run_id`, when it carries one.
fn payload_run_id(event: &StoredEvent) -> Option<&str> {
    event
        .payload
        .get("run_id")
        .and_then(serde_json::Value::as_str)
}

/// The proposal id a merge decision event references, when it parses.
fn decision_target(event: &StoredEvent) -> Option<Uuid> {
    event
        .payload
        .get("proposal_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
}

/// Fold one run's receipt over the log (pure — no IO). `None` when no
/// `run.started` carries the run id: a run id without a run has no receipt
/// (manual step reservation tokens, for instance, are not runs). Corrupt
/// payloads fail loudly — the projections' contract, never a silent gap.
pub fn render_receipt(
    events: &[StoredEvent],
    run_id: &str,
) -> Result<Option<RunReceipt>, EventError> {
    // The shared fold cursor (AD-1, Story 2.6): the receipt folds the LIVE
    // events — a run orphaned by a rollback has no receipt to replay (its
    // run.started is superseded history), and a live run's receipt drops
    // orphaned rows. Re-query still replays; it replays the read model.
    let cursor = crate::domain::checkpoints::FoldCursor::over(events);
    let events = &cursor.live_owned(events);
    // The anchor: the run's started event.
    let Some(started) = events
        .iter()
        .find(|e| e.kind == RUN_STARTED && payload_run_id(e) == Some(run_id))
    else {
        return Ok(None);
    };
    let mission_id = started
        .payload
        .get("mission_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .or_else(|| started.causes.first().copied())
        .ok_or_else(|| {
            EventError::Invalid(format!(
                "corrupt {RUN_STARTED} payload at seq {}: no mission link",
                started.seq
            ))
        })?;
    let schedule = started
        .payload
        .get("schedule")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let step = started
        .payload
        .get("step")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();

    // The terminal: the run's finished/failed event (first one — a run ends
    // once; a second terminal would be a corrupt log the fold still renders).
    let terminal = events.iter().find(|e| {
        matches!(e.kind.as_str(), RUN_FINISHED | RUN_FAILED)
            && payload_run_id(e) == Some(run_id)
    });

    // The mission (ceiling, seq label) and the board (claim membership) —
    // the projections this query reuses rather than re-implements.
    let missions = MissionsProjection::fold(events)?;
    let mission = missions.iter().find(|m| m.id == mission_id).ok_or_else(|| {
        EventError::Invalid(format!(
            "corrupt log: run `{run_id}` references mission {mission_id} that does not fold"
        ))
    })?;
    let board_ids: HashSet<Uuid> = HypothesesProjection::fold_for(events, mission_id)?
        .into_iter()
        .map(|h| h.id)
        .collect();
    let proposals = ProposalsProjection::fold(events)?;
    // The run's quarantined output: proposals its agent actor created (AD-2 —
    // the query over actor run-ids).
    let run_proposals: Vec<&crate::domain::proposals::Proposal> =
        proposals.iter().filter(|p| p.run_id == run_id).collect();
    let proposal_seqs: HashMap<Uuid, i64> = run_proposals
        .iter()
        .map(|p| (p.id, p.seq))
        .collect();

    // The run's seq window: (start, terminal] — open-ended while the run has
    // no terminal (the ledger renders what the log holds so far).
    let start_seq = started.seq;
    let end_seq = terminal.map_or(i64::MAX, |t| t.seq);
    let in_window = |seq: i64| seq > start_seq && seq <= end_seq;

    // The reservation tokens the run's dispatches ran under (AD-10): the
    // run id itself plus every token a `spend.reserved` inside the window
    // opened against the mission — released rows join by these.
    let mut tokens: HashSet<String> = HashSet::from([run_id.to_string()]);
    for event in events {
        if event.kind == SPEND_RESERVED
            && in_window(event.seq)
            && references_mission(event, mission_id)
        {
            if let Some(token) = payload_run_id(event) {
                tokens.insert(token.to_string());
            }
        }
    }

    let mut rows = RowBuilder::default();

    // The anchor row: the run started.
    rows.push(
        started.seq,
        ReceiptRow {
            seq: started.seq,
            ts: started.ts,
            action: ReceiptAction::RunStart {
                step: step.clone(),
                schedule: schedule.clone(),
            },
        },
    );
    // The search the run ran (v1: the literature-scan step over the mission's
    // question — the query the scan task was built from; FR-12's dedicated
    // search events land later, and until then the receipt names what ran).
    if step == SCAN_STEP {
        rows.push(
            started.seq,
            ReceiptRow {
                seq: started.seq,
                ts: started.ts,
                action: ReceiptAction::Search {
                    query: mission.question.clone(),
                },
            },
        );
    }

    // The run's atoms, in log order.
    let mut spend_cents = 0u64;
    let mut models: Vec<String> = Vec::new();
    let mut ceiling_hit = false;
    for event in events {
        let belongs = in_window(event.seq)
            && (references_mission(event, mission_id)
                || payload_run_id(event) == Some(run_id)
                || payload_run_id(event).is_some_and(|t| tokens.contains(t)));
        match event.kind.as_str() {
            SPEND_RECORDED if belongs => {
                let provider = event
                    .payload
                    .get("provider")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let model = event
                    .payload
                    .get("model")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let input_tokens = event
                    .payload
                    .get("input_tokens")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let output_tokens = event
                    .payload
                    .get("output_tokens")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let cost = event
                    .payload
                    .get("cost_cents")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let role = event
                    .payload
                    .get("role")
                    .and_then(serde_json::Value::as_str)
                    .map(String::from);
                spend_cents += cost;
                if !model.is_empty() && !models.contains(&model) {
                    models.push(model.clone());
                }
                rows.push(
                    event.seq,
                    ReceiptRow {
                        seq: event.seq,
                        ts: event.ts,
                        action: ReceiptAction::Call {
                            provider,
                            model,
                            role,
                            input_tokens,
                            output_tokens,
                            cost_cents: cost,
                        },
                    },
                );
            }
            SPEND_REFUSED if belongs => {
                ceiling_hit = true;
                rows.push(
                    event.seq,
                    ReceiptRow {
                        seq: event.seq,
                        ts: event.ts,
                        action: ReceiptAction::Refused {
                            scope: event
                                .payload
                                .get("scope")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            ceiling_cents: event
                                .payload
                                .get("ceiling_cents")
                                .and_then(serde_json::Value::as_u64)
                                .unwrap_or(0),
                            would_be_cost_cents: event
                                .payload
                                .get("would_be_cost_cents")
                                .and_then(serde_json::Value::as_u64)
                                .unwrap_or(0),
                        },
                    },
                );
            }
            SPEND_RELEASED
                if in_window(event.seq)
                    && payload_run_id(event).is_some_and(|t| tokens.contains(t)) =>
            {
                rows.push(
                    event.seq,
                    ReceiptRow {
                        seq: event.seq,
                        ts: event.ts,
                        action: ReceiptAction::Released {
                            reason: event
                                .payload
                                .get("reason")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        },
                    },
                );
            }
            CLAIM_REGISTERED
                if in_window(event.seq)
                    && event
                        .payload
                        .get("hypothesis_id")
                        .and_then(serde_json::Value::as_str)
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .is_some_and(|h| board_ids.contains(&h)) =>
            {
                rows.push(
                    event.seq,
                    ReceiptRow {
                        seq: event.seq,
                        ts: event.ts,
                        action: ReceiptAction::Claim {
                            text: event
                                .payload
                                .get("text")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        },
                    },
                );
            }
            MERGE_APPROVED
                if decision_target(event).is_some_and(|id| proposal_seqs.contains_key(&id)) =>
            {
                rows.push(
                    event.seq,
                    ReceiptRow {
                        seq: event.seq,
                        ts: event.ts,
                        action: ReceiptAction::Decision {
                            proposal_id: decision_target(event).unwrap_or_default(),
                            proposal_seq: proposal_seqs
                                .get(&decision_target(event).unwrap_or_default())
                                .copied()
                                .unwrap_or(0),
                            decision: "merged".into(),
                        },
                    },
                );
            }
            MERGE_REJECTED
                if decision_target(event).is_some_and(|id| proposal_seqs.contains_key(&id)) =>
            {
                rows.push(
                    event.seq,
                    ReceiptRow {
                        seq: event.seq,
                        ts: event.ts,
                        action: ReceiptAction::Decision {
                            proposal_id: decision_target(event).unwrap_or_default(),
                            proposal_seq: proposal_seqs
                                .get(&decision_target(event).unwrap_or_default())
                                .copied()
                                .unwrap_or(0),
                            decision: "rejected".into(),
                        },
                    },
                );
            }
            MERGE_SUPERSEDED
                if decision_target(event).is_some_and(|id| proposal_seqs.contains_key(&id)) =>
            {
                rows.push(
                    event.seq,
                    ReceiptRow {
                        seq: event.seq,
                        ts: event.ts,
                        action: ReceiptAction::Decision {
                            proposal_id: decision_target(event).unwrap_or_default(),
                            proposal_seq: proposal_seqs
                                .get(&decision_target(event).unwrap_or_default())
                                .copied()
                                .unwrap_or(0),
                            decision: "superseded".into(),
                        },
                    },
                );
            }
            _ => {}
        }
    }

    // The run's quarantined proposals (actor run id — whenever they landed).
    for proposal in &run_proposals {
        let to = proposal
            .proposed_payload
            .get("to")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        rows.push(
            proposal.seq,
            ReceiptRow {
                seq: proposal.seq,
                ts: proposal.ts,
                action: ReceiptAction::Proposal {
                    proposal_id: proposal.id,
                    proposal_seq: proposal.seq,
                    to,
                    status: proposal.status.as_str().to_string(),
                },
            },
        );
    }

    // The terminal row.
    let (outcome, reason, verdict, ended_ts) = match terminal {
        Some(t) if t.kind == RUN_FINISHED => {
            let verdict = t
                .payload
                .get("verdict")
                .and_then(serde_json::Value::as_str)
                .map(String::from);
            rows.push(
                t.seq,
                ReceiptRow {
                    seq: t.seq,
                    ts: t.ts,
                    action: ReceiptAction::RunEnd {
                        outcome: RunOutcome::Finished,
                        reason: None,
                        verdict: verdict.clone(),
                    },
                },
            );
            (RunOutcome::Finished, None, verdict, Some(t.ts))
        }
        Some(t) => {
            let reason = t
                .payload
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .map(String::from);
            if reason.as_deref() == Some(crate::domain::trust::COST_CEILING_RUN_REASON) {
                ceiling_hit = true;
            }
            rows.push(
                t.seq,
                ReceiptRow {
                    seq: t.seq,
                    ts: t.ts,
                    action: ReceiptAction::RunEnd {
                        outcome: RunOutcome::Failed,
                        reason: reason.clone(),
                        verdict: None,
                    },
                },
            );
            (RunOutcome::Failed, reason, None, Some(t.ts))
        }
        None => (RunOutcome::Open, None, None, None),
    };

    Ok(Some(RunReceipt {
        run_id: run_id.to_string(),
        mission_id,
        mission_seq: mission.seq,
        outcome,
        ceiling_hit,
        reason,
        verdict,
        started_ts: started.ts,
        ended_ts,
        duration_secs: ended_ts.map(|end| (end - started.ts).num_seconds()),
        spend_cents,
        ceiling_cents: mission.spend_ceiling_cents,
        models,
        rows: rows.finish(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::hypotheses::HypothesisStatus;
    use crate::domain::missions::{Autonomy, MissionCreatedPayload, MISSION_CREATED};
    use crate::domain::spend::SpendRecordedPayload;
    use crate::domain::trust::{
        SpendRefusedPayload, SpendReleasedPayload, SpendReservedPayload,
    };
    use crate::eventstore::{EventStore, NewEvent, StoredEvent};
    use rusqlite::Connection;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    fn seed_mission(store: &EventStore<'_>) -> StoredEvent {
        store
            .append(
                NewEvent::mission_created(MissionCreatedPayload {
                    question: "Does retrieval grounding reduce hallucinated citations?".into(),
                    stop_condition: "Stop after $5 spent.".into(),
                    success_criterion: "A blind rater finds zero fabricated citations.".into(),
                    autonomy: Autonomy::Suggest,
                    spend_ceiling_cents: 100,
                    roles: vec![],
                    schedule: "daily-03:00".into(),
                })
                .unwrap(),
            )
            .unwrap()
    }

    /// A full honest run: start, a reserved+recorded call under a `step-`
    /// reservation token (the runtime's per-dispatch token — NOT the run id),
    /// a claim extracted, a quarantined proposal, the finish.
    fn seed_full_run(store: &EventStore<'_>, mission_id: Uuid, run_id: &str) {
        store
            .append(NewEvent::run_started(run_id, mission_id, "daily-03:00", SCAN_STEP).unwrap())
            .unwrap();
        // the dispatch's reservation (its own token) + the recorded spend
        let token = format!("step-{}", Uuid::new_v4().simple());
        store
            .append(
                NewEvent::spend_reserved(SpendReservedPayload {
                    run_id: token.clone(),
                    target: "openrouter".into(),
                    amount_cents: 20,
                    mission_id: Some(mission_id),
                })
                .unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::spend_recorded(SpendRecordedPayload {
                    provider: "openrouter".into(),
                    model: "GLM-5.3".into(),
                    input_tokens: 3_812,
                    output_tokens: 964,
                    cost_cents: 9,
                    mission_id: Some(mission_id),
                    role: Some("drafter".into()),
                    run_id: Some(token),
                })
                .unwrap(),
            )
            .unwrap();
        // a claim extracted on the mission's board
        let hyp = store
            .append(
                NewEvent::hypothesis_created("Grounding reduces fabrications.", mission_id)
                    .unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::claim_registered(
                    "Passage length conditions the grounding effect.",
                    hyp.id,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        // the run's quarantined proposal
        crate::domain::proposals::propose_transition(
            store,
            run_id,
            hyp.id,
            HypothesisStatus::Testing,
            "the scan found three new sources",
        )
        .unwrap();
        store
            .append(
                NewEvent::run_finished(run_id, mission_id, "1 scan · 1 proposal pending", 1)
                    .unwrap(),
            )
            .unwrap();
    }

    #[test]
    fn a_full_run_receipt_contains_every_logged_atom_type_in_seq_order() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let mission = seed_mission(&store);
        seed_full_run(&store, mission.id, "ns-42");
        let events = store.events_all().unwrap();
        let receipt = render_receipt(&events, "ns-42")
            .unwrap()
            .expect("the run started — it has a receipt");

        assert_eq!(receipt.run_id, "ns-42");
        assert_eq!(receipt.mission_seq, mission.seq);
        assert_eq!(receipt.outcome, RunOutcome::Finished);
        assert_eq!(receipt.verdict.as_deref(), Some("1 scan · 1 proposal pending"));
        assert_eq!(receipt.spend_cents, 9, "the run's spend folds from its call rows");
        assert_eq!(receipt.ceiling_cents, 100);
        assert_eq!(receipt.models, vec!["GLM-5.3".to_string()]);
        assert!(!receipt.ceiling_hit);
        assert!(receipt.duration_secs.is_some());

        // every logged atom type, in seq order
        let kinds: Vec<&str> = receipt.rows.iter().map(|r| r.action.kind()).collect();
        assert_eq!(
            kinds,
            vec![
                "run_start",   // the run.started anchor
                "search",      // the literature-scan step over the mission question
                "call",        // the reserved+recorded provider call (its own token)
                "claim",       // the claim extracted on the board
                "proposal",    // the quarantined proposal (actor run id)
                "run_end",     // the finish
            ],
            "unexpected ledger: {kinds:?}"
        );
        // seq order, and every row carries its e-seq ref + timestamp
        let seqs: Vec<i64> = receipt.rows.iter().map(|r| r.seq).collect();
        let mut sorted = seqs.clone();
        sorted.sort();
        assert_eq!(seqs, sorted, "rows render in seq order");
        // the call row carries the spend atoms (provider, tokens, cost)
        let call = receipt
            .rows
            .iter()
            .find(|r| r.action.kind() == "call")
            .unwrap();
        match &call.action {
            ReceiptAction::Call {
                provider,
                model,
                input_tokens,
                output_tokens,
                cost_cents,
                role,
            } => {
                assert_eq!(provider, "openrouter");
                assert_eq!(model, "GLM-5.3");
                assert_eq!(*input_tokens, 3_812);
                assert_eq!(*output_tokens, 964);
                assert_eq!(*cost_cents, 9);
                assert_eq!(role.as_deref(), Some("drafter"));
            }
            other => panic!("expected a call row, got {other:?}"),
        }
        // the search row carries the scan's query (the mission question)
        let search = receipt
            .rows
            .iter()
            .find(|r| r.action.kind() == "search")
            .unwrap();
        match &search.action {
            ReceiptAction::Search { query } => assert!(query.contains("retrieval grounding")),
            other => panic!("expected a search row, got {other:?}"),
        }
        // the proposal row names its pr-seq label and pending status
        let proposal = receipt
            .rows
            .iter()
            .find(|r| r.action.kind() == "proposal")
            .unwrap();
        match &proposal.action {
            ReceiptAction::Proposal { to, status, proposal_seq, .. } => {
                assert_eq!(to, "testing");
                assert_eq!(status, "pending");
                assert!(*proposal_seq > 0);
            }
            other => panic!("expected a proposal row, got {other:?}"),
        }
    }

    #[test]
    fn the_receipt_replays_identically_from_the_events() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let mission = seed_mission(&store);
        seed_full_run(&store, mission.id, "ns-42");
        // replay = re-query: two folds of the same log yield the identical
        // ledger — the receipt is a projection, never a new log (AD-2)
        let events = store.events_all().unwrap();
        let first = render_receipt(&events, "ns-42").unwrap().unwrap();
        let second = render_receipt(&events, "ns-42").unwrap().unwrap();
        assert_eq!(first, second);
        // and the wire form is stable too — what the drill-down renders is
        // what it re-renders
        assert_eq!(
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap()
        );
        // the wire form is camelCase (Tauri 2 convention) with the kind tag
        let wire: serde_json::Value = serde_json::to_value(&first).unwrap();
        assert_eq!(wire["runId"], "ns-42");
        assert_eq!(wire["durationSecs"], wire["durationSecs"]);
        let call = &wire["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == "call")
            .unwrap();
        assert_eq!(call["inputTokens"], 3_812);
        assert_eq!(call["costCents"], 9);
        // unrelated later events never mutate a closed run's receipt
        let other = seed_mission(&store);
        seed_full_run(&store, other.id, "ns-43");
        let events = store.events_all().unwrap();
        let third = render_receipt(&events, "ns-42").unwrap().unwrap();
        assert_eq!(first, third, "a closed run's receipt is immutable");
    }

    #[test]
    fn the_refused_row_renders_only_when_a_refusal_exists_for_the_run() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let mission = seed_mission(&store);
        // run A: honest ceiling refusal — the runtime refused the dispatch,
        // the run failed on cost_ceiling_reached
        store
            .append(NewEvent::run_started("ns-a", mission.id, "daily-03:00", SCAN_STEP).unwrap())
            .unwrap();
        let token = "step-refused";
        store
            .append(
                NewEvent::spend_reserved(SpendReservedPayload {
                    run_id: token.into(),
                    target: "openrouter".into(),
                    amount_cents: 20,
                    mission_id: Some(mission.id),
                })
                .unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::spend_refused(SpendRefusedPayload {
                    run_id: token.into(),
                    scope: "mission".into(),
                    ceiling_cents: 100,
                    would_be_cost_cents: 101,
                    mission_id: Some(mission.id),
                    target: Some("openrouter".into()),
                })
                .unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::run_failed(
                    "ns-a",
                    mission.id,
                    crate::domain::trust::COST_CEILING_RUN_REASON,
                    Utc::now(),
                )
                .unwrap(),
            )
            .unwrap();
        // run B: a clean finish, no refusal
        let mission_b = seed_mission(&store);
        store
            .append(NewEvent::run_started("ns-b", mission_b.id, "daily-03:00", SCAN_STEP).unwrap())
            .unwrap();
        store
            .append(NewEvent::run_finished("ns-b", mission_b.id, "nothing new", 0).unwrap())
            .unwrap();

        let events = store.events_all().unwrap();
        let a = render_receipt(&events, "ns-a").unwrap().unwrap();
        assert_eq!(a.outcome, RunOutcome::Failed);
        assert!(a.ceiling_hit, "the refusal marks the receipt");
        assert!(
            a.rows.iter().any(|r| r.action.kind() == "refused"),
            "the honest refused row is present: {:?}",
            a.rows.iter().map(|r| r.action.kind()).collect::<Vec<_>>()
        );
        // the alert-line anatomy: scope, ceiling, would-be cost
        let refused = a
            .rows
            .iter()
            .find(|r| r.action.kind() == "refused")
            .unwrap();
        match &refused.action {
            ReceiptAction::Refused { scope, ceiling_cents, would_be_cost_cents } => {
                assert_eq!(scope, "mission");
                assert_eq!(*ceiling_cents, 100);
                assert_eq!(*would_be_cost_cents, 101);
            }
            other => panic!("expected a refused row, got {other:?}"),
        }

        let b = render_receipt(&events, "ns-b").unwrap().unwrap();
        assert_eq!(b.outcome, RunOutcome::Finished);
        assert!(!b.ceiling_hit);
        assert!(
            !b.rows.iter().any(|r| r.action.kind() == "refused"),
            "no refusal, no refused row — the row renders only from spend.refused"
        );
    }

    #[test]
    fn a_released_reservation_renders_as_a_row_of_its_run() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let mission = seed_mission(&store);
        store
            .append(NewEvent::run_started("ns-r", mission.id, "daily-03:00", SCAN_STEP).unwrap())
            .unwrap();
        let token = "step-died";
        store
            .append(
                NewEvent::spend_reserved(SpendReservedPayload {
                    run_id: token.into(),
                    target: "openrouter".into(),
                    amount_cents: 20,
                    mission_id: Some(mission.id),
                })
                .unwrap(),
            )
            .unwrap();
        store
            .append(
                NewEvent::spend_released(SpendReleasedPayload {
                    run_id: token.into(),
                    reason: crate::domain::trust::RELEASE_PROVIDER_ERROR.into(),
                })
                .unwrap(),
            )
            .unwrap();
        store
            .append(NewEvent::run_failed("ns-r", mission.id, "provider_error", Utc::now()).unwrap())
            .unwrap();
        let events = store.events_all().unwrap();
        let receipt = render_receipt(&events, "ns-r").unwrap().unwrap();
        let released = receipt
            .rows
            .iter()
            .find(|r| r.action.kind() == "released")
            .expect("the canceled reservation renders as a row");
        match &released.action {
            ReceiptAction::Released { reason } => assert_eq!(reason, "provider_error"),
            other => panic!("expected a released row, got {other:?}"),
        }
        assert_eq!(
            receipt.spend_cents, 0,
            "nothing was spent — the reservation was released"
        );
    }

    #[test]
    fn a_human_decision_on_a_run_proposal_lands_in_the_receipt_after_the_run() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let mission = seed_mission(&store);
        seed_full_run(&store, mission.id, "ns-42");
        // the human reviews the NEXT MORNING — after the run's window closed
        let events_before = store.events_all().unwrap();
        let proposal_id = {
            let proposals = ProposalsProjection::fold(&events_before).unwrap();
            proposals
                .iter()
                .find(|p| p.run_id == "ns-42")
                .map(|p| p.id)
                .unwrap()
        };
        let proposal_seq = ProposalsProjection::fold(&events_before)
            .unwrap()
            .iter()
            .find(|p| p.id == proposal_id)
            .map(|p| p.seq)
            .unwrap();
        store
            .append(NewEvent::merge_approved(proposal_id, false, false).unwrap())
            .unwrap();
        let events = store.events_all().unwrap();
        let receipt = render_receipt(&events, "ns-42").unwrap().unwrap();
        let decision = receipt
            .rows
            .iter()
            .find(|r| r.action.kind() == "decision")
            .expect("the decision on the run's proposal lands in its receipt");
        match &decision.action {
            ReceiptAction::Decision { proposal_id: id, decision, proposal_seq: seq } => {
                assert_eq!(*id, proposal_id);
                assert_eq!(decision, "merged");
                assert_eq!(*seq, proposal_seq, "the row carries the pr-n label seq");
            }
            other => panic!("expected a decision row, got {other:?}"),
        }
        // the proposal row now shows the merged status
        let proposal = receipt
            .rows
            .iter()
            .find(|r| r.action.kind() == "proposal")
            .unwrap();
        match &proposal.action {
            ReceiptAction::Proposal { status, .. } => assert_eq!(status, "merged"),
            other => panic!("expected a proposal row, got {other:?}"),
        }
    }

    #[test]
    fn a_run_without_a_start_has_no_receipt_and_an_open_run_renders_what_it_holds() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let mission = seed_mission(&store);
        // a reservation token is not a run — no run.started, no receipt
        let events = store.events_all().unwrap();
        assert!(render_receipt(&events, "step-not-a-run").unwrap().is_none());
        assert!(render_receipt(&events, "never-heard-of").unwrap().is_none());
        // an open run (started, not terminal) renders its rows honestly
        store
            .append(NewEvent::run_started("ns-open", mission.id, "daily-03:00", SCAN_STEP).unwrap())
            .unwrap();
        let events = store.events_all().unwrap();
        let receipt = render_receipt(&events, "ns-open").unwrap().unwrap();
        assert_eq!(receipt.outcome, RunOutcome::Open);
        assert!(receipt.ended_ts.is_none());
        assert!(receipt.duration_secs.is_none());
        assert!(receipt.rows.iter().any(|r| r.action.kind() == "run_start"));
        assert!(!receipt.rows.iter().any(|r| r.action.kind() == "run_end"));
    }

    #[test]
    fn atoms_of_other_runs_and_missions_never_leak_into_a_receipt() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let a = seed_mission(&store);
        let b = seed_mission(&store);
        seed_full_run(&store, a.id, "ns-a");
        seed_full_run(&store, b.id, "ns-b");
        let events = store.events_all().unwrap();
        let receipt = render_receipt(&events, "ns-a").unwrap().unwrap();
        assert_eq!(receipt.mission_id, a.id);
        // exactly one call, one claim, one proposal — nothing from ns-b
        assert_eq!(receipt.rows.iter().filter(|r| r.action.kind() == "call").count(), 1);
        assert_eq!(receipt.rows.iter().filter(|r| r.action.kind() == "claim").count(), 1);
        assert_eq!(receipt.rows.iter().filter(|r| r.action.kind() == "proposal").count(), 1);
        let _ = MISSION_CREATED; // the creation kind the fold anchors missions on
    }
}
