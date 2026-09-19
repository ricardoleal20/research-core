// Provider adapter layer (AD-9, Story 1.6): the single door every LLM call
// goes through — real BYOK providers, the local-CLI adapter, and the
// simulated fallback all speak one `ProviderClient` interface. Adapters are
// resolved from keychain-stored credentials (AD-16) + a model identifier;
// keys never leave this layer, never enter the database, and never enter the
// event log.
//
// Spend is evented (AD-10): every *real* provider call made through
// `ProviderLayer` appends one `spend.recorded` event via the domain's typed
// constructor. Simulated and CLI calls cost nothing measurable and append
// nothing. (Ceiling enforcement / the reservation protocol is owned by the
// runtime — a later story.)

pub mod anthropic;
pub mod cli;
pub mod google;
pub mod openai_compatible;
pub mod pricing;
pub mod simulated;

use std::future::Future;
use std::pin::Pin;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::db::Db;
use crate::domain::spend::SpendRecordedPayload;
use crate::eventstore::{EventError, EventStore};

/// One chat message. The wire shape every adapter speaks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: content.into() }
    }
    pub fn is_user(&self) -> bool {
        self.role == "user"
    }

    /// The last user message in a conversation, if any.
    pub fn last_user(messages: &[Message]) -> Option<&Message> {
        messages.iter().rev().find(|m| m.is_user())
    }
}

/// A chat request through the uniform interface: (messages, model,
/// temperature). `model` falls back to the layer's configured model when
/// empty; `mission_id` links the call's spend to a mission when the call is
/// mission-scoped; `role` tags the call's spend with the agent role that
/// made it (Story 2.1 per-role receipts).
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub model: String,
    pub temperature: f32,
    pub mission_id: Option<Uuid>,
    pub role: Option<String>,
}

impl ChatRequest {
    pub fn new(messages: Vec<Message>) -> Self {
        Self { messages, model: String::new(), temperature: 0.4, mission_id: None, role: None }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }

    /// Mark the call as mission-scoped: its `spend.recorded` event links to
    /// the mission (AD-10).
    pub fn for_mission(mut self, mission_id: Uuid) -> Self {
        self.mission_id = Some(mission_id);
        self
    }

    /// Tag the call with the agent role that made it (Story 2.1): the
    /// `spend.recorded` event carries the role so receipts can show per-role
    /// spend.
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.role = Some(role.into());
        self
    }
}

/// Token usage of one call, as reported by the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Usage {
    pub const ZERO: Usage = Usage { input_tokens: 0, output_tokens: 0 };
}

/// A completed call: (content, usage).
#[derive(Debug, Clone, PartialEq)]
pub struct ChatResponse {
    pub content: String,
    pub usage: Usage,
}

/// A chat stream's worth of content: chunks plus the final usage. Story 1.6
/// resolves the whole completion at once — `stream_chat` yields it as a
/// single chunk — true incremental SSE streaming is the runtime's later
/// story; this type is the seam it will flow through.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatStream {
    chunks: Vec<String>,
    usage: Usage,
}

impl ChatStream {
    pub fn completed(response: ChatResponse) -> Self {
        Self { chunks: vec![response.content], usage: response.usage }
    }
    pub fn chunks(&self) -> &[String] {
        &self.chunks
    }
    pub fn usage(&self) -> Usage {
        self.usage
    }
    pub fn content(&self) -> String {
        self.chunks.concat()
    }
}

/// Everything that can go wrong at the provider boundary — typed, so callers
/// and tests can match on it (never a bare string from this layer).
#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("unknown provider `{0}` — expected openai | anthropic | google | openrouter | custom, or a base_url for an OpenAI-compatible endpoint")]
    UnknownProvider(String),
    #[error("provider `{0}` requires an API key — configure one in Ajustes (it is stored in the OS keychain)")]
    MissingCredential(String),
    #[error("provider `{0}` requires a base URL")]
    MissingBaseUrl(String),
    #[error("provider `{0}` requires a model identifier")]
    MissingModel(String),
    #[error("provider `{name}` error ({status}): {body}")]
    Api { name: String, status: u16, body: String },
    #[error("provider `{name}` returned an empty completion")]
    EmptyCompletion { name: String },
    #[error("request to provider `{name}` failed: {source}")]
    Request { name: String, source: reqwest::Error },
    #[error("cli adapter: {0}")]
    Cli(String),
    #[error("spend ledger append failed (AD-10): {0}")]
    Spend(#[from] EventError),
}

impl ProviderError {
    /// Truncate a provider error body for the `Api` variant (bounded memory).
    fn api_body(body: String) -> String {
        body.chars().take(300).collect()
    }
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The one interface every provider speaks (AD-9). Real HTTP adapters, the
/// local-CLI adapter, and the simulated fallback all implement it; callers
/// never know which one answered.
pub trait ProviderClient: Send + Sync {
    /// The adapter's name (for spend attribution and errors).
    fn name(&self) -> &str;

    /// Complete a chat: (messages, model, temperature) → (content, usage).
    fn chat(&self, req: ChatRequest) -> BoxFuture<'_, Result<ChatResponse, ProviderError>>;

    /// Streaming variant. Default: resolve the whole completion via `chat`
    /// and yield it as a single chunk — the incremental SSE implementation
    /// swaps in behind this same seam.
    fn stream_chat(&self, req: ChatRequest) -> BoxFuture<'_, Result<ChatStream, ProviderError>> {
        Box::pin(async move {
            Ok(ChatStream::completed(self.chat(req).await?))
        })
    }
}

/// What kind of adapter answered — spend is recorded only for `Remote`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A real BYOK provider over HTTP — spend is recorded per call (AD-10).
    Remote,
    /// The simulated fallback — costs nothing, appends nothing.
    Simulated,
    /// A local coding CLI (claude/codex/opencode) — usage is unmeasurable.
    Cli,
}

/// The configured provider, resolved from settings + the OS keychain. This is
/// the layer's input; `ProviderSettings::load` is the only place keys are
/// read (keychain first, legacy settings row as fallback).
#[derive(Debug, Clone)]
pub struct ProviderSettings {
    /// Raw `llm_mode` setting: "cli" | "provider" | "simulate" (empty ⇒
    /// legacy auto behavior: real provider when key+endpoint exist).
    pub mode: String,
    /// Provider name: openai | anthropic | google | openrouter | custom |
    /// openai-compatible | local (or any name + base_url).
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub cli: String,
    pub cli_model: String,
}

/// Canonical endpoints for the named providers; anything else needs an
/// explicit `base_url` (custom OpenAI-compatible endpoints, Ollama, …).
pub fn default_base_url(name: &str) -> Option<&'static str> {
    match name.trim() {
        "openai" => Some("https://api.openai.com/v1"),
        "anthropic" => Some("https://api.anthropic.com"),
        "google" => Some("https://generativelanguage.googleapis.com"),
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "local" => Some("http://localhost:11434/v1"),
        _ => None,
    }
}

impl ProviderSettings {
    /// Load the provider configuration from settings + the OS keychain
    /// (service "ResearchCore", account = provider name, AD-16). The legacy
    /// `settings.api_key` row is only a fallback for keys that could not be
    /// moved to the keychain.
    pub fn load(conn: &Connection) -> Self {
        Self {
            mode: crate::db::get_setting(conn, "llm_mode"),
            name: crate::db::get_setting(conn, "provider"),
            base_url: crate::db::get_setting(conn, "base_url"),
            api_key: load_api_key(conn),
            model: crate::db::get_setting(conn, "model"),
            cli: crate::db::get_setting(conn, "llm_cli"),
            cli_model: crate::db::get_setting(conn, "llm_cli_model"),
        }
    }

    /// Would this configuration resolve to the simulated fallback? Mirrors
    /// `ProviderLayer::from_settings`'s resolution so Story 2.1's role
    /// defaults describe the adapter that would actually answer.
    pub fn is_simulated(&self) -> bool {
        match self.mode.trim() {
            "simulate" => true,
            "cli" | "provider" => false,
            _ => !ProviderLayer::has_real_provider(self),
        }
    }
}

/// API keys live in the OS keychain (AD-16); the legacy `settings.api_key`
/// row remains only as the fallback for keys that could not be moved.
fn load_api_key(conn: &Connection) -> String {
    let provider = crate::db::get_setting(conn, "provider");
    provider_key(conn, &provider)
}

/// The key for one provider: the OS keychain account named for it (AD-16),
/// with the legacy `settings.api_key` row as a fallback only for the
/// configured provider. Role-scoped resolution (Story 2.1) reads the account
/// named for the role's provider.
fn provider_key(conn: &Connection, provider: &str) -> String {
    let account = crate::eventstore::migration::keychain_account(provider);
    if let Ok(entry) =
        keyring::Entry::new(crate::eventstore::migration::KEYCHAIN_SERVICE, &account)
    {
        if let Ok(key) = entry.get_password() {
            if !key.trim().is_empty() {
                return key;
            }
        }
    }
    if provider.trim() == crate::db::get_setting(conn, "provider").trim() {
        return crate::db::get_setting(conn, "api_key");
    }
    String::new()
}

/// The provider layer: the ONLY door LLM calls go through. Resolves the
/// configured adapter (BYOK registry), dispatches through the uniform
/// `ProviderClient` interface, and appends one `spend.recorded` event per
/// real call (AD-10). Holds the `Db` handle so spend never depends on the
/// caller remembering to record it.
pub struct ProviderLayer {
    db: Db,
    kind: Kind,
    /// Spend-attribution name (the registry name, e.g. "openrouter").
    name: String,
    /// The configured model — used when a request carries none.
    model: String,
    client: Box<dyn ProviderClient>,
}

impl ProviderLayer {
    /// Resolve the configured provider from settings + keychain. With no
    /// credential configured (and no explicit provider mode), the simulated
    /// adapter keeps every AI feature usable.
    pub fn resolve(db: &Db, conn: &Connection) -> Result<Self, ProviderError> {
        Self::from_settings(db, ProviderSettings::load(conn))
    }

    /// Build the layer from an already-loaded configuration.
    pub fn from_settings(db: &Db, s: ProviderSettings) -> Result<Self, ProviderError> {
        match s.mode.trim() {
            "simulate" => Ok(Self::simulated(db)),
            "cli" => Ok(Self::cli(db, &s)),
            "provider" => Self::remote(db, &s),
            // Legacy auto behavior: real provider when key + endpoint exist,
            // else the simulated fallback.
            _ => {
                if Self::has_real_provider(&s) {
                    Self::remote(db, &s)
                } else {
                    Ok(Self::simulated(db))
                }
            }
        }
    }

    fn has_real_provider(s: &ProviderSettings) -> bool {
        !s.api_key.trim().is_empty()
            && (!s.base_url.trim().is_empty() || default_base_url(&s.name).is_some())
    }

    /// Resolve the adapter for one agent role's (provider, model) pair
    /// (Story 2.1, AD-9): the simulated fallback for `simulated`, the local
    /// CLI adapter for `cli` (the role's model on the configured CLI), and
    /// the BYOK registry otherwise — with the key read from the OS keychain
    /// account named for the role's provider (AD-16) and the base URL taken
    /// from settings when the role runs the configured provider, else the
    /// provider's canonical endpoint.
    pub fn for_role(
        db: &Db,
        conn: &Connection,
        role: &crate::domain::missions::RoleConfig,
    ) -> Result<Self, ProviderError> {
        let name = role.provider.trim().to_string();
        if name.is_empty() || name == "simulated" {
            return Ok(Self::simulated(db));
        }
        if role.model.trim().is_empty() {
            return Err(ProviderError::MissingModel(name));
        }
        let settings = ProviderSettings::load(conn);
        if name == "cli" {
            return Ok(Self {
                db: db.clone(),
                kind: Kind::Cli,
                name: "cli".into(),
                model: role.model.trim().to_string(),
                client: Box::new(cli::Cli::new(&settings.cli)),
            });
        }
        let key = provider_key(conn, &name);
        if key.trim().is_empty() {
            return Err(ProviderError::MissingCredential(name));
        }
        let base_url = if name == settings.name.trim() {
            settings.base_url.trim().to_string()
        } else {
            String::new()
        };
        Self::remote(
            db,
            &ProviderSettings {
                mode: "provider".into(),
                name,
                base_url,
                api_key: key,
                model: role.model.trim().to_string(),
                cli: String::new(),
                cli_model: String::new(),
            },
        )
    }

    fn simulated(db: &Db) -> Self {
        Self {
            db: db.clone(),
            kind: Kind::Simulated,
            name: "simulated".into(),
            model: String::new(),
            client: Box::new(simulated::Simulated),
        }
    }

    fn cli(db: &Db, s: &ProviderSettings) -> Self {
        Self {
            db: db.clone(),
            kind: Kind::Cli,
            name: "cli".into(),
            model: s.cli_model.trim().to_string(),
            client: Box::new(cli::Cli::new(&s.cli)),
        }
    }

    /// Resolve a real BYOK provider from the registry (openai, anthropic,
    /// google, openrouter, custom endpoints). Fails with typed errors when
    /// the credential, endpoint, or provider name is missing.
    fn remote(db: &Db, s: &ProviderSettings) -> Result<Self, ProviderError> {
        let name = s.name.trim().to_string();
        if s.api_key.trim().is_empty() {
            return Err(ProviderError::MissingCredential(name));
        }
        let (adapter, attribution) = Self::resolve_remote(s, &name)?;
        Ok(Self {
            db: db.clone(),
            kind: Kind::Remote,
            name: attribution,
            model: s.model.trim().to_string(),
            client: adapter,
        })
    }

    /// The registry itself: named adapters with canonical endpoints, plus the
    /// OpenAI-compatible escape hatch for custom `base_url`s.
    fn resolve_remote(
        s: &ProviderSettings,
        name: &str,
    ) -> Result<(Box<dyn ProviderClient>, String), ProviderError> {
        let base_url = s.base_url.trim();
        match name {
            "anthropic" => Ok((
                Box::new(anthropic::Anthropic::new(&s.api_key)),
                name.to_string(),
            )),
            "google" => Ok((Box::new(google::Google::new(&s.api_key)), name.to_string())),
            "openai" | "openrouter" | "local" => {
                let url = if base_url.is_empty() {
                    default_base_url(name).unwrap_or_default().to_string()
                } else {
                    base_url.to_string()
                };
                Ok((
                    Box::new(openai_compatible::OpenAiCompatible::new(name, url, &s.api_key)),
                    name.to_string(),
                ))
            }
            // "" | "custom" | "openai-compatible" | anything else: an
            // OpenAI-compatible endpoint at the configured base_url.
            _ => {
                if base_url.is_empty() {
                    if name.is_empty()
                        || name == "custom"
                        || name == "openai-compatible"
                    {
                        return Err(ProviderError::MissingBaseUrl(name.to_string()));
                    }
                    return Err(ProviderError::UnknownProvider(name.to_string()));
                }
                Ok((
                    Box::new(openai_compatible::OpenAiCompatible::new(
                        name, base_url, &s.api_key,
                    )),
                    name.to_string(),
                ))
            }
        }
    }

    /// The kind of adapter that answered (tests and callers).
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The spend-attribution name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The configured model (used when a request carries none).
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Complete a chat through the uniform interface. Real provider calls
    /// append one `spend.recorded` event; simulated and CLI calls do not.
    pub async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let req = self.prepare(req)?;
        let resp = self.client.chat(req.clone()).await?;
        self.record_spend(&req, &resp).await?;
        Ok(resp)
    }

    /// Streaming variant — see `ChatStream` for the Story 1.6 shape.
    pub async fn stream_chat(&self, req: ChatRequest) -> Result<ChatStream, ProviderError> {
        let req = self.prepare(req)?;
        let stream = self.client.stream_chat(req.clone()).await?;
        let usage = stream.usage();
        self.record_spend(&req, &ChatResponse { content: String::new(), usage })
            .await?;
        Ok(stream)
    }

    /// Fill the configured model when a request carries none; real providers
    /// refuse to dispatch without one (typed error).
    fn prepare(&self, mut req: ChatRequest) -> Result<ChatRequest, ProviderError> {
        if req.model.trim().is_empty() {
            req.model = self.model.clone();
        }
        if self.kind == Kind::Remote && req.model.trim().is_empty() {
            return Err(ProviderError::MissingModel(self.name.clone()));
        }
        Ok(req)
    }

    /// Append the `spend.recorded` event for a real call (AD-10). Fails
    /// loudly: a call whose spend cannot be recorded must not silently cost
    /// money.
    async fn record_spend(
        &self,
        req: &ChatRequest,
        resp: &ChatResponse,
    ) -> Result<(), ProviderError> {
        if self.kind != Kind::Remote {
            return Ok(());
        }
        let event = crate::eventstore::NewEvent::spend_recorded(SpendRecordedPayload {
            provider: self.name.clone(),
            model: req.model.clone(),
            input_tokens: resp.usage.input_tokens,
            output_tokens: resp.usage.output_tokens,
            cost_cents: pricing::cost_cents(&self.name, &req.model, &resp.usage),
            mission_id: req.mission_id,
            role: req.role.clone(),
        })?;
        let conn = self.db.0.lock().await;
        EventStore::new(&conn).append(event)?;
        Ok(())
    }
}

/// A fake remote client returning fixed content + usage — the no-network
/// stand-in tests across the crate use to exercise real-call paths (spend,
/// receipts) without touching HTTP.
#[cfg(test)]
pub(crate) struct FakeRemote {
    pub content: &'static str,
    pub usage: Usage,
}

#[cfg(test)]
impl ProviderClient for FakeRemote {
    fn name(&self) -> &str {
        "fake"
    }
    fn chat(&self, _req: ChatRequest) -> BoxFuture<'_, Result<ChatResponse, ProviderError>> {
        Box::pin(async move {
            Ok(ChatResponse { content: self.content.to_string(), usage: self.usage })
        })
    }
}

/// A remote-shaped layer around `FakeRemote` (Story 2.1): runtime tests
/// resolve role adapters against it so spend paths run without network.
#[cfg(test)]
pub(crate) fn fake_remote_layer(
    db: &Db,
    name: &str,
    model: &str,
    content: &'static str,
    usage: Usage,
) -> ProviderLayer {
    ProviderLayer {
        db: db.clone(),
        kind: Kind::Remote,
        name: name.into(),
        model: model.into(),
        client: Box::new(FakeRemote { content, usage }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::missions::{MissionCreatedPayload, Autonomy};
    use crate::domain::spend::SPEND_RECORDED;
    use rusqlite::Connection;
    use serde_json::json;

    /// An in-memory workspace: schema + eventstore, wrapped in the Db handle.
    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::Db::migrate(&conn).unwrap();
        EventStore::init(&conn).unwrap();
        Db(std::sync::Arc::new(tokio::sync::Mutex::new(conn)))
    }

    /// A fake remote provider returning fixed content + usage — exercises the
    /// layer's spend path without network.
    fn remote_layer(db: &Db) -> ProviderLayer {
        super::fake_remote_layer(
            db,
            "custom",
            "test-model",
            "contenido de prueba",
            Usage { input_tokens: 1200, output_tokens: 800 },
        )
    }

    async fn spend_events(db: &Db) -> Vec<crate::eventstore::StoredEvent> {
        let conn = db.0.lock().await;
        EventStore::new(&conn)
            .events_all()
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == SPEND_RECORDED)
            .collect()
    }

    #[tokio::test]
    async fn simulated_provider_answers_through_the_uniform_interface() {
        let db = test_db();
        let layer = ProviderLayer::simulated(&db);
        assert_eq!(layer.kind(), Kind::Simulated);

        // Assistant-shaped conversation (the Asistente chat flow's prompt).
        let messages = vec![
            Message::system(format!(
                "Eres el Asistente de Research Core. Proyecto activo: «Tesis». {}- Attention Is All You Need (Vaswani et al., 2017) — NeurIPS\nRedacta en español, tono académico sobrio.",
                simulated::REFS_LIST_MARKER
            )),
            Message::user("Redacta un párrafo conectando el learning rate con las scaling laws."),
        ];
        let resp = layer
            .chat(ChatRequest::new(messages).with_temperature(0.4))
            .await
            .unwrap();
        assert!(!resp.content.trim().is_empty(), "simulated reply must be sensible");
        assert!(resp.content.contains("Propuesta para el manuscrito"));
        // simulated calls cost nothing and record nothing
        assert_eq!(resp.usage, Usage::ZERO);
        assert!(spend_events(&db).await.is_empty());
    }

    #[tokio::test]
    async fn remote_call_records_spend_with_cost_and_usage() {
        let db = test_db();
        let layer = remote_layer(&db);
        assert_eq!(layer.kind(), Kind::Remote);

        let resp = layer
            .chat(ChatRequest::new(vec![Message::user("hola")]))
            .await
            .unwrap();
        assert_eq!(resp.content, "contenido de prueba");

        let events = spend_events(&db).await;
        assert_eq!(events.len(), 1, "exactly one spend.recorded per call");
        let payload = &events[0].payload;
        assert_eq!(payload["provider"], "custom");
        assert_eq!(payload["model"], "test-model");
        assert_eq!(payload["input_tokens"], json!(1200));
        assert_eq!(payload["output_tokens"], json!(800));
        assert!(
            payload["cost_cents"].as_u64().unwrap() > 0,
            "cost_cents must be positive: {payload}"
        );
        assert_eq!(
            events[0].actor,
            crate::eventstore::Actor::System {
                component: crate::eventstore::SystemComponent::Telemetry
            }
        );
        // two calls => two events, usage attributed per call
        layer.chat(ChatRequest::new(vec![Message::user("otra")])).await.unwrap();
        assert_eq!(spend_events(&db).await.len(), 2);
    }

    #[tokio::test]
    async fn mission_scoped_calls_link_their_spend_to_the_mission() {
        let db = test_db();
        let layer = remote_layer(&db);
        // create a mission to link against
        let mission = {
            let conn = db.0.lock().await;
            EventStore::new(&conn)
                .append(crate::eventstore::NewEvent::mission_created(MissionCreatedPayload {
                    question: "q".into(),
                    stop_condition: "s".into(),
                    success_criterion: "c".into(),
                    autonomy: Autonomy::Suggest,
                    spend_ceiling_cents: 500,
                roles: vec![],
                }).unwrap())
                .unwrap()
        };

        layer
            .chat(ChatRequest::new(vec![Message::user("run")]).for_mission(mission.id))
            .await
            .unwrap();

        let events = spend_events(&db).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload["mission_id"], json!(mission.id.to_string()));
        assert!(events[0].causes.is_empty() || events[0].causes == vec![mission.id]);
        // the missions fold sees the spend against the ceiling (AD-10)
        let conn = db.0.lock().await;
        let all = EventStore::new(&conn).events_all().unwrap();
        let missions = crate::domain::missions::MissionsProjection::fold(&all).unwrap();
        let m = missions.iter().find(|m| m.id == mission.id).unwrap();
        assert_eq!(m.spend_cents, events[0].payload["cost_cents"].as_u64().unwrap());
        assert_eq!(m.spend_state, crate::domain::missions::SpendState::Ok);
    }

    #[tokio::test]
    async fn stream_chat_resolves_the_whole_completion_and_records_spend() {
        let db = test_db();
        let layer = remote_layer(&db);
        let stream = layer
            .stream_chat(ChatRequest::new(vec![Message::user("hola")]))
            .await
            .unwrap();
        assert_eq!(stream.chunks().len(), 1);
        assert_eq!(stream.content(), "contenido de prueba");
        assert_eq!(stream.usage().input_tokens, 1200);
        assert_eq!(spend_events(&db).await.len(), 1);
    }

    /// Story 2.1: a role-tagged call records its role in `spend.recorded`
    /// (per-role receipts); untagged calls carry no role field at all.
    #[tokio::test]
    async fn role_tagged_calls_record_their_role_in_spend() {
        let db = test_db();
        let layer = remote_layer(&db);
        layer
            .chat(ChatRequest::new(vec![Message::user("x")]).with_role("critic"))
            .await
            .unwrap();
        let events = spend_events(&db).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload["role"], json!("critic"));
        // untagged calls (the Asistente chat flow) record no role field
        layer.chat(ChatRequest::new(vec![Message::user("y")])).await.unwrap();
        let events = spend_events(&db).await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].payload.get("role"), None);
    }

    #[tokio::test]
    async fn remote_call_without_a_model_is_a_typed_error() {
        let db = test_db();
        let mut layer = remote_layer(&db);
        layer.model = String::new();
        let err = layer.chat(ChatRequest::new(vec![Message::user("x")])).await;
        assert!(
            matches!(err, Err(ProviderError::MissingModel(ref p)) if p == "custom"),
            "expected MissingModel, got {err:?}"
        );
        // nothing was spent on a refused call
        assert!(spend_events(&db).await.is_empty());
    }

    #[test]
    fn registry_resolves_every_named_provider() {
        for (name, attribution) in [
            ("openai", "openai"),
            ("anthropic", "anthropic"),
            ("google", "google"),
            ("openrouter", "openrouter"),
            ("local", "local"),
            ("custom", "custom"),
            ("openai-compatible", "openai-compatible"),
        ] {
            let db = test_db();
            let settings = ProviderSettings {
                mode: "provider".into(),
                name: name.into(),
                base_url: "https://example.internal/v1".into(),
                api_key: "sk-test".into(),
                model: "m".into(),
                cli: String::new(),
                cli_model: String::new(),
            };
            let layer = ProviderLayer::from_settings(&db, settings)
                .unwrap_or_else(|e| panic!("{name} must resolve: {e}"));
            assert_eq!(layer.kind(), Kind::Remote, "{name}");
            assert_eq!(layer.name(), attribution);
        }
    }

    #[test]
    fn named_providers_resolve_without_a_base_url() {
        for name in ["openai", "anthropic", "google", "openrouter"] {
            let db = test_db();
            let settings = ProviderSettings {
                mode: "provider".into(),
                name: name.into(),
                base_url: String::new(),
                api_key: "sk-test".into(),
                model: "m".into(),
                cli: String::new(),
                cli_model: String::new(),
            };
            let layer = ProviderLayer::from_settings(&db, settings)
                .unwrap_or_else(|e| panic!("{name} must resolve on its canonical endpoint: {e}"));
            assert_eq!(layer.kind(), Kind::Remote);
        }
    }

    #[test]
    fn unknown_provider_and_missing_pieces_are_typed_errors() {
        let db = test_db();
        let base = ProviderSettings {
            mode: "provider".into(),
            name: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            cli: String::new(),
            cli_model: String::new(),
        };
        // unknown provider name, no base_url
        let mut s = base.clone();
        s.name = "martian-llm".into();
        s.api_key = "sk-test".into();
        assert!(matches!(
            ProviderLayer::from_settings(&db, s.clone()),
            Err(ProviderError::UnknownProvider(ref n)) if n == "martian-llm"
        ));
        // known shape but no key
        let mut s = base.clone();
        s.name = "openai".into();
        assert!(matches!(
            ProviderLayer::from_settings(&db, s.clone()),
            Err(ProviderError::MissingCredential(ref n)) if n == "openai"
        ));
        // custom with no endpoint
        let mut s = base.clone();
        s.name = "custom".into();
        s.api_key = "sk-test".into();
        assert!(matches!(
            ProviderLayer::from_settings(&db, s.clone()),
            Err(ProviderError::MissingBaseUrl(ref n)) if n == "custom"
        ));
    }

    #[test]
    fn auto_mode_falls_back_to_simulated_without_a_key() {
        let db = test_db();
        // legacy auto mode (empty llm_mode) with no key => simulated
        let settings = ProviderSettings {
            mode: String::new(),
            name: "openai".into(),
            base_url: String::new(),
            api_key: String::new(),
            model: "gpt-4o-mini".into(),
            cli: String::new(),
            cli_model: String::new(),
        };
        assert_eq!(ProviderLayer::from_settings(&db, settings).unwrap().kind(), Kind::Simulated);
        // and with key + endpoint => remote
        let settings = ProviderSettings {
            mode: String::new(),
            name: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: "sk-test".into(),
            model: "gpt-4o-mini".into(),
            cli: String::new(),
            cli_model: String::new(),
        };
        assert_eq!(ProviderLayer::from_settings(&db, settings).unwrap().kind(), Kind::Remote);
    }

    /// Resolution reads the key from the keychain config; the key value must
    /// never surface in any event the layer appends (AD-16: keychain-only
    /// credentials — the log is auditable, keys are not).
    #[tokio::test]
    async fn resolved_adapters_never_leak_the_key_into_events() {
        const SENTINEL: &str = "sk-1-6-NEVER-LEAK-THIS-KEY-c31d";
        const ACCOUNT: &str = "rc-1-6-leak-test";

        let db = test_db();
        {
            let conn = db.0.lock().await;
            crate::db::set_setting(&conn, "llm_mode", "provider").unwrap();
            crate::db::set_setting(&conn, "provider", ACCOUNT).unwrap();
            crate::db::set_setting(&conn, "base_url", "https://example.internal/v1").unwrap();
            crate::db::set_setting(&conn, "model", "test-model").unwrap();
        }

        // Put the key where Story 1.2 left it: the OS keychain (service
        // "ResearchCore", account = provider name). If the keychain is
        // unavailable, the settings fallback carries the same sentinel and
        // the no-leak assertion still runs.
        let keychain_ok = match keyring::Entry::new(
            crate::eventstore::migration::KEYCHAIN_SERVICE,
            ACCOUNT,
        ) {
            Ok(entry) => match entry.set_password(SENTINEL) {
                Ok(()) => true,
                Err(_) => {
                    let conn = db.0.lock().await;
                    crate::db::set_setting(&conn, "api_key", SENTINEL).unwrap();
                    false
                }
            },
            Err(_) => {
                let conn = db.0.lock().await;
                crate::db::set_setting(&conn, "api_key", SENTINEL).unwrap();
                false
            }
        };

        // Resolve from that config: a remote adapter, attributed to the
        // provider account.
        let layer = {
            let conn = db.0.lock().await;
            ProviderLayer::resolve(&db, &conn).unwrap()
        };
        assert_eq!(layer.kind(), Kind::Remote);
        assert_eq!(layer.name(), ACCOUNT);
        assert_eq!(layer.model(), "test-model");

        // Record the spend a real call would record — with the fake client so
        // no network is involved.
        let mut layer = layer;
        layer.client = Box::new(FakeRemote {
            content: "respuesta",
            usage: Usage { input_tokens: 100, output_tokens: 50 },
        });
        layer.chat(ChatRequest::new(vec![Message::user("hola")])).await.unwrap();

        // No event — kind, payload, actor, anything — contains the key.
        let conn = db.0.lock().await;
        let all = EventStore::new(&conn).events_all().unwrap();
        assert!(!all.is_empty());
        for event in &all {
            let whole = serde_json::to_string(event).unwrap();
            assert!(
                !whole.contains(SENTINEL),
                "event {} ({}) leaked the api key",
                event.seq,
                event.kind
            );
        }
        drop(conn);

        if keychain_ok {
            if let Ok(entry) = keyring::Entry::new(
                crate::eventstore::migration::KEYCHAIN_SERVICE,
                ACCOUNT,
            ) {
                let _ = entry.delete_credential();
            }
        }
    }
}
