// The adapter contract (Story 6.5, FR-22.4, NFR-15): the versioned
// public seam community compute-target adapters implement — the same
// seam the first-party adapters (local, ssh, scheduler, kubernetes,
// chopflow) implement, under the same trust rules. This module IS the
// contract's machine side: the version constant, the registration
// validation, and the process-global community registry the app's
// `TargetRegistry` folds in. The full prose contract — trait shape,
// JobSpec schema, error and event vocabulary, the verify checklist —
// lives in `docs/adapters/contract.md` and MUST stay in sync with this
// module's checks.
//
// THE BINDING RULES (AD-6 — first-party and community adapters EQUALLY;
// no adapter can widen them):
//
// 1. Structured specs only. `submit` receives a typed JobSpec that
//    ALREADY passed `JobSpec::validate` at the `job.submitted`
//    constructor — and the adapter re-validates at its boundary (the
//    last line of defense). There is no freeform script/manifest/body
//    input anywhere in the contract.
// 2. Never a constructed shell string. Commands execute argv-direct;
//    where a transport's protocol is a string (ssh, a batch script
//    body), the adapter ENCODES the validated argv (POSIX
//    single-quoting) — every user-supplied value arrives inert.
// 3. The allowlist binds the adapter. Hosts (ssh, scheduler) and
//    cluster contexts (kubernetes) are gated at the command layer AND
//    at the adapter boundary, before any connection is attempted.
// 4. Honest states. A down endpoint is a typed error on that target
//    only; every terminal state carries a reason; no job ends
//    silently; unreachable never becomes "finished".
// 5. Registration is validated. A non-conforming registration (bad
//    kind slug, wrong contract version, a first-party kind, a
//    duplicate) is rejected with a specific reason at registration —
//    see `verify_registration`.
//
// The registration path for an external adapter crate:
// `adapter_contract::register_community(kind, ADAPTER_CONTRACT_VERSION,
// Arc::new(MyAdapter))` — called once at startup (the template crate's
// `install()` is the reference), after which the kind resolves through
// `TargetRegistry::v1()`, appears in `target.declared` validation, in
// target selection, and in the settings listing (kind + contract
// version) exactly as a first-party kind does.

use std::sync::{Arc, OnceLock, RwLock};

use super::{ComputeTarget, ADAPTER_CONTRACT_VERSION, FIRST_PARTY_KINDS};

/// One registered community adapter: its kind slug, the contract
/// version it was validated against, and the adapter itself.
#[derive(Clone)]
pub struct CommunityAdapter {
    pub kind: String,
    pub contract_version: String,
    pub adapter: Arc<dyn ComputeTarget>,
}

fn community_registry() -> &'static RwLock<Vec<CommunityAdapter>> {
    static REGISTRY: OnceLock<RwLock<Vec<CommunityAdapter>>> = OnceLock::new();
    REGISTRY.get_or_init(|| RwLock::new(Vec::new()))
}

/// A kind slug: lowercase letters, digits, interior dashes; 2–24
/// chars; never a leading/trailing dash (the same shape as target
/// names — it lands in mono chips and event payloads).
fn valid_kind_slug(kind: &str) -> bool {
    let kind = kind.trim();
    (2..=24).contains(&kind.len())
        && !kind.starts_with('-')
        && !kind.ends_with('-')
        && kind
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The registration verify checklist (the get-style list from the
/// contract doc, enforced here): a specific reason for every refusal —
/// a malformed kind slug, a first-party kind (community adapters never
/// shadow the built-ins), a duplicate, or a contract version this
/// ResearchCore does not speak.
pub fn verify_registration(
    kind: &str,
    contract_version: &str,
    already_registered: &[String],
) -> Result<(), String> {
    if !valid_kind_slug(kind) {
        return Err(format!(
            "invalid_kind: `{kind}` — a kind is a slug of lowercase letters, digits and interior dashes, 2-24 chars (e.g. `mainframe`, `ray-cluster`)"
        ));
    }
    if FIRST_PARTY_KINDS.contains(&kind) {
        return Err(format!(
            "reserved_kind: `{kind}` is a first-party kind — a community adapter never shadows the built-ins (the trust rules bind equally, not replaceably)"
        ));
    }
    if already_registered.iter().any(|k| k == kind) {
        return Err(format!(
            "duplicate_kind: `{kind}` is already registered — one kind, one adapter"
        ));
    }
    if contract_version != ADAPTER_CONTRACT_VERSION {
        return Err(format!(
            "contract_version_mismatch: adapter `{kind}` speaks contract `{contract_version}` — this ResearchCore speaks `{ADAPTER_CONTRACT_VERSION}` (see docs/adapters/contract.md)"
        ));
    }
    Ok(())
}

/// Register one community adapter (Story 6.5's seam): validated here,
/// folded into every `TargetRegistry::v1()` afterwards. Call once at
/// startup (the template crate's `install()` is the reference).
pub fn register_community(
    kind: &str,
    contract_version: &str,
    adapter: Arc<dyn ComputeTarget>,
) -> Result<(), String> {
    let registry = community_registry();
    let existing = registry.read().expect("community registry read poisoned");
    let already: Vec<String> = existing.iter().map(|a| a.kind.clone()).collect();
    drop(existing);
    verify_registration(kind, contract_version, &already)?;
    if adapter.kind() != kind {
        return Err(format!(
            "kind_mismatch: the registration names `{kind}` but the adapter answers `{}` — one kind, spelled the same everywhere",
            adapter.kind()
        ));
    }
    registry
        .write()
        .expect("community registry write poisoned")
        .push(CommunityAdapter {
            kind: kind.to_string(),
            contract_version: contract_version.to_string(),
            adapter,
        });
    Ok(())
}

/// The registered community adapters (registration order).
pub fn community_adapters() -> Vec<CommunityAdapter> {
    community_registry()
        .read()
        .expect("community registry read poisoned")
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal stand-in adapter (rejections never insert, so this
    /// only needs to exist and answer its kind).
    struct Never;
    impl ComputeTarget for Never {
        fn kind(&self) -> &'static str {
            "never-registered"
        }
        fn submit(
            &self,
            _spec: &crate::domain::jobs::JobSpec,
            _target: &super::super::TargetInfo,
        ) -> Result<super::super::JobHandle, super::super::TargetError> {
            Err(super::super::TargetError::UnknownTarget("never".into()))
        }
        fn monitor(
            &self,
            _handle: &super::super::JobHandle,
        ) -> Result<super::super::TargetJobStatus, super::super::TargetError> {
            Err(super::super::TargetError::UnknownJob("never".into()))
        }
        fn fetch(
            &self,
            _handle: &super::super::JobHandle,
        ) -> Result<super::super::JobResult, super::super::TargetError> {
            Err(super::super::TargetError::UnknownJob("never".into()))
        }
    }

    // ---- the verify checklist: every refusal names its reason ----

    #[test]
    fn registration_refusals_are_specific() {
        // malformed slugs
        for bad in ["", "a", "-lead", "trail-", "Upper", "sp ace", "under_score", &"x".repeat(25)] {
            let err = verify_registration(bad, ADAPTER_CONTRACT_VERSION, &[]).unwrap_err();
            assert!(err.starts_with("invalid_kind:"), "{bad:?}: {err}");
        }
        // a good slug passes
        assert_eq!(verify_registration("mainframe", ADAPTER_CONTRACT_VERSION, &[]), Ok(()));
        // first-party kinds never shadow
        for kind in FIRST_PARTY_KINDS {
            let err = verify_registration(kind, ADAPTER_CONTRACT_VERSION, &[]).unwrap_err();
            assert!(err.starts_with("reserved_kind:"), "{kind}: {err}");
        }
        // duplicates refuse
        let err = verify_registration("mainframe", ADAPTER_CONTRACT_VERSION, &["mainframe".into()])
            .unwrap_err();
        assert!(err.starts_with("duplicate_kind:"), "unexpected: {err}");
        // a wrong contract version refuses — the versioned contract
        let err = verify_registration("mainframe", "0", &[]).unwrap_err();
        assert!(err.starts_with("contract_version_mismatch:"), "unexpected: {err}");
        assert!(err.contains(ADAPTER_CONTRACT_VERSION), "the error names the spoken version: {err}");
    }

    // ---- register_community enforces the same checks (no insertion
    // happens in these — the happy path lives in the template crate's
    // tests, outside src/, per the story) ----

    #[test]
    fn community_registration_refuses_non_conforming_adapters() {
        let err = register_community("Bad_Kind", ADAPTER_CONTRACT_VERSION, Arc::new(Never))
            .unwrap_err();
        assert!(err.starts_with("invalid_kind:"), "unexpected: {err}");
        let err = register_community("local", ADAPTER_CONTRACT_VERSION, Arc::new(Never))
            .unwrap_err();
        assert!(err.starts_with("reserved_kind:"), "unexpected: {err}");
        let err = register_community("mainframe", "0", Arc::new(Never)).unwrap_err();
        assert!(err.starts_with("contract_version_mismatch:"), "unexpected: {err}");
        // the adapter must answer the kind the registration names
        let err =
            register_community("mainframe", ADAPTER_CONTRACT_VERSION, Arc::new(Never)).unwrap_err();
        assert!(err.starts_with("kind_mismatch:"), "unexpected: {err}");
        assert!(
            community_adapters().is_empty(),
            "no refusal ever inserts — the registry stays clean for the app"
        );
    }
}
