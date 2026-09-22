// Morning Digest projection (FR-4.4/4.3, Story 2.3): a PURE fold over the
// last night's runs — ≤10 rows, one per mission, each a one-line verdict
// with its status chip and a receipts link; the run-outcome badge and the
// spend-vs-ceiling line come from the missions fold (Story 1.6/2.1
// role-tagged spend). The digest NEVER depends on run success: a failed run
// produces an honest row carrying its reason (FR-4.3), and a run that died
// with a stale heartbeat produces the dead-man-switch alert row (FR-9.1
// hook — Story 2.6 builds detection; here the row renders from the
// `stale_heartbeat` run.failed case).
//
// The window is the last 24 hours of run events — "last night" in digest
// terms. Rows cap at 10 (a >10-run night truncates newest-first, stable);
// alert rows are counted separately, matching the OpenDesign digest frame
// ("6 rows · 1 alert").

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::jobs::{JobLifecyclePayload, JOB_FAILED, JOB_FINISHED};
use crate::domain::missions::{MissionStatus, MissionsProjection};
use crate::domain::nightshift::{RUN_FAILED, RUN_FINISHED, RUN_STARTED};
use crate::domain::proposals::{ProposalStatus, ProposalsProjection};
use crate::domain::telemetry::{connection_health, RUN_DEAD};
use crate::eventstore::{EventError, StoredEvent};

/// The digest rows cap (FR-4.4): a >10-run night truncates newest-first.
pub const DIGEST_ROW_CAP: usize = 10;

/// The digest window: runs in the last 24 hours.
pub const DIGEST_WINDOW_HOURS: i64 = 24;

/// The run-outcome badge of the digest header (DESIGN.md digest frame):
/// everything finished, some failed, everything failed, or nothing ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestOutcome {
    NoRuns,
    AllFinished,
    PartialSuccess,
    AllFailed,
}

/// One digest row: one mission's night, in the structured form the UI
/// composes its bilingual one-line verdict from (≤2 rendered lines — the
/// verdict line plus its translation; `verdict_line` is the renderer-side
/// mono contract this story's tests enforce).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DigestRow {
    pub mission_id: Uuid,
    /// The mission's creation seq — the `M-n` label derives from it.
    pub mission_seq: i64,
    pub question: String,
    /// The mission's lifecycle chip (the fold's status — terminal states
    /// show after the AD-12 evaluator decided).
    pub status: MissionStatus,
    /// Runs in the window.
    pub runs: u32,
    pub finished: u32,
    pub failed: u32,
    /// The honest failure: the latest failed run's reason, in code form —
    /// present whenever a run failed (FR-4.3).
    pub failure_reason: Option<String>,
    /// The mission's spend reached its ceiling — the seam Story 2.4 wires
    /// enforcement to; the digest shows it (the row says "no further runs").
    pub ceiling_reached: bool,
    /// Proposals the night left in quarantine, still pending review (FR-4.2).
    pub proposals_pending: u32,
    /// The mission's folded spend vs its ceiling (AD-10) — the digest's
    /// spend line reflects the fold, not a re-count.
    pub spend_cents: u64,
    pub ceiling_cents: u64,
    /// The seq of the mission's latest `run.started` in the window — the
    /// receipts link's anchor (receipts open in the run view).
    pub receipt_seq: i64,
    /// The latest run's id (Story 2.5, FR-6.2): the receipts drill-down's
    /// target — the row's "receipts →" link opens this run's receipt.
    pub run_id: String,
    pub last_run_ts: DateTime<Utc>,
    /// Remote job completions in the window (Story 3.4, FR-11.5): the
    /// morning digest reports them with one-line verdicts — a night where
    /// only jobs ran (no scan) still earns its row.
    pub jobs_finished: u32,
    pub jobs_failed: u32,
    /// The latest completed job's one-line verdict (target · job ·
    /// finished/failed) — the structured form the UI composes its bilingual
    /// line from. `None` when no job completed in the window.
    pub job_verdict: Option<DigestJobVerdict>,
}

/// One remote job completion's verdict line (Story 3.4): which target, which
/// job, finished or failed (with the reason — no job ends silently, AD-12).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DigestJobVerdict {
    /// The named compute target the job ran on.
    pub target: String,
    /// The job's id (its `job.submitted` event id) — the UI renders it short.
    pub job_id: Uuid,
    pub failed: bool,
    /// The failure reason, in code form (present iff failed).
    pub reason: Option<String>,
}

impl DigestRow {
    /// The one-line mono verdict (the ≤2-line row contract's first line —
    /// EN here; the UI renders its bilingual pair). Never contains a
    /// newline and stays under the digest row budget.
    pub fn verdict_line(&self) -> String {
        let label = format!("M-{}", self.mission_seq);
        if self.failed > 0 && self.finished == 0 {
            let reason = self.failure_reason.as_deref().unwrap_or("unknown");
            return format!("{label} · run failed: {reason} · receipt kept");
        }
        let mut line = format!(
            "{label} · {} run{} · {} proposal{} pending",
            self.runs,
            if self.runs == 1 { "" } else { "s" },
            self.proposals_pending,
            if self.proposals_pending == 1 { "" } else { "s" },
        );
        // Remote job completions (Story 3.4): the verdict names them — a
        // night the cluster worked while the researcher slept.
        if self.jobs_finished > 0 {
            line.push_str(&format!(
                " · {} job{} finished",
                self.jobs_finished,
                if self.jobs_finished == 1 { "" } else { "s" },
            ));
        }
        if let Some(job) = &self.job_verdict {
            if job.failed {
                line.push_str(&format!(
                    " · job failed: {}",
                    job.reason.as_deref().unwrap_or("unknown")
                ));
            }
        }
        if self.ceiling_reached {
            line.push_str(" · cost ceiling reached · no further runs");
        }
        line
    }
}

/// The dead-man-switch alert row (FR-9.1/FR-4.3, Story 2.6): a run the
/// telemetry reaper detected as silently dead — rendered distinct from (and
/// counted separately of) the digest rows, from the `run.dead` alert event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DigestAlert {
    pub run_id: String,
    /// The mission the dead run belonged to (the receipts link's anchor).
    pub mission_id: Uuid,
    /// The mission's creation seq — the `M-n` label.
    pub mission_seq: i64,
    /// The heartbeat timestamp the run died at.
    pub heartbeat_ts: DateTime<Utc>,
    /// The matching `run.started` seq — the receipts link's anchor.
    pub receipt_seq: i64,
}

/// A connection alert row (FR-9.1, Story 2.6): a research connection
/// (Zotero, arXiv, Semantic Scholar) whose latest state is DOWN — rendered
/// with label + icon, never color alone (DESIGN.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionAlert {
    pub connection: String,
    /// The latest failure's error code, in code form (bilingual-safe).
    pub error_code: String,
    /// When the connection went down.
    pub failed_ts: DateTime<Utc>,
}

/// The morning digest read model (FR-4.4): the night's outcome badge, the
/// spend-vs-ceiling line, ≤10 rows, the dead-run alert rows, and the
/// connection alert rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MorningDigest {
    pub generated_at: DateTime<Utc>,
    pub outcome: DigestOutcome,
    /// Aggregate of the included rows' folded spend vs their ceilings.
    pub spend_cents: u64,
    pub ceiling_cents: u64,
    /// ≤10 rows, newest night first (stable order).
    pub rows: Vec<DigestRow>,
    /// Dead-run alert rows — present but separate (Story 2.6's dead-man
    /// switch; these render from the `run.dead` events).
    pub alerts: Vec<DigestAlert>,
    /// Connection alert rows — the research connections currently down.
    pub connection_alerts: Vec<ConnectionAlert>,
}

/// One mission's night, tallied while folding the window.
#[derive(Debug, Clone, Default)]
struct MissionNight {
    started: u32,
    finished: u32,
    failed: u32,
    latest_failed_reason: Option<String>,
    last_run_ts: Option<DateTime<Utc>>,
    receipt_seq: i64,
    latest_run_id: Option<String>,
    // Remote job completions (Story 3.4, FR-11.5): the morning digest
    // reports them with one-line verdicts.
    jobs_finished: u32,
    jobs_failed: u32,
    latest_job_verdict: Option<DigestJobVerdict>,
}

impl MissionNight {
    fn note_run(&mut self, event: &StoredEvent) {
        self.last_run_ts = Some(event.ts);
        match event.kind.as_str() {
            RUN_STARTED => {
                self.started += 1;
                self.receipt_seq = event.seq;
                self.latest_run_id = event
                    .payload
                    .get("run_id")
                    .and_then(serde_json::Value::as_str)
                    .map(String::from);
            }
            RUN_FINISHED => self.finished += 1,
            RUN_FAILED => {
                self.failed += 1;
                self.latest_failed_reason = event
                    .payload
                    .get("reason")
                    .and_then(serde_json::Value::as_str)
                    .map(String::from);
            }
            _ => {}
        }
    }

    /// Note one remote job completion (Story 3.4): `job.finished` /
    /// `job.failed` events carry the mission, target, and job in their
    /// payload — the digest's one-line verdict renders from them.
    fn note_job(&mut self, event: &StoredEvent) {
        let Ok(payload) = serde_json::from_value::<JobLifecyclePayload>(event.payload.clone())
        else {
            return; // a corrupt job event never breaks the digest
        };
        let failed = event.kind == JOB_FAILED;
        if failed {
            self.jobs_failed += 1;
        } else {
            self.jobs_finished += 1;
        }
        self.latest_job_verdict = Some(DigestJobVerdict {
            target: payload.target,
            job_id: payload.job_id,
            failed,
            reason: payload.reason,
        });
    }
}

/// The mission a run event belongs to (payload `mission_id`, then causes).
fn run_event_mission(event: &StoredEvent) -> Option<Uuid> {
    if !matches!(event.kind.as_str(), RUN_STARTED | RUN_FINISHED | RUN_FAILED) {
        return None;
    }
    if let Some(id) = event
        .payload
        .get("mission_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
    {
        return Some(id);
    }
    event.causes.first().copied()
}

/// Fold the morning digest over the log (pure — no IO): the runs of the
/// last `DIGEST_WINDOW_HOURS`, one row per mission, capped and ordered
/// newest-first (ties by mission seq), plus the dead-run alerts. Corrupt
/// payloads fail loudly (the missions fold's contract), never silently.
pub fn render_digest(events: &[StoredEvent], now: DateTime<Utc>) -> Result<MorningDigest, EventError> {
    // The shared fold cursor (AD-1, Story 2.6): the digest renders the LIVE
    // read model — a rolled-back night never happened for the morning
    // report; post-rollback runs render.
    let cursor = crate::domain::checkpoints::FoldCursor::over(events);
    let events = &cursor.live_owned(events);
    let missions = MissionsProjection::fold(events)?;
    let proposals = ProposalsProjection::fold(events)?;
    let window_start = now - Duration::hours(DIGEST_WINDOW_HOURS);

    // Dead runs in the window (FR-9.1/FR-4.3, Story 2.6): the `run.dead`
    // alert events — the dead-man switch's detections. The terminal
    // run.failed(silently_dead) that follows each one lands as the row's
    // honest failure; the ALERT row renders from the detection itself.
    let mut dead: std::collections::HashMap<String, DigestAlert> =
        std::collections::HashMap::new();
    for event in events {
        if event.ts <= window_start || event.ts > now || event.kind != RUN_DEAD {
            continue;
        }
        let Some(run_id) = event
            .payload
            .get("run_id")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let heartbeat_ts = event
            .payload
            .get("last_heartbeat_ts")
            .and_then(serde_json::Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or(event.ts);
        dead.insert(
            run_id.to_string(),
            DigestAlert {
                run_id: run_id.to_string(),
                mission_id: Uuid::nil(), // resolved below, once the mission is known
                mission_seq: 0, // resolved below, once the mission is known
                heartbeat_ts,
                receipt_seq: 0, // the matching run.started's seq
            },
        );
    }
    // The receipt anchor of each alert: its run.started's seq + mission seq.
    for event in events {
        if event.ts <= window_start || event.ts > now || event.kind != RUN_STARTED {
            continue;
        }
        let Some(run_id) = event
            .payload
            .get("run_id")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        if let Some(alert) = dead.get_mut(run_id) {
            alert.receipt_seq = event.seq;
            if let Some(mission_id) = run_event_mission(event) {
                alert.mission_id = mission_id;
                alert.mission_seq = missions
                    .iter()
                    .find(|m| m.id == mission_id)
                    .map(|m| m.seq)
                    .unwrap_or(0);
            }
        }
    }
    let mut alerts: Vec<DigestAlert> = dead.into_values().collect();
    alerts.sort_by(|a, b| b.heartbeat_ts.cmp(&a.heartbeat_ts));

    // The night's tally per mission (window only).
    let mut nights: Vec<(Uuid, MissionNight)> = Vec::new();
    let mut index: std::collections::HashMap<Uuid, usize> = std::collections::HashMap::new();
    for event in events {
        if event.ts <= window_start || event.ts > now {
            continue;
        }
        // Remote job completions (Story 3.4): the same window, the same
        // per-mission tally — job.finished / job.failed carry their mission.
        if matches!(event.kind.as_str(), JOB_FINISHED | JOB_FAILED) {
            if let Some(mission_id) = event
                .payload
                .get("mission_id")
                .and_then(serde_json::Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
            {
                let i = *index
                    .entry(mission_id)
                    .or_insert_with(|| {
                        nights.push((mission_id, MissionNight::default()));
                        nights.len() - 1
                    });
                nights[i].1.note_job(event);
            }
            continue;
        }
        let Some(mission_id) = run_event_mission(event) else {
            continue;
        };
        let i = *index
            .entry(mission_id)
            .or_insert_with(|| {
                nights.push((mission_id, MissionNight::default()));
                nights.len() - 1
            });
        nights[i].1.note_run(event);
    }

    // Rows: one per mission that RAN in the window.
    let mut rows: Vec<DigestRow> = Vec::new();
    let mut total_spend = 0u64;
    let mut total_ceiling = 0u64;
    for (mission_id, night) in &nights {
        // A night with only remote job completions (no scan run) still
        // earns its row (Story 3.4) — the digest reports job completions.
        if night.started == 0 && night.jobs_finished == 0 && night.jobs_failed == 0 {
            continue; // run terminal events without a start in-window (edge)
        }
        let Some(mission) = missions.iter().find(|m| m.id == *mission_id) else {
            continue;
        };
        let pending = proposals
            .iter()
            .filter(|p| {
                p.mission_id == Some(*mission_id) && p.status == ProposalStatus::Pending
            })
            .count() as u32;
        total_spend += mission.spend_cents;
        total_ceiling += mission.spend_ceiling_cents;
        rows.push(DigestRow {
            mission_id: mission.id,
            mission_seq: mission.seq,
            question: mission.question.clone(),
            status: mission.status,
            runs: night.started,
            finished: night.finished,
            failed: night.failed,
            failure_reason: night.latest_failed_reason.clone(),
            ceiling_reached: mission.spend_state == crate::domain::missions::SpendState::Blocked,
            proposals_pending: pending,
            spend_cents: mission.spend_cents,
            ceiling_cents: mission.spend_ceiling_cents,
            receipt_seq: night.receipt_seq,
            run_id: night.latest_run_id.clone().unwrap_or_default(),
            last_run_ts: night.last_run_ts.unwrap_or(now),
            jobs_finished: night.jobs_finished,
            jobs_failed: night.jobs_failed,
            job_verdict: night.latest_job_verdict.clone(),
        });
    }

    // Newest night first; ties broken by mission seq (stable, deterministic —
    // a >10-run night truncates the OLDEST nights out).
    rows.sort_by(|a, b| {
        b.last_run_ts
            .cmp(&a.last_run_ts)
            .then(a.mission_seq.cmp(&b.mission_seq))
    });
    rows.truncate(DIGEST_ROW_CAP);

    // The outcome badge across the whole window (all missions, not just the
    // rows that survived the cap).
    let (started, finished, failed) = nights.iter().fold(
        (0u32, 0u32, 0u32),
        |(s, f, x), (_, n)| (s + n.started, f + n.finished, x + n.failed),
    );
    let outcome = match (started, finished, failed) {
        (0, _, _) => DigestOutcome::NoRuns,
        (_, 0, f) if f > 0 => DigestOutcome::AllFailed,
        (_, f, x) if f > 0 && x > 0 => DigestOutcome::PartialSuccess,
        _ => DigestOutcome::AllFinished,
    };

    // Connection alert rows (FR-9.1, Story 2.6): the research connections
    // currently DOWN — the latest failure per connection, label + icon in
    // the UI, never color alone.
    let connection_alerts: Vec<ConnectionAlert> = connection_health(events)
        .into_iter()
        .filter(|h| !h.up)
        .map(|h| ConnectionAlert {
            connection: h.connection,
            error_code: h.last_error_code.unwrap_or_else(|| "unknown".into()),
            failed_ts: h.last_error_ts.unwrap_or(now),
        })
        .collect();

    Ok(MorningDigest {
        generated_at: now,
        outcome,
        spend_cents: total_spend,
        ceiling_cents: total_ceiling,
        rows,
        alerts,
        connection_alerts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::missions::{Autonomy, MissionCreatedPayload, SPEND_RECORDED};
    use crate::domain::nightshift::SCAN_STEP;
    use crate::domain::proposals;
    use crate::eventstore::{Actor, EventStore, NewEvent, SystemComponent};
    use chrono::TimeZone;
    use rusqlite::Connection;
    use serde_json::json;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    fn seed_mission(store: &EventStore<'_>, question: &str) -> crate::eventstore::StoredEvent {
        store
            .append(
                NewEvent::mission_created(MissionCreatedPayload {
                    question: question.into(),
                    stop_condition: "Stop after $5 spent.".into(),
                    success_criterion: "A blind rater agrees.".into(),
                    autonomy: Autonomy::Suggest,
                    spend_ceiling_cents: 100,
                    schedule: "daily-03:00".into(),
                    roles: vec![],
                })
                .unwrap(),
            )
            .unwrap()
    }

    /// Append a run event at a controlled ts (ctors stamp `now`; tests
    /// backdate by writing the pub `ts` field before appending).
    fn append_at(store: &EventStore<'_>, mut event: NewEvent, ts: DateTime<Utc>) -> StoredEvent {
        event.ts = ts;
        store.append(event).unwrap()
    }

    fn night(now: DateTime<Utc>, minutes_ago: i64) -> DateTime<Utc> {
        now - chrono::Duration::minutes(minutes_ago)
    }

    #[test]
    fn a_finished_night_renders_one_row_per_mission_with_a_verdict() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let now = Utc.with_ymd_and_hms(2026, 9, 19, 9, 4, 0).unwrap();
        let mission = seed_mission(&store, "Does retrieval grounding reduce hallucinations?");
        append_at(
            &store,
            NewEvent::run_started("ns-1", mission.id, "daily-03:00", SCAN_STEP).unwrap(),
            night(now, 60),
        );
        append_at(
            &store,
            NewEvent::run_finished("ns-1", mission.id, "1 scan · 2 proposals", 2).unwrap(),
            night(now, 55),
        );
        // a proposal the night left in quarantine (FR-4.2)
        let hyp = store
            .append(NewEvent::hypothesis_created("X holds.", mission.id).unwrap())
            .unwrap();
        proposals::propose_transition(
            &store,
            "ns-1",
            hyp.id,
            crate::domain::hypotheses::HypothesisStatus::Testing,
            "night scan suggests testing",
        )
        .unwrap();

        let digest = render_digest(&store.events_all().unwrap(), now).unwrap();
        assert_eq!(digest.outcome, DigestOutcome::AllFinished);
        assert_eq!(digest.rows.len(), 1);
        let row = &digest.rows[0];
        assert_eq!(row.mission_seq, mission.seq);
        assert_eq!(row.runs, 1);
        assert_eq!(row.finished, 1);
        assert_eq!(row.failed, 0);
        assert_eq!(row.proposals_pending, 1);
        assert_eq!(row.failure_reason, None);
        assert_eq!(row.status, MissionStatus::Active);
        assert_eq!(row.receipt_seq, store.events_all().unwrap()[1].seq);
        // Story 2.5 (FR-6.2): the row carries its latest run's id — the
        // receipts drill-down's target
        assert_eq!(row.run_id, "ns-1");
        // the renderer contract: one line, no newline, ≤120 chars
        let line = row.verdict_line();
        assert!(!line.contains('\n'));
        assert!(line.len() <= 120, "verdict line too long: {line}");
        assert!(line.starts_with(&format!("M-{}", mission.seq)));
        assert!(line.contains("1 run"));
        assert!(line.contains("1 proposal pending"));
        assert!(digest.alerts.is_empty());
    }

    #[test]
    fn a_failed_run_still_renders_an_honest_row() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let now = Utc.with_ymd_and_hms(2026, 9, 19, 9, 4, 0).unwrap();
        let mission = seed_mission(&store, "Does sparse attention scale?");
        append_at(
            &store,
            NewEvent::run_started("ns-2", mission.id, "daily-03:00", SCAN_STEP).unwrap(),
            night(now, 120),
        );
        append_at(
            &store,
            NewEvent::run_failed("ns-2", mission.id, "provider_error", night(now, 118)).unwrap(),
            night(now, 118),
        );

        // the digest is delivered even when the run failed (FR-4.3)
        let digest = render_digest(&store.events_all().unwrap(), now).unwrap();
        assert_eq!(digest.outcome, DigestOutcome::AllFailed);
        assert_eq!(digest.rows.len(), 1, "a failed run still gets its row");
        let row = &digest.rows[0];
        assert_eq!(row.failed, 1);
        assert_eq!(row.failure_reason.as_deref(), Some("provider_error"));
        let line = row.verdict_line();
        assert!(line.contains("run failed: provider_error"), "honest row: {line}");
        assert!(line.contains("receipt kept"), "honest row: {line}");
        // a normal (non-dead) failure is a row, not an alert
        assert!(digest.alerts.is_empty());
    }

    #[test]
    fn a_dead_run_renders_the_dead_man_alert_row() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let now = Utc.with_ymd_and_hms(2026, 9, 19, 9, 4, 0).unwrap();
        let mission = seed_mission(&store, "Does MoE routing stay stable?");
        let started = append_at(
            &store,
            NewEvent::run_started("ns-17", mission.id, "daily-03:00", SCAN_STEP).unwrap(),
            night(now, 390),
        );
        // Story 2.6's dead-man switch: the run.dead alert event, then the
        // terminal run.failed(silently_dead) — no run ends silently
        let died_at = night(now, 361);
        append_at(
            &store,
            NewEvent::run_dead("ns-17", mission.id, died_at, 30).unwrap(),
            night(now, 360),
        );
        append_at(
            &store,
            NewEvent::run_failed(
                "ns-17",
                mission.id,
                crate::domain::telemetry::SILENTLY_DEAD_REASON,
                died_at,
            )
            .unwrap(),
            night(now, 360),
        );

        let digest = render_digest(&store.events_all().unwrap(), now).unwrap();
        // the alert row is present, wired to the telemetry seam (FR-9.1)
        assert_eq!(digest.alerts.len(), 1);
        let alert = &digest.alerts[0];
        assert_eq!(alert.run_id, "ns-17");
        assert_eq!(alert.mission_seq, mission.seq);
        assert_eq!(alert.heartbeat_ts, died_at);
        assert_eq!(alert.receipt_seq, started.seq);
        // the dead run ALSO produces its honest digest row
        assert_eq!(digest.rows.len(), 1);
        assert_eq!(
            digest.rows[0].failure_reason.as_deref(),
            Some(crate::domain::telemetry::SILENTLY_DEAD_REASON)
        );
    }

    #[test]
    fn a_down_connection_renders_an_alert_row_and_a_restored_one_does_not() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let now = Utc.with_ymd_and_hms(2026, 9, 19, 9, 4, 0).unwrap();
        append_at(
            &store,
            NewEvent::connection_failed("zotero", "unreachable").unwrap(),
            night(now, 90),
        );
        append_at(
            &store,
            NewEvent::connection_failed("arxiv", "timeout").unwrap(),
            night(now, 80),
        );
        append_at(
            &store,
            NewEvent::connection_restored("arxiv").unwrap(),
            night(now, 30),
        );

        let digest = render_digest(&store.events_all().unwrap(), now).unwrap();
        // only the still-down connection alerts; the healed one does not
        assert_eq!(digest.connection_alerts.len(), 1);
        let alert = &digest.connection_alerts[0];
        assert_eq!(alert.connection, "zotero");
        assert_eq!(alert.error_code, "unreachable");
        assert_eq!(alert.failed_ts, night(now, 90));
        // no runs, no dead-run alerts — the connection row stands alone
        assert!(digest.rows.is_empty());
        assert!(digest.alerts.is_empty());
    }

    #[test]
    fn a_gt10_run_night_truncates_to_10_rows_newest_first() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let now = Utc.with_ymd_and_hms(2026, 9, 19, 9, 4, 0).unwrap();
        // 12 missions, each ran once — run ts strictly increasing with seq
        let mut expected_order = Vec::new();
        for i in 0..12 {
            let mission = seed_mission(&store, &format!("Mission {i}?"));
            append_at(
                &store,
                NewEvent::run_started(&format!("ns-{i}"), mission.id, "daily-03:00", SCAN_STEP)
                    .unwrap(),
                night(now, 300 - 10 * i),
            );
            append_at(
                &store,
                NewEvent::run_finished(&format!("ns-{i}"), mission.id, "ok", 0).unwrap(),
                night(now, 299 - 10 * i),
            );
            expected_order.push(mission.seq);
        }

        let digest = render_digest(&store.events_all().unwrap(), now).unwrap();
        assert_eq!(digest.rows.len(), DIGEST_ROW_CAP, "the 10-row cap holds");
        // newest night first; the two OLDEST missions truncated out —
        // ordering is stable and deterministic
        let got: Vec<i64> = digest.rows.iter().map(|r| r.mission_seq).collect();
        assert_eq!(got, expected_order.into_iter().rev().take(10).collect::<Vec<_>>());
        // every verdict line honors the one-line contract
        for row in &digest.rows {
            assert!(!row.verdict_line().contains('\n'));
        }
    }

    #[test]
    fn the_spend_line_reflects_the_missions_fold() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let now = Utc.with_ymd_and_hms(2026, 9, 19, 9, 4, 0).unwrap();
        let mission = seed_mission(&store, "Does spend fold correctly?");
        append_at(
            &store,
            NewEvent::run_started("ns-1", mission.id, "daily-03:00", SCAN_STEP).unwrap(),
            night(now, 30),
        );
        // role-tagged spend from the provider layer's fold (Story 1.6/2.1)
        store
            .append(
                NewEvent::new(
                    SPEND_RECORDED,
                    Actor::System { component: SystemComponent::Telemetry },
                    json!({
                        "mission_id": mission.id.to_string(),
                        "cost_cents": 42,
                        "role": "drafter",
                        "provider": "custom",
                        "model": "m",
                        "input_tokens": 100,
                        "output_tokens": 50,
                    }),
                )
                .unwrap()
                .with_causes(vec![mission.id]),
            )
            .unwrap();

        let digest = render_digest(&store.events_all().unwrap(), now).unwrap();
        assert_eq!(digest.spend_cents, 42, "the header aggregates the fold");
        assert_eq!(digest.ceiling_cents, 100);
        assert_eq!(digest.rows[0].spend_cents, 42);
        assert!(!digest.rows[0].ceiling_reached, "42 of 100 is not blocked");

        // spend at the ceiling: the row shows the 2.4 seam
        let conn2 = mem_conn();
        let store2 = EventStore::new(&conn2);
        let mission2 = seed_mission(&store2, "Does the ceiling show?");
        append_at(
            &store2,
            NewEvent::run_started("ns-1", mission2.id, "daily-03:00", SCAN_STEP).unwrap(),
            night(now, 30),
        );
        append_at(
            &store2,
            NewEvent::run_finished("ns-1", mission2.id, "ok", 0).unwrap(),
            night(now, 29),
        );
        store2
            .append(
                NewEvent::new(
                    SPEND_RECORDED,
                    Actor::System { component: SystemComponent::Telemetry },
                    json!({ "mission_id": mission2.id.to_string(), "cost_cents": 100 }),
                )
                .unwrap()
                .with_causes(vec![mission2.id]),
            )
            .unwrap();
        let digest = render_digest(&store2.events_all().unwrap(), now).unwrap();
        assert!(digest.rows[0].ceiling_reached);
        assert!(digest.rows[0].verdict_line().contains("cost ceiling reached"));
    }

    #[test]
    fn runs_outside_the_window_and_quiet_nights_do_not_render() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let now = Utc.with_ymd_and_hms(2026, 9, 19, 9, 4, 0).unwrap();
        let mission = seed_mission(&store, "Old mission?");
        // a run 3 days ago — outside the 24h window
        append_at(
            &store,
            NewEvent::run_started("ns-old", mission.id, "daily-03:00", SCAN_STEP).unwrap(),
            now - chrono::Duration::hours(72),
        );
        append_at(
            &store,
            NewEvent::run_finished("ns-old", mission.id, "ok", 0).unwrap(),
            now - chrono::Duration::hours(72),
        );

        let digest = render_digest(&store.events_all().unwrap(), now).unwrap();
        assert_eq!(digest.outcome, DigestOutcome::NoRuns);
        assert!(digest.rows.is_empty());
        assert!(digest.alerts.is_empty());
        assert_eq!(digest.spend_cents, 0);

        // an empty log renders an equally honest no-runs digest
        let conn2 = mem_conn();
        let store2 = EventStore::new(&conn2);
        let digest = render_digest(&store2.events_all().unwrap(), now).unwrap();
        assert_eq!(digest.outcome, DigestOutcome::NoRuns);
        assert!(digest.rows.is_empty());
    }

    #[test]
    fn a_mixed_night_badges_partial_success() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let now = Utc.with_ymd_and_hms(2026, 9, 19, 9, 4, 0).unwrap();
        let a = seed_mission(&store, "A?");
        let b = seed_mission(&store, "B?");
        append_at(
            &store,
            NewEvent::run_started("ns-a", a.id, "daily-03:00", SCAN_STEP).unwrap(),
            night(now, 60),
        );
        append_at(
            &store,
            NewEvent::run_finished("ns-a", a.id, "ok", 0).unwrap(),
            night(now, 59),
        );
        append_at(
            &store,
            NewEvent::run_started("ns-b", b.id, "daily-03:00", SCAN_STEP).unwrap(),
            night(now, 58),
        );
        append_at(
            &store,
            NewEvent::run_failed("ns-b", b.id, "provider_error", night(now, 57)).unwrap(),
            night(now, 57),
        );
        let digest = render_digest(&store.events_all().unwrap(), now).unwrap();
        assert_eq!(digest.outcome, DigestOutcome::PartialSuccess);
        assert_eq!(digest.rows.len(), 2);
    }

    // ---- remote job completions (Story 3.4, FR-11.5) ----

    /// A night where only remote jobs ran (no scan) still earns its row:
    /// the digest reports the completions with a one-line verdict — target,
    /// job, finished/failed.
    #[test]
    fn remote_job_completions_render_rows_with_one_line_verdicts() {
        use crate::domain::jobs::{
            JobLifecyclePayload, JobResources, JobSpec, JOB_FINISHED,
        };
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let now = Utc.with_ymd_and_hms(2026, 9, 19, 9, 4, 0).unwrap();
        let mission = seed_mission(&store, "Does the cluster's run hold up?");
        // a job that ran and finished overnight on a remote target
        let submitted = append_at(
            &store,
            NewEvent::job_submitted(crate::domain::jobs::JobSubmittedPayload {
                mission_id: mission.id,
                target: "cluster-1".into(),
                handle: "h-1".into(),
                spec: JobSpec {
                    cmd: "python3".into(),
                    args: vec!["train.py".into()],
                    env: Default::default(),
                    resources: Some(JobResources { cpus: None, memory_mb: None }),
                    workdir: None,
                },
            })
            .unwrap(),
            night(now, 200),
        );
        append_at(
            &store,
            NewEvent::job_finished(JobLifecyclePayload {
                mission_id: mission.id,
                job_id: submitted.id,
                target: "cluster-1".into(),
                code: Some(0),
                reason: None,
            })
            .unwrap(),
            night(now, 90),
        );
        // no run.* events at all — the jobs alone earn the row
        let digest = render_digest(&store.events_all().unwrap(), now).unwrap();
        assert_eq!(digest.rows.len(), 1, "a jobs-only night still earns its row");
        let row = &digest.rows[0];
        assert_eq!(row.jobs_finished, 1);
        assert_eq!(row.jobs_failed, 0);
        let verdict = row.job_verdict.as_ref().expect("the one-line job verdict");
        assert_eq!(verdict.target, "cluster-1");
        assert_eq!(verdict.job_id, submitted.id);
        assert!(!verdict.failed);
        assert_eq!(verdict.reason, None);
        // the verdict line names the completion
        let line = row.verdict_line();
        assert!(line.contains("1 job finished"), "the verdict line: {line}");
        assert!(!line.contains('\n'));
        // and a failed job carries its reason — no job ends silently
        append_at(
            &store,
            NewEvent::job_failed(JobLifecyclePayload {
                mission_id: mission.id,
                job_id: submitted.id,
                target: "cluster-1".into(),
                code: Some(3),
                reason: Some("exit_code_3".into()),
            })
            .unwrap(),
            night(now, 80),
        );
        let digest = render_digest(&store.events_all().unwrap(), now).unwrap();
        let row = &digest.rows[0];
        assert_eq!(row.jobs_finished, 1);
        assert_eq!(row.jobs_failed, 1);
        let verdict = row.job_verdict.as_ref().unwrap();
        assert!(verdict.failed);
        assert_eq!(verdict.reason.as_deref(), Some("exit_code_3"));
        let line = row.verdict_line();
        assert!(line.contains("job failed: exit_code_3"), "the verdict line: {line}");
        // out of the window, out of the digest
        let digest = render_digest(
            &store.events_all().unwrap(),
            now + chrono::Duration::hours(30),
        )
        .unwrap();
        assert!(digest.rows.is_empty());
    }
}
