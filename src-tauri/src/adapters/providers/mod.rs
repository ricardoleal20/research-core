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
pub mod models;
pub mod ollama;
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
/// made it (Story 2.1 per-role receipts); `reservation` is the runtime
/// reservation token (the `spend.reserved` run id) a REAL provider call must
/// carry — the layer refuses spend-bearing calls that arrive without one
/// (Story 2.4, AD-10: enforcement is solely the runtime's).
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub model: String,
    pub temperature: f32,
    pub mission_id: Option<Uuid>,
    pub role: Option<String>,
    pub reservation: Option<String>,
}

impl ChatRequest {
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            messages,
            model: String::new(),
            temperature: 0.4,
            mission_id: None,
            role: None,
            reservation: None,
        }
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

    /// Attach the runtime's reservation token (Story 2.4, AD-10): the
    /// `spend.reserved` run id. The `spend.recorded` event carries it as its
    /// run id so per-run spend folds from the ledger.
    pub fn with_reservation(mut self, run_id: impl Into<String>) -> Self {
        self.reservation = Some(run_id.into());
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
    #[error("local_not_localhost: `{0}` is not a localhost endpoint — the local provider speaks only to localhost (zero egress, FR-24.3/NFR-14) / `{0}` no es un endpoint local — el proveedor local solo habla con localhost (cero salida de datos)")]
    NotLocalhost(String),
    #[error("local_unreachable: the local endpoint `{url}` is not reachable — start the local runtime (e.g. `ollama serve`) or fix the base URL in Ajustes → IA / el endpoint local `{url}` no está disponible — inicia el runtime local (p. ej. `ollama serve`) o corrige la URL base en Ajustes → IA")]
    LocalUnreachable { url: String },
    #[error("cli adapter: {0}")]
    Cli(String),
    #[error("cli_unavailable: the `{0}` CLI was not found on PATH — install it, fix its path in Ajustes → IA, or choose another provider / el CLI `{0}` no está en PATH")]
    CliUnavailable(String),
    #[error("cli_failed: `{cli}` failed: {stderr}")]
    CliFailed { cli: String, stderr: String },
    #[error("no_provider_configured: the assistant needs a real provider (an API provider or a CLI bridge) — configure one in Ajustes → IA / el asistente necesita un proveedor real (un proveedor de API o un puente CLI) — configura uno en Ajustes → IA")]
    NoProviderConfigured,
    #[error("no_reservation: a real provider call must carry a runtime reservation (spend.reserved run id) — enforcement is solely the runtime's (AD-10)")]
    NoReservation,
    #[error("spend ledger append failed (AD-10): {0}")]
    Spend(#[from] EventError),
}

impl ProviderError {
    /// Truncate a provider error body for the `Api` variant (bounded memory).
    pub(crate) fn api_body(body: String) -> String {
        body.chars().take(300).collect()
    }
}

pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

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
    /// A local LLM runtime (Ollama or equivalent) on localhost (Story 6.1,
    /// FR-24): a REAL provider — it satisfies the assistant's real-only rule
    /// (FR-17.1 as amended by FR-24.2) — with zero egress (FR-24.3) and an
    /// honest $0 spend (`note: "local"`, NFR-14). Its token counts are
    /// metered by the local runtime itself.
    Local,
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
    /// The local provider's base URL (Story 6.1, FR-24.1): settings, not the
    /// keychain — a local URL is not a secret. Empty ⇒ the Ollama default
    /// (`http://localhost:11434`).
    pub local_base_url: String,
}

/// Canonical endpoints for the named providers; anything else needs an
/// explicit `base_url` (custom OpenAI-compatible endpoints, Ollama, …).
pub fn default_base_url(name: &str) -> Option<&'static str> {
    match name.trim() {
        "openai" => Some("https://api.openai.com/v1"),
        "anthropic" => Some("https://api.anthropic.com"),
        "google" => Some("https://generativelanguage.googleapis.com"),
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        // The local provider's canonical endpoint is the Ollama default —
        // the native adapter (Story 6.1) speaks /api/chat + /api/tags there.
        "local" => Some(ollama::DEFAULT_BASE_URL),
        _ => None,
    }
}

/// The curated per-provider model lists (Story 5.9, FR-17.4): what the chat
/// header's model picker offers in v1 for a configured API provider. Custom
/// base URLs get an empty list — free entry. CLI bridges are NOT listed
/// here: their picker is exactly ["default"] (the CLI resolves its own
/// model, "vía CLI" in the UI). The live provider's own models list (when
/// reachable) refreshes this via the models endpoint — see
/// `test_provider_connection`.
pub fn curated_models(provider: &str) -> Vec<String> {
    let list: &[&str] = match provider.trim() {
        "openai" => &["gpt-5.2", "gpt-5-mini", "gpt-4.1", "gpt-4o", "gpt-4o-mini"],
        "anthropic" => &[
            "claude-opus-4-5",
            "claude-sonnet-4-5",
            "claude-haiku-4-5",
        ],
        "google" => &["gemini-3-pro", "gemini-2-5-pro", "gemini-2-5-flash"],
        "openrouter" => &[
            "openrouter/auto",
            "anthropic/claude-sonnet-4.5",
            "openai/gpt-5.2",
            "google/gemini-3-pro",
        ],
        _ => &[],
    };
    list.iter().map(|m| m.to_string()).collect()
}

/// The model list a CLI bridge's picker offers (Story 5.8/5.9): exactly one
/// choice — "default", the CLI's own model resolution.
pub fn cli_models() -> Vec<String> {
    vec!["default".into()]
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
            local_base_url: crate::db::get_setting(conn, "local_base_url"),
        }
    }

    /// Would this configuration resolve to the simulated fallback? Mirrors
    /// `ProviderLayer::from_settings`'s resolution so Story 2.1's role
    /// defaults describe the adapter that would actually answer.
    pub fn is_simulated(&self) -> bool {
        match self.mode.trim() {
            "simulate" => true,
            "cli" | "provider" | "local" => false,
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
            "cli" => Self::cli(db, &s),
            "local" => Self::local(db, &s.local_base_url, &s.model),
            "provider" => Self::remote(db, &s),
            // Legacy auto behavior: real provider when key + endpoint exist,
            // else the simulated fallback. The local provider needs no key
            // (FR-24.1) — a configured local name is a real provider.
            _ => {
                if Self::has_real_provider(&s) {
                    Self::remote(db, &s)
                } else {
                    Ok(Self::simulated(db))
                }
            }
        }
    }

    /// The ASSISTANT-only resolution (Story 5.7, FR-17.1/NFR-11): exactly
    /// `from_settings`, except the simulated fallback is refused with the
    /// typed `no_provider_configured:` error instead of silently answering.
    /// Every other flow keeps the simulated guarantee of Stories 1.6/2.1 —
    /// only this constructor never returns `Kind::Simulated`. A configured
    /// LOCAL provider is a real provider (Story 6.1, FR-24.2 — NFR-11's
    /// cloud-only reading is amended): it resolves here, and an endpoint
    /// that is down surfaces at call time as the typed
    /// `local_unreachable:` error — never a silent simulated fallback.
    pub fn from_settings_real(db: &Db, s: ProviderSettings) -> Result<Self, ProviderError> {
        match s.mode.trim() {
            "simulate" => Err(ProviderError::NoProviderConfigured),
            "cli" => Self::cli(db, &s),
            "local" => Self::local(db, &s.local_base_url, &s.model),
            "provider" => Self::remote(db, &s),
            _ => {
                if Self::has_real_provider(&s) {
                    Self::remote(db, &s)
                } else {
                    Err(ProviderError::NoProviderConfigured)
                }
            }
        }
    }

    /// Resolve the assistant's provider from settings + keychain, refusing
    /// the simulated fallback (Story 5.7) — see `from_settings_real`.
    pub fn resolve_real(db: &Db, conn: &Connection) -> Result<Self, ProviderError> {
        Self::from_settings_real(db, ProviderSettings::load(conn))
    }

    fn has_real_provider(s: &ProviderSettings) -> bool {
        // The local provider registers like any cloud provider without a
        // key (Story 6.1, FR-24.1) — it counts as real in auto mode too.
        s.name.trim() == "local"
            || (!s.api_key.trim().is_empty()
                && (!s.base_url.trim().is_empty() || default_base_url(&s.name).is_some()))
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
            let binary = cli::Cli::binary_name(&settings.cli);
            cli::available(&binary)
                .ok_or_else(|| ProviderError::CliUnavailable(binary.to_string()))?;
            return Ok(Self {
                db: db.clone(),
                kind: Kind::Cli,
                name: format!("cli/{binary}"),
                model: role.model.trim().to_string(),
                client: Box::new(cli::Cli::new(&settings.cli)),
            });
        }
        // A local role (Story 6.1, FR-24.2): skills and mission roles run
        // local models like any cloud pair — no key, the settings-stored
        // local base URL, the role's own model.
        if name == "local" {
            return Self::local(db, &settings.local_base_url, role.model.trim());
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
                local_base_url: String::new(),
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

    /// Resolve a CLI bridge adapter (Story 5.8, FR-17.3). Availability is
    /// detected honestly HERE — never a dead spawn: a binary missing from
    /// PATH is the typed `cli_unavailable:` error.
    fn cli(db: &Db, s: &ProviderSettings) -> Result<Self, ProviderError> {
        let binary = cli::Cli::binary_name(&s.cli);
        cli::available(&binary).ok_or_else(|| ProviderError::CliUnavailable(binary.to_string()))?;
        Ok(Self {
            db: db.clone(),
            kind: Kind::Cli,
            name: format!("cli/{binary}"),
            model: s.cli_model.trim().to_string(),
            client: Box::new(cli::Cli::new(&s.cli)),
        })
    }

    /// Resolve the LOCAL provider (Story 6.1, FR-24.1): the Ollama adapter
    /// on the configured local base URL (settings — never the keychain; a
    /// local URL is not a secret), no API key, behind the hard localhost
    /// guard (FR-24.3). A REAL provider kind — the assistant's real-only
    /// rule (FR-17.1 as amended by FR-24.2) is satisfied. Reachability is
    /// deliberately NOT probed here (resolution stays fast): an endpoint
    /// that is down surfaces honestly at call time as the typed
    /// `local_unreachable:` error — never a simulated stand-in.
    fn local(db: &Db, base_url: &str, model: &str) -> Result<Self, ProviderError> {
        let adapter = ollama::Ollama::new(base_url)?;
        Ok(Self {
            db: db.clone(),
            kind: Kind::Local,
            name: "local".into(),
            model: model.trim().to_string(),
            client: Box::new(adapter),
        })
    }

    /// Resolve a real BYOK provider from the registry (openai, anthropic,
    /// google, openrouter, custom endpoints). Fails with typed errors when
    /// the credential, endpoint, or provider name is missing. The local
    /// provider registers like any cloud provider (FR-24.1) — it dispatches
    /// to the local adapter before any key is demanded.
    fn remote(db: &Db, s: &ProviderSettings) -> Result<Self, ProviderError> {
        let name = s.name.trim().to_string();
        if name == "local" {
            return Self::local(db, &s.local_base_url, &s.model);
        }
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
            "openai" | "openrouter" => {
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
    /// A real provider call that arrives WITHOUT a runtime reservation is
    /// refused (Story 2.4, AD-10): adapters never bypass the runtime's
    /// enforcement — the typed `no_reservation:` error names the contract.
    pub async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let req = self.prepare(req)?;
        if self.kind == Kind::Remote && req.reservation.as_deref().unwrap_or("").trim().is_empty() {
            return Err(ProviderError::NoReservation);
        }
        let resp = self.client.chat(req.clone()).await?;
        self.record_spend(&req, &resp).await?;
        Ok(resp)
    }

    /// Streaming variant — see `ChatStream` for the Story 1.6 shape. Same
    /// reservation contract as `chat`.
    pub async fn stream_chat(&self, req: ChatRequest) -> Result<ChatStream, ProviderError> {
        let req = self.prepare(req)?;
        if self.kind == Kind::Remote && req.reservation.as_deref().unwrap_or("").trim().is_empty() {
            return Err(ProviderError::NoReservation);
        }
        let stream = self.client.stream_chat(req.clone()).await?;
        let usage = stream.usage();
        self.record_spend(&req, &ChatResponse { content: String::new(), usage })
            .await?;
        Ok(stream)
    }

    /// Fill the configured model when a request carries none; real providers
    /// refuse to dispatch without one (typed error). CLI bridge calls fall
    /// back to "default" — the CLI's own model resolution (Story 5.8/5.9:
    /// the picker lists exactly ["default"] for a CLI bridge).
    fn prepare(&self, mut req: ChatRequest) -> Result<ChatRequest, ProviderError> {
        if req.model.trim().is_empty() {
            req.model = self.model.clone();
        }
        if self.kind == Kind::Cli && req.model.trim().is_empty() {
            req.model = "default".into();
        }
        if matches!(self.kind, Kind::Remote | Kind::Local) && req.model.trim().is_empty() {
            return Err(ProviderError::MissingModel(self.name.clone()));
        }
        Ok(req)
    }

    /// Append the `spend.recorded` event for a real call (AD-10). Fails
    /// loudly: a call whose spend cannot be recorded must not silently cost
    /// money. CLI bridge calls (Story 5.8) append their 0¢ event with
    /// `note: "cli"` — no metered cost exists to record, so the receipt
    /// says so instead of inventing a number (NFR-4 spirit, AD-10). Local
    /// calls (Story 6.1, FR-24.3/NFR-14) append their 0¢ event with
    /// `note: "local"` — nothing is charged, the receipt says so, and the
    /// local runtime's honest token counts ride verbatim. Only the
    /// simulated fallback appends nothing at all.
    async fn record_spend(
        &self,
        req: &ChatRequest,
        resp: &ChatResponse,
    ) -> Result<(), ProviderError> {
        if self.kind == Kind::Simulated {
            return Ok(());
        }
        let (cost_cents, note) = match self.kind {
            Kind::Cli => (0, Some("cli".to_string())),
            Kind::Local => (0, Some("local".to_string())),
            _ => (pricing::cost_cents(&self.name, &req.model, &resp.usage), None),
        };
        let event = crate::eventstore::NewEvent::spend_recorded(SpendRecordedPayload {
            provider: self.name.clone(),
            model: req.model.clone(),
            input_tokens: resp.usage.input_tokens,
            output_tokens: resp.usage.output_tokens,
            cost_cents,
            mission_id: req.mission_id,
            role: req.role.clone(),
            run_id: req.reservation.clone(),
            note,
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

/// The chat send-path tests' assistant layer (Stories 5.7–5.9): a
/// REAL-kind (Remote) layer over the simulated echo client — the assistant
/// path can never resolve to `Kind::Simulated` (NFR-11), so its tests run
/// against a remote-shaped stand-in that echoes the prompt markers without
/// network. Constructed only here, behind cfg(test).
#[cfg(test)]
pub(crate) fn test_remote_simulated(db: &Db) -> ProviderLayer {
    ProviderLayer {
        db: db.clone(),
        kind: Kind::Remote,
        name: "test-remote".into(),
        model: "test-model".into(),
        client: Box::new(simulated::Simulated),
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

/// A remote-shaped layer around an arbitrary test client (Story 6.9 test
/// seam): the support engine's tests answer PER REQUEST (a judge that reads
/// the claim out of the prompt), which the fixed-content `FakeRemote`
/// cannot — this constructor hands the layer a caller-built client.
#[cfg(test)]
pub(crate) fn remote_layer_with_client(
    db: &Db,
    name: &str,
    model: &str,
    client: Box<dyn ProviderClient>,
) -> ProviderLayer {
    ProviderLayer {
        db: db.clone(),
        kind: Kind::Remote,
        name: name.into(),
        model: model.into(),
        client,
    }
}

/// A fake remote that sleeps before answering (Story 2.4 test seam): keeps a
/// reservation IN FLIGHT across an await point, so the TOCTOU test can prove
/// a concurrent dispatch sees it and cannot overshoot the ceiling (AD-10).
#[cfg(test)]
pub(crate) struct SlowRemote {
    pub content: &'static str,
    pub usage: Usage,
    pub delay_ms: u64,
}

#[cfg(test)]
impl ProviderClient for SlowRemote {
    fn name(&self) -> &str {
        "fake-slow"
    }
    fn chat(&self, _req: ChatRequest) -> BoxFuture<'_, Result<ChatResponse, ProviderError>> {
        Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            Ok(ChatResponse { content: self.content.to_string(), usage: self.usage })
        })
    }
}

/// A remote-shaped layer around `SlowRemote` (Story 2.4 test seam).
#[cfg(test)]
pub(crate) fn slow_remote_layer(
    db: &Db,
    name: &str,
    model: &str,
    content: &'static str,
    usage: Usage,
    delay_ms: u64,
) -> ProviderLayer {
    ProviderLayer {
        db: db.clone(),
        kind: Kind::Remote,
        name: name.into(),
        model: model.into(),
        client: Box::new(SlowRemote { content, usage, delay_ms }),
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
            .chat(ChatRequest::new(messages).with_temperature(0.4).with_reservation("test-run"))
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
            .chat(ChatRequest::new(vec![Message::user("hola")]).with_reservation("test-run"))
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
        layer.chat(ChatRequest::new(vec![Message::user("otra")]).with_reservation("test-run")).await.unwrap();
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
                    schedule: "daily-03:00".into(),
                roles: vec![],
                }).unwrap())
                .unwrap()
        };

        layer
            .chat(ChatRequest::new(vec![Message::user("run")]).for_mission(mission.id).with_reservation("test-run"))
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
            .stream_chat(ChatRequest::new(vec![Message::user("hola")]).with_reservation("test-run"))
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
            .chat(ChatRequest::new(vec![Message::user("x")]).with_role("critic").with_reservation("test-run"))
            .await
            .unwrap();
        let events = spend_events(&db).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload["role"], json!("critic"));
        // untagged calls (the Asistente chat flow) record no role field
        layer.chat(ChatRequest::new(vec![Message::user("y")]).with_reservation("test-run")).await.unwrap();
        let events = spend_events(&db).await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].payload.get("role"), None);
    }

    #[tokio::test]
    async fn remote_call_without_a_model_is_a_typed_error() {
        let db = test_db();
        let mut layer = remote_layer(&db);
        layer.model = String::new();
        let err = layer.chat(ChatRequest::new(vec![Message::user("x")]).with_reservation("test-run")).await;
        assert!(
            matches!(err, Err(ProviderError::MissingModel(ref p)) if p == "custom"),
            "expected MissingModel, got {err:?}"
        );
        // nothing was spent on a refused call
        assert!(spend_events(&db).await.is_empty());
    }

    /// Story 2.4 (AD-10): a real provider call that arrives WITHOUT a runtime
    /// reservation is refused by the adapter itself — typed `no_reservation:`,
    /// nothing spent. Enforcement is solely the runtime's; the adapter is the
    /// last line of defense, not the first.
    #[tokio::test]
    async fn a_real_call_without_a_runtime_reservation_is_refused() {
        let db = test_db();
        let layer = remote_layer(&db);
        let err = layer.chat(ChatRequest::new(vec![Message::user("x")])).await;
        assert!(
            matches!(err, Err(ProviderError::NoReservation)),
            "expected NoReservation, got {err:?}"
        );
        assert!(spend_events(&db).await.is_empty(), "nothing was spent on a refused call");
        // a blank reservation is as good as none
        let err = layer
            .chat(ChatRequest::new(vec![Message::user("x")]).with_reservation("   "))
            .await;
        assert!(matches!(err, Err(ProviderError::NoReservation)));
        // simulated and CLI calls need no reservation (they cost nothing)
        let sim = ProviderLayer::simulated(&db);
        assert!(sim.chat(ChatRequest::new(vec![Message::user("x")])).await.is_ok());
    }

    #[test]
    fn registry_resolves_every_named_provider() {
        for (name, attribution) in [
            ("openai", "openai"),
            ("anthropic", "anthropic"),
            ("google", "google"),
            ("openrouter", "openrouter"),
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
                local_base_url: String::new(),
            };
            let layer = ProviderLayer::from_settings(&db, settings)
                .unwrap_or_else(|e| panic!("{name} must resolve: {e}"));
            assert_eq!(layer.kind(), Kind::Remote, "{name}");
            assert_eq!(layer.name(), attribution);
        }
    }

    /// Story 6.1 (FR-24.1/24.2): the local provider registers like any
    /// cloud provider — no API key, its own settings-stored base URL, the
    /// native Ollama adapter — and it is a REAL provider kind on BOTH
    /// resolution paths (the assistant's real-only rule, FR-17.1 as amended
    /// by FR-24.2, is satisfied). A non-localhost base URL is the typed
    /// zero-egress refusal (FR-24.3) — never an OpenAI-compatible escape
    /// hatch to a remote host.
    #[test]
    fn the_local_provider_registers_as_a_real_provider_without_a_key() {
        let db = test_db();
        let local = ProviderSettings {
            mode: "local".into(),
            name: "local".into(),
            base_url: String::new(),
            api_key: String::new(),
            model: "llama3.1:8b".into(),
            cli: String::new(),
            cli_model: String::new(),
            local_base_url: String::new(),
        };
        // the canonical default endpoint when no base URL is stored
        let layer = ProviderLayer::from_settings(&db, local.clone()).unwrap();
        assert_eq!(layer.kind(), Kind::Local);
        assert_eq!(layer.name(), "local");
        assert_eq!(layer.model(), "llama3.1:8b");
        // the ASSISTANT resolution accepts it — never simulated (FR-24.2)
        let layer = ProviderLayer::from_settings_real(&db, local.clone()).unwrap();
        assert_eq!(layer.kind(), Kind::Local);
        // provider mode + name "local" dispatches to the local adapter too
        let layer = ProviderLayer::from_settings(
            &db,
            ProviderSettings {
                mode: "provider".into(),
                name: "local".into(),
                base_url: String::new(),
                api_key: String::new(),
                model: "qwen2.5:14b".into(),
                cli: String::new(),
                cli_model: String::new(),
                local_base_url: "http://127.0.0.1:11434".into(),
            },
        )
        .unwrap();
        assert_eq!(layer.kind(), Kind::Local);
        // auto mode + name "local" is a real provider (no key needed)
        let layer = ProviderLayer::from_settings(
            &db,
            ProviderSettings {
                mode: String::new(),
                name: "local".into(),
                base_url: String::new(),
                api_key: String::new(),
                model: "llama3.1:8b".into(),
                cli: String::new(),
                cli_model: String::new(),
                local_base_url: String::new(),
            },
        )
        .unwrap();
        assert_eq!(layer.kind(), Kind::Local);
        // a non-localhost base URL is the typed refusal, both paths
        let mut remote = local.clone();
        remote.mode = "local".into();
        remote.local_base_url = "http://10.0.0.5:11434".into();
        for result in [
            ProviderLayer::from_settings(&db, remote.clone()),
            ProviderLayer::from_settings_real(&db, remote),
        ] {
            assert!(
                matches!(result, Err(ProviderError::NotLocalhost(ref u)) if u == "http://10.0.0.5:11434"),
                "a LAN endpoint must be refused (zero egress, FR-24.3)"
            );
        }
    }

    /// Story 6.1 (FR-24.3/NFR-14): a local call appends its 0¢
    /// `spend.recorded` with `note: "local"` and the runtime's honest token
    /// counts — a price is never invented where none is charged — and needs
    /// NO runtime reservation (nothing can overshoot a $0 ceiling).
    #[tokio::test]
    async fn local_calls_record_zero_cent_spend_with_the_local_note() {
        let db = test_db();
        let layer = ProviderLayer {
            db: db.clone(),
            kind: Kind::Local,
            name: "local".into(),
            model: "llama3.1:8b".into(),
            client: Box::new(FakeRemote {
                content: "respuesta del modelo local",
                usage: Usage { input_tokens: 320, output_tokens: 140 },
            }),
        };
        // no reservation attached — local calls cost nothing (CLI precedent)
        let resp = layer
            .chat(ChatRequest::new(vec![Message::user("hola")]))
            .await
            .unwrap();
        assert_eq!(resp.content, "respuesta del modelo local");
        let events = spend_events(&db).await;
        assert_eq!(events.len(), 1, "a local call records its (free) spend");
        let payload = &events[0].payload;
        assert_eq!(payload["provider"], "local");
        assert_eq!(payload["model"], "llama3.1:8b");
        assert_eq!(payload["cost_cents"], json!(0), "never an invented price (NFR-14)");
        assert_eq!(payload["note"], json!("local"));
        assert_eq!(payload["input_tokens"], json!(320), "honest token counts ride verbatim");
        assert_eq!(payload["output_tokens"], json!(140));
    }

    /// Story 6.1: a local layer without a model is the typed refusal — the
    /// picker's list is live from the endpoint, an empty choice never
    /// dispatches.
    #[tokio::test]
    async fn a_local_layer_without_a_model_is_a_typed_error() {
        let db = test_db();
        let mut layer = ProviderLayer {
            db: db.clone(),
            kind: Kind::Local,
            name: "local".into(),
            model: String::new(),
            client: Box::new(FakeRemote { content: "x", usage: Usage::ZERO }),
        };
        let err = layer.chat(ChatRequest::new(vec![Message::user("x")])).await;
        assert!(
            matches!(err, Err(ProviderError::MissingModel(ref p)) if p == "local"),
            "expected MissingModel, got {err:?}"
        );
        assert!(spend_events(&db).await.is_empty());
    }

    /// Story 6.1 (FR-24.2): a role configured on the local provider —
    /// skills and mission roles — resolves the local adapter with the role's
    /// own model, no key demanded (mixed local/remote configs are exactly as
    /// first-class as remote/remote).
    #[tokio::test]
    async fn a_local_role_resolves_without_a_key() {
        let db = test_db();
        {
            let conn = db.0.lock().await;
            crate::db::set_setting(&conn, "local_base_url", "http://127.0.0.1:11434").unwrap();
        }
        let layer = {
            let conn = db.0.lock().await;
            ProviderLayer::for_role(
                &db,
                &conn,
                &crate::domain::missions::RoleConfig::drafter("local", "llama3.1:8b"),
            )
            .unwrap()
        };
        assert_eq!(layer.kind(), Kind::Local);
        assert_eq!(layer.name(), "local");
        assert_eq!(layer.model(), "llama3.1:8b");
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
                local_base_url: String::new(),
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
            local_base_url: String::new(),
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

    /// Story 5.7 (FR-17.1/NFR-11): the ASSISTANT resolution
    /// (`from_settings_real`) never returns the simulated provider — no key,
    /// simulate mode, all of it is the typed `no_provider_configured:`
    /// refusal. The legacy resolution (`from_settings`) keeps the simulated
    /// fallback for every other flow (the amendment is scoped).
    #[test]
    fn the_assistant_resolution_refuses_simulated_every_way_it_could_get_there() {
        let db = test_db();
        let base = ProviderSettings {
            mode: String::new(),
            name: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            cli: String::new(),
            cli_model: String::new(),
            local_base_url: String::new(),
        };
        // auto mode, nothing configured => refused (not simulated)
        assert!(matches!(
            ProviderLayer::from_settings_real(&db, base.clone()),
            Err(ProviderError::NoProviderConfigured)
        ));
        // explicit simulate mode => refused
        let mut s = base.clone();
        s.mode = "simulate".into();
        assert!(matches!(
            ProviderLayer::from_settings_real(&db, s),
            Err(ProviderError::NoProviderConfigured)
        ));
        // a configured key + endpoint => a real remote layer
        let mut s = base.clone();
        s.mode = "provider".into();
        s.name = "openai".into();
        s.api_key = "sk-test".into();
        s.model = "gpt-5-mini".into();
        let layer = ProviderLayer::from_settings_real(&db, s).unwrap();
        assert_eq!(layer.kind(), Kind::Remote);
        // and the legacy path still falls back for the other flows
        let mut s = base;
        s.name = "openai".into();
        assert_eq!(
            ProviderLayer::from_settings(&db, s).unwrap().kind(),
            Kind::Simulated,
            "the non-assistant flows keep the simulated guarantee (Stories 1.6/2.1)"
        );
    }

    /// Story 5.8: a CLI bridge resolves as a real provider adapter with its
    /// honest `cli/<binary>` attribution — and a missing binary is the typed
    /// `cli_unavailable:` refusal, never a dead spawn.
    #[test]
    fn cli_bridges_resolve_or_refuse_honestly() {
        let db = test_db();
        let claude_available = cli::available("claude").is_some();
        let s = ProviderSettings {
            mode: "cli".into(),
            name: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            cli: "claude".into(),
            cli_model: String::new(),
            local_base_url: String::new(),
        };
        if claude_available {
            let layer = ProviderLayer::from_settings_real(&db, s).unwrap();
            assert_eq!(layer.kind(), Kind::Cli);
            assert_eq!(layer.name(), "cli/claude");
        } else {
            assert!(matches!(
                ProviderLayer::from_settings_real(&db, s),
                Err(ProviderError::CliUnavailable(ref c)) if c == "claude"
            ));
        }
        // a binary nobody has is refused with the typed error both ways
        let missing = ProviderSettings {
            mode: "cli".into(),
            name: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            cli: "rc-definitely-missing-cli".into(),
            cli_model: String::new(),
            local_base_url: String::new(),
        };
        assert!(matches!(
            ProviderLayer::from_settings_real(&db, missing.clone()),
            Err(ProviderError::CliUnavailable(ref c)) if c == "rc-definitely-missing-cli"
        ));
        // the legacy path refuses the same way (mode "cli" is explicit)
        assert!(matches!(
            ProviderLayer::from_settings(&db, missing),
            Err(ProviderError::CliUnavailable(_))
        ));
    }

    /// Story 5.8 (AD-10): a CLI bridge call appends its 0¢ `spend.recorded`
    /// with `note: "cli"` — the receipt says the cost is not observable
    /// instead of inventing a number. Tokens ride verbatim (the adapter
    /// reports ZERO usage — nothing is metered).
    #[tokio::test]
    async fn cli_calls_record_zero_cent_spend_with_the_cli_note() {
        let db = test_db();
        let layer = ProviderLayer {
            db: db.clone(),
            kind: Kind::Cli,
            name: "cli/codex".into(),
            model: String::new(),
            client: Box::new(FakeRemote {
                content: "respuesta del cli",
                usage: Usage::ZERO,
            }),
        };
        let resp = layer
            .chat(ChatRequest::new(vec![Message::user("hola")]))
            .await
            .unwrap();
        assert_eq!(resp.content, "respuesta del cli");
        let events = spend_events(&db).await;
        assert_eq!(events.len(), 1, "a CLI call records its (free) spend");
        let payload = &events[0].payload;
        assert_eq!(payload["provider"], "cli/codex");
        assert_eq!(payload["model"], "default", "the CLI picker's one choice");
        assert_eq!(payload["cost_cents"], json!(0));
        assert_eq!(payload["note"], json!("cli"));
        assert_eq!(payload["input_tokens"], json!(0));
        assert_eq!(payload["output_tokens"], json!(0));
    }

    /// Story 5.7's BYOK gate, end to end: a configured API provider routes
    /// through the keychain-stored key — a one-shot local axum server
    /// stands in for the provider and captures the request, proving the
    /// Authorization header carries the key and the request body's model
    /// is the chat's chosen one (never the layer default).
    #[tokio::test]
    async fn a_configured_api_provider_routes_through_the_byok_key() {
        use axum::extract::Request;
        use std::sync::{Arc, Mutex};
        const SENTINEL: &str = "sk-5-7-BYOK-SENTINEL-9e2f";

        // one-shot local "provider" on the shared runtime
        let captured: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let seen = captured.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(move |req: Request<axum::body::Body>| {
                let seen = seen.clone();
                async move {
                    let (parts, body) = req.into_parts();
                    let auth = parts
                        .headers
                        .get(reqwest::header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    let bytes = axum::body::to_bytes(body, 1_048_576)
                        .await
                        .unwrap_or_default();
                    *seen.lock().unwrap() =
                        Some(format!("{auth}\n{}", String::from_utf8_lossy(&bytes)));
                    axum::Json(serde_json::json!({
                        "choices": [{ "message": { "content": "hola desde el proveedor" } }],
                        "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
                    }))
                }
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let db = test_db();
        {
            let conn = db.0.lock().await;
            crate::db::set_setting(&conn, "llm_mode", "provider").unwrap();
            crate::db::set_setting(&conn, "provider", "custom").unwrap();
            crate::db::set_setting(&conn, "base_url", &format!("http://{addr}/v1")).unwrap();
            crate::db::set_setting(&conn, "model", "layer-default-model").unwrap();
            // the key: keychain when available, else the legacy settings
            // fallback (both are the BYOK store — never the prompt)
            match keyring::Entry::new(
                crate::eventstore::migration::KEYCHAIN_SERVICE,
                "custom",
            ) {
                Ok(entry) if entry.set_password(SENTINEL).is_ok() => {}
                _ => {
                    crate::db::set_setting(&conn, "api_key", SENTINEL).unwrap();
                }
            }
        }

        let layer = {
            let conn = db.0.lock().await;
            ProviderLayer::resolve_real(&db, &conn).unwrap()
        };
        assert_eq!(layer.kind(), Kind::Remote);
        // the chat's chosen model overrides the layer's configured default
        let resp = layer
            .chat(
                ChatRequest::new(vec![Message::user("hola")])
                    .with_model("chosen-by-the-picker")
                    .with_reservation("test-run"),
            )
            .await
            .unwrap();
        assert_eq!(resp.content, "hola desde el proveedor");

        let seen = captured.lock().unwrap().clone().expect("the provider saw the request");
        let (auth, body) = seen.split_once('\n').unwrap();
        assert_eq!(auth, format!("Bearer {SENTINEL}"), "the call carries the keychain-stored key");
        assert!(body.contains("\"chosen-by-the-picker\""), "the chosen model rides the call: {body}");
        assert!(
            !body.contains("layer-default-model"),
            "the picker's model overrides the layer default: {body}"
        );
        // spend recorded for the metered call, attributed to provider+model
        let events = spend_events(&db).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload["provider"], "custom");
        assert_eq!(events[0].payload["model"], "chosen-by-the-picker");
        // no event ever carries the key (AD-16)
        {
            let conn = db.0.lock().await;
            let all = EventStore::new(&conn).events_all().unwrap();
            for event in &all {
                assert!(
                    !serde_json::to_string(event).unwrap().contains(SENTINEL),
                    "event {} leaked the api key",
                    event.seq
                );
            }
            crate::db::set_setting(&conn, "api_key", "").unwrap();
        }
        if let Ok(entry) = keyring::Entry::new(
            crate::eventstore::migration::KEYCHAIN_SERVICE,
            "custom",
        ) {
            let _ = entry.delete_credential();
        }
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
            local_base_url: String::new(),
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
            local_base_url: String::new(),
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
        layer.chat(ChatRequest::new(vec![Message::user("hola")]).with_reservation("test-run")).await.unwrap();

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
