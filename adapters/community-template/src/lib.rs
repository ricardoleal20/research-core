//! The community adapter template (Story 6.5, FR-22.4, NFR-15): the
//! reference implementation of ResearchCore's versioned compute-target
//! adapter contract — a complete, working adapter in ~150 lines with no
//! access to ResearchCore internals beyond the PUBLIC contract
//! (`research_core_lib::adapters::targets`), proving the contract is
//! self-sufficient.
//!
//! What it does: a `sleep-template` target runs any validated spec's
//! argv directly on this machine with its OWN process tracking (std
//! `process` — the template deliberately does not lean on
//! ResearchCore's internal tracking plumbing, because community
//! adapters cannot: it is not part of the contract).
//!
//! The contract rules this file demonstrates (docs/adapters/contract.md):
//!
//! 1. STRUCTURED SPECS ONLY — `submit` re-validates the spec at the
//!    boundary (`spec.validate()?`); there is no freeform input.
//! 2. NEVER A SHELL STRING — the process spawns argv-direct
//!    (`Command::new(cmd).args(args)`), no `sh -c`, ever.
//! 3. HONEST STATES — spawn failures are job states (reasoned
//!    terminals), unknown handles are typed errors, fetch refuses
//!    until terminal.
//! 4. REGISTRATION IS VALIDATED — `install()` goes through
//!    `register_community`, which enforces the kind slug, the contract
//!    version, and the no-shadowing rule.
//!
//! To turn this into YOUR adapter: copy the crate, rename, implement
//! `submit`/`monitor`/`fetch` for your platform, keep the four rules,
//! and call `install()` from your app startup.

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use research_core_lib::adapters::targets::adapter_contract::register_community;
use research_core_lib::adapters::targets::ADAPTER_CONTRACT_VERSION;
use research_core_lib::adapters::targets::{
    ComputeTarget, JobHandle, JobResult, TargetError, TargetInfo, TargetJobStatus,
};
use research_core_lib::domain::jobs::JobSpec;

/// The adapter's kind — the slug `target.declared` events name and the
/// registry resolves. One kind, spelled the same everywhere.
pub const KIND: &str = "sleep-template";

/// Install the adapter into the running app (call ONCE at startup —
/// e.g. from your `main()` before the Tauri app builds). Validation is
/// the registry's: a malformed kind, a wrong contract version, a
/// first-party kind, or a duplicate is refused with a specific reason.
pub fn install() -> Result<(), String> {
    register_community(KIND, ADAPTER_CONTRACT_VERSION, Arc::new(SleepTarget))
}

/// The template adapter: argv-direct execution with its own tracking.
pub struct SleepTarget;

// ---------------------------------------------------------------------------
// The template's own process tracking (NOT part of the contract — every
// community adapter brings its own; this is the simplest correct one)
// ---------------------------------------------------------------------------

struct Exit {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    spawn_error: Option<String>,
}

struct Tracked {
    child: Option<Child>,
    exit: Option<Exit>,
}

fn jobs() -> &'static Mutex<HashMap<String, Arc<Mutex<Tracked>>>> {
    static JOBS: OnceLock<Mutex<HashMap<String, Arc<Mutex<Tracked>>>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lookup(handle: &JobHandle) -> Result<Arc<Mutex<Tracked>>, TargetError> {
    jobs().lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&handle.id)
        .cloned()
        .ok_or_else(|| TargetError::UnknownJob(handle.id.clone()))
}

fn classify(exit: &Exit) -> TargetJobStatus {
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

impl ComputeTarget for SleepTarget {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn submit(&self, spec: &JobSpec, _target: &TargetInfo) -> Result<JobHandle, TargetError> {
        // CONTRACT RULE 1: the boundary re-validates — nothing executes
        // that did not pass the never-freeform-shell gate.
        spec.validate()?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let handle = JobHandle::new(format!(
            "{KIND}-{}",
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // CONTRACT RULE 2: argv-direct — the spec's cmd with its args
        // as separate argv elements; no shell, no joined string.
        let child = Command::new(&spec.cmd)
            .args(&spec.args)
            .envs(&spec.env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let tracked = match child {
            Ok(child) => Tracked { child: Some(child), exit: None },
            // CONTRACT RULE 3: a spawn failure is a job STATE, not a
            // submit error — the handle exists, monitor observes the
            // reasoned terminal.
            Err(e) => Tracked {
                child: None,
                exit: Some(Exit {
                    code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    spawn_error: Some(e.to_string()),
                }),
            },
        };
        jobs().lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(handle.id.clone(), Arc::new(Mutex::new(tracked)));
        Ok(handle)
    }

    fn monitor(&self, handle: &JobHandle) -> Result<TargetJobStatus, TargetError> {
        let entry = lookup(handle)?;
        let mut tracked = entry.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(exit) = &tracked.exit {
            // a terminal is final — cached, never re-derived
            return Ok(classify(exit));
        }
        let Some(child) = tracked.child.as_mut() else {
            return Ok(TargetJobStatus::Running);
        };
        match child.try_wait() {
            Ok(Some(_)) => {
                let child = tracked.child.take().expect("the child was just there");
                let exit = match child.wait_with_output() {
                    Ok(output) => Exit {
                        code: output.status.code(),
                        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                        spawn_error: None,
                    },
                    Err(e) => Exit {
                        code: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        spawn_error: Some(e.to_string()),
                    },
                };
                let status = classify(&exit);
                tracked.exit = Some(exit);
                Ok(status)
            }
            Ok(None) => Ok(TargetJobStatus::Running),
            Err(e) => {
                let exit = Exit {
                    code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    spawn_error: Some(e.to_string()),
                };
                let status = classify(&exit);
                tracked.exit = Some(exit);
                Ok(status)
            }
        }
    }

    fn fetch(&self, handle: &JobHandle) -> Result<JobResult, TargetError> {
        let entry = lookup(handle)?;
        let tracked = entry.lock().unwrap_or_else(|p| p.into_inner());
        let Some(exit) = &tracked.exit else {
            // CONTRACT RULE 3: fetch waits for the terminal — typed
            // refusal while running, never a partial read.
            return Err(TargetError::NotTerminal(handle.id.clone()));
        };
        Ok(JobResult {
            code: exit.code,
            stdout: exit.stdout.clone(),
            stderr: exit
                .spawn_error
                .clone()
                .unwrap_or_else(|| exit.stderr.clone()),
        })
    }
}
