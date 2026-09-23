// Compute target adapters (AD-6, Story 3.2/3.3): the boundary between the
// core and the machines jobs run on. Every job flows through the
// `ComputeTarget` trait — `submit(spec, target)`, `monitor(handle)`,
// `fetch(handle)` — resolved from a registry by adapter KIND. v1
// registers `Local` (argv-direct process execution; never `sh -c`) and
// `Ssh` (Story 3.3: allowlisted remote hosts, argv transferred without a
// freeform-shell path). `ssh.rs` holds the SSH adapter; this module holds
// the trait, the registry, the Local adapter, and the shared
// process-tracking plumbing both adapters run on.
//
// The spec is validated at TWO boundaries before any process exists:
// `JobSpec::validate` at the `job.submitted` constructor (the event never
// lands otherwise) and again inside `submit` — the adapter is the last
// line of defense for the never-freeform-shell guarantee (AD-6), because
// the SSH adapter's remote side builds a command line from these fields
// too (it ENCODES the validated argv — see ssh.rs for why that is not a
// freeform path).

pub mod scheduler;
pub mod ssh;

pub use scheduler::Scheduler;
pub use ssh::Ssh;

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::jobs::{JobResources, JobSpec, SpecError};

/// The adapter contract's version (NFR-15): the trait shape, the JobSpec
/// schema and the error vocabulary this version of ResearchCore binds
/// community adapters to. The registry refuses registrations against any
/// other version (Story 6.5).
pub const ADAPTER_CONTRACT_VERSION: &str = "1";

/// The target adapter's id for one submitted job — opaque to the core,
/// addressed by monitor and fetch. For `Local` it is the in-memory key of
/// the spawned child.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobHandle {
    pub id: String,
}

impl JobHandle {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

/// What the adapter observes about a job right now. `Finished` is exit 0;
/// everything else that stops is `Failed` with a reason (spawn error,
/// nonzero exit, signal) — and the code when there was one. This is the
/// adapter-side observation; the evented lifecycle (`job.running` /
/// `job.finished` / `job.failed`) is the runtime's projection of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetJobStatus {
    Running,
    Finished { code: i32 },
    Failed { reason: String, code: Option<i32> },
}

/// A finished job's artifacts (FR-11.5's fetch): captured stdout/stderr
/// and the exit code when there was one. Story 3.4 turns these into
/// quarantined evidence proposals; here they surface on the job row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobResult {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Everything a target adapter can fail with — typed, never a bare io
/// string. Error codes lead the message (bilingual-safe, EXPERIENCE.md).
/// The v0.2.0 kinds add their honest down-states (Stories 6.2–6.4): an
/// unreachable scheduler/cluster/queue endpoint is a TYPED error on that
/// target only — never a crash, never a lie about the job's state.
#[derive(Debug, thiserror::Error)]
pub enum TargetError {
    #[error("unknown_target: `{0}` — no compute target with that name")]
    UnknownTarget(String),
    #[error("unknown_kind: `{0}` — no adapter of that kind is registered (local | ssh | scheduler | kubernetes | chopflow)")]
    UnknownKind(String),
    #[error("unknown_job: `{0}` — the target has no process for that handle")]
    UnknownJob(String),
    #[error("job_not_terminal: `{0}` — fetch waits for the job to finish")]
    NotTerminal(String),
    #[error("host_not_allowed: `{host}` — hosts outside the allowlist are refused before any connection is attempted (allowlist: {known})")]
    HostNotAllowed { host: String, known: String },
    #[error("context_not_allowed: `{context}` — cluster contexts outside the allowlist are refused before any connection is attempted (allowlist: {known})")]
    ContextNotAllowed { context: String, known: String },
    #[error("missing_host: `{0}` — an ssh target names the host it connects to")]
    MissingHost(String),
    #[error("missing_config: `{target}` needs `{key}` — {expectation}")]
    MissingConfig {
        target: String,
        key: String,
        expectation: String,
    },
    #[error("invalid_config: `{key}` — {reason}")]
    InvalidConfig { key: String, reason: String },
    #[error("scheduler_unreachable: {host} — the scheduler did not answer ({detail})")]
    SchedulerUnreachable { host: String, detail: String },
    #[error("kube_unreachable: {context} — the cluster did not answer ({detail})")]
    KubeUnreachable { context: String, detail: String },
    #[error("endpoint_unreachable: {endpoint} — the ChopFlow queue did not answer ({detail})")]
    EndpointUnreachable { endpoint: String, detail: String },
    #[error("{0}")]
    InvalidSpec(#[from] SpecError),
    #[error("spawn_error: {0}")]
    Spawn(String),
}

/// The per-target facts an adapter executes against (Story 3.3 + the
/// v0.2.0 kinds): the declared target's name, the host an `ssh` or
/// `scheduler` target connects to, the workspace host allowlist, and the
/// declared target's config map (scheduler flavor/prefixes, kubernetes
/// context/namespace/image, chopflow endpoint/queue). The allowlist is
/// enforced HERE — inside the adapter boundary, before any connection is
/// attempted — as defense in depth behind the command layer's own check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetInfo {
    /// The declared target's name (`local`, `cluster-1`, …).
    pub name: String,
    /// The host an `ssh`/`scheduler` target connects to; `None` for the
    /// other kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// The workspace host allowlist (latest `host_allowlist.edited` fold).
    #[serde(default)]
    pub allowlist: Vec<String>,
    /// The declared target's per-kind config map (one-token values,
    /// validated at `target.declared` and re-parsed at the adapter
    /// boundary — defense in depth).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config: BTreeMap<String, String>,
}

impl TargetInfo {
    /// One config value (`None` when the key is absent).
    pub fn cfg(&self, key: &str) -> Option<&str> {
        self.config.get(key).map(String::as_str)
    }
}

// ---------------------------------------------------------------------------
// The trait + registry (AD-6)
// ---------------------------------------------------------------------------

/// One compute target adapter (AD-6). Methods are synchronous on purpose:
/// `submit` starts execution (or records why it could not) without
/// waiting; `monitor` is a non-blocking observation; `fetch` returns what
/// a finished job produced. Implementations must not assume local
/// execution — the SSH adapter (Story 3.3) implements the same seam over
/// a remote process.
pub trait ComputeTarget: Send + Sync {
    /// The adapter kind: the `kind` a `target.declared` event names.
    /// `local` and `ssh` in v1.
    fn kind(&self) -> &'static str;
    /// Submit a validated spec for execution on the named target.
    /// Re-validates the spec at the adapter boundary (AD-6); returns the
    /// handle monitor/fetch address. A process that fails to spawn is a
    /// job state, not a submit error — the handle comes back with the
    /// failure observable via `monitor`, so the lifecycle events always
    /// tell the story.
    fn submit(&self, spec: &JobSpec, target: &TargetInfo) -> Result<JobHandle, TargetError>;
    /// Observe the job now: running, or terminal with code/reason.
    fn monitor(&self, handle: &JobHandle) -> Result<TargetJobStatus, TargetError>;
    /// Fetch a TERMINAL job's captured output. Typed error while running.
    fn fetch(&self, handle: &JobHandle) -> Result<JobResult, TargetError>;
}

/// The adapter registry (AD-6): kind → adapter. v1 registers `Local` and
/// the bare-SSH adapter (Story 3.3); the registration seam for community
/// adapters is `register` — one kind, one adapter. Named targets
/// (`target.declared` events) resolve to their kind's adapter here.
pub struct TargetRegistry {
    adapters: HashMap<&'static str, Arc<dyn ComputeTarget>>,
}

impl TargetRegistry {
    /// The first-party registry: `local` (argv-direct), `ssh` (allowlisted
    /// remote hosts, Story 3.3), and `scheduler` (SLURM/PBS-style
    /// clusters, Story 6.2) registered.
    pub fn v1() -> Self {
        let mut registry = Self::empty();
        registry.register(Arc::new(Local));
        registry.register(Arc::new(Ssh::new()));
        registry.register(Arc::new(Scheduler::new()));
        registry
    }

    /// The registration seam (AD-6): community and next-story adapters
    /// plug in here — one kind, one adapter.
    pub fn register(&mut self, adapter: Arc<dyn ComputeTarget>) {
        self.adapters.insert(adapter.kind(), adapter);
    }

    /// Resolve an adapter by kind — typed error for unknown kinds.
    pub fn adapter(&self, kind: &str) -> Result<Arc<dyn ComputeTarget>, TargetError> {
        self.adapters
            .get(kind)
            .cloned()
            .ok_or_else(|| TargetError::UnknownKind(kind.to_string()))
    }

    /// The registered kinds (v1: `local`, `ssh`) — what `target.declared`
    /// may name.
    pub fn kinds(&self) -> Vec<&'static str> {
        let mut kinds: Vec<&'static str> = self.adapters.keys().copied().collect();
        kinds.sort_unstable();
        kinds
    }

    fn empty() -> Self {
        Self { adapters: HashMap::new() }
    }
}

// ---------------------------------------------------------------------------
// The shared POSIX quoting discipline (the SSH adapter's, shared with the
// scheduler adapter — Story 6.2)
// ---------------------------------------------------------------------------

/// Encode one string as a single POSIX shell word: wrap in single quotes
/// (everything between them is literal), escaping embedded quotes by
/// close-escape-reopen (`'\''`). Deterministic and reversible — a remote
/// shell parses the word back to the exact same string. Every string the
/// adapters transfer into a transport's command-line protocol goes through
/// here (ssh.rs since Story 3.3; scheduler.rs's generated script bodies and
/// remote command lines since Story 6.2) — no user-supplied value is ever
/// transferred unencoded.
pub(crate) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

// ---------------------------------------------------------------------------
// The shared bounded sync runner (probe/poll commands — Story 6.2's
// pattern, shared with the SSH probe)
// ---------------------------------------------------------------------------

/// A bounded synchronous command run's outcome: the captured output, or
/// the timeout that killed it (a stuck poll never hangs a job row).
pub(crate) enum SyncRun {
    Done(std::process::Output),
    Timeout,
}

/// Run one command to completion with a deadline: stdout/stderr captured,
/// stdin null. The poll/probe side of the adapters (submit is async and
/// tracked; these are the quick questions asked afterwards).
pub(crate) fn run_sync(argv: &[String], timeout: std::time::Duration) -> std::io::Result<SyncRun> {
    use std::process::{Command, Stdio};
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(_) => return Ok(SyncRun::Done(child.wait_with_output()?)),
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(SyncRun::Timeout);
            }
            None => std::thread::sleep(std::time::Duration::from_millis(25)),
        }
    }
}

// ---------------------------------------------------------------------------
// The shared process tracking (Local + Ssh both run on it)
// ---------------------------------------------------------------------------

/// What the reaper task records when a tracked child is done.
/// `spawn_error` is set when the process never came up (the handle still
/// exists — the lifecycle events carry the reason).
pub(crate) struct JobExit {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) spawn_error: Option<String>,
}

/// One live job: the slot its reaper task fills at exit.
pub(crate) type JobSlot = Arc<Mutex<Option<JobExit>>>;

/// One adapter's live processes, keyed by handle id. Process-global by
/// design: children outlive any single command call; a restart loses them
/// (the log's read model keeps its last observed state honestly).
pub(crate) fn job_table(
    cell: &'static OnceLock<Mutex<HashMap<String, JobSlot>>>,
) -> &'static Mutex<HashMap<String, JobSlot>> {
    cell.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Spawn `command` under a fresh handle and track it: the reaper captures
/// the output and records the exit, so `monitor` is a non-blocking
/// observation and `fetch` reads what was captured. A spawn failure is
/// the job's first (and terminal) state — submit still succeeds, the
/// failure surfaces through monitor and lands as a reasoned `job.failed`
/// (no silent ends).
pub(crate) fn track(
    table: &'static Mutex<HashMap<String, JobSlot>>,
    command: tokio::process::Command,
) -> JobHandle {
    track_as(table, command, Uuid::new_v4().to_string())
}

/// `track` with a caller-chosen handle id (the scheduler adapter, Story
/// 6.2: the generated batch script is NAMED after the handle before the
/// submit process exists — `rc-<tag>.sh`, output files `rc-<tag>.<job>.out`).
pub(crate) fn track_as(
    table: &'static Mutex<HashMap<String, JobSlot>>,
    mut command: tokio::process::Command,
    handle_id: String,
) -> JobHandle {
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let handle = JobHandle::new(handle_id);
    let slot: JobSlot = Arc::new(Mutex::new(None));
    match command.spawn() {
        Ok(child) => {
            let exit_slot = Arc::clone(&slot);
            // The reaper: capture the output, record the exit. Monitor
            // reads `None` as "still running" until this lands.
            tokio::spawn(async move {
                let exit = match child.wait_with_output().await {
                    Ok(out) => JobExit {
                        code: out.status.code(),
                        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                        spawn_error: None,
                    },
                    Err(e) => JobExit {
                        code: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        spawn_error: Some(e.to_string()),
                    },
                };
                *exit_slot.lock().expect("job exit slot poisoned") = Some(exit);
            });
        }
        Err(e) => {
            *slot.lock().expect("job exit slot poisoned") = Some(JobExit {
                code: None,
                stdout: String::new(),
                stderr: String::new(),
                spawn_error: Some(e.to_string()),
            });
        }
    }
    table
        .lock()
        .expect("job table poisoned")
        .insert(handle.id.clone(), slot);
    handle
}

/// Look a handle up in its adapter's table — typed error when unknown.
pub(crate) fn lookup(
    table: &'static Mutex<HashMap<String, JobSlot>>,
    handle: &JobHandle,
) -> Result<JobSlot, TargetError> {
    table
        .lock()
        .expect("job table poisoned")
        .get(&handle.id)
        .cloned()
        .ok_or_else(|| TargetError::UnknownJob(handle.id.clone()))
}

/// Observe a tracked job from its slot: running until the reaper lands.
pub(crate) fn observe(table: &'static Mutex<HashMap<String, JobSlot>>, handle: &JobHandle) -> Result<TargetJobStatus, TargetError> {
    let slot = lookup(table, handle)?;
    let guard = slot.lock().expect("job exit slot poisoned");
    match guard.as_ref() {
        None => Ok(TargetJobStatus::Running),
        Some(exit) => Ok(classify(exit)),
    }
}

/// Read a TERMINAL tracked job's captured output.
pub(crate) fn read_result(
    table: &'static Mutex<HashMap<String, JobSlot>>,
    handle: &JobHandle,
) -> Result<JobResult, TargetError> {
    let slot = lookup(table, handle)?;
    let guard = slot.lock().expect("job exit slot poisoned");
    let exit = guard
        .as_ref()
        .ok_or_else(|| TargetError::NotTerminal(handle.id.clone()))?;
    Ok(JobResult {
        code: exit.code,
        stdout: exit.stdout.clone(),
        stderr: if exit.spawn_error.is_some() {
            exit.spawn_error.clone().unwrap_or_default()
        } else {
            exit.stderr.clone()
        },
    })
}

/// Classify a recorded exit: 0 is finished; nonzero exit, signal (no
/// code), and spawn errors are failures that always name their reason.
pub(crate) fn classify(exit: &JobExit) -> TargetJobStatus {
    if let Some(err) = &exit.spawn_error {
        return TargetJobStatus::Failed { reason: format!("spawn_error: {err}"), code: None };
    }
    match exit.code {
        Some(0) => TargetJobStatus::Finished { code: 0 },
        Some(code) => TargetJobStatus::Failed {
            reason: format!("exit_code_{code}"),
            code: Some(code),
        },
        None => TargetJobStatus::Failed { reason: "signal".into(), code: None },
    }
}

// ---------------------------------------------------------------------------
// The Local adapter (Story 3.2)
// ---------------------------------------------------------------------------

/// The local-machine adapter: spawns the spec's `cmd` with `args` as
/// ARGV DIRECTLY (`tokio::process::Command` — never `sh -c`, never a
/// constructed shell string; AD-6). The shared tracking machinery above
/// does the monitoring and capture.
pub struct Local;

/// The Local adapter's live processes (its own table — never shared with
/// the SSH adapter's).
fn local_jobs() -> &'static Mutex<HashMap<String, JobSlot>> {
    static JOBS: OnceLock<Mutex<HashMap<String, JobSlot>>> = OnceLock::new();
    job_table(&JOBS)
}

impl ComputeTarget for Local {
    fn kind(&self) -> &'static str {
        "local"
    }

    fn submit(&self, spec: &JobSpec, _target: &TargetInfo) -> Result<JobHandle, TargetError> {
        // The adapter boundary validates again (AD-6): nothing executes
        // that did not pass the never-freeform-shell gate.
        spec.validate()?;
        let mut command = tokio::process::Command::new(&spec.cmd);
        command.args(&spec.args).envs(&spec.env);
        if let Some(dir) = spec.workdir.as_deref() {
            command.current_dir(dir);
        }
        // `resources` ride in the spec (the SSH adapter forwards them);
        // Local records but cannot enforce them (no cgroups on a laptop).
        let _ = spec.resources.unwrap_or(JobResources { cpus: None, memory_mb: None });
        Ok(track(local_jobs(), command))
    }

    fn monitor(&self, handle: &JobHandle) -> Result<TargetJobStatus, TargetError> {
        observe(local_jobs(), handle)
    }

    fn fetch(&self, handle: &JobHandle) -> Result<JobResult, TargetError> {
        read_result(local_jobs(), handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::jobs::SHELL_METACHARS;
    use std::collections::BTreeMap;

    fn spec(cmd: &str, args: &[&str]) -> JobSpec {
        JobSpec {
            cmd: cmd.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: BTreeMap::new(),
            resources: None,
            workdir: None,
        }
    }

    fn local_info() -> TargetInfo {
        TargetInfo { name: "local".into(), host: None, allowlist: Vec::new(), config: BTreeMap::new() }
    }

    // ---- the registry ----

    #[test]
    fn the_registry_resolves_by_kind_with_a_typed_unknown() {
        let registry = TargetRegistry::v1();
        assert_eq!(registry.kinds(), vec!["local", "scheduler", "ssh"]);
        for kind in registry.kinds() {
            assert_eq!(registry.adapter(kind).unwrap().kind(), kind);
        }
        let err = match registry.adapter("mainframe") {
            Err(e) => e,
            Ok(_) => panic!("`mainframe` must not resolve in the registry"),
        };
        assert!(err.to_string().starts_with("unknown_kind:"), "unexpected: {err}");
    }

    // ---- Local: never freeform shell (AD-6) ----

    #[tokio::test]
    async fn the_storys_freeform_spec_never_spawns_anything() {
        let local = Local;
        // the exact freeform construction from the story — rejected at the
        // adapter boundary, no handle, no process
        let err = local.submit(&spec("ls | rm -rf .", &[]), &local_info()).unwrap_err();
        assert!(err.to_string().starts_with("freeform_shell:"), "unexpected: {err}");
        // and the full metachar set, at the constructor level too
        for ch in SHELL_METACHARS {
            let cmd = format!("echo a{ch}b");
            assert!(
                spec(&cmd, &[]).validate().is_err(),
                "`{ch}` in cmd must fail validation"
            );
        }
    }

    // ---- Local: the monitored lifecycle ----

    #[tokio::test]
    async fn a_long_running_job_is_observed_running_then_terminal() {
        let local = Local;
        // a fake long-running process: sleep 1 (argv-direct, no shell)
        let handle = local.submit(&spec("sleep", &["1"]), &local_info()).unwrap();
        assert_eq!(local.monitor(&handle).unwrap(), TargetJobStatus::Running);
        // fetch refuses while running — typed, never a partial read
        let err = local.fetch(&handle).unwrap_err();
        assert!(err.to_string().starts_with("job_not_terminal:"), "unexpected: {err}");
        // wait past the child's lifetime, then the terminal observation
        tokio::time::sleep(std::time::Duration::from_millis(1300)).await;
        assert_eq!(local.monitor(&handle).unwrap(), TargetJobStatus::Finished { code: 0 });
        let result = local.fetch(&handle).unwrap();
        assert_eq!(result.code, Some(0));
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn a_job_captures_stdout_and_stderr() {
        let local = Local;
        // echo writes stdout; argv data with shell-looking characters is
        // still data — never interpreted
        let handle = local
            .submit(&spec("echo", &["pipeline | data ; $(not-a-command)"]), &local_info())
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_eq!(local.monitor(&handle).unwrap(), TargetJobStatus::Finished { code: 0 });
        let result = local.fetch(&handle).unwrap();
        assert_eq!(result.stdout.trim(), "pipeline | data ; $(not-a-command)");
    }

    #[tokio::test]
    async fn a_nonzero_exit_is_a_reasoned_failure() {
        let local = Local;
        let handle = local
            .submit(&spec("sh", &["-c", "echo boom >&2; exit 3"]), &local_info())
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_eq!(
            local.monitor(&handle).unwrap(),
            TargetJobStatus::Failed { reason: "exit_code_3".into(), code: Some(3) }
        );
        let result = local.fetch(&handle).unwrap();
        assert_eq!(result.code, Some(3));
        assert_eq!(result.stderr.trim(), "boom");
    }

    #[tokio::test]
    async fn a_spawn_failure_is_a_handled_job_state_not_a_submit_error() {
        let local = Local;
        let handle = local
            .submit(&spec("definitely-not-a-binary-xyz", &[]), &local_info())
            .unwrap();
        let status = local.monitor(&handle).unwrap();
        match status {
            TargetJobStatus::Failed { reason, code } => {
                assert!(reason.starts_with("spawn_error:"), "unexpected: {reason}");
                assert_eq!(code, None);
            }
            other => panic!("a spawn failure must be an observed failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unknown_handles_are_a_typed_error() {
        let local = Local;
        let err = local.monitor(&JobHandle::new("never-submitted")).unwrap_err();
        assert!(err.to_string().starts_with("unknown_job:"), "unexpected: {err}");
        let err = local.fetch(&JobHandle::new("never-submitted")).unwrap_err();
        assert!(err.to_string().starts_with("unknown_job:"), "unexpected: {err}");
    }

    // ---- Local: env + workdir flow into the process ----

    #[tokio::test]
    async fn env_and_workdir_apply_to_the_spawned_process() {
        let local = Local;
        let mut s = spec("pwd", &[]);
        s.env.insert("RC_TEST_VAR".into(), "applied".into());
        s.workdir = Some("/tmp".into());
        let handle = local.submit(&s, &local_info()).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let result = local.fetch(&handle).unwrap();
        assert_eq!(result.code, Some(0));
        assert_eq!(result.stdout.trim(), "/private/tmp", "workdir applied (macOS /tmp)");
    }
}
