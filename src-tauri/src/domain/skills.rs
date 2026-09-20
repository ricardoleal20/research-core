// Skills domain (FR-16.6–16.8, Story 5.6): a skill is DATA — a name, a
// system-prompt role, a (provider, model) pair routed through the existing
// `RoleConfig` provider path (AD-9), and an allowed tool set. The curated
// DEFAULT scientific set ships pre-installed (Drafter, Critic, Librarian,
// Verifier, Synthesizer, Note-taker); the registry is extensible — a skill
// definition is a row, not code.
//
// NFR-3 stays mission-scoped and unchanged: the different-model-critic rule
// governs MISSION role configs (`mission.created` validation); skill roles
// are per-conversation and never grade mission homework — a chat runs one
// skill at a time.

use serde::{Deserialize, Serialize};

/// The skill role names the vocabulary grows to (Story 5.6): the mission
/// pair (drafter, critic) plus the six scientific skill roles. The
/// `AgentRoleName` vocabulary is this set — a skill's name is its identity.
pub const SKILL_DRAFTER: &str = "drafter";
pub const SKILL_CRITIC: &str = "critic";
pub const SKILL_LIBRARIAN: &str = "librarian";
pub const SKILL_VERIFIER: &str = "verifier";
pub const SKILL_SYNTHESIZER: &str = "synthesizer";
pub const SKILL_NOTE_TAKER: &str = "note_taker";

/// The closed tool vocabulary a skill's allowed set draws from (AD-15d).
/// There is deliberately NO mutation tool: no skill may mutate domain state
/// except through proposals (AD-3) — the chat path proposes, humans merge.
pub const TOOL_SEARCH: &str = "search";
pub const TOOL_READ_BOARD: &str = "read_board";
pub const TOOL_READ_LIBRARY: &str = "read_library";
pub const TOOL_VERIFY: &str = "verify";

/// A skill definition (FR-16.7): a `RoleConfig`-shaped provider binding
/// plus the system-prompt role and the allowed tool set. An empty
/// `provider` means "the configured provider's layer, with its model" —
/// skill roles may default to the configured provider model (Story 5.6);
/// `simulated` keeps every skill usable with no key configured. camelCase
/// on the wire (Tauri 2 convention for the shell).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    /// The skill's name — its identity and the `AgentRoleName` it extends.
    pub name: String,
    /// The provider-layer provider name (or `simulated` / empty = the
    /// configured layer).
    pub provider: String,
    /// The model identifier; empty = the configured layer's model.
    pub model: String,
    /// The skill's system-prompt role, merged with the conversation's
    /// context at dispatch.
    pub system_prompt: String,
    /// The allowed tool set (the closed vocabulary above) — bounds what
    /// the skill may dispatch.
    pub tools: Vec<String>,
    /// True for the curated defaults; false for user-added skills.
    pub builtin: bool,
}

/// The curated DEFAULT scientific skill set (FR-16.6) — pre-installed on
/// every fresh workspace. Provider/model default to the configured layer
/// (empty) so the set resolves exactly as the default drafter role does
/// today, simulated included.
pub fn default_skills() -> Vec<Skill> {
    vec![
        Skill {
            name: SKILL_DRAFTER.into(),
            provider: String::new(),
            model: String::new(),
            system_prompt: "Eres el Redactor científico: avanzas el manuscrito — párrafos,\
                related-work, transiciones— siempre anclados en la evidencia fijada del tablero.\
                Cuando propongas texto para el manuscrito, inclúyelo en un bloque citado."
                .into(),
            tools: vec![TOOL_READ_BOARD.into(), TOOL_READ_LIBRARY.into()],
            builtin: true,
        },
        Skill {
            name: SKILL_CRITIC.into(),
            provider: String::new(),
            model: String::new(),
            system_prompt: "Eres el Crítico metodológico: evalúas afirmaciones, diseño y\
                evidencia con rigor — señala supuestos débiles, afirmaciones que exceden la\
                evidencia y citas sin anclaje. Nunca redactas texto final; evalúas."
                .into(),
            tools: vec![TOOL_READ_BOARD.into(), TOOL_READ_LIBRARY.into()],
            builtin: true,
        },
        Skill {
            name: SKILL_LIBRARIAN.into(),
            provider: String::new(),
            model: String::new(),
            system_prompt: "Eres el Bibliotecario: buscas y curas literatura — búsquedas\
                dirigidas, síntesis comparativa de referencias, candados de cobertura. Puedes\
                despachar búsquedas; nunca mutas el dominio."
                .into(),
            tools: vec![TOOL_SEARCH.into(), TOOL_READ_LIBRARY.into()],
            builtin: true,
        },
        Skill {
            name: SKILL_VERIFIER.into(),
            provider: String::new(),
            model: String::new(),
            system_prompt: "Eres el Verificador: contrastas afirmaciones contra sus fuentes\
                — citas, números, artefactos. Distingues verificación (existencia por código)\
                de confianza (juicio de un modelo); nunca afirmas «verificado» sin código."
                .into(),
            tools: vec![TOOL_VERIFY.into(), TOOL_READ_BOARD.into()],
            builtin: true,
        },
        Skill {
            name: SKILL_SYNTHESIZER.into(),
            provider: String::new(),
            model: String::new(),
            system_prompt: "Eres el Sintetizador: cruzas hipótesis y evidencia del tablero en\
                hallazgos integradores — patrones, tensiones, vacíos — con trazabilidad a los\
                pines que los sostienen."
                .into(),
            tools: vec![TOOL_READ_BOARD.into(), TOOL_READ_LIBRARY.into()],
            builtin: true,
        },
        Skill {
            name: SKILL_NOTE_TAKER.into(),
            provider: String::new(),
            model: String::new(),
            system_prompt: "Eres el Tomador de notas: registras la sesión de investigación —\
                decisiones, hallazgos, pendientes — en notas concisas y consultables del\
                tablero. Nunca propones mutaciones al dominio."
                .into(),
            tools: vec![TOOL_READ_BOARD.into()],
            builtin: true,
        },
    ]
}

/// The closed tool vocabulary — a skill's allowed set may only draw from it.
pub fn known_tools() -> &'static [&'static str] {
    &[TOOL_SEARCH, TOOL_READ_BOARD, TOOL_READ_LIBRARY, TOOL_VERIFY]
}

/// Validate a skill definition at the edge (AD-15 spirit): a known-shape
/// name, a non-empty system prompt, and an allowed tool set drawn from the
/// closed vocabulary. Provider/model may be empty (the configured layer).
pub fn validate_skill(skill: &Skill) -> Result<(), String> {
    let name = skill.name.trim();
    if name.is_empty() {
        return Err("skill.name must not be empty — a skill's name is its identity".into());
    }
    if skill.system_prompt.trim().is_empty() {
        return Err(format!(
            "skill `{name}`: system_prompt must not be empty — a skill is its role"
        ));
    }
    for tool in &skill.tools {
        if !known_tools().contains(&tool.as_str()) {
            return Err(format!(
                "skill `{name}`: unknown tool `{tool}` — expected one of {}",
                known_tools().join(" | ")
            ));
        }
    }
    Ok(())
}

/// Dispatch permission (AD-15d): where the skill's allowed tools, the
/// autonomy dial, and mission/target policies overlap, the MOST
/// RESTRICTIVE rule wins. Pure — the runtime consults it before any tool
/// dispatch. `watch` observes, it never dispatches (AD-15d); a tool outside
/// the skill's set is refused; no skill may mutate domain state except
/// through proposals (AD-3) — "mutate" is refused for every set.
pub fn dispatch_allowed(skills_tools: &[String], tool: &str, autonomy: &crate::domain::missions::Autonomy) -> bool {
    use crate::domain::missions::Autonomy;
    if matches!(autonomy, Autonomy::Watch) {
        return false; // the dial refuses before the skill's set is even read
    }
    if tool.trim() == "mutate" {
        return false; // AD-3: no skill mutates domain state — proposals only
    }
    skills_tools.iter().any(|t| t == tool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::missions::Autonomy;

    #[test]
    fn the_default_set_is_the_six_scientific_skills() {
        let skills = default_skills();
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["drafter", "critic", "librarian", "verifier", "synthesizer", "note_taker"],
            "FR-16.6: the curated default set ships pre-installed"
        );
        for s in &skills {
            assert!(s.builtin);
            assert!(!s.system_prompt.trim().is_empty(), "{} needs a role prompt", s.name);
            assert!(!s.tools.is_empty(), "{} needs an allowed tool set", s.name);
            assert!(validate_skill(s).is_ok(), "{} must validate", s.name);
            // defaults resolve through the configured layer (empty pair),
            // exactly as the default drafter role does today
            assert!(s.provider.is_empty() && s.model.is_empty());
        }
    }

    #[test]
    fn validation_rejects_empty_names_prompts_and_unknown_tools() {
        let mut s = default_skills().remove(0);
        s.name = "  ".into();
        assert!(validate_skill(&s).is_err());
        let mut s = default_skills().remove(0);
        s.system_prompt = String::new();
        assert!(validate_skill(&s).is_err());
        let mut s = default_skills().remove(0);
        s.tools = vec!["mutate".into()];
        assert!(validate_skill(&s).is_err(), "the closed vocabulary rejects mutate");
    }

    /// NFR-8: dispatch-permission enforcement is unit-tested per skill —
    /// the Librarian may dispatch searches, no skill may mutate, and the
    /// watch dial refuses everything (AD-15d: most restrictive wins).
    #[test]
    fn dispatch_permissions_are_enforced_per_skill() {
        for skill in default_skills() {
            // the dial refuses before the skill's set is read
            for tool in known_tools() {
                assert!(
                    !dispatch_allowed(&skill.tools, tool, &Autonomy::Watch),
                    "{} on watch must never dispatch",
                    skill.name
                );
            }
            // AD-3: no skill mutates domain state — proposals only
            assert!(!dispatch_allowed(&skill.tools, "mutate", &Autonomy::ActWithReceipts));
            // every allowed tool dispatches on non-watch dials
            for tool in &skill.tools {
                assert!(dispatch_allowed(&skill.tools, tool, &Autonomy::Suggest), "{}", tool);
                assert!(dispatch_allowed(&skill.tools, tool, &Autonomy::ActWithReceipts));
            }
        }
        // the Librarian may dispatch searches …
        let librarian = default_skills().into_iter().find(|s| s.name == SKILL_LIBRARIAN).unwrap();
        assert!(dispatch_allowed(&librarian.tools, TOOL_SEARCH, &Autonomy::Suggest));
        // … and the Note-taker may not
        let note_taker = default_skills().into_iter().find(|s| s.name == SKILL_NOTE_TAKER).unwrap();
        assert!(!dispatch_allowed(&note_taker.tools, TOOL_SEARCH, &Autonomy::Suggest));
        // a tool outside the set is refused even on the loosest dial
        let drafter = default_skills().into_iter().find(|s| s.name == SKILL_DRAFTER).unwrap();
        assert!(!dispatch_allowed(&drafter.tools, TOOL_SEARCH, &Autonomy::ActWithReceipts));
    }
}
