// The scheduler compute-target adapter (Story 6.2, FR-22.1, AD-6):
// SLURM/PBS-style clusters as compute targets — the same structured specs,
// the same trust rules, argv-encoded end to end.
//
// SPEC → SCRIPT (the never-freeform guarantee): `submit` GENERATES the
// batch script from the validated spec's fields — `#SBATCH`/`#PBS`
// directives for job name, cpus, memory, workdir and the output files, an
// env-export block, and a body that is exactly `exec 'cmd' 'arg' …` with
// every word POSIX-quoted (`shell_quote`, the SSH adapter's discipline —
// metacharacters in `args` stay data on the cluster exactly as they are
// locally). There is no input that reaches a shell un-encoded: `cmd` is
// metachar-free (JobSpec::validate, re-run at this boundary), `args` and
// env values are inert inside quotes, and no script BODY is ever accepted
// from outside — a freeform spec is refused before anything is written or
// connected.
//
// TRANSPORT: argv directly. On a LOCAL scheduler (tools on this machine —
// the login-node case) the submit binary runs with the generated script's
// temp path as ONE argv element. On a REMOTE scheduler host (the target's
// optional `host`) every command rides the system `ssh` binary — spawned
// argv-direct with the same options as the SSH adapter — carrying an
// ENCODED command line: the script is materialized remotely by
// `printf '%s\n' <quoted lines> > <quoted path> && sbatch <quoted path>`,
// where every operand is `shell_quote`d and the only unquoted characters
// are the adapter's own `>`, `&&` and spaces. `submitPrefix` names the
// submit binary per cluster (module systems, wrappers, full paths).
//
// MONITOR: a bounded sync poll (`squeue`/`qstat`, then `sacct` for
// SLURM's terminal states) resolving to queued/running/finished/failed
// with the exit code from accounting. An unreachable scheduler is the
// typed `scheduler_unreachable:` error — the job keeps its last observed
// state honestly; a job that left the queue with no accounting row yet
// stays alive (accounting lag is a fact of cluster life).
//
// FETCH: the job's output files per the scheduler's convention
// (`rc-<handle8>.<jobid>.out`/`.err` — the names the generated script
// itself declares), read via `cat` (locally, or encoded over ssh). A
// missing file surfaces as the cat's own error in stderr — never a silent
// empty success.
//
// v0.2.0 limits, honestly: resources are requested via directives (the
// scheduler enforces them — that is the point of a cluster); stdout/stderr
// files are read where they were written (no artifact transfer beyond
// the captured text); per-target autonomy and `target.spend_recorded`
// apply unchanged (the command layer's generic paths).

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::domain::jobs::{validate_host, JobSpec, SpecError};

use super::{
    job_table, observe, read_result, run_sync, shell_quote, track_as, ComputeTarget, JobHandle,
    JobResult, JobSlot, SyncRun, TargetError, TargetInfo, TargetJobStatus,
};

/// How long a scheduler poll may run before the adapter gives up on it —
/// a stuck `squeue` is a typed unreachable, never a hung job row.
const POLL_TIMEOUT_SECS: u64 = 10;

/// The scheduler family a target speaks (config `flavor`, default slurm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flavor {
    Slurm,
    Pbs,
}

impl Flavor {
    fn label(self) -> &'static str {
        match self {
            Flavor::Slurm => "slurm",
            Flavor::Pbs => "pbs",
        }
    }
}

/// The per-target config, parsed and re-validated at the adapter boundary
/// (defense in depth behind the `target.declared` validation).
#[derive(Debug, Clone, PartialEq, Eq)]
struct SchedulerConfig {
    flavor: Flavor,
    /// The submit binary (config `submitPrefix`, default `sbatch`/`qsub`).
    submit: String,
    /// The live-state poll binary (config `pollPrefix`, default
    /// `squeue`/`qstat`).
    poll: String,
    /// SLURM's accounting binary (config `acctPrefix`, default `sacct`).
    acct: String,
    /// The optional submission host — commands ride ssh to it.
    host: Option<String>,
}

fn parse_config(target: &TargetInfo) -> Result<SchedulerConfig, TargetError> {
    let flavor = match target.cfg("flavor") {
        None => Flavor::Slurm,
        Some("slurm") => Flavor::Slurm,
        Some("pbs") => Flavor::Pbs,
        Some(other) => {
            return Err(TargetError::InvalidConfig {
                key: "flavor".into(),
                reason: format!("`{other}` — flavor is `slurm` or `pbs`"),
            })
        }
    };
    if let Some(host) = target.host.as_deref() {
        validate_host(host).map_err(SpecError::InvalidHost)?;
    }
    Ok(SchedulerConfig {
        submit: target
            .cfg("submitPrefix")
            .unwrap_or(match flavor {
                Flavor::Slurm => "sbatch",
                Flavor::Pbs => "qsub",
            })
            .to_string(),
        poll: target
            .cfg("pollPrefix")
            .unwrap_or(match flavor {
                Flavor::Slurm => "squeue",
                Flavor::Pbs => "qstat",
            })
            .to_string(),
        acct: target.cfg("acctPrefix").unwrap_or("sacct").to_string(),
        host: target.host.clone(),
        flavor,
    })
}

// ---------------------------------------------------------------------------
// Spec → generated batch script (the never-freeform mapping)
// ---------------------------------------------------------------------------

/// Generate the batch script from the validated spec's fields. `tag` is
/// the handle's short id — the job name and the output-file stems derive
/// from it, so fetch knows exactly where the scheduler put the output.
fn batch_script(spec: &JobSpec, flavor: Flavor, tag: &str) -> String {
    let mut lines: Vec<String> = vec!["#!/bin/sh".into()];
    match flavor {
        Flavor::Slurm => {
            lines.push(format!("#SBATCH --job-name=rc-{tag}"));
            if let Some(resources) = spec.resources {
                if let Some(cpus) = resources.cpus {
                    lines.push(format!("#SBATCH --cpus-per-task={cpus}"));
                }
                if let Some(mem) = resources.memory_mb {
                    lines.push(format!("#SBATCH --mem={mem}"));
                }
            }
            if let Some(dir) = spec.workdir.as_deref() {
                lines.push(format!("#SBATCH --chdir={dir}"));
            }
            lines.push(format!("#SBATCH --output=rc-{tag}.%j.out"));
            lines.push(format!("#SBATCH --error=rc-{tag}.%j.err"));
        }
        Flavor::Pbs => {
            lines.push(format!("#PBS -N rc-{tag}"));
            if let Some(resources) = spec.resources {
                let mut select = format!("#PBS -l select=1:ncpus={}", resources.cpus.unwrap_or(1));
                if let Some(mem) = resources.memory_mb {
                    select.push_str(&format!(":mem={mem}mb"));
                }
                lines.push(select);
            }
            if let Some(dir) = spec.workdir.as_deref() {
                lines.push(format!("#PBS -d {dir}"));
            }
            lines.push(format!("#PBS -o rc-{tag}.out"));
            lines.push(format!("#PBS -e rc-{tag}.err"));
            lines.push("cd \"$PBS_O_WORKDIR\"".into());
        }
    }
    for (key, value) in &spec.env {
        lines.push(format!("export {key}={}", shell_quote(value)));
    }
    // The body: the validated argv, every word POSIX-quoted — the remote
    // shell parses it back to the exact same argv (the SSH adapter's
    // discipline; there is no freeform body to inject).
    let mut exec = String::from("exec ");
    exec.push_str(&shell_quote(&spec.cmd));
    for arg in &spec.args {
        exec.push(' ');
        exec.push_str(&shell_quote(arg));
    }
    lines.push(exec);
    let mut script = lines.join("\n");
    script.push('\n');
    script
}

/// The output-file names the generated script declared, once the
/// scheduler's own job id is known (SLURM's `%j` substituted; PBS names
/// carry no substitution).
fn output_files(flavor: Flavor, tag: &str, scheduler_id: &str) -> (String, String) {
    match flavor {
        Flavor::Slurm => (
            format!("rc-{tag}.{scheduler_id}.out"),
            format!("rc-{tag}.{scheduler_id}.err"),
        ),
        Flavor::Pbs => (format!("rc-{tag}.out"), format!("rc-{tag}.err")),
    }
}

/// Parse the scheduler job id out of the submit command's stdout:
/// SLURM's `--parsable` `12345` (or the classic `Submitted batch job
/// 12345`), PBS's `12345.server`.
fn parse_scheduler_job_id(flavor: Flavor, stdout: &str) -> Option<String> {
    let line = stdout.lines().map(str::trim).find(|l| !l.is_empty())?;
    let numeric = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    match flavor {
        Flavor::Slurm => line
            .split_whitespace()
            .last()
            .filter(|token| numeric(token))
            .map(str::to_string),
        Flavor::Pbs => line
            .split('.')
            .next()
            .map(str::trim)
            .filter(|token| numeric(token))
            .map(str::to_string),
    }
}

// ---------------------------------------------------------------------------
// The transport: argv directly, encoded over ssh when a host is named
// ---------------------------------------------------------------------------

/// The ssh client's argv carrying one ENCODED command line (the SSH
/// adapter's options and quoting discipline).
fn ssh_argv(host: &str, encoded_line: String) -> Vec<String> {
    vec![
        "ssh".into(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ConnectTimeout=10".into(),
        "-o".into(),
        "StrictHostKeyChecking=accept-new".into(),
        host.to_string(),
        encoded_line,
    ]
}

/// One command's argv, ready to spawn: the words as-is locally, or the
/// ssh client's argv with every word `shell_quote`d when the target
/// names a host (polls and cats — simple commands, every operand data).
fn transport_argv(cfg: &SchedulerConfig, words: &[String]) -> Vec<String> {
    match &cfg.host {
        None => words.to_vec(),
        Some(host) => {
            let encoded = words
                .iter()
                .map(|w| shell_quote(w))
                .collect::<Vec<_>>()
                .join(" ");
            ssh_argv(host, encoded)
        }
    }
}

/// The remote submit command line: the generated script materialized on
/// the host (`printf '%s\n' <quoted lines> > <quoted path>`) and then
/// submitted — every OPERAND is `shell_quote`d; the only bare characters
/// are the adapter's own `>`, `&&` and spaces (they are the adapter's
/// syntax, never data).
fn remote_submit_line(cfg: &SchedulerConfig, script: &str, path: &str) -> String {
    let mut line = format!("printf {} ", shell_quote("%s\\n"));
    for script_line in script.lines() {
        line.push_str(&shell_quote(script_line));
        line.push(' ');
    }
    line.push_str(&format!("> {} && ", shell_quote(path)));
    line.push_str(&shell_quote(&cfg.submit));
    line.push_str(&format!(" {}", shell_quote(path)));
    line
}

/// The typed unreachable: `detail` is the poll's own words (first line of
/// its stderr, or the spawn/timeout error).
fn unreachable(cfg: &SchedulerConfig, detail: &str) -> TargetError {
    let host = cfg
        .host
        .clone()
        .unwrap_or_else(|| "local scheduler".to_string());
    let detail = detail
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("no output")
        .trim();
    TargetError::SchedulerUnreachable {
        host,
        detail: if detail.is_empty() { "no output".into() } else { detail.to_string() },
    }
}

/// What a poll resolved to.
enum ClusterState {
    /// Queued or running — alive on the cluster (the adapter-level
    /// vocabulary has no queued state: "alive, not terminal" is Running).
    Alive,
    Terminal { code: Option<i32>, reason: Option<String> },
}

/// Poll a SLURM job: `squeue` answers for live jobs; once it stops
/// knowing the job, `sacct` carries the terminal state and exit code.
fn poll_slurm(cfg: &SchedulerConfig, id: &str) -> Result<ClusterState, TargetError> {
    let argv = transport_argv(
        cfg,
        &[
            cfg.poll.clone(),
            "-h".into(),
            "-j".into(),
            id.into(),
            "-o".into(),
            "%T".into(),
        ],
    );
    match run_sync(&argv, Duration::from_secs(POLL_TIMEOUT_SECS)) {
        Ok(SyncRun::Done(out)) if out.status.success() => {
            let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !state.is_empty() {
                return Ok(match state.as_str() {
                    "RUNNING" | "COMPLETING" | "PENDING" | "CONFIGURING" | "REQUEUED"
                    | "REQUEUE_HOLD" | "RESIZING" | "SUSPENDED" | "REVOKED" | "SIGNALING" => {
                        ClusterState::Alive
                    }
                    other => ClusterState::Terminal {
                        code: None,
                        reason: Some(format!("slurm_{other}")),
                    },
                });
            }
            // squeue knows nothing — accounting is the next witness
        }
        Ok(SyncRun::Done(out)) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !stderr.contains("Invalid job id") {
                return Err(unreachable(cfg, &stderr));
            }
            // not an error to squeue — the job simply left the queue
        }
        Ok(SyncRun::Timeout) => return Err(unreachable(cfg, "poll timed out")),
        Err(e) => return Err(unreachable(cfg, &e.to_string())),
    }
    let argv = transport_argv(
        cfg,
        &[
            cfg.acct.clone(),
            "-n".into(),
            "-j".into(),
            id.into(),
            "-o".into(),
            "State,ExitCode".into(),
        ],
    );
    match run_sync(&argv, Duration::from_secs(POLL_TIMEOUT_SECS)) {
        Ok(SyncRun::Done(out)) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let Some(line) = stdout.lines().map(str::trim).find(|l| !l.is_empty()) else {
                // accounting knows nothing yet — alive is the honest read
                // (lag is a fact of cluster life; the next poll sees more)
                return Ok(ClusterState::Alive);
            };
            let mut parts = line.split_whitespace();
            let state = parts.next().unwrap_or_default();
            let code = parts
                .next()
                .and_then(|exit| exit.split(':').next())
                .and_then(|c| c.parse::<i32>().ok());
            Ok(match state {
                "COMPLETED" => ClusterState::Terminal { code: Some(0), reason: None },
                "FAILED" => ClusterState::Terminal {
                    code,
                    reason: Some(match code {
                        Some(c) => format!("exit_code_{c}"),
                        None => "slurm_failed".into(),
                    }),
                },
                "CANCELLED" => ClusterState::Terminal { code, reason: Some("cancelled".into()) },
                "TIMEOUT" => ClusterState::Terminal { code, reason: Some("timeout".into()) },
                "OUT_OF_MEMORY" | "NODE_FAIL" | "BOOT_FAIL" | "DEADLINE" => {
                    ClusterState::Terminal { code, reason: Some(state.to_lowercase()) }
                }
                _ => ClusterState::Alive, // PENDING/RUNNING/REQUEUED… — alive
            })
        }
        Ok(SyncRun::Done(out)) => Err(unreachable(cfg, &String::from_utf8_lossy(&out.stderr))),
        Ok(SyncRun::Timeout) => Err(unreachable(cfg, "accounting poll timed out")),
        Err(e) => Err(unreachable(cfg, &e.to_string())),
    }
}

/// Poll a PBS job: `qstat -x -f` (finished jobs included) carries
/// `job_state` and `exit_status`.
fn poll_pbs(cfg: &SchedulerConfig, id: &str) -> Result<ClusterState, TargetError> {
    let argv = transport_argv(cfg, &[cfg.poll.clone(), "-x".into(), "-f".into(), id.into()]);
    match run_sync(&argv, Duration::from_secs(POLL_TIMEOUT_SECS)) {
        Ok(SyncRun::Done(out)) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let field = |name: &str| {
                stdout
                    .lines()
                    .map(str::trim)
                    .find_map(|l| l.strip_prefix(&format!("{name} =")).map(str::trim))
            };
            let state = field("job_state").unwrap_or_default().to_string();
            let exit = field("exit_status").and_then(|s| s.trim().parse::<i32>().ok());
            Ok(match state.as_str() {
                "Q" | "H" | "R" | "E" | "W" | "T" | "M" | "S" => ClusterState::Alive,
                "C" | "F" => ClusterState::Terminal {
                    code: exit.or(Some(0)),
                    reason: match exit {
                        Some(0) | None => None,
                        Some(code) => Some(format!("exit_code_{code}")),
                    },
                },
                "" => ClusterState::Alive, // no state yet — alive, next poll knows
                other => ClusterState::Terminal {
                    code: exit,
                    reason: Some(format!("pbs_{other}")),
                },
            })
        }
        Ok(SyncRun::Done(out)) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stderr.contains("Unknown Job Id") {
                // gone from the server's memory — the read model keeps its
                // last observed state honestly (typed unknown, never a lie)
                return Err(TargetError::UnknownJob(id.to_string()));
            }
            Err(unreachable(cfg, &stderr))
        }
        Ok(SyncRun::Timeout) => Err(unreachable(cfg, "poll timed out")),
        Err(e) => Err(unreachable(cfg, &e.to_string())),
    }
}

fn poll_cluster(cfg: &SchedulerConfig, id: &str) -> Result<ClusterState, TargetError> {
    match cfg.flavor {
        Flavor::Slurm => poll_slurm(cfg, id),
        Flavor::Pbs => poll_pbs(cfg, id),
    }
}

// ---------------------------------------------------------------------------
// The adapter
// ---------------------------------------------------------------------------

/// The allowlist gate shared by submit and probe — BEFORE any connection
/// is attempted (defense in depth behind the command layer's own check).
fn gate_allowlist(target: &TargetInfo, host: &str) -> Result<(), TargetError> {
    if !target.allowlist.iter().any(|allowed| allowed == host) {
        return Err(TargetError::HostNotAllowed {
            host: host.to_string(),
            known: if target.allowlist.is_empty() {
                "empty — add hosts in Settings → Compute targets".to_string()
            } else {
                target.allowlist.join(" | ")
            },
        });
    }
    Ok(())
}

/// The submit-side tracked processes (the shared reaper machinery — the
/// submit command runs async; its captured output carries the
/// scheduler's answer).
fn submit_jobs() -> &'static Mutex<HashMap<String, JobSlot>> {
    static JOBS: OnceLock<Mutex<HashMap<String, JobSlot>>> = OnceLock::new();
    job_table(&JOBS)
}

/// What one scheduler job is right now, from the adapter's side.
#[derive(Debug)]
enum Phase {
    /// The submit command is still running (tracked by the shared
    /// machinery — its slot fills when sbatch/qsub answers).
    Submitting,
    /// The scheduler accepted the job — its own id, and the terminal
    /// facts once a poll resolved them (cached: a terminal is final).
    Cluster { scheduler_id: String, terminal: Option<Terminal> },
    /// The submit itself failed — the captured submit output is the
    /// result (there is no cluster job to ask).
    SubmitFailed { code: Option<i32>, reason: String, stdout: String, stderr: String },
}

/// A resolved terminal: exit 0 with no reason is Finished; everything
/// else is Failed with its reason.
#[derive(Debug, Clone)]
struct Terminal {
    code: Option<i32>,
    reason: Option<String>,
}

impl Terminal {
    fn status(&self) -> TargetJobStatus {
        match (self.code, self.reason.clone()) {
            (Some(0), None) => TargetJobStatus::Finished { code: 0 },
            (code, reason) => TargetJobStatus::Failed {
                reason: reason.unwrap_or_else(|| "unknown".into()),
                code,
            },
        }
    }
}

/// One live scheduler job: its config, its tag (the handle's short id —
/// the script and output names derive from it), its workdir, and its
/// phase. Shared by Arc so monitor/fetch mutate THE stored entry.
/// Process-global like every adapter table: a restart loses them (the
/// log's read model keeps its last observed state honestly).
struct Entry {
    cfg: SchedulerConfig,
    tag: String,
    workdir: Option<String>,
    phase: Mutex<Phase>,
}

fn entries() -> &'static Mutex<HashMap<String, Arc<Entry>>> {
    static ENTRIES: OnceLock<Mutex<HashMap<String, Arc<Entry>>>> = OnceLock::new();
    ENTRIES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Poison-tolerant lock (a panicking holder never cascades — the locks
/// serialize, they guard no invariant).
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lookup_entry(handle: &JobHandle) -> Result<Arc<Entry>, TargetError> {
    lock(entries())
        .get(&handle.id)
        .cloned()
        .ok_or_else(|| TargetError::UnknownJob(handle.id.clone()))
}

/// Read one output file through the transport; a missing file surfaces as
/// the cat's own error in stderr (never a silent empty success).
fn cat_output(cfg: &SchedulerConfig, workdir: Option<&str>, name: &str) -> String {
    let path = match workdir {
        Some(dir) => format!("{dir}/{name}"),
        None => name.to_string(),
    };
    let argv = transport_argv(cfg, &["cat".to_string(), path]);
    match run_sync(&argv, Duration::from_secs(POLL_TIMEOUT_SECS)) {
        Ok(SyncRun::Done(out)) => {
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            if out.status.success() {
                stdout
            } else {
                // the file is missing or unreadable — say so honestly
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                if stdout.is_empty() { stderr } else { stdout }
            }
        }
        Ok(SyncRun::Timeout) => "fetch timed out reading the output file".into(),
        Err(e) => format!("fetch failed reading the output file: {e}"),
    }
}

fn tokio_command(argv: &[String]) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(&argv[0]);
    command.args(&argv[1..]);
    command
}

/// Write the generated script to a private temp file (the local
/// submission case) — 0600.
fn write_script(script: &str, tag: &str) -> std::io::Result<std::path::PathBuf> {
    let path = std::env::temp_dir().join(format!("rc-sched-{tag}.sh"));
    let mut file = std::fs::File::create(&path)?;
    file.write_all(script.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(path)
}

fn one_line(s: &str) -> String {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .chars()
        .take(120)
        .collect()
}

/// The SLURM/PBS scheduler adapter (Story 6.2): generated batch scripts,
/// argv-encoded transport, monitored through the scheduler's own state.
pub struct Scheduler;

impl Scheduler {
    /// The adapter the registry registers (Story 3.2's seam).
    pub fn new() -> Self {
        Scheduler
    }

    /// The reachability probe (the settings row's discovery state): a
    /// cheap live query against the scheduler (`squeue -h` / `qstat -Q`)
    /// — typed unreachable when it does not answer.
    pub fn probe(&self, target: &TargetInfo) -> Result<String, TargetError> {
        let cfg = parse_config(target)?;
        if let Some(host) = cfg.host.as_deref() {
            gate_allowlist(target, host)?;
        }
        let words: Vec<String> = match cfg.flavor {
            Flavor::Slurm => vec![cfg.poll.clone(), "-h".into()],
            Flavor::Pbs => vec![cfg.poll.clone(), "-Q".into()],
        };
        let argv = transport_argv(&cfg, &words);
        match run_sync(&argv, Duration::from_secs(POLL_TIMEOUT_SECS)) {
            Ok(SyncRun::Done(out)) if out.status.success() => Ok(format!(
                "scheduler answered ({}{})",
                cfg.flavor.label(),
                cfg.host
                    .as_deref()
                    .map(|h| format!(" via {h}"))
                    .unwrap_or_default()
            )),
            Ok(SyncRun::Done(out)) => {
                Err(unreachable(&cfg, &String::from_utf8_lossy(&out.stderr)))
            }
            Ok(SyncRun::Timeout) => Err(unreachable(&cfg, "probe timed out")),
            Err(e) => Err(unreachable(&cfg, &e.to_string())),
        }
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl ComputeTarget for Scheduler {
    fn kind(&self) -> &'static str {
        "scheduler"
    }

    fn submit(&self, spec: &JobSpec, target: &TargetInfo) -> Result<JobHandle, TargetError> {
        // The adapter boundary validates again (AD-6): no freeform spec
        // ever becomes a script.
        spec.validate()?;
        let cfg = parse_config(target)?;
        // A workdir lands in a directive field (whitespace would split
        // it) — one token or a typed refusal.
        if let Some(dir) = spec.workdir.as_deref()
            && dir.chars().any(char::is_whitespace)
        {
            return Err(TargetError::InvalidConfig {
                key: "workdir".into(),
                reason: "scheduler directives take one-token paths — a workdir with spaces cannot ride a #SBATCH/#PBS line".into(),
            });
        }
        // The allowlist gate — BEFORE any connection is attempted (before
        // the ssh binary is even spawned). Defense in depth: the command
        // layer already refused hosts outside the allowlist.
        if let Some(host) = cfg.host.as_deref() {
            gate_allowlist(target, host)?;
        }
        let handle_id = uuid::Uuid::new_v4().to_string();
        let tag = handle_id[..8].to_string();
        let script = batch_script(spec, cfg.flavor, &tag);
        // The transport: locally the script is a temp file handed to
        // sbatch/qsub as ONE argv element; remotely the encoded
        // printf-line materializes it on the host first.
        let command = match cfg.host.as_deref() {
            None => {
                let path = write_script(&script, &tag)
                    .map_err(|e| TargetError::Spawn(format!("script write failed: {e}")))?;
                let argv = vec![cfg.submit.clone(), path.to_string_lossy().into_owned()];
                tokio_command(&argv)
            }
            Some(host) => {
                let remote_path = format!("/tmp/rc-{tag}.sh");
                let argv = ssh_argv(host, remote_submit_line(&cfg, &script, &remote_path));
                tokio_command(&argv)
            }
        };
        let handle = track_as(submit_jobs(), command, handle_id);
        lock(entries())
            .insert(handle.id.clone(), Arc::new(Entry {
                cfg,
                tag,
                workdir: spec.workdir.clone(),
                phase: Mutex::new(Phase::Submitting),
            }));
        Ok(handle)
    }

    fn monitor(&self, handle: &JobHandle) -> Result<TargetJobStatus, TargetError> {
        let entry = lookup_entry(handle)?;
        // What this monitor owes: decided under a short lock, acted on
        // outside it (a poll is a bounded sync run — no lock held).
        enum Due {
            PollCluster(String),
            CheckSubmit,
        }
        let due = {
            let phase = entry.phase.lock().unwrap_or_else(|p| p.into_inner());
            match &*phase {
                Phase::SubmitFailed { code, reason, .. } => {
                    return Ok(TargetJobStatus::Failed { reason: reason.clone(), code: *code })
                }
                Phase::Cluster { scheduler_id, terminal } => match terminal {
                    Some(terminal) => return Ok(terminal.status()),
                    None => Due::PollCluster(scheduler_id.clone()),
                },
                Phase::Submitting => Due::CheckSubmit,
            }
        };
        match due {
            Due::PollCluster(scheduler_id) => {
                let state = poll_cluster(&entry.cfg, &scheduler_id)?;
                let mut phase = entry.phase.lock().unwrap_or_else(|p| p.into_inner());
                let Phase::Cluster { terminal, .. } = &mut *phase else {
                    // the phase moved under us — the next monitor tells the story
                    return Ok(TargetJobStatus::Running);
                };
                match state {
                    ClusterState::Alive => Ok(TargetJobStatus::Running),
                    ClusterState::Terminal { code, reason } => {
                        let terminal_value = Terminal { code, reason };
                        let status = terminal_value.status();
                        *terminal = Some(terminal_value);
                        Ok(status)
                    }
                }
            }
            Due::CheckSubmit => {
                // The shared machinery observes the submit process; its
                // exit carries the scheduler's answer.
                match observe(submit_jobs(), handle)? {
                    TargetJobStatus::Running => Ok(TargetJobStatus::Running),
                    TargetJobStatus::Finished { .. } => {
                        let result = read_result(submit_jobs(), handle)?;
                        let mut phase = entry.phase.lock().unwrap_or_else(|p| p.into_inner());
                        match parse_scheduler_job_id(entry.cfg.flavor, &result.stdout) {
                            Some(scheduler_id) => {
                                *phase = Phase::Cluster { scheduler_id, terminal: None };
                                Ok(TargetJobStatus::Running)
                            }
                            None => {
                                let reason = format!(
                                    "no_scheduler_job_id: the submit output named no job — `{}`",
                                    one_line(&result.stdout)
                                );
                                *phase = Phase::SubmitFailed {
                                    code: result.code,
                                    reason: reason.clone(),
                                    stdout: result.stdout.clone(),
                                    stderr: result.stderr.clone(),
                                };
                                Ok(TargetJobStatus::Failed { reason, code: result.code })
                            }
                        }
                    }
                    TargetJobStatus::Failed { reason, code } => {
                        let result = read_result(submit_jobs(), handle)?;
                        let reason = format!("submit_failed: {reason}");
                        let mut phase = entry.phase.lock().unwrap_or_else(|p| p.into_inner());
                        *phase = Phase::SubmitFailed {
                            code: result.code,
                            reason: reason.clone(),
                            stdout: result.stdout.clone(),
                            stderr: result.stderr.clone(),
                        };
                        Ok(TargetJobStatus::Failed { reason, code })
                    }
                }
            }
        }
    }

    fn fetch(&self, handle: &JobHandle) -> Result<JobResult, TargetError> {
        let entry = lookup_entry(handle)?;
        let phase = entry.phase.lock().unwrap_or_else(|p| p.into_inner());
        match &*phase {
            Phase::Submitting => Err(TargetError::NotTerminal(handle.id.clone())),
            Phase::SubmitFailed { code, stdout, stderr, .. } => Ok(JobResult {
                code: *code,
                stdout: stdout.clone(),
                stderr: stderr.clone(),
            }),
            Phase::Cluster { scheduler_id, terminal } => {
                let Some(terminal) = terminal else {
                    return Err(TargetError::NotTerminal(handle.id.clone()));
                };
                let (out_name, err_name) =
                    output_files(entry.cfg.flavor, &entry.tag, scheduler_id);
                Ok(JobResult {
                    code: terminal.code,
                    stdout: cat_output(&entry.cfg, entry.workdir.as_deref(), &out_name),
                    stderr: cat_output(&entry.cfg, entry.workdir.as_deref(), &err_name),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::targets::TargetInfo;
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

    fn info(config: &[(&str, &str)], host: Option<&str>, allowlist: &[&str]) -> TargetInfo {
        TargetInfo {
            name: "cluster-1".into(),
            host: host.map(|h| h.into()),
            allowlist: allowlist.iter().map(|h| h.to_string()).collect(),
            config: config
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    /// The fake-binary harness: write an executable stand-in script. The
    /// config keys (submitPrefix/pollPrefix/acctPrefix) point the adapter
    /// at them — the same mechanism a real cluster uses for module
    /// systems and wrappers.
    fn fake_bin(name: &str, body: &str) -> String {
        let dir = std::env::temp_dir().join(format!("rc-sched-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path.to_string_lossy().into_owned()
    }

    /// Rewrite a fake binary in place (the poll's second act: the job
    /// left the queue).
    fn refake_bin(path: &str, body: &str) {
        std::fs::write(std::path::Path::new(path), format!("#!/bin/sh\n{body}\n")).unwrap();
    }

    fn workdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rc-sched-wd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The stored entry's tag + scheduler id (test visibility into the
    /// naming convention the script declared).
    fn cluster_id(handle: &JobHandle) -> (String, String) {
        let map = entries().lock().unwrap();
        let entry = map.get(&handle.id).unwrap();
        match &*entry.phase.lock().unwrap() {
            Phase::Cluster { scheduler_id, .. } => (entry.tag.clone(), scheduler_id.clone()),
            other => panic!("expected the cluster phase, got {other:?}"),
        }
    }

    // ---- spec → generated script (the never-freeform mapping) ----

    #[test]
    fn slurm_directives_map_from_the_spec_fields() {
        let mut s = spec("python3", &["train.py", "--note=a | b && c"]);
        s.env.insert("EPOCHS".into(), "10; rm -rf /".into());
        s.resources = Some(crate::domain::jobs::JobResources {
            cpus: Some(4),
            memory_mb: Some(8192),
        });
        s.workdir = Some("/lab/exp".into());
        let script = batch_script(&s, Flavor::Slurm, "abcd1234");
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains("#SBATCH --job-name=rc-abcd1234"));
        assert!(script.contains("#SBATCH --cpus-per-task=4"));
        assert!(script.contains("#SBATCH --mem=8192"));
        assert!(script.contains("#SBATCH --chdir=/lab/exp"));
        assert!(script.contains("#SBATCH --output=rc-abcd1234.%j.out"));
        assert!(script.contains("#SBATCH --error=rc-abcd1234.%j.err"));
        // env values ride quoted-inert; args keep their metacharacters as
        // DATA; the body is exactly the encoded argv
        assert!(script.contains("export EPOCHS='10; rm -rf /'"));
        assert!(
            script.contains("exec 'python3' 'train.py' '--note=a | b && c'"),
            "the body is the encoded argv: {script}"
        );
    }

    #[test]
    fn pbs_directives_map_from_the_spec_fields() {
        let s = spec("python3", &["train.py"]);
        let script = batch_script(&s, Flavor::Pbs, "abcd1234");
        assert!(script.contains("#PBS -N rc-abcd1234"));
        assert!(!script.contains("select="), "no resources requested — no select line");
        assert!(script.contains("#PBS -o rc-abcd1234.out"));
        assert!(script.contains("#PBS -e rc-abcd1234.err"));
        assert!(script.contains("exec 'python3' 'train.py'"));
        let mut s = spec("python3", &[]);
        s.resources = Some(crate::domain::jobs::JobResources {
            cpus: Some(2),
            memory_mb: Some(4096),
        });
        s.workdir = Some("/lab/exp".into());
        let script = batch_script(&s, Flavor::Pbs, "abcd1234");
        assert!(script.contains("#PBS -l select=1:ncpus=2:mem=4096mb"));
        assert!(script.contains("#PBS -d /lab/exp"));
        assert!(script.contains("cd \"$PBS_O_WORKDIR\""));
    }

    #[test]
    fn the_remote_transport_encodes_every_operand() {
        let cfg = SchedulerConfig {
            flavor: Flavor::Slurm,
            submit: "/opt/slurm/bin/sbatch".into(),
            poll: "squeue".into(),
            acct: "sacct".into(),
            host: Some("login.hpc.edu".into()),
        };
        let line = remote_submit_line(
            &cfg,
            "#!/bin/sh\nexec 'python3' 'a; b'\n",
            "/tmp/rc-abcd1234.sh",
        );
        // every operand quoted; only the adapter's own redirection and
        // chaining are bare
        assert!(line.contains("'/opt/slurm/bin/sbatch'"), "submit prefix quoted: {line}");
        assert!(
            line.contains(&shell_quote("exec 'python3' 'a; b'")),
            "the script lines travel as quoted data: {line}"
        );
        assert!(
            line.contains(&format!("> {} && ", shell_quote("/tmp/rc-abcd1234.sh"))),
            "the adapter's own redirection is bare, the path quoted: {line}"
        );
        // a poll over the same transport: every operand data
        let argv = transport_argv(
            &cfg,
            &["squeue".into(), "-j".into(), "42".into(), "-o".into(), "%T".into()],
        );
        assert_eq!(argv[0], "ssh");
        assert!(
            argv.contains(&"login.hpc.edu".to_string()),
            "the host is one argv element"
        );
        assert_eq!(argv.last().unwrap(), "'squeue' '-j' '42' '-o' '%T'");
    }

    #[test]
    fn submit_output_parses_for_both_flavors() {
        assert_eq!(
            parse_scheduler_job_id(Flavor::Slurm, "Submitted batch job 4242\n"),
            Some("4242".into())
        );
        assert_eq!(parse_scheduler_job_id(Flavor::Slurm, "4242\n"), Some("4242".into()));
        assert_eq!(parse_scheduler_job_id(Flavor::Slurm, "garbage\n"), None);
        assert_eq!(
            parse_scheduler_job_id(Flavor::Pbs, "4242.server.cluster\n"),
            Some("4242".into())
        );
        assert_eq!(parse_scheduler_job_id(Flavor::Pbs, "\n"), None);
    }

    // ---- the gates: freeform, config, allowlist ----

    #[tokio::test]
    async fn a_freeform_spec_is_refused_before_any_script_or_connection() {
        let scheduler = Scheduler::new();
        let err = scheduler
            .submit(&spec("ls | rm -rf .", &[]), &info(&[], None, &[]))
            .unwrap_err();
        assert!(err.to_string().starts_with("freeform_shell:"), "unexpected: {err}");
        // an unknown flavor and a spaced workdir are typed refusals too
        let err = scheduler
            .submit(&spec("python3", &[]), &info(&[("flavor", "lsf")], None, &[]))
            .unwrap_err();
        assert!(err.to_string().starts_with("invalid_config:"), "unexpected: {err}");
        let mut s = spec("python3", &[]);
        s.workdir = Some("/lab/exp dir".into());
        let err = scheduler.submit(&s, &info(&[], None, &[])).unwrap_err();
        assert!(err.to_string().starts_with("invalid_config:"), "unexpected: {err}");
    }

    #[tokio::test]
    async fn a_host_outside_the_allowlist_is_refused_before_any_connection() {
        let scheduler = Scheduler::new();
        // the fake submit binary would leave a marker if it ever ran
        let marker = std::env::temp_dir().join(format!("rc-sched-marker-{}", uuid::Uuid::new_v4()));
        let submit = fake_bin(
            "sbatch",
            &format!("printf ran >> {}", shell_quote(marker.to_str().unwrap())),
        );
        for allowlist in [vec![], vec!["other-host.lab"]] {
            let err = scheduler
                .submit(
                    &spec("python3", &["train.py"]),
                    &info(&[("submitPrefix", &submit)], Some("login.hpc.edu"), &allowlist),
                )
                .unwrap_err();
            assert!(err.to_string().starts_with("host_not_allowed:"), "unexpected: {err}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(!marker.exists(), "the refusal predates any connection");
        // a malformed host never connects either
        let err = scheduler
            .submit(&spec("python3", &[]), &info(&[], Some("login;rm"), &["login;rm"]))
            .unwrap_err();
        assert!(err.to_string().contains("invalid_host:"), "unexpected: {err}");
    }

    // ---- the lifecycle over the fake-binary harness ----

    fn slurm_harness(submit_body: &str) -> (String, String, String) {
        (
            fake_bin("sbatch", submit_body),
            fake_bin("squeue", "printf 'RUNNING\\n'"),
            fake_bin("sacct", "printf 'COMPLETED 0:0\\n'"),
        )
    }

    /// Whether the stored entry reached its Cluster phase (the submit's
    /// answer was consumed and a scheduler job id was parsed).
    fn is_cluster(handle: &JobHandle) -> bool {
        let map = lock(entries());
        map.get(&handle.id)
            .map(|e| matches!(&*lock(&e.phase), Phase::Cluster { .. }))
            .unwrap_or(false)
    }

    /// Drive the phase machine past its submit transition: monitor until
    /// the submit process's answer is consumed (a parsed cluster job, or
    /// a reasoned submit failure) — bounded, so a truly stuck submit
    /// still fails the test. The CLUSTER is not polled past this point.
    async fn until_parsed(
        scheduler: &Scheduler,
        handle: &JobHandle,
    ) -> Result<TargetJobStatus, TargetError> {
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            match scheduler.monitor(handle) {
                Err(e) => return Err(e),
                Ok(status @ (TargetJobStatus::Finished { .. } | TargetJobStatus::Failed { .. })) => {
                    return Ok(status)
                }
                Ok(TargetJobStatus::Running) => {
                    if is_cluster(handle) {
                        return Ok(TargetJobStatus::Running);
                    }
                }
            }
        }
        panic!("the scheduler job never settled past its submit transition");
    }

    #[tokio::test]
    async fn a_slurm_job_lives_its_lifecycle_with_fetched_output() {
        let (submit, squeue, sacct) = slurm_harness("printf 'Submitted batch job 4242\\n'");
        let dir = workdir();
        let mut s = spec("python3", &["train.py"]);
        s.workdir = Some(dir.to_string_lossy().into_owned());
        let scheduler = Scheduler::new();
        let handle = scheduler
            .submit(
                &s,
                &info(
                    &[
                        ("submitPrefix", &submit),
                        ("pollPrefix", &squeue),
                        ("acctPrefix", &sacct),
                    ],
                    None,
                    &[],
                ),
            )
            .unwrap();
        // submitting → the fake answers fast → the cluster job is known
        assert_eq!(
            until_parsed(&scheduler, &handle).await.unwrap(),
            TargetJobStatus::Running
        );
        // fetch refuses while alive — typed, never a partial read
        let err = scheduler.fetch(&handle).unwrap_err();
        assert!(err.to_string().starts_with("job_not_terminal:"), "unexpected: {err}");
        // the job left the queue: squeue no longer knows it, sacct says
        // COMPLETED 0:0 — and the test plays the scheduler by writing
        // the output file exactly where the script declared it
        let (tag, scheduler_id) = cluster_id(&handle);
        let (out_name, err_name) = output_files(Flavor::Slurm, &tag, &scheduler_id);
        std::fs::write(dir.join(&out_name), b"trained 3 epochs\n").unwrap();
        std::fs::write(dir.join(&err_name), b"warning: lr warmup\n").unwrap();
        refake_bin(
            &squeue,
            "printf 'squeue: error: Invalid job id specified\\n' >&2\nexit 1",
        );
        assert_eq!(
            scheduler.monitor(&handle).unwrap(),
            TargetJobStatus::Finished { code: 0 }
        );
        let result = scheduler.fetch(&handle).unwrap();
        assert_eq!(result.code, Some(0));
        assert_eq!(result.stdout.trim(), "trained 3 epochs");
        assert_eq!(result.stderr.trim(), "warning: lr warmup");
        // the terminal is cached — a second monitor agrees without repolling
        refake_bin(&squeue, "exit 7"); // a broken poll must not matter now
        assert_eq!(
            scheduler.monitor(&handle).unwrap(),
            TargetJobStatus::Finished { code: 0 }
        );
    }

    #[tokio::test]
    async fn a_slurm_failure_carries_its_exit_code_from_accounting() {
        let (submit, squeue, sacct) = slurm_harness("printf '4242\\n'");
        let scheduler = Scheduler::new();
        let handle = scheduler
            .submit(
                &spec("python3", &["train.py"]),
                &info(
                    &[
                        ("submitPrefix", &submit),
                        ("pollPrefix", &squeue),
                        ("acctPrefix", &sacct),
                    ],
                    None,
                    &[],
                ),
            )
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        until_parsed(&scheduler, &handle).await.unwrap();
        // the job failed with exit 3: squeue gone, sacct says FAILED 3:0
        refake_bin(
            &squeue,
            "printf 'squeue: error: Invalid job id specified\\n' >&2\nexit 1",
        );
        refake_bin(&sacct, "printf 'FAILED 3:0\\n'");
        assert_eq!(
            scheduler.monitor(&handle).unwrap(),
            TargetJobStatus::Failed { reason: "exit_code_3".into(), code: Some(3) }
        );
    }

    #[tokio::test]
    async fn an_unreachable_scheduler_is_the_typed_down_state() {
        let (submit, squeue, sacct) = slurm_harness("printf '4242\\n'");
        let scheduler = Scheduler::new();
        let handle = scheduler
            .submit(
                &spec("python3", &[]),
                &info(
                    &[
                        ("submitPrefix", &submit),
                        ("pollPrefix", &squeue),
                        ("acctPrefix", &sacct),
                    ],
                    None,
                    &[],
                ),
            )
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        until_parsed(&scheduler, &handle).await.unwrap();
        // the fake plays an unreachable controller
        refake_bin(
            &squeue,
            "printf 'squeue: Socket timed out on send/recv operation\\n' >&2\nexit 1",
        );
        let err = scheduler.monitor(&handle).unwrap_err();
        assert!(err.to_string().starts_with("scheduler_unreachable:"), "unexpected: {err}");
        // the probe agrees — the settings row's discovery state
        let err = scheduler
            .probe(&info(&[("pollPrefix", &squeue)], None, &[]))
            .unwrap_err();
        assert!(err.to_string().starts_with("scheduler_unreachable:"), "unexpected: {err}");
        // and a healthy probe answers
        let ok = scheduler
            .probe(&info(&[("pollPrefix", &fake_bin("squeue", "exit 0"))], None, &[]))
            .unwrap();
        assert!(ok.contains("slurm"), "{ok}");
    }

    #[tokio::test]
    async fn a_failed_submit_is_a_reasoned_terminal_with_the_captured_output() {
        let submit = fake_bin(
            "sbatch",
            "printf 'sbatch: error: Batch job submission failed\\n' >&2\nexit 1",
        );
        let scheduler = Scheduler::new();
        let handle = scheduler
            .submit(&spec("python3", &[]), &info(&[("submitPrefix", &submit)], None, &[]))
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let status = scheduler.monitor(&handle).unwrap();
        match status {
            TargetJobStatus::Failed { reason, code } => {
                assert!(reason.starts_with("submit_failed:"), "unexpected: {reason}");
                assert_eq!(code, Some(1));
            }
            other => panic!("a failed submit is an observed failure, got {other:?}"),
        }
        let result = scheduler.fetch(&handle).unwrap();
        assert!(result.stderr.contains("Batch job submission failed"), "{}", result.stderr);
    }

    #[tokio::test]
    async fn submit_output_that_names_no_job_is_a_reasoned_terminal() {
        let submit = fake_bin("sbatch", "printf 'garbage\\n'");
        let scheduler = Scheduler::new();
        let handle = scheduler
            .submit(&spec("python3", &[]), &info(&[("submitPrefix", &submit)], None, &[]))
            .unwrap();
        match until_parsed(&scheduler, &handle).await.unwrap() {
            TargetJobStatus::Failed { reason, .. } => {
                assert!(reason.starts_with("no_scheduler_job_id:"), "unexpected: {reason}");
            }
            other => panic!("expected a reasoned terminal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_pbs_job_lives_its_lifecycle() {
        let submit = fake_bin("qsub", "printf '4242.server\\n'");
        // the fake qstat cats a state file the test rewrites between polls
        let state_file = workdir().join("qstat-state");
        std::fs::write(&state_file, "    job_state = R\n").unwrap();
        let qstat = fake_bin(
            "qstat",
            &format!("cat {}", shell_quote(state_file.to_str().unwrap())),
        );
        let dir = workdir();
        let mut s = spec("python3", &["train.py"]);
        s.workdir = Some(dir.to_string_lossy().into_owned());
        let scheduler = Scheduler::new();
        let handle = scheduler
            .submit(
                &s,
                &info(
                    &[("flavor", "pbs"), ("submitPrefix", &submit), ("pollPrefix", &qstat)],
                    None,
                    &[],
                ),
            )
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_eq!(
            until_parsed(&scheduler, &handle).await.unwrap(),
            TargetJobStatus::Running
        );
        // completed with exit 2 — and the output files exist per the
        // PBS convention the script declared
        std::fs::write(&state_file, "    job_state = C\n    exit_status = 2\n").unwrap();
        let (tag, _) = cluster_id(&handle);
        std::fs::write(dir.join(format!("rc-{tag}.out")), b"pbs out\n").unwrap();
        std::fs::write(dir.join(format!("rc-{tag}.err")), b"pbs err\n").unwrap();
        assert_eq!(
            scheduler.monitor(&handle).unwrap(),
            TargetJobStatus::Failed { reason: "exit_code_2".into(), code: Some(2) }
        );
        let result = scheduler.fetch(&handle).unwrap();
        assert_eq!(result.stdout.trim(), "pbs out");
        assert_eq!(result.stderr.trim(), "pbs err");
    }

    #[tokio::test]
    async fn an_unknown_handle_is_a_typed_error() {
        let scheduler = Scheduler::new();
        let err = scheduler.monitor(&JobHandle::new("never-submitted")).unwrap_err();
        assert!(err.to_string().starts_with("unknown_job:"), "unexpected: {err}");
        let err = scheduler.fetch(&JobHandle::new("never-submitted")).unwrap_err();
        assert!(err.to_string().starts_with("unknown_job:"), "unexpected: {err}");
    }
}
