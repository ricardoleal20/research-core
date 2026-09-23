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
use crate::domain::hypotheses::{Hypothesis, HypothesisStatus, HypothesesProjection};
use crate::domain::missions::{
    Mission, MissionsProjection, RoleConfig, MISSION_CREATED, ROLE_CRITIC, ROLE_DRAFTER,
};
use crate::domain::proposals::{self, Proposal};
use crate::eventstore::EventStore;
use serde::Serialize;
use uuid::Uuid;

/// Resolves a role's adapter through the provider layer; tests inject a fake
/// remote so real-call paths (spend, receipts) run without network.
pub(crate) type RoleResolver =
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
    #[error("not_found: hypothesis `{hypothesis_id}` is not on mission `{mission_id}`")]
    HypothesisNotOnMission { hypothesis_id: Uuid, mission_id: Uuid },
    #[error("unknown_status: `{0}` — expected proposed | testing | supported | refuted | revised")]
    UnknownStatus(String),
    #[error("task must not be empty — an agent step needs something to do")]
    EmptyTask,
    #[error("proposal basis must not be empty — every proposal names its basis")]
    EmptyBasis,
    #[error("run id must not be empty — a proposal names its proposing run")]
    EmptyRunId,
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Proposal(#[from] proposals::ProposalError),
    #[error(transparent)]
    Store(#[from] crate::eventstore::EventError),
    #[error(transparent)]
    Trust(#[from] crate::trust::TrustError),
}

impl AgentRuntime {
    pub fn new(db: Db) -> Self {
        Self { db, resolver: ProviderLayer::for_role }
    }

    /// Test seam (test builds only — no dead code in release): a runtime whose adapters resolve through the injected
    /// resolver (the Night Shift scheduler's tests share this seam).
    #[cfg(test)]
    pub(crate) fn with_resolver(db: Db, resolver: RoleResolver) -> Self {
        Self { db, resolver }
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
        // The trust dispatch (Story 2.4, AD-10/15d/15e): reserve against the
        // ceilings (kill switch + dial checked first), dispatch with the
        // reservation token attached, settle after — the adapter refuses
        // real calls that arrive without one.
        let plan = crate::trust::CallPlan {
            run_id: format!("step-{}", Uuid::new_v4().simple()),
            target: layer.name().to_string(),
            model: config.model.clone(),
            mission_id: Some(mission_id),
            estimate_cents: crate::trust::estimate_cents(&layer, &config.model),
            autonomous: true,
        };
        let resp = crate::trust::reserve_and_chat(
            &self.db,
            &layer,
            ChatRequest::new(messages)
                .with_model(&config.model)
                .for_mission(mission_id)
                .with_role(&config.name),
            plan,
        )
        .await;
        // AD-12 (Story 2.3 + 2.4): the step's events are mission-scoped — its
        // role-tagged spend may have reached the ceiling, and a REFUSED step
        // lands `spend.refused` (a cost-ceiling refusal counts toward the
        // terminal state). Evaluate the terminators on both paths; a decided
        // mission never rests active.
        {
            let conn = self.db.0.lock().await;
            let store = EventStore::new(&conn);
            crate::domain::nightshift::evaluate_terminals(&store)?;
        }
        let resp = resp?;
        Ok(AgentStepResult {
            mission_id,
            role: config.name.clone(),
            provider: layer.name().to_string(),
            model: config.model.clone(),
            content: resp.content,
        })
    }

    /// The quarantine seam (AD-3, Story 2.2): the one way an agent-actor
    /// change enters the log — as a `proposal.created` event, EXCLUDED from
    /// projections until a human merges it. Story 2.1's `run_step` produces
    /// the step's content; this seam turns an intended hypothesis
    /// transition into the proposal, with the from-status and basis derived
    /// from the current fold (never asserted by the caller). Future stories
    /// connect the model's emitted intent to this seam directly; it stays
    /// explicit so nothing dispatches into quarantine by accident.
    pub async fn propose_transition(
        &self,
        mission_id: Uuid,
        hypothesis_id: Uuid,
        to: &str,
        basis_note: &str,
        run_id: &str,
    ) -> Result<Proposal, RuntimeError> {
        let to = HypothesisStatus::parse(to)
            .ok_or_else(|| RuntimeError::UnknownStatus(to.to_string()))?;
        if basis_note.trim().is_empty() {
            return Err(RuntimeError::EmptyBasis);
        }
        if run_id.trim().is_empty() {
            return Err(RuntimeError::EmptyRunId);
        }
        let conn = self.db.0.lock().await;
        let store = EventStore::new(&conn);
        let events = store.events_all()?;
        // The mission must exist and the hypothesis must belong to it — a
        // proposal never targets another mission's board.
        if !events
            .iter()
            .any(|e| e.id == mission_id && e.kind == MISSION_CREATED)
        {
            return Err(RuntimeError::NotFound(mission_id));
        }
        let hyps = HypothesesProjection::fold_for(&events, mission_id)?;
        if !hyps.iter().any(|h| h.id == hypothesis_id) {
            return Err(RuntimeError::HypothesisNotOnMission {
                hypothesis_id,
                mission_id,
            });
        }
        let proposal = proposals::propose_transition(
            &store,
            run_id,
            hypothesis_id,
            to,
            basis_note,
        )?;
        // AD-12 (Story 2.3): a proposal is a mission-scoped event — evaluate
        // the terminators right after it lands.
        crate::domain::nightshift::evaluate_terminals(&store)?;
        Ok(proposal)
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
    use chrono::{Local, TimeZone};
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
            local_base_url: String::new(),
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
            schedule: "daily-03:00".into(),
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

    // ---- The quarantine seam (AD-3, Story 2.2) ----

    /// The loop the product is built on (AD-3): an agent step proposes a
    /// hypothesis transition through the seam — the proposal is EXCLUDED
    /// from the board until a human merges it, and the merge is what
    /// applies the change. Nothing an agent does touches the board directly.
    #[tokio::test]
    async fn an_agent_step_proposal_flows_through_quarantine_to_the_board() {
        let db = test_db();
        let mission = create_mission(
            &db,
            vec![
                RoleConfig::drafter("simulated", "simulated"),
                RoleConfig::critic("simulated", "simulated"),
            ],
        )
        .await;
        // the step: the drafter runs (simulated — no spend), its run id names
        // the proposal's actor
        let runtime = AgentRuntime::new(db.clone());
        let step = runtime
            .run_step(mission.id, ROLE_DRAFTER, "Evalúa la hipótesis central y propone el siguiente paso.")
            .await
            .unwrap();
        let run_id = format!("{}:{}", step.provider, step.role);
        // the board grows a hypothesis to target
        let hyp = {
            let conn = db.0.lock().await;
            let store = EventStore::new(&conn);
            store
                .append(NewEvent::hypothesis_created("El método reduce citas alucinadas.", mission.id).unwrap())
                .unwrap()
        };
        // the seam: the step's intended transition becomes a proposal —
        // excluded from the board (AD-3)
        let proposal = runtime
            .propose_transition(mission.id, hyp.id, "testing", "el crítico pidió una corrida de prueba", &run_id)
            .await
            .unwrap();
        assert_eq!(proposal.run_id, run_id);
        assert_eq!(proposal.status, crate::domain::proposals::ProposalStatus::Pending);
        {
            let conn = db.0.lock().await;
            let events = EventStore::new(&conn).events_all().unwrap();
            let board = HypothesesProjection::fold(&events).unwrap();
            assert_eq!(board[0].status, crate::domain::hypotheses::HypothesisStatus::Proposed,
                "a pending proposal never touches the board");
        }
        // the human merges: the change applies at the approval
        let outcome = {
            let conn = db.0.lock().await;
            let store = EventStore::new(&conn);
            crate::domain::proposals::approve(&store, proposal.id, false).unwrap()
        };
        assert_eq!(outcome.proposal.status, crate::domain::proposals::ProposalStatus::Merged);
        {
            let conn = db.0.lock().await;
            let events = EventStore::new(&conn).events_all().unwrap();
            let board = HypothesesProjection::fold(&events).unwrap();
            assert_eq!(board[0].status, crate::domain::hypotheses::HypothesisStatus::Testing,
                "the merge is what applied the change");
        }
    }

    #[tokio::test]
    async fn propose_transition_fails_loudly_on_bad_input() {
        let db = test_db();
        let mission = create_mission(&db, vec![]).await;
        let runtime = AgentRuntime::new(db.clone());
        let hyp = {
            let conn = db.0.lock().await;
            let store = EventStore::new(&conn);
            store
                .append(NewEvent::hypothesis_created("H.", mission.id).unwrap())
                .unwrap()
        };
        // unknown target status
        let err = runtime
            .propose_transition(mission.id, hyp.id, "archived", "basis", "run-1")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown_status"), "unexpected: {err}");
        // empty basis / run id
        let err = runtime
            .propose_transition(mission.id, hyp.id, "testing", "  ", "run-1")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("basis"), "unexpected: {err}");
        let err = runtime
            .propose_transition(mission.id, hyp.id, "testing", "basis", "  ")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("run id"), "unexpected: {err}");
        // hypothesis not on this mission
        let other = Uuid::new_v4();
        let err = runtime
            .propose_transition(mission.id, other, "testing", "basis", "run-1")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not on mission"), "unexpected: {err}");
        // unknown mission
        let err = runtime
            .propose_transition(Uuid::new_v4(), hyp.id, "testing", "basis", "run-1")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not_found"), "unexpected: {err}");
        // nothing was appended on refused proposals
        let conn = db.0.lock().await;
        let events = EventStore::new(&conn).events_all().unwrap();
        assert!(!events.iter().any(|e| e.kind == "proposal.created"));
    }

    // ---- Story 2.4: the trust dispatch (AD-10/15d/15e) ----

    use crate::domain::trust::{
        SPEND_REFUSED, SPEND_RESERVED, TARGET_SPEND_RECORDED as TARGET_SPEND_RECORDED_KIND,
    };
    use crate::domain::missions::MISSION_STOPPED;

    /// A slow-remote resolver: every role answers through a delayed
    /// no-network remote layer (claude rates ⇒ a 67¢ nominal estimate), so a
    /// reservation stays IN FLIGHT across an await point — the TOCTOU shape.
    fn slow_resolver(
        db: &Db,
        _conn: &Connection,
        role: &RoleConfig,
    ) -> Result<ProviderLayer, ProviderError> {
        Ok(crate::adapters::providers::slow_remote_layer(
            db,
            &role.provider,
            &role.model,
            "Hallazgo: la afirmación central quedó anclada; propongo avanzar a prueba.",
            Usage { input_tokens: 1_000, output_tokens: 500 },
            150,
        ))
    }

    fn slow_runtime(db: &Db) -> AgentRuntime {
        AgentRuntime { db: db.clone(), resolver: slow_resolver }
    }

    async fn remote_mission(db: &Db, ceiling: u64, autonomy: Autonomy) -> StoredEvent {
        let conn = db.0.lock().await;
        EventStore::new(&conn)
            .append(
                NewEvent::mission_created(MissionCreatedPayload {
                    question: "Does X hold?".into(),
                    stop_condition: "Stop after $5.".into(),
                    success_criterion: "A rater agrees.".into(),
                    autonomy,
                    spend_ceiling_cents: ceiling,
                    // rate choices so the nominal estimate per call is 68¢
                    // (sonnet rates) and 50¢ (gpt-4o rates): either order,
                    // two concurrent reservations exceed a 100¢ headroom
                    roles: vec![
                        RoleConfig::drafter("anthropic", "claude-sonnet-4-5"),
                        RoleConfig::critic("openai", "gpt-4o"),
                    ],
                    schedule: "off".into(),
                })
                .unwrap(),
            )
            .unwrap()
    }

    /// AD-10's TOCTOU test: TWO CONCURRENT dispatches over a $1.00 headroom
    /// cannot both pass. Each call's reservation estimate is 67¢; the
    /// ceiling leaves exactly 100¢ of headroom. The first dispatch reserves
    /// and is in flight (the slow remote holds the call open); the second
    /// check reads recorded spend PLUS the in-flight reservation —
    /// 0 + 67 + 67 > 100 — and is refused as an event.
    #[tokio::test]
    async fn two_concurrent_steps_over_a_dollar_headroom_one_succeeds_one_is_refused() {
        let db = test_db();
        let mission = remote_mission(&db, 100, Autonomy::Suggest).await;
        let runtime = slow_runtime(&db);
        let (a, b) = tokio::join!(
            runtime.run_step(mission.id, ROLE_DRAFTER, "Escanea la literatura."),
            runtime.run_step(mission.id, ROLE_CRITIC, "Evalúa el borrador."),
        );
        // exactly one succeeded, exactly one was refused on the ceiling
        let (ok, refused) = match (a, b) {
            (Ok(_), Err(e)) | (Err(e), Ok(_)) => (true, e),
            (Ok(_), Ok(_)) => panic!("both concurrent dispatches passed — TOCTOU violation"),
            (Err(a), Err(b)) => panic!("both refused: {a} / {b}"),
        };
        assert!(ok);
        assert!(refused.to_string().starts_with("cost_ceiling:"), "unexpected: {refused}");

        let (reserved, refusals, recorded, target_recorded) = {
            let conn = db.0.lock().await;
            let events = EventStore::new(&conn).events_all().unwrap();
            (
                events.iter().filter(|e| e.kind == SPEND_RESERVED).count(),
                events.iter().filter(|e| e.kind == SPEND_REFUSED).cloned().collect::<Vec<_>>(),
                events.iter().filter(|e| e.kind == SPEND_RECORDED).count(),
                events.iter().filter(|e| e.kind == TARGET_SPEND_RECORDED_KIND).count(),
            )
        };
        assert_eq!(reserved, 1, "exactly one reservation went through");
        assert_eq!(refusals.len(), 1, "the refusal is an event");
        assert_eq!(refusals[0].payload["scope"], serde_json::json!("mission"));
        assert_eq!(refusals[0].payload["ceiling_cents"], serde_json::json!(100));
        assert!(refusals[0].payload["would_be_cost_cents"].as_u64().unwrap() > 100);
        assert_eq!(refusals[0].payload["mission_id"], serde_json::json!(mission.id.to_string()));
        // the successful call's full protocol: recorded + target-attributed
        assert_eq!(recorded, 1);
        assert_eq!(target_recorded, 1, "the cost is attributed to the compute target");

        // the mission's spend NEVER exceeded its ceiling, and the refusal
        // counted toward the terminal state: mission.stopped(cost_ceiling_reached)
        let (spend, stopped) = {
            let conn = db.0.lock().await;
            let events = EventStore::new(&conn).events_all().unwrap();
            let missions = MissionsProjection::fold(&events).unwrap();
            let m = missions.iter().find(|m| m.id == mission.id).unwrap();
            let stopped = events
                .iter()
                .find(|e| e.kind == MISSION_STOPPED)
                .cloned();
            (m.spend_cents, stopped)
        };
        assert!(spend <= 100, "the ceiling was never overshot: {spend}¢");
        let stopped = stopped.expect("the refusal decided the terminal state");
        assert_eq!(stopped.payload["signal"], serde_json::json!("cost_ceiling_reached"));
        assert_eq!(
            stopped.actor,
            crate::eventstore::Actor::System {
                component: crate::eventstore::SystemComponent::Runtime
            }
        );
    }

    /// AD-15e: while `runtime.killed` is the latest runtime-state event, the
    /// dispatcher refuses every dispatch — run_step AND the Night Shift tick;
    /// `runtime.resumed` restores both.
    #[tokio::test]
    async fn a_killed_runtime_refuses_run_step_and_the_tick_until_resumed() {
        let db = test_db();
        let mission = remote_mission(&db, 500, Autonomy::Suggest).await;
        {
            let conn = db.0.lock().await;
            EventStore::new(&conn).append(NewEvent::runtime_killed().unwrap()).unwrap();
        }
        let runtime = slow_runtime(&db);
        // run_step is refused — typed killed:
        let err = runtime
            .run_step(mission.id, ROLE_DRAFTER, "Escanea la literatura.")
            .await
            .unwrap_err();
        assert!(err.to_string().starts_with("killed:"), "unexpected: {err}");
        // the Night Shift tick is refused too (AD-15e: every dispatch)
        let shift = crate::nightshift::NightShift::with_resolver(db.clone(), slow_resolver);
        let err = shift
            .tick(Local::now())
            .await
            .unwrap_err();
        assert!(err.to_string().starts_with("killed:"), "unexpected: {err}");
        {
            let conn = db.0.lock().await;
            let events = EventStore::new(&conn).events_all().unwrap();
            assert!(
                !events.iter().any(|e| e.kind == crate::domain::nightshift::RUN_STARTED),
                "no scan ran while killed"
            );
        }
        // resume restores both
        {
            let conn = db.0.lock().await;
            EventStore::new(&conn).append(NewEvent::runtime_resumed().unwrap()).unwrap();
        }
        runtime
            .run_step(mission.id, ROLE_DRAFTER, "Escanea la literatura.")
            .await
            .expect("resumed runtime dispatches again");
        shift.tick(Local::now()).await.expect("resumed tick runs");
    }

    /// AD-15d: the dial gates autonomous dispatch (watch refuses; suggest
    /// and act-with-receipts dispatch) — and NO position enables an
    /// auto-merge: every state mutation stays a proposal at every stop.
    #[tokio::test]
    async fn the_dial_gates_dispatch_but_no_position_ever_auto_merges() {
        let db = test_db();
        let mission = remote_mission(&db, 500, Autonomy::ActWithReceipts).await;
        // a watch mission dial (most restrictive wins over the permissive
        // creation autonomy)
        {
            let conn = db.0.lock().await;
            EventStore::new(&conn)
                .append(
                    NewEvent::autonomy_configured(crate::domain::trust::AutonomyConfiguredPayload {
                        scope: crate::domain::trust::Scope::Mission,
                        scope_id: Some(mission.id.to_string()),
                        mode: Autonomy::Watch,
                    })
                    .unwrap(),
                )
                .unwrap();
        }
        let runtime = slow_runtime(&db);
        let err = runtime
            .run_step(mission.id, ROLE_DRAFTER, "Escanea la literatura.")
            .await
            .unwrap_err();
        assert!(err.to_string().starts_with("autonomy:"), "unexpected: {err}");

        // back to act_with_receipts — the most permissive stop: dispatch
        // runs, and the step's intended change STILL lands as a pending
        // proposal (AD-3 holds at every dial position — no auto-merge)
        {
            let conn = db.0.lock().await;
            EventStore::new(&conn)
                .append(
                    NewEvent::autonomy_configured(crate::domain::trust::AutonomyConfiguredPayload {
                        scope: crate::domain::trust::Scope::Mission,
                        scope_id: Some(mission.id.to_string()),
                        mode: Autonomy::ActWithReceipts,
                    })
                    .unwrap(),
                )
                .unwrap();
        }
        let step = runtime
            .run_step(mission.id, ROLE_DRAFTER, "Evalúa y propone el siguiente paso.")
            .await
            .unwrap();
        let hyp = {
            let conn = db.0.lock().await;
            EventStore::new(&conn)
                .append(NewEvent::hypothesis_created("X holds.", mission.id).unwrap())
                .unwrap()
        };
        let proposal = runtime
            .propose_transition(
                mission.id,
                hyp.id,
                "testing",
                "el redactor pidió una corrida de prueba",
                &format!("{}:{}", step.provider, step.role),
            )
            .await
            .unwrap();
        assert_eq!(
            proposal.status,
            crate::domain::proposals::ProposalStatus::Pending,
            "even at act_with_receipts a mutation waits for a human merge"
        );
        let conn = db.0.lock().await;
        let events = EventStore::new(&conn).events_all().unwrap();
        let board = HypothesesProjection::fold(&events).unwrap();
        assert_eq!(
            board[0].status,
            crate::domain::hypotheses::HypothesisStatus::Proposed,
            "the board never moves on the agent's own authority"
        );
    }

    /// A ceiling-refused Night Shift scan ends run.failed(cost_ceiling_reached)
    /// and the AD-12 evaluator decides mission.stopped from it (the "$X
    /// spent" stop condition, reached by refusal).
    #[tokio::test]
    async fn a_ceiling_refused_scan_fails_the_run_and_stops_the_mission() {
        let db = test_db();
        // ceiling 30¢: a single 67¢ estimate cannot fit — the scan is refused
        let mission = remote_mission(&db, 30, Autonomy::Suggest).await;
        let shift = crate::nightshift::NightShift::with_resolver(db.clone(), slow_resolver);
        let records = shift.run_all().await.unwrap();
        assert_eq!(records.len(), 1);
        assert!(!records[0].finished, "the scan was refused, not run");
        assert_eq!(records[0].detail, "cost_ceiling_reached");
        let conn = db.0.lock().await;
        let events = EventStore::new(&conn).events_all().unwrap();
        let failed: Vec<&StoredEvent> = events
            .iter()
            .filter(|e| e.kind == crate::domain::nightshift::RUN_FAILED)
            .collect();
        assert_eq!(failed.len(), 1);
        assert_eq!(
            failed[0].payload["reason"],
            serde_json::json!("cost_ceiling_reached")
        );
        let stopped = events
            .iter()
            .find(|e| e.kind == MISSION_STOPPED)
            .expect("the refusal stopped the mission (AD-12)");
        assert_eq!(stopped.payload["signal"], serde_json::json!("cost_ceiling_reached"));
        let missions = MissionsProjection::fold(&events).unwrap();
        assert_eq!(
            missions.iter().find(|m| m.id == mission.id).unwrap().status,
            crate::domain::missions::MissionStatus::Stopped
        );
    }
}
