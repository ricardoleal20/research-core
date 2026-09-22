// Indicative pricing for the spend ledger (AD-10). BYOK: the exact invoice
// lives with the provider — the ledger needs an attributable, non-zero
// estimate so ceilings are computable from the log. Rates are cents per 1M
// tokens; unknown models fall back to their provider's default.

use super::Usage;

/// (input, output) cents per 1M tokens.
fn rates(provider: &str, model: &str) -> (u64, u64) {
    let m = model.trim().to_lowercase();
    match () {
        _ if m.starts_with("gpt-4o-mini") => (15, 60),
        _ if m.starts_with("gpt-4o") => (250, 1000),
        _ if m.starts_with("gpt-4.1-mini") => (40, 160),
        _ if m.starts_with("gpt-4.1") => (200, 800),
        _ if m.starts_with("gpt-5-mini") || m.starts_with("gpt-5-nano") => (35, 140),
        _ if m.starts_with("gpt-5") => (125, 1000),
        _ if m.contains("haiku") => (80, 400),
        _ if m.contains("opus") => (1500, 7500),
        _ if m.contains("sonnet") || m.contains("claude") => (300, 1500),
        _ if m.contains("flash") => (35, 150),
        _ if m.contains("gemini") && m.contains("pro") => (1250, 5000),
        _ if m.contains("gemini") => (100, 400),
        _ => default_rates(provider),
    }
}

fn default_rates(provider: &str) -> (u64, u64) {
    match provider.trim() {
        "openai" => (150, 600),
        "anthropic" => (300, 1500),
        "google" => (100, 400),
        // openrouter, custom, local-compatible endpoints
        _ => (100, 400),
    }
}

/// The indicative cost of one call, in cents (rounded up; any call with
/// tokens costs at least one cent).
pub fn cost_cents(provider: &str, model: &str, usage: &Usage) -> u64 {
    if usage.input_tokens + usage.output_tokens == 0 {
        return 0;
    }
    let (in_rate, out_rate) = rates(provider, model);
    let cents =
        (usage.input_tokens * in_rate + usage.output_tokens * out_rate + 999_999) / 1_000_000;
    cents.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_models_rate_from_the_table() {
        let usage = Usage { input_tokens: 1_000_000, output_tokens: 1_000_000 };
        assert_eq!(cost_cents("openai", "gpt-4o-mini", &usage), 15 + 60);
        assert_eq!(cost_cents("openai", "gpt-4o", &usage), 250 + 1000);
        assert_eq!(cost_cents("anthropic", "claude-3-5-sonnet", &usage), 300 + 1500);
        assert_eq!(cost_cents("google", "gemini-2.0-flash", &usage), 35 + 150);
    }

    #[test]
    fn unknown_models_fall_back_to_provider_defaults() {
        let usage = Usage { input_tokens: 1_000_000, output_tokens: 0 };
        assert_eq!(cost_cents("openai", "gpt-x-future", &usage), 150);
        assert_eq!(cost_cents("openrouter", "some/new-model", &usage), 100);
        assert_eq!(cost_cents("custom", "whatever", &usage), 100);
    }

    #[test]
    fn every_tokened_call_costs_at_least_one_cent() {
        let usage = Usage { input_tokens: 10, output_tokens: 5 };
        assert_eq!(cost_cents("custom", "m", &usage), 1);
        let zero = Usage::ZERO;
        assert_eq!(cost_cents("custom", "m", &zero), 0, "free calls cost nothing");
    }

    #[test]
    fn costs_round_up() {
        // 1.2M in + 0.8M out on the (100, 400) default rates = $1.20 + $3.20 = 440 cents
        let usage = Usage { input_tokens: 1_200_000, output_tokens: 800_000 };
        assert_eq!(cost_cents("custom", "m", &usage), 440);
    }
}
