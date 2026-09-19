// Agent runtime (Story 2.1, Epic 2 seam): mission-configurable agent roles —
// drafter + critic — each bound to a (provider, model) pair and run through
// the Story 1.6 provider layer (AD-9). The different-model critic rule is
// enforced at mission construction (NFR-3 — see `domain::missions`), so the
// runtime only ever dispatches configs that already satisfy it. This story
// seeds the runtime with ONE agent step per call; terminal-state evaluation
// (AD-12) arrives in Story 2.3 — the seams stay open.

use crate::adapters::providers::{
    ChatRequest, Message, ProviderError, ProviderLayer, ProviderSettings,
};
use crate::db::Db;
use crate::domain::hypotheses::{Hypothesis, HypothesesProjection};
use crate::domain::missions::{
    Mission, MissionsProjection, RoleConfig, ROLE_CRITIC, ROLE_DRAFTER,
};
use crate::eventstore::EventStore;
use serde::Serialize;
use uuid::Uuid;

/// Resolves a role's adapter through the provider layer; tests inject a fake
/// remote so real-call paths (spend, receipts) run without network.
type RoleResolver =
    fn(&Db, &rusqlite::Connection, &RoleConfig) -> Result<ProviderLayer, ProviderError>;

/// The seeded agent runtime (Story 2.1): runs ONE agent step for a role of a
/// mission, through the provider layer (AD-9). Every real call's
/// `spend.recorded` event is role-tagged (per-role receipts); simulated
/// calls cost nothing and append nothing. Terminal-state evaluation (AD-12)
/// is Story 2.3's — the seams stay open.
pub struct AgentRuntime {
    db: Db,
    resolver: RoleResolver,
}

/// One completed agent step (Story 2.1): which role ran, on which
/// provider+model, and what it produced. Field names are camelCase on the
/// wire (Tauri 2 convention).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStepResult {
    pub mission_id: Uuid,
    pub role: String,
    pub provider: String,
    pub model: String,
    pub content: String,
}

/// Everything that can go wrong running an agent step — typed, never a bare
/// string from the runtime.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("not_found: no mission with id `{0}`")]
    NotFound(Uuid),
    #[error("not_found: mission `{mission_id}` has no role named `{role}` — expected drafter | critic")]
    UnknownRole { mission_id: Uuid, role: String },
    #[error("task must not be empty — an agent step needs something to do")]
    EmptyTask,
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Store(#[from] crate::eventstore::EventError),
}

impl AgentRuntime {
    pub fn new(db: Db) -> Self {
        Self { db, resolver: ProviderLayer::for_role }
    }

    /// Run ONE agent step for a role: resolve the role's (provider, model)
    /// from the mission's config (or the layer defaults for pre-2.1
    /// missions), build the role prompt over the board context, and dispatch
    /// through the provider layer — which appends the role-tagged
    /// `spend.recorded` for real calls (AD-10).
    pub async fn run_step(
        &self,
        mission_id: Uuid,
        role: &str,
        task: &str,
    ) -> Result<AgentStepResult, RuntimeError> {
        if task.trim().is_empty() {
            return Err(RuntimeError::EmptyTask);
        }
        // Board context from the projections (read side, AD-8) — one lock,
        // dropped before any await.
        let (mission, hypotheses, settings) = {
            let conn = self.db.0.lock().await;
            let events = EventStore::new(&conn).events_all()?;
            let mission = MissionsProjection::fold(&events)?
                .into_iter()
                .find(|m| m.id == mission_id)
                .ok_or(RuntimeError::NotFound(mission_id))?;
            let hypotheses = HypothesesProjection::fold_for(&events, mission_id)?;
            let settings = ProviderSettings::load(&conn);
            (mission, hypotheses, settings)
        };
        // The role's config: the mission's own roles, else the layer
        // defaults (pre-2.1 missions resolve at step time).
        let config = mission
            .roles
            .iter()
            .find(|r| r.name == role)
            .cloned()
            .or_else(|| default_roles(&settings).into_iter().find(|r| r.name == role))
            .ok_or(RuntimeError::UnknownRole {
                mission_id,
                role: role.to_string(),
            })?;
        // Resolve the role's adapter (BYOK registry; simulated fallback).
        let layer = {
            let conn = self.db.0.lock().await;
            (self.resolver)(&self.db, &conn, &config)?
        };
        let messages = vec![
            Message::system(role_prompt(&config)),
            Message::user(format!("{task}\n\n{}", board_context(&mission, &hypotheses))),
        ];
        let resp = layer
            .chat(
                ChatRequest::new(messages)
                    .with_model(&config.model)
                    .for_mission(mission_id)
                    .with_role(&config.name),
            )
            .await?;
        Ok(AgentStepResult {
            mission_id,
            role: config.name.clone(),
            provider: layer.name().to_string(),
            model: config.model.clone(),
            content: resp.content,
        })
    }
}

/// The system prompt of one role: the drafter advances; the critic
/// evaluates. (Spanish — the app's default working language, matching the
/// simulated provider's prompt markers.)
fn role_prompt(config: &RoleConfig) -> String {
    if config.name == ROLE_CRITIC {
        "Eres el crítico de una misión de investigación. Evalúa el trabajo del \
         redactor con escepticismo académico: señala afirmaciones sin evidencia, \
         generalizaciones indebidas y citas no verificadas. Responde de forma \
         concreta y accionable."
            .to_string()
    } else {
        "Eres el redactor de una misión de investigación. Avanza la misión con \
         propuestas concretas y verificables; afirma solo lo respaldado por el \
         tablero. Responde de forma concreta y accionable."
            .to_string()
    }
}

/// The board context a role steps over: the mission's terminators plus its
/// hypotheses with their states — the projections' current state (AD-8).
fn board_context(mission: &Mission, hypotheses: &[Hypothesis]) -> String {
    let mut ctx = format!(
        "Misión: «{}»\nCondición de parada: {}\nCriterio de éxito: {}",
        mission.question, mission.stop_condition, mission.success_criterion
    );
    if hypotheses.is_empty() {
        ctx.push_str("\nTablero: sin hipótesis todavía.");
        return ctx;
    }
    ctx.push_str("\nTablero (hipótesis):");
    for h in hypotheses {
        ctx.push_str(&format!("\n- [{}] {}", h.status.as_str(), h.statement));
    }
    ctx
}

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
    use crate::adapters::providers::{fake_remote_layer, Usage};
    use crate::domain::missions::{
        Autonomy, MissionCreatedPayload, SpendState, SPEND_RECORDED,
    };
    use crate::eventstore::{NewEvent, StoredEvent};
    use rusqlite::Connection;

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

    // ---- Agent step (the Epic 2 seam) ----

    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::Db::migrate(&conn).unwrap();
        EventStore::init(&conn).unwrap();
        Db(std::sync::Arc::new(tokio::sync::Mutex::new(conn)))
    }

    fn mission_payload(roles: Vec<RoleConfig>) -> MissionCreatedPayload {
        MissionCreatedPayload {
            question: "Does retrieval-augmented drafting reduce hallucinated citations?".into(),
            stop_condition: "Stop after 3 rounds or $5.00 spent.".into(),
            success_criterion: "A blind rater finds zero fabricated citations.".into(),
            autonomy: Autonomy::Suggest,
            spend_ceiling_cents: 500,
            roles,
        }
    }

    async fn create_mission(db: &Db, roles: Vec<RoleConfig>) -> StoredEvent {
        let conn = db.0.lock().await;
        EventStore::new(&conn)
            .append(NewEvent::mission_created(mission_payload(roles)).unwrap())
            .unwrap()
    }

    async fn spend_events(db: &Db) -> Vec<StoredEvent> {
        let conn = db.0.lock().await;
        EventStore::new(&conn)
            .events_all()
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == SPEND_RECORDED)
            .collect()
    }

    /// A fake remote resolver: every role answers through a no-network
    /// remote layer, so the real-call spend path runs end-to-end.
    fn fake_resolver(
        db: &Db,
        _conn: &Connection,
        role: &RoleConfig,
    ) -> Result<ProviderLayer, ProviderError> {
        Ok(fake_remote_layer(
            db,
            &role.provider,
            &role.model,
            "crítica: la afirmación central carece de evidencia anclada",
            Usage { input_tokens: 300, output_tokens: 120 },
        ))
    }

    fn runtime_with_fake(db: &Db) -> AgentRuntime {
        AgentRuntime { db: db.clone(), resolver: fake_resolver }
    }

    #[tokio::test]
    async fn role_step_runs_end_to_end_through_the_simulated_provider() {
        let db = test_db();
        // default roles: both simulated — the no-key path (Story 2.1 AC)
        let mission = create_mission(
            &db,
            vec![
                RoleConfig::drafter("simulated", "simulated"),
                RoleConfig::critic("simulated", "simulated"),
            ],
        )
        .await;
        let runtime = AgentRuntime::new(db.clone());
        let step = runtime
            .run_step(mission.id, ROLE_DRAFTER, "Redacta un párrafo sobre scaling laws.")
            .await
            .unwrap();
        assert_eq!(step.mission_id, mission.id);
        assert_eq!(step.role, ROLE_DRAFTER);
        assert_eq!(step.provider, "simulated");
        assert!(
            !step.content.trim().is_empty(),
            "the simulated provider must answer a role step sensibly"
        );
        // simulated calls cost nothing and record nothing (Story 1.6)
        assert!(spend_events(&db).await.is_empty());
    }

    #[tokio::test]
    async fn remote_role_step_records_role_tagged_spend_against_the_mission() {
        let db = test_db();
        // different-model roles on the same provider — valid (NFR-3 compares
        // the pair)
        let mission = create_mission(
            &db,
            vec![
                RoleConfig::drafter("custom", "drafter-model"),
                RoleConfig::critic("custom", "critic-model"),
            ],
        )
        .await;
        let runtime = runtime_with_fake(&db);
        let step = runtime
            .run_step(mission.id, ROLE_CRITIC, "Evalúa el borrador de la hipótesis central.")
            .await
            .unwrap();
        assert_eq!(step.role, ROLE_CRITIC);
        assert_eq!(step.provider, "custom");
        assert_eq!(step.model, "critic-model");
        assert!(step.content.contains("crítica"));

        // the call's spend is recorded, role-tagged, mission-linked
        let events = spend_events(&db).await;
        assert_eq!(events.len(), 1, "exactly one role-tagged spend.recorded");
        assert_eq!(events[0].payload["role"], serde_json::json!("critic"));
        assert_eq!(events[0].payload["model"], serde_json::json!("critic-model"));
        assert_eq!(events[0].payload["mission_id"], serde_json::json!(mission.id.to_string()));
        // the missions fold sees the spend against the ceiling (AD-10)
        let conn = db.0.lock().await;
        let all = EventStore::new(&conn).events_all().unwrap();
        let folded = MissionsProjection::fold(&all).unwrap();
        let m = folded.iter().find(|m| m.id == mission.id).unwrap();
        assert_eq!(m.spend_cents, events[0].payload["cost_cents"].as_u64().unwrap());
        assert_eq!(m.spend_state, SpendState::Ok);
    }

    #[tokio::test]
    async fn pre_roles_missions_resolve_their_roles_from_the_layer_defaults() {
        let db = test_db();
        // a pre-2.1 mission: no roles in the payload (serde default)
        let mission = create_mission(&db, vec![]).await;
        let runtime = runtime_with_fake(&db);
        // empty settings => default roles are both simulated; the fake
        // resolver stands in for the adapter, and the step still runs
        let step = runtime
            .run_step(mission.id, ROLE_CRITIC, "Evalúa el estado del tablero.")
            .await
            .unwrap();
        assert_eq!(step.role, ROLE_CRITIC);
        assert_eq!(step.provider, "simulated");
    }

    #[tokio::test]
    async fn run_step_fails_loudly_on_bad_input() {
        let db = test_db();
        let mission = create_mission(
            &db,
            vec![
                RoleConfig::drafter("custom", "drafter-model"),
                RoleConfig::critic("custom", "critic-model"),
            ],
        )
        .await;
        let runtime = runtime_with_fake(&db);
        // empty task
        let err = runtime.run_step(mission.id, ROLE_DRAFTER, "   ").await.unwrap_err();
        assert!(err.to_string().contains("task must not be empty"), "unexpected: {err}");
        // unknown mission
        let err = runtime
            .run_step(Uuid::new_v4(), ROLE_DRAFTER, "task")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not_found"), "unexpected: {err}");
        // unknown role
        let err = runtime.run_step(mission.id, "judge", "task").await.unwrap_err();
        assert!(err.to_string().contains("no role named `judge`"), "unexpected: {err}");
        // nothing was spent on refused steps
        assert!(spend_events(&db).await.is_empty());
    }
}
