# The Compute-Target Adapter Contract

**Contract version: 1** — the `` `ADAPTER_CONTRACT_VERSION` `` constant in
`src-tauri/src/adapters/targets/mod.rs`. A community adapter registers
against this version; the registry refuses any other version at
registration (specific reason:
`contract_version_mismatch`). This document is the contract's prose; the
machine side lives in `src-tauri/src/adapters/targets/adapter_contract.rs`
and MUST stay in sync with this file.

ResearchCore runs jobs on compute targets through one trait
(`ComputeTarget`), and the trait's contract has three parts: the trait
shape, the JobSpec schema, and the behavior/trust rules. First-party
adapters (`local`, `ssh`, `scheduler`, `kubernetes`, `chopflow`) and
community adapters are bound by exactly the same rules (AD-6) — a
community adapter can never widen them.

## 1. The trait shape (what you implement)

```rust
pub trait ComputeTarget: Send + Sync {
    fn kind(&self) -> &'static str;                          // the `target.declared` kind
    fn submit(&self, spec: &JobSpec, target: &TargetInfo)
        -> Result<JobHandle, TargetError>;                   // enqueue/start, do not wait
    fn monitor(&self, handle: &JobHandle)
        -> Result<TargetJobStatus, TargetError>;             // observe now, non-blocking
    fn fetch(&self, handle: &JobHandle) -> Result<JobResult, TargetError>; // terminal only
}
```

- `kind()` is a slug: lowercase letters, digits, interior dashes, 2–24
  characters, never a leading/trailing dash. It is the kind
  `target.declared` events name, what the registry resolves, and what
  the settings row lists alongside its contract version.
- `submit` starts execution (or records why it could not) and returns a
  handle; the runtime never waits on it. A process/request that fails to
  start is a JOB STATE (the handle exists; `monitor` observes a reasoned
  terminal), not a submit error — unless the adapter's transport makes
  that impossible (the chopflow adapter's HTTP submit is a typed
  `endpoint_unreachable:` error on a down queue; both are honest).
- `monitor` returns `Running`, `Finished { code }`, or `Failed { reason,
  code }`. There is no queued state in the adapter vocabulary — "alive,
  not terminal" is `Running`.
- `fetch` refuses while not terminal with the typed `job_not_terminal:`
  error — never a partial read.

The required types are all public under
`research_core_lib::adapters::targets`: `JobHandle { id }`,
`JobResult { code, stdout, stderr }`, `TargetInfo { name, host,
allowlist, config }`, `TargetError`, `TargetJobStatus`.

## 2. The JobSpec schema (what every job is)

A job is typed JSON, validated at `job.submitted` time (a freeform
construction never enters the log) — and RE-VALIDATED at the adapter
boundary again in every `submit` (defense in depth; do not skip this).

```jsonc
{
  "cmd": "python3",          // a single executable — shell metacharacters
                             // (; & | ` < > $ newlines) are REFUSED here
  "args": ["train.py", "--note=a | b"], // data — metacharacters allowed
  "env": { "EPOCHS": "10" },           // names, not values, are validated
  "resources": { "cpus": 4, "memoryMb": 8192 },  // optional, positive
  "workdir": "/lab/exp"               // optional
}
```

The executable may be a path with spaces; `args` and `env` values are
opaque data. Your adapter must never interpret them: when your
transport's protocol is a string (ssh's command line, a batch-script
body, a Job manifest), ENCODE the argv — every user-supplied value
POSIX-single-quoted (ResearchCore's `shell_quote` reachable only in-tree;
a community adapter ships its own encoding of the same, deterministic
and reversible rule). There is no freeform script, manifest body, or
shell string in the contract.

## 3. The error vocabulary (typed, bilingual-safe)

Every failure is a typed `TargetError` whose message leads with a stable
code. `unknown_job:` and `job_not_terminal:` and the spec errors bind
everyone. Your down-state is a typed error on YOUR target only — never a
crash, never a silent success:

| code | when |
|---|---|
| `unknown_job:` | the handle/remote id no longer exists — the read model keeps its last observed state honestly |
| `job_not_terminal:` | `fetch` on a live job |
| `host_not_allowed:` / `context_not_allowed:` | the allowlist gate, BEFORE any connection |
| `missing_config:` / `invalid_config:` | the target's config lacks or violates what your kind needs |
| `scheduler_unreachable:` / `kube_unreachable:` / `endpoint_unreachable:` | your transport's honest down-state (first-party examples; a community adapter's file names its own) |

Surface the down-state with the remote's own words as the detail.

## 4. The trust rules (bind everyone, AD-6)

1. **Structured specs only.** `submit` re-validates the spec at the
   boundary. Freeform input of any kind is refused before any side
   effect.
2. **Never a constructed shell string.** Commands run argv-direct;
   string-protocol transports encode the validated argv.
3. **The allowlist binds you.** Hosts (ssh, scheduler) and cluster
   contexts (kubernetes) are gated at the command layer AND at your
   boundary, before any connection. A community adapter enforces the
   gate it has (`TargetInfo.allowlist`).
4. **Honest states.** A down endpoint is a typed error on that target
   only. Every terminal carries a reason; no job ends silently;
   unreachable never becomes finished.
5. **Spend.** When your job goes terminal the runtime lands
   `target.spend_recorded` automatically — you never emit it; you just
   report truthfully through `monitor`.

## 5. The event vocabulary (your states, their events)

`job.submitted` (actor=user, your `submit`'s handle) → `job.running`
(first `monitor` seeing the job alive) → `job.finished` / `job.failed`
(terminal, reason + stamp), each terminal followed by its
`target.spend_recorded`. You observe; the runtime evented. The log is
append-only; the fold is the current state — your adapter carries no
state that the log does not replay.

## 6. The verify checklist (registration)

Before registering, walk this list — the registry enforces the first
five mechanically, reviewers use the rest:

- [ ] The kind is a slug (lowercase letters, digits, interior dashes,
      2–24 chars).
- [ ] The kind is not a first-party kind (`local`, `ssh`, `scheduler`,
      `kubernetes`, `chopflow`) and not already registered.
- [ ] The contract version matches `` `ADAPTER_CONTRACT_VERSION` ``.
- [ ] The adapter's `kind()` returns the same string the registration
      names.
- [ ] The crate compiles against the public contract alone (the template
      proves it: no access to `research_core_lib` internals).
- [ ] `submit` re-validates the spec (`spec.validate()` first line).
- [ ] Nothing ever reaches a shell un-encoded (grep your `submit` for
      `sh -c`, string-joined commands, `format!` into a command line).
- [ ] The allowlist gate runs before any connection.
- [ ] Terminal states are reasoned; down-states are typed; `fetch` waits
      for terminal.
- [ ] Unit tests cover: spec-to-your-transport mapping, state mapping,
      allowlist refusal, and the honest down-state.

## 7. Distribution

Docs live with the repo (open source, Apache-2.0). No ResearchCore-
operated service is involved in adapter distribution — a contributor
publishes their crate and documents its `install()` like the template's.