//! The template's contract tests (Story 6.5, FR-22.4): a FAKE community
//! adapter implemented OUTSIDE src/ — it compiles against the public
//! contract and registers through the validated seam, exercising the
//! lifecycle, the never-freeform rule, and the registration refusals.
//! These run like any unit tests: `cargo test` in this crate.

use std::time::Duration;

use research_core_lib::adapters::targets::adapter_contract::register_community;
use research_core_lib::adapters::targets::TargetRegistry;
use research_core_lib::domain::jobs::JobSpec;
use research_core_lib::domain::jobs::SpecError;
use research_core_lib::adapters::targets::{ComputeTarget, TargetInfo, TargetJobStatus};
use sleep_template::{install, SleepTarget, KIND};

fn spec(cmd: &str, args: &[&str]) -> JobSpec {
    JobSpec {
        cmd: cmd.into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        env: Default::default(),
        resources: None,
        workdir: None,
    }
}

fn info() -> TargetInfo {
    TargetInfo {
        name: "template-demo".into(),
        host: None,
        allowlist: Vec::new(),
        config: Default::default(),
    }
}

/// install is idempotent-across-tests (registration is process-global):
/// the FIRST install wins, duplicates are the registry's refusal to
/// scream about.
fn ensure_installed() {
    let _ = install();
}

// ---- the fake community adapter registers and appears as first-party ----

#[test]
fn the_community_adapter_registers_and_resolves_through_the_registry() {
    ensure_installed();
    let registry = TargetRegistry::v1();
    assert!(
        registry.kinds().contains(&KIND),
        "the community kind appears in registration: {:?}",
        registry.kinds()
    );
    let adapter = registry.adapter(KIND).expect("it resolves by kind");
    assert_eq!(adapter.kind(), KIND);
    // a duplicate registration is refused with a specific reason
    let err = install().unwrap_err();
    assert!(err.starts_with("duplicate_kind:"), "unexpected: {err}");
}

#[test]
fn registration_refusals_name_their_reason() {
    // the reserved kinds, a wrong contract version, a bad slug
    let err = register_community("local", "1", std::sync::Arc::new(SleepTarget)).unwrap_err();
    assert!(err.starts_with("reserved_kind:"), "unexpected: {err}");
    let err = register_community("my-target", "0", std::sync::Arc::new(SleepTarget)).unwrap_err();
    assert!(err.starts_with("contract_version_mismatch:"), "unexpected: {err}");
    let err = register_community("Bad_Slug", "1", std::sync::Arc::new(SleepTarget)).unwrap_err();
    assert!(err.starts_with("invalid_kind:"), "unexpected: {err}");
}

// ---- the lifecycle through the adapter's own tracking ----

#[test]
fn a_job_lives_its_lifecycle_and_fetches_its_output() {
    let target = SleepTarget;
    let handle = target.submit(&spec("printf", &["hello from the template"]), &info()).unwrap();
    // bounded wait — a fixed sleep flakes under parallel test runs
    let terminal = loop {
        match target.monitor(&handle).unwrap() {
            TargetJobStatus::Running => std::thread::sleep(Duration::from_millis(50)),
            status => break status,
        }
    };
    assert_eq!(terminal, TargetJobStatus::Finished { code: 0 });
    let result = target.fetch(&handle).unwrap();
    assert_eq!(result.code, Some(0));
    assert_eq!(result.stdout.trim(), "hello from the template");
}

#[test]
fn a_failed_job_is_reasoned_and_unknown_handles_are_typed() {
    let target = SleepTarget;
    // nonzero exit → reasoned failure with its code
    let handle = target.submit(&spec("sh", &["-c", "exit 3"]), &info()).unwrap();
    let terminal = loop {
        match target.monitor(&handle).unwrap() {
            TargetJobStatus::Running => std::thread::sleep(Duration::from_millis(50)),
            status => break status,
        }
    };
    assert_eq!(
        terminal,
        TargetJobStatus::Failed { reason: "exit_code_3".into(), code: Some(3) }
    );
    // a spawn failure is a reasoned terminal, not a submit error
    let handle = target.submit(&spec("definitely-not-a-binary-xyz", &[]), &info()).unwrap();
    match target.monitor(&handle).unwrap() {
        TargetJobStatus::Failed { reason, .. } => {
            assert!(reason.starts_with("spawn_error:"), "unexpected: {reason}")
        }
        other => panic!("a spawn failure is observed, got {other:?}"),
    }
    // unknown handles are typed errors
    let err = target.monitor(&research_core_lib::adapters::targets::JobHandle::new("nope")).unwrap_err();
    assert!(err.to_string().starts_with("unknown_job:"), "unexpected: {err}");
}

// ---- the contract's binding rules (AD-6 — bind this adapter too) ----

#[test]
fn a_freeform_spec_is_refused_at_the_community_boundary() {
    let target = SleepTarget;
    let err = target.submit(&spec("ls | rm -rf .", &[]), &info()).unwrap_err();
    assert!(err.to_string().starts_with("freeform_shell:"), "unexpected: {err}");
    // the full metachar rule rides on JobSpec::validate (shared by every
    // first-party adapter — community adapters cannot widen it)
    let err = spec("echo a; echo b", &[]).validate().unwrap_err();
    assert!(matches!(err, SpecError::FreeformShell { .. }));
}