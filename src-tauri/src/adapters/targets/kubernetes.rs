// The Kubernetes compute-target adapter (Story 6.3, FR-22.2, AD-6):
// allowlisted cluster contexts running the SAME structured job specs as
// containers — the same trust rules, no freeform manifests.
//
// SPEC → MANIFEST (the never-freeform guarantee): `submit` GENERATES the
// Job manifest from the validated spec's fields — `cmd` → the container
// command (already metachar-free, JobSpec::validate re-run at this
// boundary), `args` → the container args, `env` → env entries,
// `resources` → container limits (cpus → `cpu`, memoryMb → `memory` in
// Mi), `workdir` → `workingDir`. The mapping is the pure function
// `job_manifest` (unit-tested); there is no path that accepts a manifest
// from outside. Values ride as JSON-quoted YAML flow scalars
// (`serde_json::to_string` — a JSON string IS a YAML 1.2 double-quoted
// scalar), so metacharacters and newlines in `args` stay DATA in the
// manifest exactly as they are in the spec.
//
// CLIENT CHOICE (documented, the pragmatic path): the system `kubectl`
// spawned ARGV DIRECTLY — no kube client crate, no constructed shell
// string. The kubectl CLI is the one Kubernetes dependency every
// cluster operator already has; a heavy in-process client would add a
// dependency surface this adapter does not need for submit/monitor/
// fetch. `kubectlPrefix` names the binary per target (wrappers, full
// paths — and the test seam).
//
// THE GATE: a kubernetes target's CONTEXT (config `context`) must be on
// the workspace allowlist — refused at the command layer AND here,
// before any connection is attempted (the ssh host rule, applied to
// contexts; typed `context_not_allowed:`).
//
// MONITOR: `kubectl get job <name> -o json`, parsed for
// status.conditions — Complete → finished, Failed → failed (reasoned).
// A deleted Job is the typed `unknown_job:` (the read model keeps its
// last observed state honestly); an unreachable cluster is the typed
// `kube_unreachable:` — never a crash, never a lie.
//
// FETCH: `kubectl logs job/<name>` — the container's captured stdout/
// stderr. v0.2.0 limits, honestly: a failed job's container exit code
// is not surfaced (the condition's reason is); per-target autonomy and
// `target.spend_recorded` apply unchanged (the command layer's generic
// paths).

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::domain::jobs::{validate_host, JobSpec, SpecError};

use super::{
    job_table, observe, read_result, run_sync, track_as, ComputeTarget, JobHandle, JobResult,
    JobSlot, SyncRun, TargetError, TargetInfo, TargetJobStatus,
};

/// How long a kubectl poll may run before the adapter gives up on it —
/// a stuck kubectl is a typed unreachable, never a hung job row.
const POLL_TIMEOUT_SECS: u64 = 15;

/// The per-target config, parsed and re-validated at the adapter
/// boundary (defense in depth behind the `target.declared` validation).
#[derive(Debug, Clone, PartialEq, Eq)]
struct KubeConfig {
    /// The kube context the target runs on (required — the allowlist's
    /// gate value).
    context: String,
    /// The namespace jobs land in (default `default`).
    namespace: String,
    /// The container image the generated Job manifest runs (required).
    image: String,
    /// The kubectl binary (config `kubectlPrefix`, default `kubectl`).
    kubectl: String,
}

fn parse_config(target: &TargetInfo) -> Result<KubeConfig, TargetError> {
    let expectation = |key: &str, what: &str| TargetError::MissingConfig {
        target: target.name.clone(),
        key: key.into(),
        expectation: what.into(),
    };
    let context = target
        .cfg("context")
        .ok_or_else(|| expectation("context", "the cluster context the target runs on"))?
        .to_string();
    validate_host(&context).map_err(SpecError::InvalidHost)?;
    let image = target
        .cfg("image")
        .ok_or_else(|| expectation("image", "the container image its jobs run in"))?
        .to_string();
    Ok(KubeConfig {
        namespace: target.cfg("namespace").unwrap_or("default").to_string(),
        image,
        kubectl: target.cfg("kubectlPrefix").unwrap_or("kubectl").to_string(),
        context,
    })
}

// ---------------------------------------------------------------------------
// Spec → generated Job manifest (the never-freeform mapping)
// ---------------------------------------------------------------------------

/// One YAML flow scalar: the JSON encoding of the string (a JSON string
/// is a YAML 1.2 double-quoted scalar — escapes and all, so every byte
/// of an arg or env value rides as data).
fn yaml_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Generate the Job manifest from the validated spec's fields. `tag` is
/// the handle's short id — the Job's name derives from it, so monitor
/// and fetch know exactly which Job is theirs.
fn job_manifest(spec: &JobSpec, tag: &str, namespace: &str, image: &str) -> String {
    let mut manifest = String::new();
    manifest.push_str("apiVersion: batch/v1\n");
    manifest.push_str("kind: Job\n");
    manifest.push_str("metadata:\n");
    manifest.push_str(&format!("  name: {}\n", yaml_string(&format!("rc-job-{tag}"))));
    manifest.push_str(&format!("  namespace: {}\n", yaml_string(namespace)));
    manifest.push_str("spec:\n");
    // One attempt: a failed container is a failed job (the lifecycle
    // carries the reason) — never an invisible retry storm.
    manifest.push_str("  backoffLimit: 0\n");
    manifest.push_str("  template:\n");
    manifest.push_str("    spec:\n");
    manifest.push_str("      restartPolicy: Never\n");
    manifest.push_str("      containers:\n");
    manifest.push_str(&format!("        - name: {}\n", yaml_string("rc")));
    manifest.push_str(&format!("          image: {}\n", yaml_string(image)));
    manifest.push_str(&format!("          command: [{}]\n", yaml_string(&spec.cmd)));
    manifest.push_str(&format!(
        "          args: [{}]\n",
        spec.args.iter().map(|a| yaml_string(a)).collect::<Vec<_>>().join(", ")
    ));
    if !spec.env.is_empty() {
        manifest.push_str("          env:\n");
        for (key, value) in &spec.env {
            manifest.push_str(&format!("            - name: {}\n", yaml_string(key)));
            manifest.push_str(&format!("              value: {}\n", yaml_string(value)));
        }
    }
    if let Some(dir) = spec.workdir.as_deref() {
        manifest.push_str(&format!("          workingDir: {}\n", yaml_string(dir)));
    }
    if let Some(resources) = spec.resources {
        manifest.push_str("          resources:\n            limits:\n");
        if let Some(cpus) = resources.cpus {
            manifest.push_str(&format!("              cpu: {cpus}\n"));
        }
        if let Some(mem) = resources.memory_mb {
            manifest.push_str(&format!("              memory: {mem}Mi\n"));
        }
    }
    manifest
}

/// The Job's name for one handle's tag (a valid DNS-1123 label: the
/// handle's hex short id).
fn job_name(tag: &str) -> String {
    format!("rc-job-{tag}")
}

// ---------------------------------------------------------------------------
// The kubectl transport: argv directly
// ---------------------------------------------------------------------------

fn kubectl_argv(cfg: &KubeConfig, verbs: &[&str]) -> Vec<String> {
    let mut argv = vec![
        cfg.kubectl.clone(),
        "--context".into(),
        cfg.context.clone(),
        "-n".into(),
        cfg.namespace.clone(),
    ];
    argv.extend(verbs.iter().map(|v| v.to_string()));
    argv
}

/// The typed unreachable: the context plus kubectl's own words.
fn kube_unreachable(cfg: &KubeConfig, detail: &str) -> TargetError {
    let detail = detail
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("no output")
        .trim();
    TargetError::KubeUnreachable {
        context: cfg.context.clone(),
        detail: if detail.is_empty() { "no output".into() } else { detail.to_string() },
    }
}

/// What a poll resolved to.
enum JobState {
    Alive,
    Terminal { code: Option<i32>, reason: Option<String> },
}

/// Poll the Job: its conditions tell the story.
fn poll_job(cfg: &KubeConfig, name: &str) -> Result<JobState, TargetError> {
    let argv = kubectl_argv(cfg, &["get", "job", name, "-o", "json"]);
    match run_sync(&argv, Duration::from_secs(POLL_TIMEOUT_SECS)) {
        Ok(SyncRun::Done(out)) if out.status.success() => {
            let json: serde_json::Value = match serde_json::from_slice(&out.stdout) {
                Ok(json) => json,
                Err(e) => return Err(kube_unreachable(cfg, &format!("unparsable job status: {e}"))),
            };
            let status = &json["status"];
            if let Some(conditions) = status["conditions"].as_array() {
                for condition in conditions {
                    let kind = condition["type"].as_str().unwrap_or_default();
                    let state = condition["status"].as_str().unwrap_or_default();
                    if state != "True" {
                        continue;
                    }
                    return Ok(match kind {
                        "Complete" => JobState::Terminal { code: Some(0), reason: None },
                        "Failed" => JobState::Terminal {
                            code: None,
                            reason: Some("job_failed".into()),
                        },
                        _ => continue,
                    });
                }
            }
            // No terminal condition — a failed count still means failed
            // (conditions can lag the counters).
            if status["failed"].as_i64().unwrap_or(0) > 0 {
                return Ok(JobState::Terminal { code: None, reason: Some("job_failed".into()) });
            }
            Ok(JobState::Alive)
        }
        Ok(SyncRun::Done(out)) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stderr.contains("NotFound") {
                // the Job is gone (deleted, namespace cleaned) — the read
                // model keeps its last observed state honestly
                return Err(TargetError::UnknownJob(name.to_string()));
            }
            Err(kube_unreachable(cfg, &stderr))
        }
        Ok(SyncRun::Timeout) => Err(kube_unreachable(cfg, "poll timed out")),
        Err(e) => Err(kube_unreachable(cfg, &e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// The adapter
// ---------------------------------------------------------------------------

/// The allowlist gate — BEFORE any connection is attempted (defense in
/// depth behind the command layer's own check): a kubernetes target's
/// context must be on the workspace allowlist.
fn gate_allowlist(target: &TargetInfo, context: &str) -> Result<(), TargetError> {
    if !target.allowlist.iter().any(|allowed| allowed == context) {
        return Err(TargetError::ContextNotAllowed {
            context: context.to_string(),
            known: if target.allowlist.is_empty() {
                "empty — add contexts in Settings → Compute targets".to_string()
            } else {
                target.allowlist.join(" | ")
            },
        });
    }
    Ok(())
}

/// The submit-side tracked processes (the shared reaper machinery — the
/// `kubectl apply` runs async; its captured output carries the result).
fn submit_jobs() -> &'static Mutex<HashMap<String, JobSlot>> {
    static JOBS: OnceLock<Mutex<HashMap<String, JobSlot>>> = OnceLock::new();
    job_table(&JOBS)
}

/// What one kubernetes job is right now, from the adapter's side.
#[derive(Debug)]
enum Phase {
    /// The `kubectl apply` is still running.
    Submitting,
    /// The Job exists on the cluster — its name, and the terminal facts
    /// once a poll resolved them (cached: a terminal is final).
    Live { terminal: Option<Terminal> },
    /// The apply itself failed — the captured kubectl output is the
    /// result (there is no Job to ask).
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

/// One live kubernetes job: its config, its tag, its phase. Shared by
/// Arc so monitor/fetch mutate THE stored entry. Process-global like
/// every adapter table: a restart loses them (the log's read model
/// keeps its last observed state honestly).
struct Entry {
    cfg: KubeConfig,
    tag: String,
    phase: Mutex<Phase>,
}

/// Poison-tolerant lock (a panicking holder never cascades).
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn entries() -> &'static Mutex<HashMap<String, Arc<Entry>>> {
    static ENTRIES: OnceLock<Mutex<HashMap<String, Arc<Entry>>>> = OnceLock::new();
    ENTRIES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lookup_entry(handle: &JobHandle) -> Result<Arc<Entry>, TargetError> {
    lock(entries())
        .get(&handle.id)
        .cloned()
        .ok_or_else(|| TargetError::UnknownJob(handle.id.clone()))
}

/// Write the generated manifest to a private temp file — 0600.
fn write_manifest(manifest: &str, tag: &str) -> std::io::Result<std::path::PathBuf> {
    let path = std::env::temp_dir().join(format!("rc-kube-{tag}.yaml"));
    let mut file = std::fs::File::create(&path)?;
    file.write_all(manifest.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(path)
}

fn tokio_command(argv: &[String]) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(&argv[0]);
    command.args(&argv[1..]);
    command
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

/// The Kubernetes compute-target adapter (Story 6.3): generated Job
/// manifests, argv-direct kubectl, monitored through the Job's
/// conditions.
pub struct Kubernetes;

impl Kubernetes {
    /// The adapter the registry registers (Story 3.2's seam).
    pub fn new() -> Self {
        Kubernetes
    }

    /// The reachability probe (the settings row's discovery state): a
    /// cheap `kubectl --context C cluster-info` — context-gated before
    /// any connection, typed unreachable when the cluster does not
    /// answer.
    pub fn probe(&self, target: &TargetInfo) -> Result<String, TargetError> {
        let cfg = parse_config(target)?;
        gate_allowlist(target, &cfg.context)?;
        let argv = kubectl_argv(&cfg, &["cluster-info"]);
        match run_sync(&argv, Duration::from_secs(POLL_TIMEOUT_SECS)) {
            Ok(SyncRun::Done(out)) if out.status.success() => {
                Ok(format!("cluster answered (context {})", cfg.context))
            }
            Ok(SyncRun::Done(out)) => {
                Err(kube_unreachable(&cfg, &String::from_utf8_lossy(&out.stderr)))
            }
            Ok(SyncRun::Timeout) => Err(kube_unreachable(&cfg, "probe timed out")),
            Err(e) => Err(kube_unreachable(&cfg, &e.to_string())),
        }
    }
}

impl Default for Kubernetes {
    fn default() -> Self {
        Self::new()
    }
}

impl ComputeTarget for Kubernetes {
    fn kind(&self) -> &'static str {
        "kubernetes"
    }

    fn submit(&self, spec: &JobSpec, target: &TargetInfo) -> Result<JobHandle, TargetError> {
        // The adapter boundary validates again (AD-6): no freeform spec
        // ever becomes a manifest.
        spec.validate()?;
        let cfg = parse_config(target)?;
        // The context gate — BEFORE any connection is attempted. Defense
        // in depth: the command layer already refused contexts outside
        // the allowlist.
        gate_allowlist(target, &cfg.context)?;
        let handle_id = uuid::Uuid::new_v4().to_string();
        let tag = handle_id[..8].to_string();
        let manifest = job_manifest(spec, &tag, &cfg.namespace, &cfg.image);
        let path = write_manifest(&manifest, &tag)
            .map_err(|e| TargetError::Spawn(format!("manifest write failed: {e}")))?;
        // `kubectl apply -f <file>`: argv-direct, the manifest file as
        // ONE argv element (no shell, no stdin pipe, no freeform string).
        let argv = kubectl_argv(&cfg, &["apply", "-f", &path.to_string_lossy()]);
        let handle = track_as(submit_jobs(), tokio_command(&argv), handle_id);
        lock(entries()).insert(handle.id.clone(), Arc::new(Entry {
            cfg,
            tag,
            phase: Mutex::new(Phase::Submitting),
        }));
        Ok(handle)
    }

    fn monitor(&self, handle: &JobHandle) -> Result<TargetJobStatus, TargetError> {
        let entry = lookup_entry(handle)?;
        // What this monitor owes: decided under a short lock, acted on
        // outside it (a poll is a bounded sync run — no lock held).
        enum Due {
            PollJob,
            CheckSubmit,
        }
        let due = {
            let phase = lock(&entry.phase);
            match &*phase {
                Phase::SubmitFailed { code, reason, .. } => {
                    return Ok(TargetJobStatus::Failed { reason: reason.clone(), code: *code })
                }
                Phase::Live { terminal } => match terminal {
                    Some(terminal) => return Ok(terminal.status()),
                    None => Due::PollJob,
                },
                Phase::Submitting => Due::CheckSubmit,
            }
        };
        match due {
            Due::PollJob => {
                let state = poll_job(&entry.cfg, &job_name(&entry.tag))?;
                let mut phase = lock(&entry.phase);
                let Phase::Live { terminal } = &mut *phase else {
                    // the phase moved under us — the next monitor tells the story
                    return Ok(TargetJobStatus::Running);
                };
                match state {
                    JobState::Alive => Ok(TargetJobStatus::Running),
                    JobState::Terminal { code, reason } => {
                        let terminal_value = Terminal { code, reason };
                        let status = terminal_value.status();
                        *terminal = Some(terminal_value);
                        Ok(status)
                    }
                }
            }
            Due::CheckSubmit => {
                // The shared machinery observes the apply; its exit
                // carries the answer.
                match observe(submit_jobs(), handle)? {
                    TargetJobStatus::Running => Ok(TargetJobStatus::Running),
                    TargetJobStatus::Finished { .. } => {
                        let result = read_result(submit_jobs(), handle)?;
                        let mut phase = lock(&entry.phase);
                        // `kubectl apply` answers "job.batch/<name>
                        // created" (or "configured") — the name is ours,
                        // so success is the Job existing.
                        *phase = Phase::Live { terminal: None };
                        Ok(TargetJobStatus::Running)
                    }
                    TargetJobStatus::Failed { reason, code } => {
                        let result = read_result(submit_jobs(), handle)?;
                        let reason = format!("submit_failed: {reason}");
                        let mut phase = lock(&entry.phase);
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
        let phase = lock(&entry.phase);
        match &*phase {
            Phase::Submitting => Err(TargetError::NotTerminal(handle.id.clone())),
            Phase::SubmitFailed { code, stdout, stderr, .. } => Ok(JobResult {
                code: *code,
                stdout: stdout.clone(),
                stderr: stderr.clone(),
            }),
            Phase::Live { terminal } => {
                let Some(terminal) = terminal else {
                    return Err(TargetError::NotTerminal(handle.id.clone()));
                };
                let name = job_name(&entry.tag);
                let argv = kubectl_argv(&entry.cfg, &["logs", &format!("job/{name}")]);
                let (stdout, stderr) = match run_sync(&argv, Duration::from_secs(POLL_TIMEOUT_SECS)) {
                    Ok(SyncRun::Done(out)) => (
                        String::from_utf8_lossy(&out.stdout).into_owned(),
                        String::from_utf8_lossy(&out.stderr).into_owned(),
                    ),
                    Ok(SyncRun::Timeout) => (
                        String::new(),
                        "fetch timed out reading the job logs".into(),
                    ),
                    Err(e) => (String::new(), format!("fetch failed reading the job logs: {e}")),
                };
                Ok(JobResult { code: terminal.code, stdout, stderr })
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

    fn info(config: &[(&str, &str)]) -> TargetInfo {
        TargetInfo {
            name: "cluster-1".into(),
            host: None,
            allowlist: config
                .iter()
                .filter(|(k, _)| *k == "context")
                .map(|(_, v)| v.to_string())
                .collect(),
            config: config
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    /// The fake-kubectl harness: a stand-in dispatching on the verb,
    /// with the job status served from a state file the test rewrites
    /// between polls, the logs from a log file, and every applied
    /// manifest captured for inspection.
    struct FakeKubectl {
        kubectl: String,
        state: std::path::PathBuf,
        logs: std::path::PathBuf,
        applied: std::path::PathBuf,
    }

    fn fake_kubectl() -> FakeKubectl {
        let dir = std::env::temp_dir().join(format!("rc-kube-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (state, logs, applied) = (dir.join("state"), dir.join("logs"), dir.join("applied"));
        let kubectl = dir.join("kubectl");
        std::fs::write(
            &kubectl,
            format!(
                "#!/bin/sh\ncase \"$*\" in\n  *\" apply \"*) cp \"${{7}}\" {} ;;\n  *\" get \"*) cat {} ;;\n  *\" logs \"*) cat {} ;;\n  *) exit 0 ;;\nesac\n",
                crate::adapters::targets::shell_quote(applied.to_str().unwrap()),
                crate::adapters::targets::shell_quote(state.to_str().unwrap()),
                crate::adapters::targets::shell_quote(logs.to_str().unwrap()),
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&kubectl, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        FakeKubectl {
            kubectl: kubectl.to_string_lossy().into_owned(),
            state,
            logs,
            applied,
        }
    }

    fn kube_config(kubectl: &str) -> Vec<(&'static str, String)> {
        vec![
            ("context", "lab-gpu".to_string()),
            ("namespace", "research".to_string()),
            ("image", "ghcr.io/lab/trainer:latest".to_string()),
            ("kubectlPrefix", kubectl.to_string()),
        ]
    }

    fn info_with(config: &[(&str, String)]) -> TargetInfo {
        let mut target = info(&[]);
        for (k, v) in config {
            target.config.insert(k.to_string(), v.clone());
        }
        target.allowlist = vec!["lab-gpu".into()];
        target
    }

    // ---- spec → manifest (the never-freeform mapping) ----

    #[test]
    fn the_manifest_maps_from_the_spec_fields() {
        let mut s = spec("python3", &["train.py", "--note=a | b && c"]);
        s.env.insert("EPOCHS".into(), "10; rm -rf /".into());
        s.resources = Some(crate::domain::jobs::JobResources {
            cpus: Some(4),
            memory_mb: Some(8192),
        });
        s.workdir = Some("/lab/exp".into());
        let manifest = job_manifest(&s, "abcd1234", "research", "ghcr.io/lab/trainer:latest");
        assert!(manifest.contains("kind: Job"));
        assert!(manifest.contains("name: \"rc-job-abcd1234\""));
        assert!(manifest.contains("namespace: \"research\""));
        assert!(manifest.contains("backoffLimit: 0"));
        assert!(manifest.contains("restartPolicy: Never"));
        assert!(manifest.contains("image: \"ghcr.io/lab/trainer:latest\""));
        assert!(manifest.contains("command: [\"python3\"]"));
        // args with metacharacters ride as DATA (JSON-quoted YAML)
        assert!(
            manifest.contains("args: [\"train.py\", \"--note=a | b && c\"]"),
            "{manifest}"
        );
        assert!(manifest.contains("- name: \"EPOCHS\"\n              value: \"10; rm -rf /\""));
        assert!(manifest.contains("workingDir: \"/lab/exp\""));
        assert!(manifest.contains("cpu: 4"));
        assert!(manifest.contains("memory: 8192Mi"));
        // a newline in an arg is data, escaped — never manifest syntax
        let weird = spec("python3", &["a\nb"]);
        let manifest = job_manifest(&weird, "abcd1234", "research", "img");
        assert!(manifest.contains("\"a\\nb\""), "{manifest}");
    }

    // ---- the gates: freeform, config, context allowlist ----

    #[tokio::test]
    async fn a_freeform_spec_is_refused_before_any_manifest() {
        let kube = Kubernetes::new();
        let err = kube
            .submit(&spec("ls | rm -rf .", &[]), &info_with(&kube_config("kubectl")))
            .unwrap_err();
        assert!(err.to_string().starts_with("freeform_shell:"), "unexpected: {err}");
        // missing context / image are typed refusals
        let err = kube.submit(&spec("python3", &[]), &info(&[])).unwrap_err();
        assert!(err.to_string().starts_with("missing_config:"), "unexpected: {err}");
        assert!(err.to_string().contains("context"), "unexpected: {err}");
        let err = kube
            .submit(
                &spec("python3", &[]),
                &info_with(&[("context", "lab-gpu".into())]),
            )
            .unwrap_err();
        assert!(err.to_string().starts_with("missing_config:"), "unexpected: {err}");
        assert!(err.to_string().contains("image"), "unexpected: {err}");
    }

    #[tokio::test]
    async fn a_context_outside_the_allowlist_is_refused_before_any_connection() {
        let kube = Kubernetes::new();
        let fake = fake_kubectl();
        let mut config = kube_config(&fake.kubectl);
        config[0] = ("context", "other-cluster".to_string());
        let err = kube
            .submit(&spec("python3", &[]), &info_with(&config))
            .unwrap_err();
        assert!(
            err.to_string().starts_with("context_not_allowed:"),
            "unexpected: {err}"
        );
        assert!(err.to_string().contains("other-cluster"), "the error names the context: {err}");
        assert!(err.to_string().contains("lab-gpu"), "the error names the allowlist: {err}");
        assert!(
            !fake.applied.exists(),
            "the refusal predates any connection (no manifest was applied)"
        );
    }

    // ---- the lifecycle over the fake-kubectl harness ----

    /// Drive the phase machine past its apply transition (bounded).
    async fn until_live(kube: &Kubernetes, handle: &JobHandle) -> Result<TargetJobStatus, TargetError> {
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            match kube.monitor(handle) {
                Err(e) => return Err(e),
                Ok(status @ (TargetJobStatus::Finished { .. } | TargetJobStatus::Failed { .. })) => {
                    return Ok(status)
                }
                Ok(TargetJobStatus::Running) => {
                    let map = lock(entries());
                    if map.get(&handle.id)
                        .map(|e| matches!(&*lock(&e.phase), Phase::Live { .. }))
                        .unwrap_or(false)
                    {
                        return Ok(TargetJobStatus::Running);
                    }
                }
            }
        }
        panic!("the kubernetes job never settled past its apply transition");
    }

    #[tokio::test]
    async fn a_kube_job_lives_its_lifecycle_with_applied_manifest_and_logs() {
        let fake = fake_kubectl();
        std::fs::write(&fake.state, "{}\n").unwrap(); // no conditions yet — alive
        std::fs::write(&fake.logs, "trained 3 epochs\n").unwrap();
        let kube = Kubernetes::new();
        let mut s = spec("python3", &["train.py", "--epochs=3"]);
        s.env.insert("EPOCHS".into(), "3".into());
        s.resources = Some(crate::domain::jobs::JobResources {
            cpus: Some(2),
            memory_mb: None,
        });
        let handle = kube
            .submit(&s, &info_with(&kube_config(&fake.kubectl)))
            .unwrap();
        // the apply transition lands, the Job is alive
        assert_eq!(
            until_live(&kube, &handle).await.unwrap(),
            TargetJobStatus::Running
        );
        // the applied manifest is EXACTLY the generated one (no freeform
        // path — the file the adapter wrote is the file kubectl got)
        let applied = std::fs::read_to_string(&fake.applied).unwrap();
        assert_eq!(
            applied,
            job_manifest(&s, &handle.id[..8], "research", "ghcr.io/lab/trainer:latest")
        );
        // fetch refuses while alive — typed, never a partial read
        let err = kube.fetch(&handle).unwrap_err();
        assert!(err.to_string().starts_with("job_not_terminal:"), "unexpected: {err}");
        // the Job completes: the conditions say so
        std::fs::write(
            &fake.state,
            r#"{"status": {"conditions": [{"type": "Complete", "status": "True"}], "succeeded": 1}}"#,
        )
        .unwrap();
        assert_eq!(
            kube.monitor(&handle).unwrap(),
            TargetJobStatus::Finished { code: 0 }
        );
        let result = kube.fetch(&handle).unwrap();
        assert_eq!(result.code, Some(0));
        assert_eq!(result.stdout.trim(), "trained 3 epochs");
        // the terminal is cached — a second monitor agrees without repolling
        std::fs::write(&fake.state, "garbage\n").unwrap();
        assert_eq!(
            kube.monitor(&handle).unwrap(),
            TargetJobStatus::Finished { code: 0 }
        );
    }

    #[tokio::test]
    async fn a_failed_job_condition_is_a_reasoned_terminal() {
        let fake = fake_kubectl();
        std::fs::write(
            &fake.state,
            r#"{"status": {"conditions": [{"type": "Failed", "status": "True"}], "failed": 1}}"#,
        )
        .unwrap();
        let kube = Kubernetes::new();
        let handle = kube
            .submit(&spec("python3", &[]), &info_with(&kube_config(&fake.kubectl)))
            .unwrap();
        until_live(&kube, &handle).await.unwrap();
        assert_eq!(
            kube.monitor(&handle).unwrap(),
            TargetJobStatus::Failed { reason: "job_failed".into(), code: None }
        );
    }

    #[tokio::test]
    async fn an_unreachable_cluster_and_a_deleted_job_are_typed() {
        let fake = fake_kubectl();
        std::fs::write(&fake.state, "{}\n").unwrap();
        let kube = Kubernetes::new();
        let handle = kube
            .submit(&spec("python3", &[]), &info_with(&kube_config(&fake.kubectl)))
            .unwrap();
        until_live(&kube, &handle).await.unwrap();
        // the fake plays an unreachable cluster
        std::fs::write(
            std::path::PathBuf::from(&fake.kubectl),
            "#!/bin/sh\nprintf 'Unable to connect to the server: dial tcp: connection refused\\n' >&2\nexit 1\n",
        )
        .unwrap();
        let err = kube.monitor(&handle).unwrap_err();
        assert!(err.to_string().starts_with("kube_unreachable:"), "unexpected: {err}");
        assert!(err.to_string().contains("lab-gpu"), "the error names the context: {err}");
        // the probe agrees — the settings row's discovery state
        let err = kube
            .probe(&info_with(&kube_config(&fake.kubectl)))
            .unwrap_err();
        assert!(err.to_string().starts_with("kube_unreachable:"), "unexpected: {err}");
        // a deleted Job is the typed unknown (honest last state)
        std::fs::write(
            std::path::PathBuf::from(&fake.kubectl),
            "#!/bin/sh\nprintf 'Error from server (NotFound): jobs.batch \"x\" not found\\n' >&2\nexit 1\n",
        )
        .unwrap();
        let err = kube.monitor(&handle).unwrap_err();
        assert!(err.to_string().starts_with("unknown_job:"), "unexpected: {err}");
    }

    #[tokio::test]
    async fn a_failed_apply_is_a_reasoned_terminal_with_the_captured_output() {
        let fake = fake_kubectl();
        std::fs::write(
            std::path::PathBuf::from(&fake.kubectl),
            "#!/bin/sh\nprintf 'error: no context exists with the name: \"lab-gpu\"\\n' >&2\nexit 1\n",
        )
        .unwrap();
        let kube = Kubernetes::new();
        let handle = kube
            .submit(&spec("python3", &[]), &info_with(&kube_config(&fake.kubectl)))
            .unwrap();
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if let TargetJobStatus::Failed { .. } = kube.monitor(&handle).unwrap() {
                break;
            }
        }
        match kube.monitor(&handle).unwrap() {
            TargetJobStatus::Failed { reason, code } => {
                assert!(reason.starts_with("submit_failed:"), "unexpected: {reason}");
                assert_eq!(code, Some(1));
            }
            other => panic!("a failed apply is an observed failure, got {other:?}"),
        }
        let result = kube.fetch(&handle).unwrap();
        assert!(result.stderr.contains("no context exists"), "{}", result.stderr);
    }

    #[tokio::test]
    async fn an_unknown_handle_is_a_typed_error() {
        let kube = Kubernetes::new();
        let err = kube.monitor(&JobHandle::new("never-submitted")).unwrap_err();
        assert!(err.to_string().starts_with("unknown_job:"), "unexpected: {err}");
        let err = kube.fetch(&JobHandle::new("never-submitted")).unwrap_err();
        assert!(err.to_string().starts_with("unknown_job:"), "unexpected: {err}");
    }
}
