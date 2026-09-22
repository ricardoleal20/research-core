// Adapters (architecture spine): the boundary between the core and the
// outside world — LLM providers, MCP, compute targets. All LLM calls flow
// through `providers` (AD-9): no direct reqwest fetches to provider APIs
// happen anywhere outside `adapters/providers/`. All compute jobs flow
// through `targets` (AD-6): typed specs, adapter trait + registry, never
// a constructed shell string.

pub mod providers;
pub mod targets;
