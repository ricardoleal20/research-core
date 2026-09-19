// Agent runtime (Story 2.1, Epic 2 seam): mission-configurable agent roles —
// drafter + critic — each bound to a (provider, model) pair and run through
// the Story 1.6 provider layer (AD-9). The different-model critic rule is
// enforced at mission construction (NFR-3 — see `domain::missions`), so the
// runtime only ever dispatches configs that already satisfy it. This story
// seeds the runtime with ONE agent step per call; terminal-state evaluation
// (AD-12) arrives in Story 2.3 — the seams stay open.

use crate::adapters::providers::ProviderSettings;
use crate::domain::missions::{RoleConfig, ROLE_CRITIC, ROLE_DRAFTER};

/// The default role config when a mission creation carries no overrides
/// (Story 2.1): the drafter runs on the layer's configured pair; the critic
/// NEVER shares that pair (NFR-3) — it falls back to the simulated provider
/// until the user configures a different model. With no key configured at
/// all, both roles run simulated (the app stays fully usable, Story 2.1 AC).
pub fn default_roles(s: &ProviderSettings) -> Vec<RoleConfig> {
    let simulated = |name: &str| RoleConfig { name: name.into(), provider: "simulated".into(), model: "simulated".into() };
    if s.is_simulated() {
        return vec![simulated(ROLE_DRAFTER), simulated(ROLE_CRITIC)];
    }
    if s.mode.trim() == "cli" {
        // CLI mode: the drafter runs the configured local CLI (with its own
        // model); an incomplete CLI config falls back to simulated so mission
        // creation never fails over a missing setting.
        let model = s.cli_model.trim();
        if model.is_empty() {
            return vec![simulated(ROLE_DRAFTER), simulated(ROLE_CRITIC)];
        }
        return vec![RoleConfig::drafter("cli", model), simulated(ROLE_CRITIC)];
    }
    // Real provider mode: the drafter takes the configured pair when it is
    // complete; the critic defaults to the simulated provider (a different
    // pair by construction — NFR-3 holds without user action).
    let provider = s.name.trim();
    let model = s.model.trim();
    if provider.is_empty() || model.is_empty() {
        return vec![simulated(ROLE_DRAFTER), simulated(ROLE_CRITIC)];
    }
    vec![RoleConfig::drafter(provider, model), simulated(ROLE_CRITIC)]
}

/// Apply a mission creation's role overrides onto the defaults: each override
/// replaces the default role with the same name (a role's name is its
/// identity); unknown names pass through for the domain validation to reject
/// loudly. `None` (or an empty list) keeps the defaults whole.
pub fn merge_roles(
    defaults: Vec<RoleConfig>,
    overrides: Option<Vec<RoleConfig>>,
) -> Vec<RoleConfig> {
    let Some(overrides) = overrides else {
        return defaults;
    };
    if overrides.is_empty() {
        return defaults;
    }
    let mut roles = defaults;
    for role in overrides {
        match roles.iter_mut().find(|r| r.name == role.name) {
            Some(slot) => *slot = role,
            None => roles.push(role),
        }
    }
    roles
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(mode: &str, name: &str, model: &str) -> ProviderSettings {
        ProviderSettings {
            mode: mode.into(),
            name: name.into(),
            base_url: String::new(),
            api_key: "sk-test".into(),
            model: model.into(),
            cli: String::new(),
            cli_model: String::new(),
        }
    }

    #[test]
    fn default_roles_follow_the_layer_configuration() {
        // no key (auto mode falls back to simulated): both roles simulated —
        // the app stays fully usable with no key (Story 2.1 AC)
        let s = ProviderSettings {
            api_key: String::new(),
            ..settings("", "openai", "gpt-4o")
        };
        assert_eq!(
            default_roles(&s),
            vec![
                RoleConfig::drafter("simulated", "simulated"),
                RoleConfig::critic("simulated", "simulated"),
            ]
        );
        // explicit simulate mode: same
        assert_eq!(
            default_roles(&settings("simulate", "openai", "gpt-4o")),
            default_roles(&s)
        );
        // real provider with a complete pair: the drafter takes it, the
        // critic falls back to simulated — never the same pair (NFR-3)
        let roles = default_roles(&settings("provider", "openai", "gpt-4o"));
        assert_eq!(
            roles,
            vec![
                RoleConfig::drafter("openai", "gpt-4o"),
                RoleConfig::critic("simulated", "simulated"),
            ]
        );
        // an incomplete pair cannot run through the layer: simulated defaults
        let roles = default_roles(&settings("provider", "openai", ""));
        assert_eq!(roles[0].provider, "simulated");
        // CLI mode with a model: the drafter runs the CLI
        let s = ProviderSettings { cli_model: "claude-sonnet".into(), ..settings("cli", "", "") };
        assert_eq!(
            default_roles(&s),
            vec![
                RoleConfig::drafter("cli", "claude-sonnet"),
                RoleConfig::critic("simulated", "simulated"),
            ]
        );
    }

    #[test]
    fn merge_roles_replaces_by_name_and_passes_unknowns_through() {
        let defaults = default_roles(&settings("provider", "openai", "gpt-4o"));
        // None and empty overrides keep the defaults
        assert_eq!(merge_roles(defaults.clone(), None), defaults);
        assert_eq!(merge_roles(defaults.clone(), Some(vec![])), defaults);
        // overriding only the critic leaves the default drafter in place
        let merged = merge_roles(
            defaults,
            Some(vec![RoleConfig::critic("anthropic", "claude-sonnet-4-5")]),
        );
        assert_eq!(
            merged,
            vec![
                RoleConfig::drafter("openai", "gpt-4o"),
                RoleConfig::critic("anthropic", "claude-sonnet-4-5"),
            ]
        );
        // unknown names pass through for the domain validation to reject
        let merged = merge_roles(
            default_roles(&settings("provider", "openai", "gpt-4o")),
            Some(vec![RoleConfig { name: "judge".into(), provider: "openai".into(), model: "m".into() }]),
        );
        assert!(merged.iter().any(|r| r.name == "judge"));
    }
}
