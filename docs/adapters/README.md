# Compute-Target Adapters

ResearchCore runs jobs on compute targets through one trait. The
first-party kinds ship in-tree:

| kind | adapter | transports |
|---|---|---|
| `local` | argv-direct processes | this machine |
| `ssh` | allowlisted remote hosts | ssh (argv-encoded) |
| `scheduler` | SLURM/PBS clusters (Story 6.2) | sbatch/qsub + squeue/sacct/qstat |
| `kubernetes` | cluster contexts (Story 6.3) | kubectl (argv-direct, generated Job manifests) |
| `chopflow` | ChopFlow queue endpoints (Story 6.4) | minimal HTTP/JSON slice of the queue API |

Configuration is per-target, via the `config` map a `target.declared`
event carries (validated at declaration: one-token values, unknown keys
refused):

- `scheduler`: `flavor` (`slurm`|`pbs`), `submitPrefix`, `pollPrefix`,
  `acctPrefix` — plus the optional `host` (the submission node, ssh-ridden
  and allowlist-gated like an ssh target).
- `kubernetes`: `context` (required; allowlist-gated), `namespace`
  (default `default`), `image` (required), `kubectlPrefix`.
- `chopflow`: `endpoint` (required, http(s)), `queue` (optional).

Allowlist entries are hosts for `ssh`/`scheduler` targets and cluster
contexts for `kubernetes` targets (Settings → Compute targets).

## Registering an external (community) adapter

The ComputeTarget trait + JobSpec schema are a versioned public contract
— see [contract.md](./contract.md). The reference implementation is the
in-repo template crate: `adapters/community-template/`.

Three steps:

1. **Implement the trait** against the contract (start from the
   template). Your crate depends on the app as a library
   (`research-core = { path = "../../src-tauri" }` in the template's
   layout) and uses only the public `research_core_lib::adapters::targets`
   surface — the template's own process tracking proves no internal
   plumbing is required.
2. **Register at startup**: call `install()` (yours, or the template's)
   once — it routes through
   `adapter_contract::register_community(kind, ADAPTER_CONTRACT_VERSION,
   Arc::new(adapter))`, which validates the kind slug, the contract
   version, the no-shadowing rule, and rejects duplicates with a
   specific reason before anything is registered.
3. **Verify**: from the moment registration succeeds, the kind resolves
   through the registry exactly as a first-party one does — it appears
   in `target.declared` validation, in target selection, and in the
   settings listing (kind + contract version), with the same allowlist,
   autonomy, spend-recording, and quarantine paths.

`list_registered_adapters` (the settings row) reports every registered
kind with its contract version, `builtin: true` for the five in-tree
kinds. A community adapter needs REGISTRATION at compile time (fork or a
bundled build registers it) — a running app never loads adapters from a
network; distribution is the contributor's crate, not a service.