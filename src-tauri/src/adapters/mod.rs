// Adapters (architecture spine): the boundary between the core and the
// outside world — LLM providers, MCP, compute targets. All LLM calls flow
// through `providers` (AD-9): no direct reqwest fetches to provider APIs
// happen anywhere outside `adapters/providers/`.

pub mod providers;
