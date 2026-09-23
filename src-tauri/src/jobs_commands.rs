// Compute-target shell commands (AD-15a, Story 3.2/3.3): the typed core
// APIs the mission card's jobs area and the trust center's target rows
// call. Specs arrive as typed JSON (JobSpec), are validated before submit
// (the `job.submitted` constructor is the gate — AD-6), and execute
// through the target's adapter with argv directly. The lifecycle events
// (`job.running` / `job.finished` / `job.failed`) are appended by the poll
// loop — invoked per command (`poll_jobs`) and per Night Shift tick — and
// every terminal job attributes its usage to its target
// (`target.spend_recorded`, AD-10) so per-target ceilings fold from the
// log.
//
// Story 3.3: `ssh` targets carry a host; hosts outside the allowlist are
// refused at this layer AND inside the adapter (defense in depth) before
// any connection is attempted. Agent-initiated submits are governed by
// the target's effective autonomy dial (FR-5.3 — auto-enqueue allowed on
// a cluster, never on a laptop); a user-initiated submit always proceeds.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::adapters::targets::{
    ComputeTarget, JobHandle, JobResult, TargetError, TargetInfo, TargetJobStatus, TargetRegistry,
};
use crate::db::Db;
use crate::domain::hypotheses::HypothesesProjection;
use crate::domain::jobs::{
    fold_declared_targets, fold_host_allowlist, DeclaredTarget, Job, JobPhase,
    JobSubmittedPayload, JobsProjection, JobLifecyclePayload, TargetDeclaredPayload,
    DEFAULT_TARGET_KIND, DEFAULT_TARGET_NAME,
};
use crate::domain::missions::MissionsProjection;
use crate::domain::proposals::{propose_evidence_pin, Proposal, ProposalsProjection};
use crate::domain::trust::TargetSpendRecordedPayload;
use crate::eventstore::{EventError, EventStore, NewEvent, StoredEvent};
use tauri::State;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// One compute target as the mission card's target row and the trust
/// center's target rows render it: a name (mono chip), the adapter kind
/// behind it, the host an `ssh`/`scheduler` target connects to plus
/// whether it is on the allowlist, and the per-kind config map the
/// v0.2.0 kinds carry. The built-in `local` needs no declaration
/// (FR-11.1: v1 ships local).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputeTargetView {
    pub name: String,
    pub kind: String,
    /// The host an `ssh`/`scheduler` target connects to (`None` for the
    /// other kinds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Whether the gate value is on the allowlist (`None` when the kind
    /// has no allowlist gate — the allowlist governs ssh/scheduler hosts
    /// and kubernetes contexts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlisted: Option<bool>,
    /// The per-kind config map (scheduler flavor/prefixes, kubernetes
    /// context/namespace/image, chopflow endpoint/queue — empty for
    /// `local`/`ssh`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config: BTreeMap<String, String>,
    /// The built-in `local` target (no `target.declared` event behind it).
    pub builtin: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<DateTime<Utc>>,
}

/// One registered adapter kind as the settings row lists it (Story 6.5:
/// kind + contract version — community adapters appear exactly as
/// first-party ones do).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredAdapterView {
    pub kind: String,
    /// The adapter contract version the adapter was validated against.
    pub contract_version: String,
    /// Whether the kind ships with ResearchCore (local/ssh/scheduler/
    /// kubernetes/chopflow) or registered from outside.
    pub builtin: bool,
}

/// A target probe's outcome (the settings row's discovery/unreachable
/// state, per adapter): `ok` carries the adapter's own detail line,
/// `unreachable` its typed reason. `unsupported` is honest for kinds
/// with no probe (a community adapter need not implement one).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetProbeView {
    pub status: String,
    pub detail: String,
}

/// Resolve a target NAME to its adapter kind: a declared target's kind,
/// else the built-in `local`. `None` when nothing answers to the name.
fn resolve_kind(declared: &[DeclaredTarget], name: &str) -> Option<String> {
    if let Some(target) = declared.iter().find(|t| t.name == name) {
        return Some(target.kind.clone());
    }
    if name == DEFAULT_TARGET_NAME {
        return Some(DEFAULT_TARGET_KIND.to_string());
    }
    None
}

/// Resolve a target NAME to its declared host (ssh/scheduler targets);
/// `None` for the built-in `local` and unknown names.
fn resolve_host(declared: &[DeclaredTarget], name: &str) -> Option<Option<String>> {
    declared
        .iter()
        .find(|t| t.name == name)
        .map(|t| t.host.clone())
}

/// Resolve a target NAME to its declared config map (the v0.2.0 kinds);
/// `None` for the built-in `local` and unknown names.
fn resolve_config(declared: &[DeclaredTarget], name: &str) -> Option<BTreeMap<String, String>> {
    declared
        .iter()
        .find(|t| t.name == name)
        .map(|t| t.config.clone())
}

/// The allowlist's gate value for a target (defense in depth — the
/// adapters gate again at their own boundary): the host an
/// `ssh`/`scheduler` target connects to, the context a `kubernetes`
/// target runs on; `None` for kinds with no gate (`local`, `chopflow` —
/// its endpoint is validated at declaration).
fn gate_value(kind: &str, host: Option<&String>, config: &BTreeMap<String, String>) -> Option<String> {
    match kind {
        "ssh" | "scheduler" => host.cloned(),
        "kubernetes" => config.get("context").cloned(),
        _ => None,
    }
}

/// The target names a submitter can choose from (for error messages).
fn known_names(declared: &[DeclaredTarget]) -> Vec<String> {
    let mut names: Vec<String> = vec![DEFAULT_TARGET_NAME.into()];
    names.extend(declared.iter().map(|t| t.name.clone()));
    names.sort();
    names.dedup();
    names
}

/// The compute target list (FR-11.1): declared targets from the log, with
/// the built-in `local` always present. `ssh` targets carry their host
/// and their allowlisted status (Story 3.3); the v0.2.0 kinds carry
/// their config maps and their own gate values (the scheduler's host,
/// the kubernetes context).
pub(crate) fn list_targets_inner(
    events: &[StoredEvent],
) -> Result<Vec<ComputeTargetView>, EventError> {
    let declared = fold_declared_targets(events);
    let allowlist = fold_host_allowlist(events);
    let mut views: Vec<ComputeTargetView> = Vec::new();
    if !declared.iter().any(|t| t.name == DEFAULT_TARGET_NAME) {
        views.push(ComputeTargetView {
            name: DEFAULT_TARGET_NAME.into(),
            kind: DEFAULT_TARGET_KIND.into(),
            host: None,
            allowlisted: None,
            config: BTreeMap::new(),
            builtin: true,
            seq: None,
            ts: None,
        });
    }
    views.extend(declared.into_iter().map(|t| {
        let allowlisted = gate_value(&t.kind, t.host.as_ref(), &t.config)
            .map(|gate| allowlist.iter().any(|allowed| *allowed == gate));
        ComputeTargetView {
            name: t.name,
            kind: t.kind,
            host: t.host,
            allowlisted,
            config: t.config,
            builtin: false,
            seq: Some(t.seq),
            ts: Some(t.ts),
        }
    }));
    Ok(views)
}

// ---------------------------------------------------------------------------
// The poll loop — the monitor half of the lifecycle (FR-11.4)
// ---------------------------------------------------------------------------

/// The lifecycle payload for one observed transition.
fn lifecycle(job: &Job, code: Option<i64>, reason: Option<String>) -> JobLifecyclePayload {
    JobLifecyclePayload {
        mission_id: job.mission_id,
        job_id: job.id,
        target: job.target.clone(),
        code,
        reason,
    }
}

/// Attribute one job's compute usage to its target (AD-10): the
/// `target.spend_recorded` event lands beside the terminal event,
/// cause-linked to it. Local compute costs nothing measurable — the event
/// still lands (usage is attributable; per-target ceilings fold from
/// these events or not at all), at 0 cents.
fn append_target_spend(
    store: &EventStore<'_>,
    job: &Job,
    terminal: &StoredEvent,
) -> Result<StoredEvent, EventError> {
    store.append(
        NewEvent::target_spend_recorded(TargetSpendRecordedPayload {
            target: job.target.clone(),
            cost_cents: 0,
            run_id: None,
            mission_id: Some(job.mission_id),
            job_id: Some(job.id),
        })?
        .with_causes(vec![terminal.id]),
    )
}

/// Poll every live (queued/running) job — optionally scoped to one
/// mission — appending the observed transitions: `job.running` when a
/// queued job is first seen running, `job.finished` / `job.failed` (with
/// the terminal's stamp + reason) when a job ends, each followed by its
/// `target.spend_recorded`. Jobs whose handle the adapter no longer knows
/// (e.g. a restart lost the process table) are skipped honestly — the
/// read model keeps its last observed state. Called per command
/// (`poll_jobs`) and per Night Shift tick.
pub(crate) async fn poll_live_jobs(
    db: &Db,
    mission: Option<Uuid>,
) -> Result<Vec<StoredEvent>, EventError> {
    let registry = TargetRegistry::v1();
    // Snapshot the live jobs + their adapters under one lock, then observe
    // with no lock held (monitor is non-blocking).
    let live: Vec<(Job, Arc<dyn ComputeTarget>)> = {
        let conn = db.0.lock().await;
        let events = EventStore::new(&conn).events_all()?;
        let declared = fold_declared_targets(&events);
        JobsProjection::fold(&events)?
            .into_iter()
            .filter(|j| matches!(j.phase, JobPhase::Queued | JobPhase::Running))
            .filter(|j| mission.map(|id| j.mission_id == id).unwrap_or(true))
            .filter_map(|job| {
                resolve_kind(&declared, &job.target)
                    .and_then(|kind| registry.adapter(&kind).ok())
                    .map(|adapter| (job, adapter))
            })
            .collect()
    };
    // Observe outside the lock: each monitor is a non-blocking read.
    let mut observed: Vec<(Job, TargetJobStatus)> = Vec::new();
    for (job, adapter) in live {
        match adapter.monitor(&JobHandle::new(job.handle.clone())) {
            Ok(status) => observed.push((job, status)),
            Err(TargetError::UnknownJob(_)) => continue, // process table lost — keep the last observed state
            Err(_) => continue,
        }
    }
    if observed.is_empty() {
        return Ok(Vec::new());
    }
    let mut appended = Vec::new();
    let conn = db.0.lock().await;
    let store = EventStore::new(&conn);
    for (job, status) in observed {
        match status {
            TargetJobStatus::Running => {
                if job.phase == JobPhase::Queued {
                    appended.push(store.append(NewEvent::job_running(lifecycle(&job, None, None))?)?);
                }
            }
            TargetJobStatus::Finished { code } => {
                let terminal =
                    store.append(NewEvent::job_finished(lifecycle(&job, Some(code as i64), None))?)?;
                appended.push(terminal.clone());
                appended.push(append_target_spend(&store, &job, &terminal)?);
            }
            TargetJobStatus::Failed { reason, code } => {
                let terminal = store.append(NewEvent::job_failed(lifecycle(
                    &job,
                    code.map(|c| c as i64),
                    Some(reason),
                ))?)?;
                appended.push(terminal.clone());
                appended.push(append_target_spend(&store, &job, &terminal)?);
            }
        }
    }
    Ok(appended)
}

// ---------------------------------------------------------------------------
// Shell commands
// ---------------------------------------------------------------------------

/// Declare a named compute target (FR-11.1, Stories 3.3 + 6.2–6.4):
/// `{ name, kind, host?, config? }` where the kind names a registered
/// adapter, an `ssh` target carries the host it connects to (a
/// `scheduler` target MAY — its submission node), and the v0.2.0 kinds
/// carry their per-kind config map. Appends one `target.declared` event
/// (actor=user) and returns the fresh target list.
#[tauri::command]
pub async fn declare_compute_target(
    db: State<'_, Db>,
    name: String,
    kind: String,
    host: Option<String>,
    config: Option<std::collections::HashMap<String, String>>,
) -> Result<Vec<ComputeTargetView>, String> {
    let config: BTreeMap<String, String> =
        config.unwrap_or_default().into_iter().collect();
    declare_compute_target_inner(db.inner(), &name, &kind, host.as_deref(), &config)
        .await
}

/// The host allowlist (Story 3.3): the hosts SSH targets may connect to.
/// Read is a pure fold of the latest `host_allowlist.edited` event.
#[tauri::command]
pub async fn get_host_allowlist(db: State<'_, Db>) -> Result<Vec<String>, String> {
    let conn = db.0.lock().await;
    let events = EventStore::new(&conn).events_all().map_err(err)?;
    Ok(fold_host_allowlist(&events))
}

/// Edit the host allowlist (Story 3.3): replaces the whole list (the
/// latest `host_allowlist.edited` event wins). Hosts are one-token
/// entries; malformed ones never land. Returns the fresh allowlist.
#[tauri::command]
pub async fn set_host_allowlist(
    db: State<'_, Db>,
    hosts: Vec<String>,
) -> Result<Vec<String>, String> {
    set_host_allowlist_inner(db.inner(), hosts).await
}

pub(crate) async fn set_host_allowlist_inner(
    db: &Db,
    hosts: Vec<String>,
) -> Result<Vec<String>, String> {
    let event = NewEvent::host_allowlist_edited(crate::domain::jobs::HostAllowlistEditedPayload {
        hosts,
    })
    .map_err(err)?;
    let conn = db.0.lock().await;
    let store = EventStore::new(&conn);
    store.append(event).map_err(err)?;
    let events = store.events_all().map_err(err)?;
    Ok(fold_host_allowlist(&events))
}

/// The compute target list (FR-11.1): declared targets plus the built-in
/// `local`. Read-only — a pure fold.
#[tauri::command]
pub async fn list_compute_targets(
    db: State<'_, Db>,
) -> Result<Vec<ComputeTargetView>, String> {
    let conn = db.0.lock().await;
    let events = EventStore::new(&conn).events_all().map_err(err)?;
    list_targets_inner(&events).map_err(err)
}

/// Who started a submit (FR-5.3, Story 3.3): the user's shell command, or
/// an agent dispatch. The dial governs agent-initiated submits only — a
/// user-initiated submit always proceeds (subject to the allowlist).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Initiator {
    User,
    Agent,
}

/// Submit a job to a mission's declared compute target (FR-11.1/11.2,
/// Story 3.2): the spec is typed JSON, validated before submit (AD-6 — a
/// freeform shell construction never lands), executed by the target's
/// adapter with argv directly. Appends `job.submitted` (actor=user),
/// observes the fresh process once (a fast spawn failure fails terminal
/// immediately), and returns the folded job — queued or running.
#[tauri::command]
pub async fn submit_job(
    db: State<'_, Db>,
    mission_id: String,
    target: String,
    spec: crate::domain::jobs::JobSpec,
) -> Result<Job, String> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|e| format!("invalid mission id `{mission_id}`: {e}"))?;
    submit_job_initiated(db.inner(), mission_id, &target, spec, Initiator::User)
        .await
        .map_err(err)
}

pub(crate) async fn submit_job_inner(
    db: &Db,
    mission_id: Uuid,
    target: &str,
    spec: crate::domain::jobs::JobSpec,
) -> Result<Job, EventError> {
    submit_job_initiated(db, mission_id, target, spec, Initiator::User).await
}

/// The submit seam both initiators flow through (Story 3.3): the user
/// command above, and the agent-dispatch path a later story wires (its
/// jobs enqueue through THIS gate, never around it).
pub(crate) async fn submit_job_initiated(
    db: &Db,
    mission_id: Uuid,
    target: &str,
    spec: crate::domain::jobs::JobSpec,
    initiator: Initiator,
) -> Result<Job, EventError> {
    // Validation before anything else — before the mission lookup, before
    // the adapter, before any event (AD-6).
    spec.validate()
        .map_err(|e| EventError::Invalid(e.to_string()))?;
    // Resolve the mission, the target's adapter, and the per-target facts
    // (host + allowlist) under one read; an agent-initiated submit also
    // faces the per-target dial here (FR-5.3).
    let (adapter, target_info) = {
        let conn = db.0.lock().await;
        let events = EventStore::new(&conn).events_all()?;
        let missions = MissionsProjection::fold(&events)?;
        let Some(mission) = missions.iter().find(|m| m.id == mission_id) else {
            return Err(EventError::Invalid(format!(
                "not_found: no mission with id `{mission_id}`"
            )));
        };
        let declared = fold_declared_targets(&events);
        let Some(kind) = resolve_kind(&declared, target) else {
            return Err(EventError::Invalid(format!(
                "unknown_target: `{target}` — declared targets: {}",
                known_names(&declared).join(" | ")
            )));
        };
        // Per-target autonomy (FR-5.3, AD-15d): an agent-initiated submit
        // needs act-with-receipts on this target — the most restrictive of
        // the global, mission, and target dials. Auto-enqueue can be
        // allowed on a cluster and refused on a laptop; a USER submit
        // always proceeds (the dial governs agent dispatches only).
        if initiator == Initiator::Agent {
            let config = crate::domain::trust::trust_config(&events);
            if config.runtime_killed {
                return Err(EventError::Invalid(
                    "killed: the runtime is killed — every dispatch is refused until runtime.resumed (AD-15e)"
                        .into(),
                ));
            }
            let dial = crate::domain::trust::effective_autonomy(
                &config,
                Some((mission.id, mission.autonomy)),
                Some(target),
            );
            if crate::domain::trust::restrictiveness(dial)
                < crate::domain::trust::restrictiveness(crate::domain::missions::Autonomy::ActWithReceipts)
            {
                return Err(EventError::Invalid(format!(
                    "autonomy_refused: an agent-initiated submit to `{target}` needs act-with-receipts on that target — the effective dial is `{}` (FR-5.3); a user-initiated submit always proceeds",
                    dial.as_str()
                )));
            }
        }
        // The allowlist, at the command layer (defense in depth — the
        // adapter refuses again at its own boundary): an ssh/scheduler
        // target's host must be allowlisted BEFORE any connection is
        // attempted, and a kubernetes target's context likewise.
        let allowlist = fold_host_allowlist(&events);
        let host = resolve_host(&declared, target).flatten();
        let config = resolve_config(&declared, target).unwrap_or_default();
        if kind == "ssh" {
            let Some(host) = host.as_deref() else {
                return Err(EventError::Invalid(format!(
                    "missing_host: `{target}` — an ssh target names the host it connects to"
                )));
            };
            if !allowlist.iter().any(|allowed| allowed == host) {
                return Err(EventError::Invalid(
                    TargetError::HostNotAllowed {
                        host: host.to_string(),
                        known: if allowlist.is_empty() {
                            "empty — add hosts in Settings → Compute targets".to_string()
                        } else {
                            allowlist.join(" | ")
                        },
                    }
                    .to_string(),
                ));
            }
        }
        if kind == "scheduler"
            && let Some(host) = host.as_deref()
            && !allowlist.iter().any(|allowed| allowed == host)
        {
            return Err(EventError::Invalid(
                TargetError::HostNotAllowed {
                    host: host.to_string(),
                    known: if allowlist.is_empty() {
                        "empty — add hosts in Settings → Compute targets".to_string()
                    } else {
                        allowlist.join(" | ")
                    },
                }
                .to_string(),
            ));
        }
        if kind == "kubernetes" {
            let Some(context) = config.get("context") else {
                return Err(EventError::Invalid(format!(
                    "missing_config: `{target}` needs `context` — the cluster context it runs on"
                )));
            };
            if !allowlist.iter().any(|allowed| allowed == context) {
                return Err(EventError::Invalid(
                    TargetError::ContextNotAllowed {
                        context: context.clone(),
                        known: if allowlist.is_empty() {
                            "empty — add contexts in Settings → Compute targets".to_string()
                        } else {
                            allowlist.join(" | ")
                        },
                    }
                    .to_string(),
                ));
            }
        }
        let adapter = TargetRegistry::v1()
            .adapter(&kind)
            .map_err(|e| EventError::Invalid(e.to_string()))?;
        let target_info = TargetInfo {
            name: target.to_string(),
            host,
            allowlist,
            config,
        };
        (adapter, target_info)
    };
    // Submit through the adapter (no lock held): argv-direct execution —
    // locally, or argv-encoded over ssh (never a freeform string).
    let handle = adapter
        .submit(&spec, &target_info)
        .map_err(|e| EventError::Invalid(e.to_string()))?;
    // The submission event — the constructor re-validates the spec (the
    // one way a job enters the log, AD-15).
    let submitted = {
        let conn = db.0.lock().await;
        let store = EventStore::new(&conn);
        store.append(NewEvent::job_submitted(JobSubmittedPayload {
            mission_id,
            target: target.to_string(),
            handle: handle.id.clone(),
            spec,
        })?)?
    };
    // Observe the fresh process once: a healthy spawn lands
    // `job.running` right away; a spawn that already failed goes terminal
    // (reasoned, stamped) without waiting for a poll.
    poll_live_jobs(db, Some(mission_id)).await?;
    let conn = db.0.lock().await;
    let events = EventStore::new(&conn).events_all()?;
    JobsProjection::fold_for(&events, mission_id)?
        .into_iter()
        .find(|j| j.id == submitted.id)
        .ok_or_else(|| EventError::Invalid("internal: the submitted job did not fold".into()))
}

/// The mission's jobs with their live state (FR-11.4's monitor): polls the
/// mission's queued/running jobs first (appending the observed
/// transitions — the runtime's monitor loop, per command), then returns
/// the folded read model the card renders.
#[tauri::command]
pub async fn poll_jobs(
    db: State<'_, Db>,
    mission_id: String,
) -> Result<Vec<Job>, String> {
    let mission_id: Uuid = mission_id
        .parse()
        .map_err(|e| format!("invalid mission id `{mission_id}`: {e}"))?;
    poll_live_jobs(db.inner(), Some(mission_id)).await.map_err(err)?;
    let conn = db.0.lock().await;
    let events = EventStore::new(&conn).events_all().map_err(err)?;
    JobsProjection::fold_for(&events, mission_id).map_err(err)
}

/// Fetch a TERMINAL job's captured results (FR-11.5's fetch, Story 3.2's
/// scope: they surface on the job row; the quarantine flow is 3.4).
/// Typed error while the job is still running.
#[tauri::command]
pub async fn fetch_job(db: State<'_, Db>, job_id: String) -> Result<JobResult, String> {
    let job_id: Uuid = job_id
        .parse()
        .map_err(|e| format!("invalid job id `{job_id}`: {e}"))?;
    fetch_job_inner(db.inner(), job_id).await.map_err(err)
}

pub(crate) async fn fetch_job_inner(db: &Db, job_id: Uuid) -> Result<JobResult, EventError> {
    let (job, adapter) = {
        let conn = db.0.lock().await;
        let events = EventStore::new(&conn).events_all()?;
        let job = JobsProjection::fold(&events)?
            .into_iter()
            .find(|j| j.id == job_id)
            .ok_or_else(|| EventError::Invalid(format!("not_found: no job with id `{job_id}`")))?;
        if !matches!(job.phase, JobPhase::Finished | JobPhase::Failed) {
            return Err(EventError::Invalid(format!(
                "job_not_terminal: the job is still {} — fetch waits for it to end",
                match job.phase {
                    JobPhase::Queued => "queued",
                    JobPhase::Running => "running",
                    _ => unreachable!(),
                }
            )));
        }
        let declared = fold_declared_targets(&events);
        let kind = resolve_kind(&declared, &job.target).ok_or_else(|| {
            EventError::Invalid(format!("unknown_target: `{}` — its adapter kind is gone", job.target))
        })?;
        let adapter = TargetRegistry::v1()
            .adapter(&kind)
            .map_err(|e| EventError::Invalid(e.to_string()))?;
        (job, adapter)
    };
    adapter
        .fetch(&JobHandle::new(job.handle))
        .map_err(|e| EventError::Invalid(e.to_string()))
}

// ---------------------------------------------------------------------------
// Fetch results → quarantined evidence (Story 3.4, FR-11.5, AD-3/AD-5)
// ---------------------------------------------------------------------------

/// The neutral confidence of a fetched result's pin candidate (AD-5): v1
/// has no verification engine (v0.2.0+), so the runtime does not assess
/// support at fetch time — the candidate carries the neutral 0.5,
/// ATTRIBUTED to the configured model (never anonymous, FR-3.6). The
/// human's merge is the act of judgment; a later re-pin can carry a real
/// assessment.
pub(crate) const RESULT_PIN_CONFIDENCE: f64 = 0.5;

/// What `fetch_job_results` produced (Story 3.4): the captured output and
/// the quarantined proposals it landed as — one per meaningful artifact,
/// each awaiting `merge.approved` like every other proposal (AD-3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchedJobResults {
    pub job: Job,
    /// The captured output (the adapter's fetch of the terminal job).
    pub results: JobResult,
    /// The job's result proposals — freshly created ones plus any that
    /// already existed (an idempotent re-fetch appends nothing).
    pub proposals: Vec<Proposal>,
    /// How many proposals THIS fetch created (0 on an idempotent re-fetch).
    pub created: usize,
}

/// Fetch a finished job's results as QUARANTINED EVIDENCE (Story 3.4,
/// FR-11.5, AD-3/AD-5): the runtime fetches the result artifacts (the
/// target adapter's `fetch`), and each MEANINGFUL artifact (v1: a non-empty
/// stdout — the captured output is the artifact) lands as one
/// `proposal.created` whose intended payload is an `evidence.pinned` of
/// kind numerical — `artifact_ref` naming the artifact, the sha-256 digest
/// of the content COMPUTED at proposal time (AD-5), confidence attributed
/// to the configured model. Fully automatic result pinning does not exist
/// in v1: the proposals await a human `merge.approved` like every other
/// proposal (auto-pinning is v0.2.0).
///
/// The v1 TARGETING RULE: a pin attaches to a claim/hypothesis (AD-5), so
/// each artifact first registers an UNPINNED claim (actor=user — the user's
/// fetch command; it renders amber until the merge pins it, FR-3.4) on the
/// mission's FIRST hypothesis (creation order), and the proposal targets
/// that hypothesis — the merge pins the claim. A rejection leaves the
/// claim honestly unpinned.
///
/// Idempotent: a second fetch appends nothing when proposals already exist
/// for the job (cause-linked), and returns them.
#[tauri::command]
pub async fn fetch_job_results(
    db: State<'_, Db>,
    job_id: String,
) -> Result<FetchedJobResults, String> {
    let job_id: Uuid = job_id
        .parse()
        .map_err(|e| format!("invalid job id `{job_id}`: {e}"))?;
    fetch_job_results_inner(db.inner(), job_id)
        .await
        .map_err(err)
}

pub(crate) async fn fetch_job_results_inner(
    db: &Db,
    job_id: Uuid,
) -> Result<FetchedJobResults, EventError> {
    // Resolve the job + its adapter + the attribution fields under one read.
    let (job, adapter, assessing_model) = {
        let conn = db.0.lock().await;
        let events = EventStore::new(&conn).events_all()?;
        let job = JobsProjection::fold(&events)?
            .into_iter()
            .find(|j| j.id == job_id)
            .ok_or_else(|| EventError::Invalid(format!("not_found: no job with id `{job_id}`")))?;
        // Only a `job.finished` job produced result artifacts — a failed
        // job's output is its reasoned terminal, not evidence.
        if job.phase != JobPhase::Finished {
            return Err(EventError::Invalid(format!(
                "job_not_finished: the job is {} — results land as evidence proposals only \
                 when a job finishes",
                match job.phase {
                    JobPhase::Queued => "queued",
                    JobPhase::Running => "running",
                    JobPhase::Failed => "failed",
                    JobPhase::Finished => unreachable!(),
                }
            )));
        }
        let declared = fold_declared_targets(&events);
        let kind = resolve_kind(&declared, &job.target).ok_or_else(|| {
            EventError::Invalid(format!(
                "unknown_target: `{}` — its adapter kind is gone",
                job.target
            ))
        })?;
        let adapter = TargetRegistry::v1()
            .adapter(&kind)
            .map_err(|e| EventError::Invalid(e.to_string()))?;
        // Attribution (FR-3.6): the configured model — the adapter that
        // would assess the candidate. The simulated fallback names itself.
        let settings = crate::adapters::providers::ProviderSettings::load(&conn);
        let assessing_model = if settings.model.trim().is_empty() {
            "simulated".to_string()
        } else {
            settings.model.trim().to_string()
        };
        (job, adapter, assessing_model)
    };
    // Fetch the artifacts outside the lock (the adapter's own seam).
    let results = adapter
        .fetch(&JobHandle::new(job.handle.clone()))
        .map_err(|e| EventError::Invalid(e.to_string()))?;
    // The append phase, under one lock: idempotency check, the anchor
    // claims, and one proposal per meaningful artifact.
    let (proposals, created) = {
        let conn = db.0.lock().await;
        let store = EventStore::new(&conn);
        let events = store.events_all()?;
        // Idempotency: proposals already cause-linked to this job — a
        // second fetch appends nothing and returns them.
        let existing = ProposalsProjection::live_for_job(&events, job.id)?;
        if !existing.is_empty() {
            (existing, 0)
        } else if results.stdout.trim().is_empty() {
            // No meaningful artifact — an honest empty fetch (nothing to
            // pin; the job still finished and its terminal is stamped).
            (Vec::new(), 0)
        } else {
            // The v1 targeting rule: the mission's FIRST hypothesis
            // (creation order) carries the result.
            let hyps = HypothesesProjection::fold_for(&events, job.mission_id)?;
            let Some(hyp) = hyps.first() else {
                return Err(EventError::Invalid(format!(
                    "no_hypothesis: the mission has no hypothesis to pin results onto — a \
                     result pin targets the mission's first hypothesis (v1)"
                )));
            };
            // The producing event: the job's `job.finished` (the terminal
            // that made the artifacts fetchable); the submitted event is
            // the fallback identity.
            let cause = events
                .iter()
                .filter(|e| e.kind == crate::domain::jobs::JOB_FINISHED)
                .find(|e| {
                    e.payload
                        .get("job_id")
                        .and_then(serde_json::Value::as_str)
                        .map(|s| s == job.id.to_string())
                        .unwrap_or(false)
                })
                .map(|e| e.id)
                .unwrap_or(job.id);
            let artifact_ref = format!("jobs/{}/stdout", job.id.simple());
            let run_id = format!("fetch-{}", job.id.simple());
            // The anchor claim (AD-5 — a pin attaches to a claim): the
            // fetch's user command registers it UNPINNED; the merge pins it.
            let claim = store.append(NewEvent::claim_registered(
                format!(
                    "Result artifact `{artifact_ref}` from job e-{} on `{}`",
                    job.seq, job.target
                ),
                hyp.id,
                None,
            )?)?;
            let proposal = propose_evidence_pin(
                &store,
                &run_id,
                claim.id,
                hyp.id,
                &artifact_ref,
                &results.stdout,
                RESULT_PIN_CONFIDENCE,
                &assessing_model,
                cause,
                vec![job.id, job.mission_id],
            )
            .map_err(|e| EventError::Invalid(e.to_string()))?;
            // A proposal is a mission-scoped event — evaluate the
            // terminators right after it lands (AD-12).
            let _ = crate::domain::nightshift::evaluate_terminals(&store);
            (vec![proposal], 1)
        }
    };
    // The re-folded job — the log is the only truth.
    let job = {
        let conn = db.0.lock().await;
        let events = EventStore::new(&conn).events_all()?;
        JobsProjection::fold(&events)?
            .into_iter()
            .find(|j| j.id == job_id)
            .ok_or_else(|| EventError::Invalid(format!("not_found: no job with id `{job_id}`")))?
    };
    Ok(FetchedJobResults {
        job,
        results,
        proposals,
        created,
    })
}

/// The result proposals of one job (read-only — the server shell's route):
/// every proposal cause-linked to the job, decided ones included (history
/// is honest, nothing disappears).
pub(crate) fn list_job_result_proposals_inner(
    events: &[StoredEvent],
    job_id: Uuid,
) -> Result<Vec<Proposal>, EventError> {
    ProposalsProjection::live_for_job(events, job_id)
}

/// Declare a named compute target (shared by the command and tests): the
/// kind must name a registered adapter, the name is a slug, an `ssh`
/// target requires its host (a `scheduler` target MAY carry one — its
/// submission node; the other kinds carry none), and the config map is
/// validated per kind — then one `target.declared` event and the fresh
/// list.
async fn declare_compute_target_inner(
    db: &Db,
    name: &str,
    kind: &str,
    host: Option<&str>,
    config: &BTreeMap<String, String>,
) -> Result<Vec<ComputeTargetView>, String> {
    let name = name.trim().to_string();
    let kind = kind.trim().to_string();
    let host = host.map(str::trim).filter(|h| !h.is_empty());
    let registry = TargetRegistry::v1();
    if !registry.kinds().contains(&kind.as_str()) {
        return Err(format!(
            "unknown_kind: `{kind}` — no adapter of that kind is registered ({})",
            registry.kinds().join(" | ")
        ));
    }
    if !matches!(kind.as_str(), "ssh" | "scheduler") && host.is_some() {
        return Err(format!(
            "invalid_host: a `{kind}` target carries no host — only ssh and scheduler targets name the machine they connect to"
        ));
    }
    let event = NewEvent::target_declared(TargetDeclaredPayload {
        name,
        kind,
        host: host.map(str::to_string),
        config: config.clone(),
    })
    .map_err(err)?;
    let conn = db.0.lock().await;
    let store = EventStore::new(&conn);
    store.append(event).map_err(err)?;
    let events = store.events_all().map_err(err)?;
    list_targets_inner(&events).map_err(err)
}

/// The registered adapter kinds with their contract versions (Story 6.5,
/// NFR-15): what the settings row lists — community adapters appear
/// exactly as first-party ones do.
#[tauri::command]
pub async fn list_registered_adapters() -> Result<Vec<RegisteredAdapterView>, String> {
    Ok(all_registered_adapters())
}

pub(crate) fn all_registered_adapters() -> Vec<RegisteredAdapterView> {
    let mut views: Vec<RegisteredAdapterView> =
        crate::adapters::targets::FIRST_PARTY_KINDS
            .iter()
            .map(|kind| RegisteredAdapterView {
                kind: kind.to_string(),
                contract_version: crate::adapters::targets::ADAPTER_CONTRACT_VERSION.to_string(),
                builtin: true,
            })
            .collect();
    for community in crate::adapters::targets::adapter_contract::community_adapters() {
        views.push(RegisteredAdapterView {
            kind: community.kind,
            contract_version: community.contract_version,
            builtin: false,
        });
    }
    views
}

/// Probe one compute target (the settings row's discovery/unreachable
/// state): a cheap reachability check through the target's own adapter —
/// never a submit, never a job. Unreachable targets answer with their
/// typed reason; kinds with no probe say so honestly.
#[tauri::command]
pub async fn probe_compute_target(
    db: State<'_, Db>,
    name: String,
) -> Result<TargetProbeView, String> {
    let (kind, info) = {
        let conn = db.0.lock().await;
        let events = EventStore::new(&conn).events_all().map_err(err)?;
        let declared = fold_declared_targets(&events);
        let Some(target) = declared.iter().find(|t| t.name == name) else {
            return Err(format!(
                "unknown_target: `{name}` — declared targets: {}",
                known_names(&declared).join(" | ")
            ));
        };
        (
            target.kind.clone(),
            TargetInfo {
                name: target.name.clone(),
                host: target.host.clone(),
                allowlist: fold_host_allowlist(&events),
                config: target.config.clone(),
            },
        )
    };
    Ok(match kind.as_str() {
        "local" => TargetProbeView {
            status: "ok".into(),
            detail: "local machine — always reachable".into(),
        },
        "ssh" => crate::adapters::targets::Ssh::new()
            .probe(&info)
            .map(|detail| TargetProbeView { status: "ok".into(), detail })
            .unwrap_or_else(|e| TargetProbeView { status: "unreachable".into(), detail: e.to_string() }),
        "scheduler" => crate::adapters::targets::Scheduler::new()
            .probe(&info)
            .map(|detail| TargetProbeView { status: "ok".into(), detail })
            .unwrap_or_else(|e| TargetProbeView { status: "unreachable".into(), detail: e.to_string() }),
        "kubernetes" => crate::adapters::targets::Kubernetes::new()
            .probe(&info)
            .map(|detail| TargetProbeView { status: "ok".into(), detail })
            .unwrap_or_else(|e| TargetProbeView { status: "unreachable".into(), detail: e.to_string() }),
        "chopflow" => crate::adapters::targets::ChopFlow::new()
            .probe(&info)
            .map(|detail| TargetProbeView { status: "ok".into(), detail })
            .unwrap_or_else(|e| TargetProbeView { status: "unreachable".into(), detail: e.to_string() }),
        other => TargetProbeView {
            status: "unsupported".into(),
            detail: format!("no probe for kind `{other}`"),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::jobs::{JOB_FAILED, JOB_FINISHED, JOB_RUNNING, JOB_SUBMITTED};
    use crate::domain::missions::{Autonomy, MissionCreatedPayload, MissionsProjection};
    use crate::domain::trust::{
        self, CeilingConfiguredPayload, Scope, TARGET_SPEND_RECORDED,
    };
    use crate::eventstore::NewEvent;
    use rusqlite::Connection;
    use std::collections::BTreeMap;

    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::Db::migrate(&conn).unwrap();
        EventStore::init(&conn).unwrap();
        Db(std::sync::Arc::new(tokio::sync::Mutex::new(conn)))
    }

    async fn create_mission(db: &Db) -> Uuid {
        let conn = db.0.lock().await;
        let stored = EventStore::new(&conn)
            .append(
                NewEvent::mission_created(MissionCreatedPayload {
                    question: "Does X hold?".into(),
                    stop_condition: "Stop after 3 rounds.".into(),
                    success_criterion: "A rater agrees.".into(),
                    autonomy: Autonomy::Suggest,
                    spend_ceiling_cents: 500,
                    roles: vec![],
                    schedule: "off".into(),
                })
                .unwrap(),
            )
            .unwrap();
        stored.id
    }

    fn spec(cmd: &str, args: &[&str]) -> crate::domain::jobs::JobSpec {
        crate::domain::jobs::JobSpec {
            cmd: cmd.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: BTreeMap::new(),
            resources: None,
            workdir: None,
        }
    }

    async fn events(db: &Db) -> Vec<crate::eventstore::StoredEvent> {
        let conn = db.0.lock().await;
        EventStore::new(&conn).events_all().unwrap()
    }

    /// Poll a mission's live jobs (bounded) until one job reaches a
    /// terminal — a fixed sleep flakes under a fully parallel test run,
    /// a bounded retry does not.
    async fn poll_until_finished(db: &Db, mission: Uuid, job_id: Uuid) -> Vec<Job> {
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            poll_live_jobs(db, Some(mission)).await.unwrap();
            let jobs = {
                let conn = db.0.lock().await;
                let all = EventStore::new(&conn).events_all().unwrap();
                JobsProjection::fold_for(&all, mission).unwrap()
            };
            if let Some(job) = jobs.iter().find(|j| j.id == job_id)
                && matches!(job.phase, JobPhase::Finished | JobPhase::Failed)
            {
                return jobs;
            }
        }
        panic!("the job never reached its terminal");
    }

    // ---- the registered-adapter listing (Story 6.5) ----

    #[test]
    fn registered_adapters_list_kind_and_contract_version() {
        let adapters = super::all_registered_adapters();
        assert_eq!(adapters.len(), crate::adapters::targets::FIRST_PARTY_KINDS.len());
        for view in &adapters {
            assert!(view.builtin, "the first-party kinds are builtin: {view:?}");
            assert_eq!(
                view.contract_version,
                crate::adapters::targets::ADAPTER_CONTRACT_VERSION
            );
        }
        assert!(adapters.iter().any(|a| a.kind == "local"));
        assert!(adapters.iter().any(|a| a.kind == "kubernetes"));
    }

    // ---- validation before submit (AD-6) ----

    #[tokio::test]
    async fn a_freeform_shell_spec_never_lands_anything() {
        let db = test_db();
        let mission = create_mission(&db).await;
        // the story's exact freeform construction — refused before the
        // mission lookup, the adapter, or any event
        let err = submit_job_inner(&db, mission, "local", spec("ls | rm -rf .", &[]))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("freeform_shell:"), "unexpected: {err}");
        // unknown mission / unknown target are typed, and nothing lands
        let err = submit_job_inner(&db, Uuid::new_v4(), "local", spec("echo", &["hi"]))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not_found"), "unexpected: {err}");
        let err = submit_job_inner(&db, mission, "the-cloud", spec("echo", &["hi"]))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown_target:"), "unexpected: {err}");
        assert!(err.to_string().contains("local"), "the error names what exists: {err}");
        assert!(events(&db).await.len() == 1, "only the mission creation exists");
    }

    // ---- the full lifecycle as events ----

    #[tokio::test]
    async fn a_local_job_lives_its_whole_lifecycle_in_the_log() {
        let db = test_db();
        let mission = create_mission(&db).await;
        // a fake long-running process: the poll-once at submit sees running
        let job = submit_job_inner(&db, mission, "local", spec("sleep", &["1"]))
            .await
            .unwrap();
        assert_eq!(job.target, "local");
        assert!(
            matches!(job.phase, JobPhase::Queued | JobPhase::Running),
            "a fresh job is queued or running, got {:?}",
            job.phase
        );
        // the mission card's runs drill-down sees the job events (AD-2)
        let runs = {
            let conn = db.0.lock().await;
            let all = EventStore::new(&conn).events_all().unwrap();
            MissionsProjection::runs_for(&all, mission)
        };
        assert!(runs.iter().any(|r| r.kind == JOB_SUBMITTED));

        // past the child's lifetime: the poll loop lands the terminal
        tokio::time::sleep(std::time::Duration::from_millis(1300)).await;
        poll_live_jobs(&db, Some(mission)).await.unwrap();
        let jobs = {
            let conn = db.0.lock().await;
            let all = EventStore::new(&conn).events_all().unwrap();
            JobsProjection::fold_for(&all, mission).unwrap()
        };
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].phase, JobPhase::Finished);
        assert_eq!(jobs[0].exit_code, Some(0));
        assert!(jobs[0].finished_ts.is_some(), "the terminal is always stamped");

        // the event order tells the story: submitted → running → finished
        // → target.spend_recorded (AD-10 — usage attributed to the target)
        let all = events(&db).await;
        let job_events: Vec<&crate::eventstore::StoredEvent> = all
            .iter()
            .filter(|e| {
                e.kind == JOB_SUBMITTED
                    || e.kind == JOB_RUNNING
                    || e.kind == JOB_FINISHED
                    || e.kind == JOB_FAILED
                    || e.kind == TARGET_SPEND_RECORDED
            })
            .collect();
        let kinds: Vec<&str> = job_events.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&JOB_RUNNING), "the lifecycle was observed: {kinds:?}");
        assert_eq!(*kinds.last().unwrap(), TARGET_SPEND_RECORDED);
        let spend = job_events.last().unwrap();
        assert_eq!(spend.payload["target"], serde_json::json!("local"));
        assert_eq!(spend.payload["mission_id"], serde_json::json!(mission.to_string()));
        assert_eq!(spend.payload["job_id"], serde_json::json!(job.id.to_string()));
        assert_eq!(spend.payload["cost_cents"], serde_json::json!(0), "local compute costs nothing measurable");
    }

    #[tokio::test]
    async fn a_spawn_failure_lands_a_reasoned_terminal_immediately() {
        let db = test_db();
        let mission = create_mission(&db).await;
        let job = submit_job_inner(&db, mission, "local", spec("definitely-not-a-binary-xyz", &[]))
            .await
            .unwrap();
        // the poll-once at submit already observed the terminal — no job
        // ends silently, not even one that never started
        assert_eq!(job.phase, JobPhase::Failed);
        assert!(job.reason.as_deref().unwrap_or("").starts_with("spawn_error:"));
        assert!(job.finished_ts.is_some());
        let all = events(&db).await;
        assert!(all.iter().any(|e| e.kind == JOB_FAILED));
        assert!(all.iter().any(|e| e.kind == TARGET_SPEND_RECORDED));
    }

    #[tokio::test]
    async fn a_nonzero_exit_is_a_failed_job_with_its_code() {
        let db = test_db();
        let mission = create_mission(&db).await;
        submit_job_inner(&db, mission, "local", spec("sh", &["-c", "exit 3"]))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        poll_live_jobs(&db, Some(mission)).await.unwrap();
        let jobs = {
            let conn = db.0.lock().await;
            let all = EventStore::new(&conn).events_all().unwrap();
            JobsProjection::fold_for(&all, mission).unwrap()
        };
        assert_eq!(jobs[0].phase, JobPhase::Failed);
        assert_eq!(jobs[0].exit_code, Some(3));
        assert_eq!(jobs[0].reason.as_deref(), Some("exit_code_3"));
    }

    // ---- fetch ----

    #[tokio::test]
    async fn fetch_returns_captured_output_only_after_terminal() {
        let db = test_db();
        let mission = create_mission(&db).await;
        let job = submit_job_inner(&db, mission, "local", spec("sleep", &["1"]))
            .await
            .unwrap();
        // while running: typed refusal
        let err = fetch_job_inner(&db, job.id).await.unwrap_err();
        assert!(err.to_string().contains("job_not_terminal:"), "unexpected: {err}");
        tokio::time::sleep(std::time::Duration::from_millis(1300)).await;
        poll_live_jobs(&db, Some(mission)).await.unwrap();
        let result = fetch_job_inner(&db, job.id).await.unwrap();
        assert_eq!(result.code, Some(0));
        // an unknown job id is honest about it
        let err = fetch_job_inner(&db, Uuid::new_v4()).await.unwrap_err();
        assert!(err.to_string().contains("not_found:"), "unexpected: {err}");
    }

    // ---- declared targets + the registry ----

    #[tokio::test]
    async fn targets_declare_list_and_refuse_unknown_kinds() {
        let db = test_db();
        // the registry registers the first-party kinds; anything else is
        // a typed error.
        let err = declare_compute_target_inner(&db, "cluster", "mainframe", None, &BTreeMap::new())
            .await
            .unwrap_err();
        assert!(err.starts_with("unknown_kind:"), "unexpected: {err}");
        assert!(err.contains("local"), "the error names the kinds: {err}");
        assert!(err.contains("kubernetes"), "the error names the kinds: {err}");
        // an ssh target requires its host; a local target carries none
        let err = declare_compute_target_inner(&db, "cluster", "ssh", None, &BTreeMap::new())
            .await
            .unwrap_err();
        assert!(err.contains("host"), "unexpected: {err}");
        let err = declare_compute_target_inner(&db, "laptop", "local", Some("gpu-01.lab"), &BTreeMap::new())
            .await
            .unwrap_err();
        assert!(err.starts_with("invalid_host:"), "unexpected: {err}");
        // declare a second local target — the list grows past the builtin
        declare_compute_target_inner(&db, "laptop", "local", None, &BTreeMap::new()).await.unwrap();
        let targets = {
            let conn = db.0.lock().await;
            let events = EventStore::new(&conn).events_all().unwrap();
            list_targets_inner(&events).unwrap()
        };
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].name, "local");
        assert!(targets[0].builtin, "the built-in local needs no declaration");
        assert!(targets[0].host.is_none() && targets[0].allowlisted.is_none());
        assert_eq!(targets[1].name, "laptop");
        assert_eq!(targets[1].kind, "local");
        assert!(!targets[1].builtin);
        // the declared name submits through the same local adapter
        let mission = create_mission(&db).await;
        let job = submit_job_inner(&db, mission, "laptop", spec("echo", &["on-laptop"]))
            .await
            .unwrap();
        assert_eq!(job.target, "laptop");
    }

    // ---- ssh targets: the allowlist (Story 3.3) ----

    /// The loopback harness: a stand-in ssh binary returning canned
    /// output, installed for the registry's adapter. Behaviorally
    /// identical across tests, so parallel installs are benign.
    fn install_fake_ssh() {
        let dir = std::env::temp_dir().join(format!("rc-ssh-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ssh");
        std::fs::write(&path, "#!/bin/sh\nprintf 'remote ok\\n'\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        crate::adapters::targets::ssh::install_ssh_bin(path.to_string_lossy().into_owned());
    }

    async fn declare_ssh_target(db: &Db, name: &str, host: &str) {
        declare_compute_target_inner(db, name, "ssh", Some(host), &BTreeMap::new())
            .await
            .unwrap();
    }

    /// The scheduler fake-binary harness (the config keys point the
    /// adapter at stand-ins — the same mechanism a real cluster uses).
    fn scheduler_fakes(submit_body: &str, poll_body: &str, acct_body: &str) -> BTreeMap<String, String> {
        let dir = std::env::temp_dir().join(format!("rc-cmd-sched-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut config = BTreeMap::new();
        for (name, body) in [("sbatch", submit_body), ("squeue", poll_body), ("sacct", acct_body)] {
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            let key = match name {
                "sbatch" => "submitPrefix",
                "squeue" => "pollPrefix",
                _ => "acctPrefix",
            };
            config.insert(key.to_string(), path.to_string_lossy().into_owned());
        }
        config
    }

    // ---- chopflow targets: first-class optional + spend (Story 6.4) ----

    #[tokio::test]
    async fn a_chopflow_target_records_its_spend_and_down_states_never_crash() {
        let queue = crate::adapters::targets::chopflow::test_harness::spawn_fake_queue();
        let db = test_db();
        let mission = create_mission(&db).await;
        let config = BTreeMap::from([
            ("endpoint".to_string(), queue.endpoint.clone()),
            ("queue".to_string(), "gpu-queue".to_string()),
        ]);
        declare_compute_target_inner(&db, "queue-1", "chopflow", None, &config)
            .await
            .unwrap();
        // the declared target lists its kind and config
        {
            let conn = db.0.lock().await;
            let events = EventStore::new(&conn).events_all().unwrap();
            let targets = list_targets_inner(&events).unwrap();
            let target = targets.iter().find(|t| t.name == "queue-1").unwrap();
            assert_eq!(target.kind, "chopflow");
            assert_eq!(target.config.get("endpoint").map(String::as_str), Some(queue.endpoint.as_str()));
            assert_eq!(target.allowlisted, None, "no gate — the endpoint is the config");
        }
        // the queue goes down MID-FLIGHT: an accepted job keeps its last
        // observed state (typed error, the poll skips, nothing crashes)
        let job = submit_job_initiated(
            &db,
            mission,
            "queue-1",
            spec("python3", &["train.py"]),
            Initiator::User,
        )
        .await
        .unwrap();
        assert_eq!(job.target, "queue-1");
        poll_live_jobs(&db, Some(mission)).await.unwrap();
        *queue.state.lock().unwrap() = r#"{"state":"finished"}"#.into();
        let jobs = poll_until_finished(&db, mission, job.id).await;
        assert_eq!(jobs[0].phase, JobPhase::Finished);
        // usage attributed to the chopflow target like any other (AD-10)
        let all = events(&db).await;
        assert!(
            all.iter().any(|e| {
                e.kind == TARGET_SPEND_RECORDED
                    && e.payload["target"] == serde_json::json!("queue-1")
            }),
            "the chopflow job's spend landed"
        );
        // an unreachable endpoint on a NEW submit is a typed error and
        // nothing lands in the log (first-class optional, never a crash)
        let dead = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
            drop(listener);
            endpoint
        };
        let config = BTreeMap::from([("endpoint".to_string(), dead)]);
        declare_compute_target_inner(&db, "queue-down", "chopflow", None, &config)
            .await
            .unwrap();
        let head = {
            let conn = db.0.lock().await;
            EventStore::new(&conn).head_seq().unwrap()
        };
        let err = submit_job_initiated(
            &db,
            mission,
            "queue-down",
            spec("python3", &[]),
            Initiator::User,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("endpoint_unreachable:"), "unexpected: {err}");
        let head_after = {
            let conn = db.0.lock().await;
            EventStore::new(&conn).head_seq().unwrap()
        };
        assert_eq!(head_after, head, "a refused submit lands nothing");
    }

    // ---- kubernetes targets: the context gate + lifecycle + spend (Story 6.3) ----

    /// The fake-kubectl harness (kubectlPrefix points the adapter at a
    /// stand-in; the state file is what `get job` answers).
    fn fake_kubectl(state: &str) -> BTreeMap<String, String> {
        let dir = std::env::temp_dir().join(format!("rc-cmd-kube-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let state_path = dir.join("state");
        std::fs::write(&state_path, state).unwrap();
        let bin = dir.join("kubectl");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\ncase \"$*\" in\n  *\" get \"*) cat {} ;;\n  *\" logs \"*) printf 'container out\\n' ;;\n  *) exit 0 ;;\nesac\n",
                crate::adapters::targets::shell_quote(state_path.to_str().unwrap())
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let config = BTreeMap::from([
            ("context".to_string(), "lab-gpu".to_string()),
            ("namespace".to_string(), "research".to_string()),
            ("image".to_string(), "ghcr.io/lab/trainer:latest".to_string()),
            ("kubectlPrefix".to_string(), bin.to_string_lossy().into_owned()),
        ]);
        config
    }

    #[tokio::test]
    async fn a_kubernetes_target_gates_its_context_and_records_its_spend() {
        let db = test_db();
        let mission = create_mission(&db).await;
        let config =
            fake_kubectl(r#"{"status": {"conditions": [{"type": "Complete", "status": "True"}]}}"#);
        declare_compute_target_inner(&db, "k8s-lab", "kubernetes", None, &config)
            .await
            .unwrap();
        // the declared target lists its kind, config, and context gate
        {
            let conn = db.0.lock().await;
            let events = EventStore::new(&conn).events_all().unwrap();
            let targets = list_targets_inner(&events).unwrap();
            let k8s = targets.iter().find(|t| t.name == "k8s-lab").unwrap();
            assert_eq!(k8s.kind, "kubernetes");
            assert_eq!(k8s.config.get("context").map(String::as_str), Some("lab-gpu"));
            assert_eq!(k8s.allowlisted, Some(false), "the context is not on the allowlist yet");
        }
        // the context is not on the allowlist — the submit is refused
        // with the typed error, before any connection
        let err = submit_job_initiated(
            &db,
            mission,
            "k8s-lab",
            spec("python3", &["train.py"]),
            Initiator::User,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("context_not_allowed:"), "unexpected: {err}");
        // allowlisting the context lets the job through
        set_host_allowlist_inner(&db, vec!["lab-gpu".into()])
            .await
            .unwrap();
        let job = submit_job_initiated(
            &db,
            mission,
            "k8s-lab",
            spec("python3", &["train.py"]),
            Initiator::User,
        )
        .await
        .unwrap();
        assert_eq!(job.target, "k8s-lab");
        let jobs = poll_until_finished(&db, mission, job.id).await;
        assert_eq!(jobs[0].phase, JobPhase::Finished);
        assert_eq!(jobs[0].exit_code, Some(0));
        // usage attributed to the kubernetes target like any other (AD-10)
        let all = events(&db).await;
        assert!(
            all.iter().any(|e| {
                e.kind == TARGET_SPEND_RECORDED && e.payload["target"] == serde_json::json!("k8s-lab")
            }),
            "the kubernetes job's spend landed"
        );
    }

    // ---- scheduler targets: the whole lifecycle + spend (Story 6.2) ----

    #[tokio::test]
    async fn a_scheduler_job_lives_its_lifecycle_and_records_its_spend() {
        let db = test_db();
        let mission = create_mission(&db).await;
        let config = scheduler_fakes(
            "printf 'Submitted batch job 4242\\n'",
            "printf 'squeue: error: Invalid job id specified\\n' >&2\nexit 1",
            "printf 'COMPLETED 0:0\\n'",
        );
        declare_compute_target_inner(&db, "cluster-1", "scheduler", None, &config)
            .await
            .unwrap();
        // the declared target lists its kind and config
        {
            let conn = db.0.lock().await;
            let events = EventStore::new(&conn).events_all().unwrap();
            let targets = list_targets_inner(&events).unwrap();
            let cluster = targets.iter().find(|t| t.name == "cluster-1").unwrap();
            assert_eq!(cluster.kind, "scheduler");
            assert_eq!(cluster.config.get("flavor"), None);
            assert!(cluster.config.contains_key("submitPrefix"));
            assert_eq!(cluster.allowlisted, None, "no host — no gate");
        }
        // a host-carrying scheduler target is allowlist-gated like ssh
        let err = declare_compute_target_inner(
            &db,
            "cluster-2",
            "scheduler",
            Some("login.hpc.edu"),
            &BTreeMap::new(),
        )
        .await;
        assert!(err.is_ok(), "declaring is fine — the submit is gated");
        let err = submit_job_initiated(
            &db,
            mission,
            "cluster-2",
            spec("python3", &["train.py"]),
            Initiator::User,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("host_not_allowed:"), "unexpected: {err}");
        // the free host-less target submits through the fake harness
        let job = submit_job_initiated(
            &db,
            mission,
            "cluster-1",
            spec("python3", &["train.py"]),
            Initiator::User,
        )
        .await
        .unwrap();
        assert_eq!(job.target, "cluster-1");
        // the phase machine needs its polls: submitting → the cluster job
        // is known → the poll resolves the terminal
        let jobs = poll_until_finished(&db, mission, job.id).await;
        assert_eq!(jobs[0].phase, JobPhase::Finished, "phase: {:?}", jobs[0].phase);
        assert_eq!(jobs[0].exit_code, Some(0));
        // usage attributed to the scheduler target like any other (AD-10)
        let all = events(&db).await;
        assert!(all.iter().any(|e| {
            e.kind == TARGET_SPEND_RECORDED && e.payload["target"] == serde_json::json!("cluster-1")
        }), "the scheduler job's spend landed");
    }

    #[tokio::test]
    async fn an_ssh_target_declares_with_host_and_lists_its_allowlisted_status() {
        let db = test_db();
        declare_ssh_target(&db, "cluster-1", "gpu-01.lab").await;
        // not allowlisted yet — the view says so
        let targets = {
            let conn = db.0.lock().await;
            let events = EventStore::new(&conn).events_all().unwrap();
            list_targets_inner(&events).unwrap()
        };
        let cluster = targets.iter().find(|t| t.name == "cluster-1").unwrap();
        assert_eq!(cluster.kind, "ssh");
        assert_eq!(cluster.host.as_deref(), Some("gpu-01.lab"));
        assert_eq!(cluster.allowlisted, Some(false), "not on the allowlist yet");
        // the allowlist edit persists and flips the status
        let allowlist = set_host_allowlist_inner(&db, vec!["gpu-01.lab".into(), "10.0.0.4".into()])
            .await
            .unwrap();
        assert_eq!(allowlist, vec!["gpu-01.lab".to_string(), "10.0.0.4".to_string()]);
        let targets = {
            let conn = db.0.lock().await;
            let events = EventStore::new(&conn).events_all().unwrap();
            list_targets_inner(&events).unwrap()
        };
        let cluster = targets.iter().find(|t| t.name == "cluster-1").unwrap();
        assert_eq!(cluster.allowlisted, Some(true));
        // malformed entries never land
        let err = set_host_allowlist_inner(&db, vec!["bad host".into()])
            .await
            .unwrap_err();
        assert!(err.contains("invalid_host:"), "unexpected: {err}");
    }

    #[tokio::test]
    async fn a_host_outside_the_allowlist_is_refused_before_any_connection() {
        let db = test_db();
        let mission = create_mission(&db).await;
        declare_ssh_target(&db, "cluster-1", "gpu-01.lab").await;
        // the allowlist names a DIFFERENT host — the submit is refused
        // with a typed error, and nothing lands in the log
        set_host_allowlist_inner(&db, vec!["other-host.lab".into()])
            .await
            .unwrap();
        let err = submit_job_initiated(
            &db,
            mission,
            "cluster-1",
            spec("python3", &["train.py"]),
            Initiator::User,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("host_not_allowed:"), "unexpected: {err}");
        assert!(err.to_string().contains("gpu-01.lab"), "the error names the host: {err}");
        assert!(err.to_string().contains("other-host.lab"), "the error names the allowlist: {err}");
        // no job.submitted ever landed — the refusal predates the adapter
        let all = events(&db).await;
        assert!(
            !all.iter().any(|e| e.kind == crate::domain::jobs::JOB_SUBMITTED),
            "a refused host never enters the log"
        );
    }

    #[tokio::test]
    async fn an_ssh_job_lives_its_lifecycle_over_the_loopback_harness() {
        let _lock = crate::adapters::targets::ssh::ssh_test_lock();
        install_fake_ssh();
        let db = test_db();
        let mission = create_mission(&db).await;
        declare_ssh_target(&db, "cluster-1", "gpu-01.lab").await;
        set_host_allowlist_inner(&db, vec!["gpu-01.lab".into()])
            .await
            .unwrap();
        // submit → running → finished → fetched, all through the ssh seam
        let job = submit_job_initiated(
            &db,
            mission,
            "cluster-1",
            spec("python3", &["train.py", "--epochs=3"]),
            Initiator::User,
        )
        .await
        .unwrap();
        assert_eq!(job.target, "cluster-1");
        let jobs = poll_until_finished(&db, mission, job.id).await;
        assert_eq!(jobs[0].phase, JobPhase::Finished);
        assert_eq!(jobs[0].exit_code, Some(0));
        let result = fetch_job_inner(&db, job.id).await.unwrap();
        assert_eq!(result.code, Some(0));
        assert_eq!(result.stdout.trim(), "remote ok");
        // usage attributed to the ssh target like any other (AD-10)
        let all = events(&db).await;
        assert!(all
            .iter()
            .any(|e| e.kind == TARGET_SPEND_RECORDED && e.payload["target"] == serde_json::json!("cluster-1")));
    }

    // ---- per-target autonomy governs agent-initiated submits (FR-5.3) ----

    #[tokio::test]
    async fn an_agent_submit_needs_act_with_receipts_on_the_target() {
        let _lock = crate::adapters::targets::ssh::ssh_test_lock();
        install_fake_ssh();
        let db = test_db();
        // an act-with-receipts mission — the per-TARGET dial is then the
        // only thing that can tighten (the story's cluster/laptop split)
        let mission = {
            let conn = db.0.lock().await;
            EventStore::new(&conn)
                .append(
                    NewEvent::mission_created(MissionCreatedPayload {
                        question: "Does X hold?".into(),
                        stop_condition: "Stop after 3 rounds.".into(),
                        success_criterion: "A rater agrees.".into(),
                        autonomy: Autonomy::ActWithReceipts,
                        spend_ceiling_cents: 500,
                        roles: vec![],
                        schedule: "off".into(),
                    })
                    .unwrap(),
                )
                .unwrap()
                .id
        };
        declare_ssh_target(&db, "cluster-1", "gpu-01.lab").await;
        declare_compute_target_inner(&db, "laptop", "local", None, &BTreeMap::new())
            .await
            .unwrap();
        set_host_allowlist_inner(&db, vec!["gpu-01.lab".into()])
            .await
            .unwrap();
        // the laptop's dial is watch — an agent may NEVER auto-enqueue
        // there; the cluster is unconfigured (act-with-receipts by default)
        {
            let conn = db.0.lock().await;
            EventStore::new(&conn)
                .append(
                    NewEvent::autonomy_configured(
                        crate::domain::trust::AutonomyConfiguredPayload {
                            scope: Scope::Target,
                            scope_id: Some("laptop".into()),
                            mode: Autonomy::Watch,
                        },
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let err = submit_job_initiated(
            &db,
            mission,
            "laptop",
            spec("python3", &["train.py"]),
            Initiator::Agent,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("autonomy_refused:"), "unexpected: {err}");
        assert!(err.to_string().contains("laptop"), "the error names the target: {err}");
        // the cluster allows the agent submit — and the user submits to
        // the laptop regardless (the dial governs agent dispatches only)
        let job = submit_job_initiated(
            &db,
            mission,
            "cluster-1",
            spec("python3", &["train.py"]),
            Initiator::Agent,
        )
        .await
        .unwrap();
        assert_eq!(job.target, "cluster-1");
        let job = submit_job_initiated(
            &db,
            mission,
            "laptop",
            spec("echo", &["user-submits-always-proceed"]),
            Initiator::User,
        )
        .await
        .unwrap();
        assert_eq!(job.target, "laptop");
        // and a suggest-mission refuses agent submits on ANY target —
        // enqueueing a job is acting (FR-5.1); the story's cluster/laptop
        // split assumes an act-with-receipts mission
        let suggest_mission = create_mission(&db).await;
        let err = submit_job_initiated(
            &db,
            suggest_mission,
            "cluster-1",
            spec("python3", &["train.py"]),
            Initiator::Agent,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("autonomy_refused:"), "unexpected: {err}");
        // and a killed runtime refuses every agent submit (AD-15e)
        {
            let conn = db.0.lock().await;
            EventStore::new(&conn).append(NewEvent::runtime_killed().unwrap()).unwrap();
        }
        let err = submit_job_initiated(
            &db,
            mission,
            "cluster-1",
            spec("python3", &["train.py"]),
            Initiator::Agent,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("killed:"), "unexpected: {err}");
    }

    // ---- per-target ceilings start being computable (AD-10) ----

    #[tokio::test]
    async fn job_spend_folds_into_per_target_ceilings() {
        let db = test_db();
        let mission = create_mission(&db).await;
        // one real local job: its target.spend_recorded lands (cost 0 —
        // local compute is free, the usage is still attributed)
        submit_job_inner(&db, mission, "local", spec("sh", &["-c", "exit 0"]))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        poll_live_jobs(&db, Some(mission)).await.unwrap();
        // a target-scoped ceiling: local capped at 100¢
        {
            let conn = db.0.lock().await;
            EventStore::new(&conn)
                .append(
                    NewEvent::ceiling_configured(CeilingConfiguredPayload {
                        scope: Scope::Target,
                        scope_id: Some("local".into()),
                        ceiling_cents: 100,
                    })
                    .unwrap(),
                )
                .unwrap();
        }
        let all = events(&db).await;
        let ledger = trust::spend_ledger(&all);
        assert!(ledger.target_cents.contains_key("local"), "the job's usage folded per target");
        // a further 150¢ target spend crosses the 100¢ ceiling — the
        // check the runtime runs pre-dispatch now sees target spend
        {
            let conn = db.0.lock().await;
            EventStore::new(&conn)
                .append(
                    NewEvent::target_spend_recorded(TargetSpendRecordedPayload {
                        target: "local".into(),
                        cost_cents: 150,
                        run_id: None,
                        mission_id: None,
                        job_id: None,
                    })
                    .unwrap(),
                )
                .unwrap();
        }
        let all = events(&db).await;
        let config = trust::trust_config(&all);
        let ledger = trust::spend_ledger(&all);
        assert_eq!(ledger.target_cents["local"], 150);
        let refusal = trust::check_ceilings(
            &config,
            &ledger,
            &Default::default(),
            None,
            "local",
            1,
        )
        .expect("150¢ recorded against a 100¢ target ceiling refuses the next cent");
        assert_eq!(refusal.scope, "target");
        assert_eq!(refusal.ceiling_cents, 100);
    }

    // ---- the poll loop is mission-scoped when asked ----

    #[tokio::test]
    async fn polling_one_mission_leaves_another_missions_jobs_alone() {
        let db = test_db();
        let a = create_mission(&db).await;
        let b = create_mission(&db).await;
        submit_job_inner(&db, a, "local", spec("sleep", &["1"])).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(1300)).await;
        // poll B: A's job stays as last observed
        poll_live_jobs(&db, Some(b)).await.unwrap();
        let jobs_a = {
            let conn = db.0.lock().await;
            let all = EventStore::new(&conn).events_all().unwrap();
            JobsProjection::fold_for(&all, a).unwrap()
        };
        assert!(
            matches!(jobs_a[0].phase, JobPhase::Queued | JobPhase::Running),
            "B's poll never touched A's job"
        );
        // poll A: the terminal lands
        poll_live_jobs(&db, Some(a)).await.unwrap();
        let jobs_a = {
            let conn = db.0.lock().await;
            let all = EventStore::new(&conn).events_all().unwrap();
            JobsProjection::fold_for(&all, a).unwrap()
        };
        assert_eq!(jobs_a[0].phase, JobPhase::Finished);
    }

    // ---- fetch results → quarantined evidence (Story 3.4, FR-11.5) ----

    use crate::domain::evidence::{excerpt_digest, EvidenceProjection, EVIDENCE_PINNED};
    use crate::domain::proposals::{self, ProposalStatus};

    async fn create_mission_with_hypothesis(db: &Db) -> (Uuid, Uuid) {
        let mission = create_mission(db).await;
        let conn = db.0.lock().await;
        let hyp = EventStore::new(&conn)
            .append(NewEvent::hypothesis_created("The cluster's run holds up.", mission).unwrap())
            .unwrap()
            .id;
        (mission, hyp)
    }

    /// Submit an `echo` job, wait past its lifetime, land its terminal.
    async fn finished_echo_job(db: &Db, mission: Uuid, text: &str) -> Job {
        let job = submit_job_inner(db, mission, "local", spec("echo", &[text]))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        poll_live_jobs(db, Some(mission)).await.unwrap();
        let conn = db.0.lock().await;
        let all = EventStore::new(&conn).events_all().unwrap();
        JobsProjection::fold_for(&all, mission)
            .unwrap()
            .into_iter()
            .find(|j| j.id == job.id)
            .unwrap()
    }

    #[tokio::test]
    async fn fetch_results_lands_quarantined_numerical_pin_candidates() {
        let db = test_db();
        let (mission, hyp) = create_mission_with_hypothesis(&db).await;
        // attribution comes from the current provider config (FR-3.6)
        {
            let conn = db.0.lock().await;
            crate::db::set_setting(&conn, "model", "glm-test").unwrap();
        }
        let job = finished_echo_job(&db, mission, "accuracy: 0.912, n=5").await;
        assert_eq!(job.phase, JobPhase::Finished);

        let fetched = fetch_job_results_inner(&db, job.id).await.unwrap();
        assert_eq!(fetched.created, 1, "one proposal per meaningful artifact");
        assert_eq!(fetched.proposals.len(), 1);
        assert_eq!(fetched.results.stdout.trim(), "accuracy: 0.912, n=5");
        let p = &fetched.proposals[0];
        assert_eq!(p.status, ProposalStatus::Pending);
        assert_eq!(p.proposed_kind, EVIDENCE_PINNED);
        assert_eq!(p.target_entity, hyp, "the v1 targeting rule: the mission's first hypothesis");
        assert_eq!(p.mission_id, Some(mission));
        // the intended pin candidate: artifact_ref + digest computed at
        // proposal time (AD-5) + attributed confidence
        let artifact_ref = format!("jobs/{}/stdout", job.id.simple());
        assert_eq!(p.proposed_payload["artifact_ref"], serde_json::json!(artifact_ref));
        assert_eq!(p.proposed_payload["excerpt"], serde_json::json!(fetched.results.stdout));
        assert_eq!(
            p.proposed_payload["digest"],
            serde_json::json!(excerpt_digest(&fetched.results.stdout))
        );
        assert_eq!(p.proposed_payload["kind"], serde_json::json!("numerical"));
        assert_eq!(p.proposed_payload["confidence"], serde_json::json!(RESULT_PIN_CONFIDENCE));
        assert_eq!(p.proposed_payload["assessing_model"], serde_json::json!("glm-test"));
        // cause-linked to the job + the mission (AD-2), actor = the fetch run
        let created = events(&db)
            .await
            .into_iter()
            .find(|e| e.id == p.id)
            .unwrap();
        assert!(created.causes.contains(&job.id), "cause-linked to the job");
        assert!(created.causes.contains(&mission), "cause-linked to the mission");
        assert_eq!(
            created.actor,
            crate::eventstore::Actor::Agent { run_id: format!("fetch-{}", job.id.simple()) }
        );
        // the anchor claim: registered UNPINNED on the first hypothesis
        // (FR-3.4 — amber until the merge pins it)
        let claims = {
            let conn = db.0.lock().await;
            let all = EventStore::new(&conn).events_all().unwrap();
            EvidenceProjection::fold_for(&all, hyp).unwrap()
        };
        assert_eq!(claims.len(), 1);
        assert!(!claims[0].pinned, "the candidate waits in quarantine (AD-3)");
        assert_eq!(claims[0].hypothesis_id, hyp);
        // THE STRUCTURAL GUARANTEE: no evidence.pinned event ever lands from
        // the fetch path — the pin applies only through a merged proposal
        // (auto-pinning is v0.2.0); in particular no actor=agent append
        assert!(
            !events(&db).await.iter().any(|e| e.kind == EVIDENCE_PINNED),
            "fetch appends proposals, never pins"
        );
        // the proposal surfaces in the mission's runs drill-down (AD-2)
        let runs = {
            let conn = db.0.lock().await;
            let all = EventStore::new(&conn).events_all().unwrap();
            MissionsProjection::runs_for(&all, mission)
        };
        assert!(runs.iter().any(|r| r.kind == "proposal.created"));
    }

    #[tokio::test]
    async fn merging_the_result_proposal_pins_the_numerical_evidence() {
        let db = test_db();
        let (mission, hyp) = create_mission_with_hypothesis(&db).await;
        let job = finished_echo_job(&db, mission, "gain: 12.3%, n=48").await;
        let fetched = fetch_job_results_inner(&db, job.id).await.unwrap();
        let p = fetched.proposals[0].clone();
        // the human merges — the only path to a pin
        let outcome = {
            let conn = db.0.lock().await;
            let store = EventStore::new(&conn);
            proposals::approve(&store, p.id, false)
                .map_err(|e| EventError::Invalid(e.to_string()))
                .unwrap()
        };
        assert_eq!(outcome.proposal.status, ProposalStatus::Merged);
        // still no evidence.pinned event in the log — the fold applies the
        // intended pin at the approval (AD-13)
        assert!(
            !events(&db).await.iter().any(|e| e.kind == EVIDENCE_PINNED),
            "the merge applies the pin in-fold; nothing auto-pins"
        );
        let claims = {
            let conn = db.0.lock().await;
            let all = EventStore::new(&conn).events_all().unwrap();
            EvidenceProjection::fold_for(&all, hyp).unwrap()
        };
        assert_eq!(claims.len(), 1);
        let claim = &claims[0];
        assert!(claim.pinned, "the merge pinned the result claim");
        let pin = claim.pin.as_ref().unwrap();
        assert_eq!(pin.kind, crate::domain::evidence::PinKind::Numerical);
        assert_eq!(
            pin.artifact_ref.as_deref(),
            Some(format!("jobs/{}/stdout", job.id.simple()).as_str())
        );
        assert_eq!(pin.digest, excerpt_digest(&fetched.results.stdout));
        assert_eq!(pin.assessing_model, "simulated", "the unconfigured fallback names itself");
        // and the hypothesis board is untouched — a pin never transitions
        let hyps = {
            let conn = db.0.lock().await;
            let all = EventStore::new(&conn).events_all().unwrap();
            crate::domain::hypotheses::HypothesesProjection::fold_for(&all, mission).unwrap()
        };
        assert_eq!(
            hyps[0].status,
            crate::domain::hypotheses::HypothesisStatus::Proposed
        );
    }

    #[tokio::test]
    async fn a_second_fetch_appends_nothing() {
        let db = test_db();
        let (mission, _hyp) = create_mission_with_hypothesis(&db).await;
        let job = finished_echo_job(&db, mission, "values").await;
        let first = fetch_job_results_inner(&db, job.id).await.unwrap();
        assert_eq!(first.created, 1);
        let head = {
            let conn = db.0.lock().await;
            EventStore::new(&conn).head_seq().unwrap()
        };
        // the idempotent re-fetch: the proposals come back, nothing appends
        let second = fetch_job_results_inner(&db, job.id).await.unwrap();
        assert_eq!(second.created, 0, "a second fetch appends nothing");
        assert_eq!(second.proposals.len(), 1);
        assert_eq!(second.proposals[0].id, first.proposals[0].id);
        let head_after = {
            let conn = db.0.lock().await;
            EventStore::new(&conn).head_seq().unwrap()
        };
        assert_eq!(head_after, head, "no claim, no proposal — nothing appended");
    }

    #[tokio::test]
    async fn fetch_results_needs_a_finished_job_and_a_hypothesis() {
        let db = test_db();
        let (mission, _hyp) = create_mission_with_hypothesis(&db).await;
        // a live job: typed refusal, nothing appended
        let job = submit_job_inner(&db, mission, "local", spec("sleep", &["1"]))
            .await
            .unwrap();
        let head = {
            let conn = db.0.lock().await;
            EventStore::new(&conn).head_seq().unwrap()
        };
        let err = fetch_job_results_inner(&db, job.id).await.unwrap_err();
        assert!(err.to_string().contains("job_not_finished"), "unexpected: {err}");
        assert_eq!(
            {
                let conn = db.0.lock().await;
                EventStore::new(&conn).head_seq().unwrap()
            },
            head
        );
        // a failed job produced no result artifacts either — its reasoned
        // terminal is the record, not evidence
        tokio::time::sleep(std::time::Duration::from_millis(1300)).await;
        poll_live_jobs(&db, Some(mission)).await.unwrap();
        let failed = submit_job_inner(&db, mission, "local", spec("sh", &["-c", "exit 3"]))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        poll_live_jobs(&db, Some(mission)).await.unwrap();
        let err = fetch_job_results_inner(&db, failed.id).await.unwrap_err();
        assert!(err.to_string().contains("job_not_finished"), "unexpected: {err}");
        // a finished job on a mission with NO hypothesis: the targeting rule
        // refuses honestly — nothing to pin onto
        let bare = create_mission(&db).await;
        let done = finished_echo_job(&db, bare, "orphan values").await;
        let err = fetch_job_results_inner(&db, done.id).await.unwrap_err();
        assert!(err.to_string().contains("no_hypothesis"), "unexpected: {err}");
        // an unknown job id is honest about it
        let err = fetch_job_results_inner(&db, Uuid::new_v4()).await.unwrap_err();
        assert!(err.to_string().contains("not_found"), "unexpected: {err}");
    }

    /// A finished job with no meaningful artifact (empty stdout) is an
    /// honest empty fetch: zero proposals, no error, nothing pinned.
    #[tokio::test]
    async fn a_job_with_no_output_proposes_nothing() {
        let db = test_db();
        let (mission, _hyp) = create_mission_with_hypothesis(&db).await;
        // `sleep 0` finishes silently — stdout is empty
        let job = submit_job_inner(&db, mission, "local", spec("sleep", &["0"]))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        poll_live_jobs(&db, Some(mission)).await.unwrap();
        let fetched = fetch_job_results_inner(&db, job.id).await.unwrap();
        assert_eq!(fetched.created, 0);
        assert!(fetched.proposals.is_empty());
        assert!(
            !events(&db).await.iter().any(|e| e.kind == "claim.registered"),
            "no artifact, no anchor claim"
        );
    }
}
