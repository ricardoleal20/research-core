// The bare-SSH compute target adapter (Story 3.3, FR-11.1/11.2, AD-6):
// allowlisted remote hosts running the SAME structured job specs, with no
// freeform-shell path anywhere.
//
// Transport: the system `ssh` binary, spawned with ARGV DIRECTLY
// (`tokio::process::Command` — every element a separate argv argument, so
// no LOCAL shell ever interprets anything). The remote side of OpenSSH
// hands the joined command string to the user's login shell; the adapter
// therefore never TRANSFERS a command string — it ENCODES the validated
// spec's argv into the transport's string protocol with POSIX
// single-quote quoting (`remote_command_line`), a deterministic and
// reversible encoding: the remote shell parses it back into the exact
// same argv. `cmd` is already metachar-free (JobSpec::validate, re-run
// here at the adapter boundary); `args` are DATA and quoting makes their
// metacharacters inert remotely exactly as they are locally. This is why
// the adapter exposes no freeform path: there is no input that reaches a
// shell un-encoded.
//
// The allowlist is enforced HERE — before any connection is attempted
// (before the ssh binary is even spawned) — as defense in depth behind
// the command layer's own check. A host not on the allowlist is a typed
// `host_not_allowed:` submit error.
//
// Monitor: the spawned `ssh` client stays alive exactly as long as the
// remote process does, so the local child's liveness IS the remote job's
// state — the shared tracking machinery (reaper + slot table) observes it
// non-blockingly. Connection failures surface as reasoned terminals: ssh
// exits nonzero (255) with its error on stderr, which lands as the job's
// failure reason — visible on the job row (the health/telemetry seam).
//
// v1 limits, honestly: `resources` ride in the spec but are advisory
// (no remote cgroup enforcement); fetch returns the captured
// stdout/stderr/exit of the remote process — remote artifact files (scp)
// are not transferred in v1 (Story 3.4 shapes results into evidence).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::domain::jobs::{validate_host, JobSpec, SpecError};

use super::{
    job_table, observe, read_result, track, ComputeTarget, JobHandle, JobResult, JobSlot,
    TargetError, TargetInfo, TargetJobStatus,
};

/// How long ssh may spend establishing the connection before giving up —
/// a stuck connection is a reasoned failure, never a hung job row.
const CONNECT_TIMEOUT_SECS: u32 = 10;

/// The bare-SSH adapter: one host per declared target, allowlist-gated,
/// argv-encoded transfer, monitored through the ssh client's own
/// lifetime.
pub struct Ssh;

impl Ssh {
    /// The adapter the registry registers (Story 3.2's seam).
    pub fn new() -> Self {
        Ssh
    }
}

impl Default for Ssh {
    fn default() -> Self {
        Self::new()
    }
}

/// The ssh binary to spawn: the system `ssh`, unless a test installed a
/// stand-in (the loopback harness — a script returning canned output, so
/// submit/monitor/fetch are exercised without a reachable host).
fn ssh_bin() -> String {
    #[cfg(test)]
    {
        let cell = SSH_BIN_OVERRIDE.get_or_init(|| Mutex::new(None));
        if let Some(bin) = cell.lock().expect("ssh bin override poisoned").clone() {
            return bin;
        }
    }
    "ssh".to_string()
}

/// Test seam (loopback harness): a stand-in binary for `ssh`. The fakes
/// have test-specific behavior, so every test that installs one (and
/// every test that submits through the registry's adapter) holds
/// `ssh_test_lock` for its duration — the harness is serialized, never
/// racy.
#[cfg(test)]
static SSH_BIN_OVERRIDE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[cfg(test)]
pub(crate) fn install_ssh_bin(path: String) {
    let cell = SSH_BIN_OVERRIDE.get_or_init(|| Mutex::new(None));
    *cell.lock().expect("ssh bin override poisoned") = Some(path);
}

/// The serialized-test lock: held by every test that installs a fake ssh
/// binary or submits through the ssh adapter (ssh.rs's own tests AND the
/// command-layer tests in jobs_commands.rs).
#[cfg(test)]
static SSH_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(test)]
pub(crate) fn ssh_test_lock() -> std::sync::MutexGuard<'static, ()> {
    // A panicking holder must not cascade into every other ssh test —
    // the lock serializes, it guards no invariant.
    SSH_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The SSH adapter's live processes (its own table — never shared with
/// the Local adapter's).
fn ssh_jobs() -> &'static Mutex<HashMap<String, JobSlot>> {
    static JOBS: OnceLock<Mutex<HashMap<String, JobSlot>>> = OnceLock::new();
    job_table(&JOBS)
}

/// Encode one string as a single POSIX shell word: wrap in single quotes
/// (everything between them is literal), escaping embedded quotes by
/// close-escape-reopen (`'\''`). Deterministic and reversible — the
/// remote shell parses the word back to the exact same string.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Encode the validated spec's argv into the ssh transport's command
/// string: env assignments (`'KEY'='value'` — BOTH sides single-quoted;
/// keys are validated POSIX names, and quoting the key is harmless for a
/// valid name while keeping a hostile key inert even if validation ever
/// regresses — review R-01), an optional `cd -- 'dir' &&` prefix for
/// workdir, then the cmd and every arg as quoted words. Nothing
/// user-supplied is transferred unencoded; see the module comment for why
/// this is not a freeform path.
fn remote_command_line(spec: &JobSpec) -> String {
    let mut words: Vec<String> = Vec::new();
    for (key, value) in &spec.env {
        words.push(format!("{}={}", shell_quote(key), shell_quote(value)));
    }
    if let Some(dir) = spec.workdir.as_deref() {
        words.push(format!("cd -- {} &&", shell_quote(dir)));
    }
    words.push(shell_quote(&spec.cmd));
    for arg in &spec.args {
        words.push(shell_quote(arg));
    }
    words.join(" ")
}

impl ComputeTarget for Ssh {
    fn kind(&self) -> &'static str {
        "ssh"
    }

    fn submit(&self, spec: &JobSpec, target: &TargetInfo) -> Result<JobHandle, TargetError> {
        // The adapter boundary re-validates the spec (AD-6): nothing is
        // transferred that did not pass the never-freeform-shell gate.
        spec.validate()?;
        // An ssh target names its host; a target without one never
        // connects anywhere.
        let host = target
            .host
            .as_deref()
            .ok_or_else(|| TargetError::MissingHost(target.name.clone()))?;
        // The host is one token (it becomes one argv element of ssh);
        // re-validated here — the adapter is the last line of defense.
        validate_host(host).map_err(SpecError::InvalidHost)?;
        // The allowlist gate — BEFORE any connection is attempted (before
        // the ssh binary is even spawned). Defense in depth: the command
        // layer already refused hosts outside the allowlist.
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
        // `resources` ride in the spec as an advisory request (documented
        // v1 limit: no remote enforcement).
        let _ = spec.resources;
        // The transfer: ssh with its options as argv elements, the host as
        // ONE validated argv element, and the ENCODED argv (never a
        // user-supplied command string) as the command. BatchMode: no
        // interactive prompts, ever (a prompt would hang a monitored job);
        // accept-new: the first connection auto-accepts the host key
        // (documented v1 tradeoff — the allowlist already gates WHERE we
        // connect; a changed key still refuses).
        let mut command = tokio::process::Command::new(ssh_bin());
        command
            .arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg(format!("ConnectTimeout={CONNECT_TIMEOUT_SECS}"))
            .arg("-o")
            .arg("StrictHostKeyChecking=accept-new")
            .arg(host)
            .arg(remote_command_line(spec));
        Ok(track(ssh_jobs(), command))
    }

    fn monitor(&self, handle: &JobHandle) -> Result<TargetJobStatus, TargetError> {
        observe(ssh_jobs(), handle)
    }

    fn fetch(&self, handle: &JobHandle) -> Result<JobResult, TargetError> {
        read_result(ssh_jobs(), handle)
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

    fn info(host: Option<&str>, allowlist: &[&str]) -> TargetInfo {
        TargetInfo {
            name: "cluster-1".into(),
            host: host.map(|h| h.to_string()),
            allowlist: allowlist.iter().map(|h| h.to_string()).collect(),
        }
    }

    /// The loopback harness: write a stand-in `ssh` script and install it.
    /// `body` is the shell the fake runs after the standard prelude (the
    /// prelude is where a marker proves execution happened).
    fn fake_ssh(marker: Option<&str>, body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rc-ssh-fake-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ssh");
        let marker_line = marker
            .map(|m| format!("printf ran >> {}\n", shell_quote(m)))
            .unwrap_or_default();
        std::fs::write(
            &path,
            format!("#!/bin/sh\n{marker_line}{body}\n"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        install_ssh_bin(path.to_string_lossy().into_owned());
        path
    }

    // ---- the allowlist gate: before any connection ----

    #[tokio::test]
    async fn a_host_outside_the_allowlist_is_refused_before_any_connection() {
        let _lock = ssh_test_lock();
        // the fake would leave a marker if it were ever executed
        let marker = std::env::temp_dir().join(format!("rc-ssh-marker-{}", uuid::Uuid::new_v4()));
        let _fake = fake_ssh(Some(marker.to_str().unwrap()), "exit 0");
        let ssh = Ssh::new();
        // an empty allowlist, and one that names a different host
        for allowlist in [vec![], vec!["other-host.lab"]] {
            let err = ssh
                .submit(&spec("python3", &["train.py"]), &info(Some("gpu-01.lab"), &allowlist))
                .unwrap_err();
            assert!(err.to_string().starts_with("host_not_allowed:"), "unexpected: {err}");
            assert!(err.to_string().contains("gpu-01.lab"), "the error names the host: {err}");
        }
        // no connection was attempted — no handle, no process, no marker
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(
            !marker.exists(),
            "the refusal must happen before any connection is attempted"
        );
    }

    #[tokio::test]
    async fn a_target_without_a_host_and_a_malformed_host_never_connect() {
        let ssh = Ssh::new();
        let err = ssh.submit(&spec("python3", &[]), &info(None, &["gpu-01.lab"])).unwrap_err();
        assert!(err.to_string().starts_with("missing_host:"), "unexpected: {err}");
        // a host with whitespace or shell syntax is refused at the adapter
        // boundary even if it somehow reached the allowlist
        for bad in ["gpu 01", "a;b", "-oProxyCommand=evil"] {
            let err = ssh
                .submit(&spec("python3", &[]), &info(Some(bad), &[bad]))
                .unwrap_err();
            assert!(
                err.to_string().contains("invalid_host:"),
                "{bad:?}: unexpected: {err}"
            );
        }
    }

    // ---- the transfer: argv encoded, never a freeform string ----

    #[tokio::test]
    async fn the_remote_side_parses_the_transferred_argv_back_exactly() {
        let _lock = ssh_test_lock();
        // the fake ssh evals the command line back into argv and prints
        // one argument per line — proving the encoding is lossless and
        // metacharacters in args stay DATA remotely
        let fake = fake_ssh(
            None,
            r#"eval "set -- ${!#}"
printf '%s\n' "$@""#,
        );
        let ssh = Ssh::new();
        let weird = spec("python3", &[
            "train.py",
            "--note=pipeline | data ; $(not-a-command) `backticks` > out",
            "it's got a quote",
            "",
        ]);
        let handle = ssh.submit(&weird, &info(Some("gpu-01.lab"), &["gpu-01.lab"])).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let result = ssh.fetch(&handle).unwrap();
        assert_eq!(result.code, Some(0), "stdout: {}", result.stdout);
        // cmd + args round-trip EXACTLY — one per line, empty args as
        // empty lines, trailing newline included
        let mut expected = vec![weird.cmd.clone()];
        expected.extend(weird.args.iter().cloned());
        assert_eq!(
            result.stdout,
            format!("{}\n", expected.join("\n")),
            "the remote argv must equal the spec's argv"
        );
        // the fake proves the point: this ran the stand-in, not real ssh
        assert!(fake.exists());
    }

    #[tokio::test]
    async fn env_and_workdir_travel_as_quoted_assignments_and_cd() {
        let _lock = ssh_test_lock();
        // the fake echoes every argument; the LAST is the command line
        let _fake = fake_ssh(None, r#"printf '%s\n' "$@""#);
        let ssh = Ssh::new();
        let mut s = spec("python3", &["train.py", "--data=x;y"]);
        s.env.insert("EPOCHS".into(), "10; rm -rf /".into());
        s.workdir = Some("/tmp/lab dir".into());
        let handle = ssh.submit(&s, &info(Some("gpu-01.lab"), &["gpu-01.lab"])).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let result = ssh.fetch(&handle).unwrap();
        // the ssh options, the host as ONE argv element, then the line
        let lines: Vec<&str> = result.stdout.trim_end_matches('\n').split('\n').collect();
        assert!(lines.contains(&"gpu-01.lab"), "the host is one argv element: {lines:?}");
        let line = lines.last().unwrap();
        assert!(line.contains("'EPOCHS'='10; rm -rf /'"), "env assignment fully quoted (key AND value) inert: {line}");
        assert!(line.contains("cd -- '/tmp/lab dir' &&"), "workdir quoted: {line}");
        assert!(line.contains("'--data=x;y'"), "args quoted inert: {line}");
    }

    // ---- env keys are names (review R-01): hostile keys never transfer ----

    #[tokio::test]
    async fn a_hostile_env_key_is_refused_at_the_adapter_boundary() {
        let _lock = ssh_test_lock();
        // The fake would leave a marker if the remote shell ever executed
        // anything beyond the intended assignment.
        let marker = std::env::temp_dir().join(format!("rc-ssh-pwned-{}", uuid::Uuid::new_v4()));
        let _fake = fake_ssh(
            Some(marker.to_str().unwrap()),
            "exit 0",
        );
        let ssh = Ssh::new();
        for hostile in ["X;touch /tmp/pwned", "KEY a b", "X$(id)", "X`id`"] {
            let mut s = spec("python3", &["train.py"]);
            s.env.insert(hostile.into(), "1".into());
            let err = ssh
                .submit(&s, &info(Some("gpu-01.lab"), &["gpu-01.lab"]))
                .unwrap_err();
            assert!(
                err.to_string().starts_with("invalid_spec: env key"),
                "{hostile:?}: unexpected: {err}"
            );
        }
        // and nothing ever connected — no marker, no process
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(
            !marker.exists(),
            "a hostile env key must never reach a remote shell"
        );
    }

    // ---- the lifecycle over the loopback harness ----

    #[tokio::test]
    async fn an_ssh_job_lives_its_lifecycle_with_captured_results() {
        let _lock = ssh_test_lock();
        let _fake = fake_ssh(
            None,
            "printf 'trained 3 epochs\\n'\nprintf 'warning: lr warmup\\n' >&2\nexit 0",
        );
        let ssh = Ssh::new();
        let handle = ssh
            .submit(&spec("python3", &["train.py"]), &info(Some("gpu-01.lab"), &["gpu-01.lab"]))
            .unwrap();
        assert_eq!(ssh.monitor(&handle).unwrap(), TargetJobStatus::Running);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert_eq!(ssh.monitor(&handle).unwrap(), TargetJobStatus::Finished { code: 0 });
        let result = ssh.fetch(&handle).unwrap();
        assert_eq!(result.code, Some(0));
        assert_eq!(result.stdout.trim(), "trained 3 epochs");
        assert_eq!(result.stderr.trim(), "warning: lr warmup");
    }

    #[tokio::test]
    async fn an_ssh_connection_failure_is_a_reasoned_terminal() {
        let _lock = ssh_test_lock();
        // the fake plays ssh's connection-refused behavior: exit 255, the
        // reason on stderr — exactly what a real unreachable host does
        let _fake = fake_ssh(
            None,
            "printf 'ssh: connect to host gpu-01.lab port 22: Connection refused\\n' >&2\nexit 255",
        );
        let ssh = Ssh::new();
        let handle = ssh
            .submit(&spec("python3", &["train.py"]), &info(Some("gpu-01.lab"), &["gpu-01.lab"]))
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert_eq!(
            ssh.monitor(&handle).unwrap(),
            TargetJobStatus::Failed { reason: "exit_code_255".into(), code: Some(255) }
        );
        let result = ssh.fetch(&handle).unwrap();
        assert!(result.stderr.contains("Connection refused"), "the failure is visible: {}", result.stderr);
    }

    #[tokio::test]
    async fn a_freeform_spec_is_refused_at_the_adapter_boundary() {
        let _lock = ssh_test_lock();
        let _fake = fake_ssh(None, "exit 0");
        let ssh = Ssh::new();
        let err = ssh
            .submit(&spec("ls | rm -rf .", &[]), &info(Some("gpu-01.lab"), &["gpu-01.lab"]))
            .unwrap_err();
        assert!(err.to_string().starts_with("freeform_shell:"), "unexpected: {err}");
    }
}
