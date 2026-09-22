// Compute-job domain (Story 3.2, FR-11.1/11.2/11.4, AD-6): structured job
// specs and the job lifecycle as events. A job is a typed JSON spec
// (`cmd, args, env, resources, workdir`) submitted to a named compute
// target — validated BEFORE submit, executed by the target's adapter with
// argv directly, never through a shell string (AD-6). The lifecycle
// (submit → monitor → terminal) is evented; every terminal state carries
// a timestamp and a reason (AD-12 style — no job ends silently).
//
// Event kinds owned here:
//
// - `job.submitted` — the spec + target + handle, actor=user (a shell
//   command submits; agents enqueue through the same seam in a later
//   story). The typed constructor runs `JobSpec::validate` — a freeform
//   shell construction never enters the log.
// - `job.running` / `job.finished` / `job.failed` — the adapter-observed
//   lifecycle, actor `system (runtime)`. `job.finished` carries the exit
//   code; `job.failed` carries a reason (spawn error, nonzero exit,
//   signal) and the code when there was one.
// - `target.declared` — a named compute target declared by the user
//   (actor=user): `{ name, kind, host? }`. The kind names the adapter that
//   runs its jobs (v1: `local`, `ssh`); an `ssh` target carries the host it
//   connects to (a single token — it becomes one argv element of the ssh
//   binary, so it is validated like one). The built-in target `local` needs
//   no declaration.
// - `host_allowlist.edited` — the workspace host allowlist (Story 3.3,
//   actor=user): the full list of hosts SSH targets may connect to, latest
//   event wins. Hosts outside it are refused before any connection is
//   attempted (typed `host_not_allowed:` error).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::eventstore::{Actor, EventError, NewEvent, StoredEvent, SystemComponent};

pub const JOB_SUBMITTED: &str = "job.submitted";
pub const JOB_RUNNING: &str = "job.running";
pub const JOB_FINISHED: &str = "job.finished";
pub const JOB_FAILED: &str = "job.failed";
pub const TARGET_DECLARED: &str = "target.declared";
pub const HOST_ALLOWLIST_EDITED: &str = "host_allowlist.edited";

/// The built-in compute target every workspace has (FR-11.1: v1 ships
/// local): name `local`, adapter kind `local`. Declared targets may reuse
/// the kind under other names; the built-in needs no `target.declared`.
pub const DEFAULT_TARGET_NAME: &str = "local";
pub const DEFAULT_TARGET_KIND: &str = "local";

// ---------------------------------------------------------------------------
// The job spec (AD-6: typed JSON, validated before submit)
// ---------------------------------------------------------------------------

/// The characters a `cmd` must never carry: each one is shell syntax the
/// runtime would have to interpret — and never does (AD-6). The set covers
/// the story's freeform examples (`;`, `&&`, `|`, backticks, `$(`) at the
/// single-character level, plus the redirections.
pub const SHELL_METACHARS: &[char] = &[';', '&', '|', '`', '<', '>', '$', '\n', '\r'];

/// A job's resource request (FR-11.1, optional): what the spec asks the
/// target for. The Local adapter records but does not enforce it (no
/// cgroups on a laptop); the SSH adapter (Story 3.3) forwards it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobResources {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpus: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u64>,
}

/// The structured job spec (AD-6): a single executable (`cmd`), its
/// arguments as a list, environment overlays, an optional resource
/// request, and an optional working directory. Field names are camelCase
/// on the wire (Tauri 2 convention).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSpec {
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<JobResources>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<String>,
}

impl JobSpec {
    /// Validate the spec before submit (AD-6). `cmd` is a single
    /// executable path — shell syntax (`;`, `&&`, `|`, backticks, `$(`,
    /// redirections, newlines) is rejected outright: the runtime executes
    /// argv directly and never constructs shell strings, and validation is
    /// the second layer of that guarantee (the future SSH adapter's remote
    /// side benefits from it too). `args` need no metacharacter check —
    /// argv entries are data, never interpreted.
    pub fn validate(&self) -> Result<(), SpecError> {
        if self.cmd.trim().is_empty() {
            return Err(SpecError::EmptyCmd);
        }
        if let Some(ch) = self.cmd.chars().find(|c| SHELL_METACHARS.contains(c)) {
            return Err(SpecError::FreeformShell { cmd: self.cmd.clone(), ch });
        }
        for key in self.env.keys() {
            if key.trim().is_empty() || key.contains('=') || key.contains('\0') {
                return Err(SpecError::InvalidEnvKey(key.clone()));
            }
        }
        if let Some(r) = self.resources {
            if r.cpus == Some(0) || r.memory_mb == Some(0) {
                return Err(SpecError::InvalidResources);
            }
        }
        if let Some(dir) = self.workdir.as_deref() {
            if dir.trim().is_empty() {
                return Err(SpecError::BlankWorkdir);
            }
        }
        Ok(())
    }
}

/// Everything a spec can fail validation on — typed, and error strings
/// lead with a stable code so they stay bilingual-safe (EXPERIENCE.md).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SpecError {
    #[error("invalid_spec: cmd must not be empty — a job names its executable (AD-6)")]
    EmptyCmd,
    #[error("freeform_shell: cmd `{cmd}` carries shell syntax (`{ch}`) — the runtime never constructs shell strings; pass arguments in args (AD-6)")]
    FreeformShell { cmd: String, ch: char },
    #[error("invalid_spec: env key `{0}` — keys are names: never empty, never `=`")]
    InvalidEnvKey(String),
    #[error("invalid_host: {0}")]
    InvalidHost(String),
    #[error("invalid_spec: resources must be positive — cpus and memoryMb are 1 or more")]
    InvalidResources,
    #[error("invalid_spec: workdir must not be blank when present")]
    BlankWorkdir,
}

// ---------------------------------------------------------------------------
// Payloads + typed constructors (AD-15)
// ---------------------------------------------------------------------------

/// The `job.submitted` payload: which mission, which named target, the
/// adapter's handle (the target's own id for the running process — monitor
/// and fetch address it), and the full validated spec. Snake_case keys —
/// the log's payload convention (`mission_id`, like the spend events).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobSubmittedPayload {
    pub mission_id: Uuid,
    pub target: String,
    pub handle: String,
    pub spec: JobSpec,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `job.submitted` event comes
    /// into being — and the validation gate before submit (AD-6): a spec
    /// that fails `JobSpec::validate` never enters the log. Actor is the
    /// user (the submit shell command); agent enqueuing arrives through the
    /// same constructor.
    pub fn job_submitted(payload: JobSubmittedPayload) -> Result<Self, EventError> {
        payload.spec.validate().map_err(|e| EventError::Invalid(e.to_string()))?;
        if payload.target.trim().is_empty() {
            return Err(EventError::Invalid(
                "job.submitted requires a target — a job names the compute target it runs on (AD-6)"
                    .into(),
            ));
        }
        if payload.handle.trim().is_empty() {
            return Err(EventError::Invalid(
                "job.submitted requires a handle — monitor and fetch address the target's process"
                    .into(),
            ));
        }
        Ok(Self::new(
            JOB_SUBMITTED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?
        .with_causes(vec![payload.mission_id]))
    }

    /// Typed constructor (AD-15): the one way a submitted job is first
    /// observed running on its target. Actor is `system (runtime)`.
    pub fn job_running(payload: JobLifecyclePayload) -> Result<Self, EventError> {
        let value = serde_json::to_value(&payload)?;
        Self::lifecycle(JOB_RUNNING, payload, value)
    }

    /// Typed constructor (AD-15): the one way a job ends well — exit code
    /// 0. Actor is `system (runtime)`; the event's `ts` is the terminal
    /// stamp (AD-12 style).
    pub fn job_finished(payload: JobLifecyclePayload) -> Result<Self, EventError> {
        let value = serde_json::to_value(&payload)?;
        Self::lifecycle(JOB_FINISHED, payload, value)
    }

    /// Typed constructor (AD-15): the one way a job ends badly — spawn
    /// error, nonzero exit, or signal. The reason always names why; the
    /// code rides along when there was one. No job ends silently.
    pub fn job_failed(payload: JobLifecyclePayload) -> Result<Self, EventError> {
        if payload.reason.as_deref().map(str::trim).unwrap_or("").is_empty() {
            return Err(EventError::Invalid(
                "job.failed requires a reason — every terminal state names why it ended (AD-12)"
                    .into(),
            ));
        }
        let value = serde_json::to_value(&payload)?;
        Self::lifecycle(JOB_FAILED, payload, value)
    }

    fn lifecycle(
        kind: &str,
        payload: JobLifecyclePayload,
        value: serde_json::Value,
    ) -> Result<Self, EventError> {
        if payload.job_id == Uuid::nil() {
            return Err(EventError::Invalid(
                "job events require a job_id — the lifecycle addresses the job.submitted event"
                    .into(),
            ));
        }
        if payload.target.trim().is_empty() {
            return Err(EventError::Invalid(
                "job events require a target — the lifecycle names the compute target (AD-6)"
                    .into(),
            ));
        }
        Ok(Self::new(kind, Actor::System { component: SystemComponent::Runtime }, value)?
            .with_causes(vec![payload.mission_id, payload.job_id]))
    }

    /// Typed constructor (AD-15): the one way a named compute target is
    /// declared — `{ name, kind, host? }`, actor=user. The kind must name a
    /// registered adapter (checked by the shell command against the
    /// registry before this runs); an `ssh` target requires its host, and
    /// the host is validated as the single argv token it becomes.
    pub fn target_declared(payload: TargetDeclaredPayload) -> Result<Self, EventError> {
        if !valid_target_name(&payload.name) {
            return Err(EventError::Invalid(
                "target.declared requires a name of lowercase letters, digits and dashes — e.g. `laptop`, `cluster-1`"
                    .into(),
            ));
        }
        if payload.kind.trim().is_empty() {
            return Err(EventError::Invalid(
                "target.declared requires a kind — the adapter that runs its jobs (v1: local | ssh)"
                    .into(),
            ));
        }
        if let Some(host) = payload.host.as_deref() {
            if let Err(reason) = validate_host(host) {
                return Err(EventError::Invalid(format!(
                    "invalid_host: {reason} — a host is one token (it becomes one argv element of ssh): `gpu-01.lab`, `user@10.0.0.4`"
                )));
            }
        }
        if payload.kind == "ssh" && payload.host.is_none() {
            return Err(EventError::Invalid(
                "target.declared requires a host for kind `ssh` — the target names the machine it connects to"
                    .into(),
            ));
        }
        Self::new(TARGET_DECLARED, Actor::User, serde_json::to_value(&payload)?)
    }

    /// Typed constructor (AD-15): the one way the host allowlist changes —
    /// the FULL list, latest event wins (Story 3.3). Every entry is one
    /// token (hosts become argv elements of ssh); blank or malformed
    /// entries never enter the log.
    pub fn host_allowlist_edited(payload: HostAllowlistEditedPayload) -> Result<Self, EventError> {
        for host in &payload.hosts {
            if let Err(reason) = validate_host(host) {
                return Err(EventError::Invalid(format!(
                    "invalid_host: `{host}` — {reason}; the allowlist holds one-token hosts (e.g. `gpu-01.lab`, `user@10.0.0.4`)"
                )));
            }
        }
        Self::new(
            HOST_ALLOWLIST_EDITED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )
    }
}

/// A host is one token: never blank, never whitespace inside, never shell
/// syntax, never option-leading — it is passed to ssh as a single argv
/// element and must stay one remotely too. Shared with the SSH adapter,
/// which re-runs it at its boundary (defense in depth).
pub(crate) fn validate_host(host: &str) -> Result<(), String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("the host is blank".into());
    }
    if host.chars().any(char::is_whitespace) {
        return Err("the host carries whitespace".into());
    }
    if host.starts_with('-') {
        return Err("the host starts with a dash (it would read as an ssh option)".into());
    }
    if let Some(ch) = host.chars().find(|c| SHELL_METACHARS.contains(c)) {
        return Err(format!("the host carries shell syntax (`{ch}`)"));
    }
    Ok(())
}

/// A `job.running` / `job.finished` / `job.failed` payload: the job (its
/// `job.submitted` event id), the mission, the target — plus the terminal
/// facts (`code`, `reason`) the finished/failed kinds carry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobLifecyclePayload {
    pub mission_id: Uuid,
    pub job_id: Uuid,
    pub target: String,
    /// The exit code: present on `job.finished` (0) and on `job.failed`
    /// when the process exited; absent on spawn errors and signals.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i64>,
    /// Why the job ended (required on `job.failed`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The `target.declared` payload: a named compute target of an adapter
/// kind (`local` | `ssh`), with the host an `ssh` target connects to (a
/// single token — validated like the argv element it becomes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetDeclaredPayload {
    pub name: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

/// The `host_allowlist.edited` payload (Story 3.3): the FULL allowlist —
/// every host an SSH target may connect to. Latest event wins; hosts not
/// on it are refused before any connection is attempted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostAllowlistEditedPayload {
    pub hosts: Vec<String>,
}

/// Target names are slugs: lowercase letters, digits, interior dashes —
/// never a leading/trailing dash (they land in mono chips and commands).
fn valid_target_name(name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() || name.starts_with('-') || name.ends_with('-') {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// ---------------------------------------------------------------------------
// The folds (AD-1: current state is exclusively a projection)
// ---------------------------------------------------------------------------

/// A job's lifecycle phase, folded from the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPhase {
    Queued,
    Running,
    Finished,
    Failed,
}

/// A job as read from the log — the read model the mission card's jobs
/// area renders. Terminal states always carry a timestamp (`running_ts` /
/// `finished_ts`) and, on failure, a reason (AD-12 style).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    /// The `job.submitted` event's id — the job's identity.
    pub id: Uuid,
    pub seq: i64,
    /// When the job was submitted.
    pub ts: DateTime<Utc>,
    pub mission_id: Uuid,
    /// The named compute target the job runs on.
    pub target: String,
    /// The target adapter's handle for the live process (monitor/fetch).
    pub handle: String,
    pub spec: JobSpec,
    pub phase: JobPhase,
    pub exit_code: Option<i64>,
    pub reason: Option<String>,
    pub running_ts: Option<DateTime<Utc>>,
    pub finished_ts: Option<DateTime<Utc>>,
}

/// A declared compute target as read from the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredTarget {
    pub name: String,
    pub kind: String,
    /// The host an `ssh` target connects to (`None` for `local`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub seq: i64,
    pub ts: DateTime<Utc>,
}

pub struct JobsProjection;

impl JobsProjection {
    /// Pure fold of the log into job read models, `seq` order. A corrupt
    /// payload fails loudly rather than being skipped. Rollback-aware
    /// through the shared fold cursor (Story 2.6): orphaned lifecycle
    /// events leave the fold — the checkpoint read model lists them.
    pub fn fold(events: &[StoredEvent]) -> Result<Vec<Job>, EventError> {
        let cursor = crate::domain::checkpoints::FoldCursor::over(events);
        let events = &cursor.live_owned(events);
        let mut jobs: Vec<Job> = Vec::new();
        let mut index: std::collections::HashMap<Uuid, usize> = std::collections::HashMap::new();
        for event in events {
            match event.kind.as_str() {
                JOB_SUBMITTED => {
                    let payload: JobSubmittedPayload =
                        serde_json::from_value(event.payload.clone())?;
                    let job = Job {
                        id: event.id,
                        seq: event.seq,
                        ts: event.ts,
                        mission_id: payload.mission_id,
                        target: payload.target,
                        handle: payload.handle,
                        spec: payload.spec,
                        phase: JobPhase::Queued,
                        exit_code: None,
                        reason: None,
                        running_ts: None,
                        finished_ts: None,
                    };
                    index.insert(job.id, jobs.len());
                    jobs.push(job);
                }
                JOB_RUNNING | JOB_FINISHED | JOB_FAILED => {
                    let payload: JobLifecyclePayload =
                        serde_json::from_value(event.payload.clone())?;
                    let Some(&i) = index.get(&payload.job_id) else {
                        continue; // a lifecycle event with no submitted event is not a job
                    };
                    let job = &mut jobs[i];
                    match event.kind.as_str() {
                        JOB_RUNNING => {
                            job.phase = JobPhase::Running;
                            job.running_ts = Some(event.ts);
                        }
                        JOB_FINISHED => {
                            job.phase = JobPhase::Finished;
                            job.exit_code = payload.code;
                            job.finished_ts = Some(event.ts);
                        }
                        _ => {
                            job.phase = JobPhase::Failed;
                            job.exit_code = payload.code;
                            job.reason = payload.reason;
                            job.finished_ts = Some(event.ts);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(jobs)
    }

    /// The jobs of one mission, oldest first.
    pub fn fold_for(events: &[StoredEvent], mission_id: Uuid) -> Result<Vec<Job>, EventError> {
        Ok(Self::fold(events)?
            .into_iter()
            .filter(|j| j.mission_id == mission_id)
            .collect())
    }
}

/// Pure fold of declared compute targets: one per name, the latest
/// `target.declared` wins. The built-in `local` needs no declaration —
/// the shell command layers it onto this list.
pub fn fold_declared_targets(events: &[StoredEvent]) -> Vec<DeclaredTarget> {
    let cursor = crate::domain::checkpoints::FoldCursor::over(events);
    let events = &cursor.live_owned(events);
    let mut by_name: BTreeMap<String, DeclaredTarget> = BTreeMap::new();
    for event in events {
        if event.kind != TARGET_DECLARED {
            continue;
        }
        let Ok(payload) = serde_json::from_value::<TargetDeclaredPayload>(event.payload.clone())
        else {
            continue; // a corrupt declaration never breaks the fold
        };
        by_name.insert(
            payload.name.clone(),
            DeclaredTarget {
                name: payload.name,
                kind: payload.kind,
                host: payload.host,
                seq: event.seq,
                ts: event.ts,
            },
        );
    }
    by_name.into_values().collect()
}

/// Pure fold of the host allowlist (Story 3.3): the LATEST
/// `host_allowlist.edited` event's full list wins — the allowlist is a
/// single setting, not an accumulation. Rollback-aware through the shared
/// fold cursor (Story 2.6).
pub fn fold_host_allowlist(events: &[StoredEvent]) -> Vec<String> {
    let cursor = crate::domain::checkpoints::FoldCursor::over(events);
    let events = &cursor.live_owned(events);
    let mut allowlist: Option<Vec<String>> = None;
    for event in events {
        if event.kind != HOST_ALLOWLIST_EDITED {
            continue;
        }
        let Ok(payload) =
            serde_json::from_value::<HostAllowlistEditedPayload>(event.payload.clone())
        else {
            continue; // a corrupt edit never breaks the fold
        };
        allowlist = Some(payload.hosts);
    }
    allowlist.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventstore::EventStore;
    use rusqlite::Connection;
    use serde_json::json;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    fn spec(cmd: &str) -> JobSpec {
        JobSpec {
            cmd: cmd.into(),
            args: vec!["train.py".into()],
            env: BTreeMap::from([("EPOCHS".into(), "10".into())]),
            resources: Some(JobResources { cpus: Some(4), memory_mb: None }),
            workdir: Some("/tmp/exp".into()),
        }
    }

    // ---- JobSpec::validate — the never-freeform-shell matrix (AD-6) ----

    #[test]
    fn validate_rejects_every_shell_metacharacter_in_cmd() {
        // the story's canonical freeform construction, plus the rest of the set
        for cmd in [
            "ls | rm -rf .", // the unit test's exact spec
            "ls; rm -rf .",
            "ls && rm -rf .",
            "echo `whoami`",
            "echo $(whoami)",
            "cat x > y",
            "cat x < y",
            "echo a\necho b",
            "echo a\r\necho b",
        ] {
            let err = spec(cmd).validate().expect_err(cmd);
            assert!(
                matches!(err, SpecError::FreeformShell { .. }),
                "{cmd:?} should read as freeform shell, got {err:?}"
            );
            assert!(err.to_string().starts_with("freeform_shell:"), "unexpected: {err}");
        }
    }

    #[test]
    fn validate_accepts_a_typed_spec_and_never_polices_args() {
        // a plain executable with data args — metacharacters in ARGS are
        // data (argv is never interpreted), only cmd is a command
        let mut s = spec("python3");
        s.args = vec!["-c".into(), "print('a | b && c')".into()];
        assert_eq!(s.validate(), Ok(()));
        // an executable path with spaces is a path, not a shell string
        assert_eq!(spec("/Applications/My App/tool").validate(), Ok(()));
    }

    #[test]
    fn validate_rejects_blank_and_malformed_fields() {
        assert_eq!(spec("  ").validate(), Err(SpecError::EmptyCmd));
        assert_eq!(spec("").validate(), Err(SpecError::EmptyCmd));
        let mut s = spec("python3");
        s.env.insert("BAD=KEY".into(), "1".into());
        assert_eq!(s.validate(), Err(SpecError::InvalidEnvKey("BAD=KEY".into())));
        let mut s = spec("python3");
        s.resources = Some(JobResources { cpus: Some(0), memory_mb: None });
        assert_eq!(s.validate(), Err(SpecError::InvalidResources));
        let mut s = spec("python3");
        s.workdir = Some("   ".into());
        assert_eq!(s.validate(), Err(SpecError::BlankWorkdir));
    }

    // ---- typed constructors ----

    #[test]
    fn job_submitted_is_the_validation_gate_before_submit() {
        let payload = JobSubmittedPayload {
            mission_id: Uuid::new_v4(),
            target: "local".into(),
            handle: "h-1".into(),
            spec: spec("python3"),
        };
        let ev = NewEvent::job_submitted(payload.clone()).unwrap();
        assert_eq!(ev.kind, JOB_SUBMITTED);
        assert_eq!(ev.actor, Actor::User);
        assert_eq!(ev.causes.len(), 1, "the submission cause-links its mission");
        // the story's freeform spec never enters the log
        let mut bad = payload.clone();
        bad.spec = spec("ls | rm -rf .");
        let err = NewEvent::job_submitted(bad).unwrap_err();
        assert!(err.to_string().contains("freeform_shell:"), "unexpected: {err}");
        // blank target / handle are refused
        let mut p = payload;
        p.target = " ".into();
        assert!(NewEvent::job_submitted(p).unwrap_err().to_string().contains("target"));
    }

    #[test]
    fn lifecycle_constructors_stamp_the_runtime_actor_and_reasoned_failure() {
        let p = JobLifecyclePayload {
            mission_id: Uuid::new_v4(),
            job_id: Uuid::new_v4(),
            target: "local".into(),
            code: Some(1),
            reason: Some("nonzero_exit".into()),
        };
        for (ev, kind) in [
            (NewEvent::job_running(p.clone()).unwrap(), JOB_RUNNING),
            (NewEvent::job_finished(JobLifecyclePayload {
                reason: None,
                code: Some(0),
                ..p.clone()
            })
            .unwrap(), JOB_FINISHED),
            (NewEvent::job_failed(p.clone()).unwrap(), JOB_FAILED),
        ] {
            assert_eq!(ev.kind, kind);
            assert_eq!(
                ev.actor,
                Actor::System { component: SystemComponent::Runtime }
            );
            assert_eq!(ev.causes.len(), 2, "cause-links mission + job");
        }
        // a failed job without a reason never lands — no job ends silently
        let err = NewEvent::job_failed(JobLifecyclePayload {
            reason: None,
            code: None,
            ..p
        })
        .unwrap_err();
        assert!(err.to_string().contains("reason"), "unexpected: {err}");
    }

    #[test]
    fn target_declared_validates_the_slug_and_kind() {
        let ev = NewEvent::target_declared(TargetDeclaredPayload {
            name: "cluster-1".into(),
            kind: "ssh".into(),
            host: Some("gpu-01.lab".into()),
        })
        .unwrap();
        assert_eq!(ev.kind, TARGET_DECLARED);
        assert_eq!(ev.actor, Actor::User);
        assert_eq!(ev.payload["host"], json!("gpu-01.lab"));
        for bad in ["", " ", "-lead", "trail-", "Upper", "sp ace", "under_score"] {
            let err = NewEvent::target_declared(TargetDeclaredPayload {
                name: bad.into(),
                kind: "local".into(),
                host: None,
            })
            .unwrap_err();
            assert!(err.to_string().contains("name"), "{bad:?}: {err}");
        }
    }

    #[test]
    fn an_ssh_target_requires_one_valid_host() {
        // ssh without a host never lands
        let err = NewEvent::target_declared(TargetDeclaredPayload {
            name: "cluster-1".into(),
            kind: "ssh".into(),
            host: None,
        })
        .unwrap_err();
        assert!(err.to_string().contains("host"), "unexpected: {err}");
        // local targets carry no host (and may not: one is refused)
        NewEvent::target_declared(TargetDeclaredPayload {
            name: "laptop".into(),
            kind: "local".into(),
            host: None,
        })
        .unwrap();
        // the host is one token: whitespace, shell syntax, option-leading
        // dashes and blank hosts never land — it becomes one argv element
        for bad in ["", "  ", "gpu 01", "-oProxyCommand=evil", "a;b", "a|b", "a$b"] {
            let err = NewEvent::target_declared(TargetDeclaredPayload {
                name: "cluster-1".into(),
                kind: "ssh".into(),
                host: Some(bad.into()),
            })
            .unwrap_err();
            assert!(err.to_string().contains("invalid_host:"), "{bad:?}: {err}");
        }
        // user@host is a host (ssh's own syntax, one token, no metachars)
        NewEvent::target_declared(TargetDeclaredPayload {
            name: "cluster-1".into(),
            kind: "ssh".into(),
            host: Some("ricardo@gpu-01.lab".into()),
        })
        .unwrap();
    }

    // ---- the host allowlist (Story 3.3) ----

    #[test]
    fn the_allowlist_edits_validate_and_fold_latest_wins() {
        let conn = conn();
        let store = EventStore::new(&conn);
        // malformed entries never land
        let err = NewEvent::host_allowlist_edited(HostAllowlistEditedPayload {
            hosts: vec!["gpu-01.lab".into(), "bad host".into()],
        })
        .unwrap_err();
        assert!(err.to_string().contains("invalid_host:"), "unexpected: {err}");
        // a full-list edit lands, then a second replaces it entirely
        store
            .append(
                NewEvent::host_allowlist_edited(HostAllowlistEditedPayload {
                    hosts: vec!["gpu-01.lab".into(), "10.0.0.4".into()],
                })
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            fold_host_allowlist(&store.events_all().unwrap()),
            vec!["gpu-01.lab".to_string(), "10.0.0.4".to_string()]
        );
        store
            .append(
                NewEvent::host_allowlist_edited(HostAllowlistEditedPayload {
                    hosts: vec!["cluster.hpc.edu".into()],
                })
                .unwrap(),
            )
            .unwrap();
        // latest wins — the allowlist is a setting, not an accumulation
        assert_eq!(
            fold_host_allowlist(&store.events_all().unwrap()),
            vec!["cluster.hpc.edu".to_string()]
        );
        // and an empty list clears it
        store
            .append(NewEvent::host_allowlist_edited(HostAllowlistEditedPayload { hosts: vec![] }).unwrap())
            .unwrap();
        assert!(fold_host_allowlist(&store.events_all().unwrap()).is_empty());
    }

    // ---- the fold ----

    #[test]
    fn the_lifecycle_folds_queued_running_terminal() {
        let conn = conn();
        let store = EventStore::new(&conn);
        let mission = Uuid::new_v4();
        let submitted = store
            .append(
                NewEvent::job_submitted(JobSubmittedPayload {
                    mission_id: mission,
                    target: "local".into(),
                    handle: "h-1".into(),
                    spec: spec("python3"),
                })
                .unwrap(),
            )
            .unwrap();
        let jobs = JobsProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].phase, JobPhase::Queued);
        assert_eq!(jobs[0].target, "local");
        assert_eq!(jobs[0].spec.cmd, "python3");

        store
            .append(
                NewEvent::job_running(JobLifecyclePayload {
                    mission_id: mission,
                    job_id: submitted.id,
                    target: "local".into(),
                    code: None,
                    reason: None,
                })
                .unwrap(),
            )
            .unwrap();
        let finished = store
            .append(
                NewEvent::job_finished(JobLifecyclePayload {
                    mission_id: mission,
                    job_id: submitted.id,
                    target: "local".into(),
                    code: Some(0),
                    reason: None,
                })
                .unwrap(),
            )
            .unwrap();
        let jobs = JobsProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(jobs[0].phase, JobPhase::Finished);
        assert_eq!(jobs[0].exit_code, Some(0));
        assert_eq!(jobs[0].finished_ts, Some(finished.ts), "terminal is always stamped");
        assert!(jobs[0].running_ts.is_some(), "the running observation is stamped too");

        // fold_for scopes to the mission
        assert_eq!(JobsProjection::fold_for(&store.events_all().unwrap(), mission).unwrap().len(), 1);
        assert_eq!(
            JobsProjection::fold_for(&store.events_all().unwrap(), Uuid::new_v4())
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn a_failed_job_folds_with_reason_code_and_stamp() {
        let conn = conn();
        let store = EventStore::new(&conn);
        let mission = Uuid::new_v4();
        let submitted = store
            .append(
                NewEvent::job_submitted(JobSubmittedPayload {
                    mission_id: mission,
                    target: "local".into(),
                    handle: "h-2".into(),
                    spec: spec("false"),
                })
                .unwrap(),
            )
            .unwrap();
        let failed = store
            .append(
                NewEvent::job_failed(JobLifecyclePayload {
                    mission_id: mission,
                    job_id: submitted.id,
                    target: "local".into(),
                    code: Some(1),
                    reason: Some("exit_code_1".into()),
                })
                .unwrap(),
            )
            .unwrap();
        let jobs = JobsProjection::fold(&store.events_all().unwrap()).unwrap();
        assert_eq!(jobs[0].phase, JobPhase::Failed);
        assert_eq!(jobs[0].reason.as_deref(), Some("exit_code_1"));
        assert_eq!(jobs[0].exit_code, Some(1));
        assert_eq!(jobs[0].finished_ts, Some(failed.ts), "the failure is stamped");
        // the payload shape on the wire
        assert_eq!(failed.payload["reason"], json!("exit_code_1"));
        assert_eq!(failed.payload["mission_id"], json!(mission.to_string()));
    }

    #[test]
    fn declared_targets_fold_latest_per_name() {
        let conn = conn();
        let store = EventStore::new(&conn);
        for (name, kind) in [("laptop", "local"), ("cluster-1", "ssh"), ("laptop", "local")] {
            store
                .append(
                    NewEvent::target_declared(TargetDeclaredPayload {
                        name: name.into(),
                        kind: kind.into(),
                        host: if kind == "ssh" { Some("gpu-01.lab".into()) } else { None },
                    })
                    .unwrap(),
                )
                .unwrap();
        }
        let targets = fold_declared_targets(&store.events_all().unwrap());
        assert_eq!(targets.len(), 2, "one per name, latest wins");
        assert!(targets.iter().any(|t| t.name == "laptop" && t.kind == "local" && t.host.is_none()));
        assert!(targets.iter().any(|t| t.name == "cluster-1" && t.kind == "ssh" && t.host.as_deref() == Some("gpu-01.lab")));
    }
}
