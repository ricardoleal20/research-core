// The bridge (Story 6.14, FR-21.1, NFR-13, AD-7): the ONE pluggable channel
// every remote surface reaches the home machine through — never a second
// network path, never a second writer. Off by default; enabling it is an
// explicit setting (`RC_BRIDGE=off|tunnel|chopflow`, or the `enable_bridge`
// command). ONE channel at a time: a second start is refused with
// `bridge_already_active:`.
//
// Security model (single writer, AD-14, preserved by construction):
// - Pairing is required. Every `/api` request through the bridge carries the
//   `X-RC-Pairing` header; its sha-256 must match a paired device's row.
//   Unpaired requests are refused with `unpaired:` — no open endpoints, no
//   unauthenticated requests. The `/m` shell itself is static (no data).
// - Remote surfaces READ through the same read-only API slice the localhost
//   server serves, and DECIDE through the SAME typed core commands the
//   desktop review surface uses (`proposals::approve` / `reject`, the
//   quick-capture command) — executed in-process, on the home machine, by
//   the home writer. The bridge process never appends to the store
//   directly; there is no remote append endpoint, and remote decisions are
//   evented with actor=user + surface attribution (`mobile`).
// - The tunnel adapter binds a configurable address (default
//   `0.0.0.0:4762`) and serves the mobile companion at `/m` plus the read
//   and remote-command slice — the owner's own VPN/Tailscale/SSH
//   reachability carries it (default adapter when the bridge is on).
// - The ChopFlow adapter is first-class but optional (off by default,
//   never a dependency): the home machine connects OUTBOUND to a ChopFlow
//   deployment, pushes verdict-summary notifications, and polls for remote
//   commands it then executes through the same typed path. An
//   unconfigured/unreachable ChopFlow is an honest error, never a crash.

use crate::db::Db;
use crate::domain::bridge as bridge_domain;
use crate::domain::proposals::{self, ApproveOutcome, Proposal};
use crate::eventstore::EventStore;
use crate::server::{read_api_router, ServerState};
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Mutex;
use tower_http::services::ServeDir;
use uuid::Uuid;

/// Default bridge listen address (override with `RC_BRIDGE_ADDR` or
/// `RC_BRIDGE_PORT`). The bridge is OFF unless `RC_BRIDGE` says otherwise —
/// nothing listens beyond the existing localhost view unless enabled.
pub const DEFAULT_BRIDGE_ADDR: &str = "0.0.0.0:4762";
/// How often the ChopFlow adapter polls its deployment for remote commands.
const CHOPFLOW_POLL_SECS: u64 = 30;

/// The bridge's channel modes (FR-21.1). `Off` is the default — the bridge
/// only exists when the owner explicitly enables it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeMode {
    Off,
    Tunnel,
    ChopFlow,
}

impl BridgeMode {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "tunnel" => Some(Self::Tunnel),
            "chopflow" => Some(Self::ChopFlow),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Tunnel => "tunnel",
            Self::ChopFlow => "chopflow",
        }
    }
}

/// Everything that can go wrong enabling/disabling the bridge — typed, with
/// stable codes the UI never translates (EXPERIENCE.md).
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("bridge_already_active: a bridge channel is already active ({active}) — one channel at a time (AD-7); disable it first")]
    AlreadyActive { active: String },
    #[error("bridge_bind_failed: cannot listen on {addr}: {source}")]
    BindFailed { addr: String, source: std::io::Error },
    #[error("chopflow_unconfigured: the ChopFlow adapter needs a deployment base URL (RC_CHOPFLOW_URL) and its pairing token (RC_CHOPFLOW_TOKEN) — an unconfigured ChopFlow is an honest error, never a crash (FR-22.3 spirit)")]
    ChopFlowUnconfigured,
    #[error("invalid_listen_addr: `{raw}` — expected `host:port` (e.g. 0.0.0.0:4762)")]
    InvalidListenAddr { raw: String },
    #[error(transparent)]
    Store(#[from] crate::eventstore::EventError),
}

/// The bridge status read (FR-21.1): what channel (if any) is active, what
/// it listens on / talks to, and how many devices are paired. Field names
/// are camelCase on the wire (Tauri 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStatusView {
    /// The active mode — `off` when the bridge is disabled.
    pub mode: String,
    pub active: bool,
    /// Human description of the active channel (`tunnel 0.0.0.0:4762`,
    /// `chopflow https://chopflow.example`); None when off.
    pub describe: Option<String>,
    pub paired_devices: usize,
}

/// A verdict-summary notification (Story 6.16, FR-21.4, NFR-13): what a
/// push carries — a summary line in code form, never research content
/// beyond it. Served by the bridge's `/api/notifications` read and pushed
/// through the active adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationItem {
    /// `proposal` (pending quarantine proposal) | `digest` (morning digest
    /// ready).
    pub kind: String,
    pub seq: i64,
    pub ts: chrono::DateTime<chrono::Utc>,
    pub proposal_id: Option<Uuid>,
    /// Verdict summary in code form (e.g. `pr-12 · hypothesis.status_changed
    /// → testing`) — summaries only, never statements or excerpts (NFR-13
    /// extending NFR-1).
    pub summary: String,
    pub basis_stale: bool,
}

/// The one bridge-adapter interface (AD-7, FR-21.1): every channel a remote
/// surface reaches the home machine through implements this. v0.2.0 ships
/// two — the direct tunnel (the default when the bridge is on) and ChopFlow
/// (first-class, optional, never a dependency). Both satisfy the same
/// contract: same reads, same typed remote commands, same summary-only push
/// payload, and neither ever appends to the store directly.
pub trait BridgeAdapter: Send + Sync {
    fn mode(&self) -> BridgeMode;
    fn describe(&self) -> String;
    /// Best-effort push delivery of a verdict-summary notification. The
    /// tunnel delivers by reachability — the paired surface polls while the
    /// machine is reachable, so delivery is a no-op Ok. ChopFlow delivers
    /// through its channel; an `Err` is a visible failure the caller
    /// records as a `bridge.push_failed` event — never a silent miss.
    async fn deliver(&self, notice: &NotificationItem) -> Result<(), String>;
}

// ---------------------------------------------------------------------------
// The tunnel adapter — direct reachability (the default)
// ---------------------------------------------------------------------------

/// The direct tunnel (FR-21.1): the owner's own VPN/Tailscale/SSH
/// reachability carries the paired surface to the bridge server. Push
/// delivery is the paired surface's poll — when the machine is reachable
/// the notifications read answers; when it is not, the companion renders
/// the honest offline state (FR-9.1 spirit), never stale data as live.
#[derive(Debug, Clone)]
pub struct TunnelAdapter;

impl BridgeAdapter for TunnelAdapter {
    fn mode(&self) -> BridgeMode {
        BridgeMode::Tunnel
    }

    fn describe(&self) -> String {
        "tunnel (direct reachability)".into()
    }

    async fn deliver(&self, _notice: &NotificationItem) -> Result<(), String> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The ChopFlow adapter — first-class, optional (off by default)
// ---------------------------------------------------------------------------

/// A remote command polled from a ChopFlow deployment (FR-21.1): the same
/// typed verbs the bridge's HTTP slice serves — approve / reject (Story
/// 6.16) and quick-capture (Story 6.15). The adapter executes each through
/// the home writer; the deployment never touches the store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCommand {
    pub id: String,
    /// `approve` | `reject` | `capture`
    pub kind: String,
    #[serde(default)]
    pub proposal_id: Option<Uuid>,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub question: Option<String>,
}

/// The ChopFlow HTTP client (FR-21.1, mirroring the provider-client shape):
/// an outbound connection to the user's ChopFlow deployment. The documented
/// slice, all authorized with the pairing token as a bearer:
/// - `GET  {base}/v1/devices/{device}/commands` — pending remote commands
/// - `POST {base}/v1/devices/{device}/commands/{id}/result` — the typed
///   command's outcome (ok or the typed error string)
/// - `POST {base}/v1/devices/{device}/notifications` — verdict-summary push
#[derive(Debug, Clone)]
pub struct ChopFlowClient {
    base_url: String,
    token: String,
    device: String,
}

impl ChopFlowClient {
    pub fn new(base_url: &str, token: &str, device: &str) -> Self {
        Self {
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            token: token.to_string(),
            device: device.to_string(),
        }
    }

    fn client() -> Result<reqwest::Client, String> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(|e| format!("chopflow_request: cannot build client: {e}"))
    }

    /// Poll the deployment for pending remote commands.
    pub async fn poll_commands(&self) -> Result<Vec<BridgeCommand>, String> {
        let url = format!(
            "{}/v1/devices/{}/commands",
            self.base_url, self.device
        );
        let resp = Self::client()?
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| format!("chopflow_request: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("chopflow_api: {status} — {body}"));
        }
        let v: Value = resp
            .json()
            .await
            .map_err(|e| format!("chopflow_request: {e}"))?;
        Ok(v["commands"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|r| serde_json::from_value(r.clone()).ok())
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Post a command's typed outcome back to the deployment.
    pub async fn post_result(&self, command_id: &str, ok: bool, error: Option<&str>) -> Result<(), String> {
        let url = format!(
            "{}/v1/devices/{}/commands/{command_id}/result",
            self.base_url, self.device
        );
        let body = json!({ "ok": ok, "error": error });
        let resp = Self::client()?
            .post(&url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("chopflow_request: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("chopflow_api: {status} — {body}"));
        }
        Ok(())
    }

    /// Deliver a verdict-summary notification through the deployment's
    /// channel.
    pub async fn deliver(&self, notice: &NotificationItem) -> Result<(), String> {
        let url = format!(
            "{}/v1/devices/{}/notifications",
            self.base_url, self.device
        );
        let resp = Self::client()?
            .post(&url)
            .bearer_auth(&self.token)
            .json(notice)
            .send()
            .await
            .map_err(|e| format!("chopflow_request: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("chopflow_api: {status} — {body}"));
        }
        Ok(())
    }
}

/// The ChopFlow adapter: satisfies the same bridge-adapter contract as the
/// tunnel. Carries the client; the loop (`spawn_chopflow_loop`) polls,
/// executes remote commands through the home writer, and pushes
/// notifications.
#[derive(Debug, Clone)]
pub struct ChopFlowAdapter {
    client: ChopFlowClient,
}

impl ChopFlowAdapter {
    pub fn new(client: ChopFlowClient) -> Self {
        Self { client }
    }
}

impl BridgeAdapter for ChopFlowAdapter {
    fn mode(&self) -> BridgeMode {
        BridgeMode::ChopFlow
    }

    fn describe(&self) -> String {
        format!("chopflow {}", self.client.base_url)
    }

    async fn deliver(&self, notice: &NotificationItem) -> Result<(), String> {
        self.client.deliver(notice).await
    }
}

/// One poll cycle (the loop's body, isolated for tests): fetch pending
/// commands, execute each through the SAME typed core commands the desktop
/// surface uses (single writer, AD-14 — surface `mobile`), and post each
/// outcome. Returns the number of commands executed. Poll/transport errors
/// are returned honestly — the loop logs them; only push-delivery failures
/// are evented (`bridge.push_failed`).
pub async fn chopflow_run_once(db: &Db, client: &ChopFlowClient) -> Result<usize, String> {
    let commands = client.poll_commands().await?;
    let mut executed = 0;
    for cmd in &commands {
        let outcome = execute_remote_command(db, cmd).await;
        let (ok, error) = match outcome {
            Ok(_) => (true, None),
            Err(e) => (false, Some(e)),
        };
        client.post_result(&cmd.id, ok, error.as_deref()).await?;
        executed += 1;
    }
    Ok(executed)
}

/// The ChopFlow adapter loop: poll → execute → report, every
/// `CHOPFLOW_POLL_SECS`. A down deployment is an honest logged error, never
/// a crash and never a silent miss of an executed command's result.
async fn chopflow_loop(db: Db, client: ChopFlowClient) {
    loop {
        if let Err(e) = chopflow_run_once(&db, &client).await {
            eprintln!("[bridge:chopflow] poll cycle failed: {e}");
        }
        tokio::time::sleep(std::time::Duration::from_secs(CHOPFLOW_POLL_SECS)).await;
    }
}

// ---------------------------------------------------------------------------
// Remote commands — the typed slice (single writer, AD-14)
// ---------------------------------------------------------------------------

/// Execute one remote command in-process, through the home writer — the
/// SAME typed core commands the desktop review surface calls. The bridge
/// never appends to the store directly; there is no remote append path.
/// Remote decisions are evented with actor=user and surface `mobile`
/// (NFR-13).
pub(crate) async fn execute_remote_command(
    db: &Db,
    cmd: &BridgeCommand,
) -> Result<Value, String> {
    match cmd.kind.as_str() {
        "approve" => {
            let proposal_id = cmd
                .proposal_id
                .ok_or("invalid_command: an approve names its proposal (proposal_id)")?;
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let outcome: ApproveOutcome =
                proposals::approve(&store, proposal_id, cmd.force, Some("mobile"))
                    .map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(&outcome).map_err(|e| e.to_string())?)
        }
        "reject" => {
            let proposal_id = cmd
                .proposal_id
                .ok_or("invalid_command: a reject names its proposal (proposal_id)")?;
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let proposal: Proposal =
                proposals::reject(&store, proposal_id, Some("mobile"))
                    .map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(&proposal).map_err(|e| e.to_string())?)
        }
        other => Err(format!(
            "invalid_command: `{other}` — the remote vocabulary is approve | reject (PRD §10)"
        )),
    }
}

// ---------------------------------------------------------------------------
// The bridge server (tunnel mode)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct BridgeState {
    db: Db,
    dist_dir: std::path::PathBuf,
}

/// The bridge router: the same read-only API slice the localhost server
/// serves (one read contract), plus the mobile companion shell at `/m` and
/// the explicitly typed remote-command endpoints. Everything under `/api`
/// is behind pairing (the `/m` shell and static assets carry no data).
pub fn bridge_router(
    db: Db,
    dist_dir: std::path::PathBuf,
    data_dir: std::path::PathBuf,
) -> Router {
    let core = ServerState { db: db.clone(), data_dir: data_dir.clone() };
    let mobile = Router::new()
        .route("/m", get(mobile_page))
        .route(
            "/api/proposals/{proposal_id}/approve",
            post(remote_approve),
        )
        .route("/api/proposals/{proposal_id}/reject", post(remote_reject))
        .with_state(BridgeState {
            db,
            dist_dir: dist_dir.clone(),
        });
    read_api_router()
        .with_state(core.clone())
        .merge(mobile)
        .layer(middleware::from_fn_with_state(core, paired_only))
        .fallback_service(ServeDir::new(dist_dir))
}

/// Pairing gate (NFR-13): every `/api` request must carry a pairing token
/// (`X-RC-Pairing`) whose sha-256 matches a paired device. A missing or
/// unknown token is the same refusal — `unpaired:`, never data. The `/m`
/// shell and static assets are exempt: they carry no data until paired.
async fn paired_only(
    State(state): State<ServerState>,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if !path.starts_with("/api/") {
        return next.run(req).await;
    }
    let token = req
        .headers()
        .get("x-rc-pairing")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let device = {
        let c = state.db.0.lock().await;
        bridge_domain::verify_pairing(&c, &token)
    };
    match device {
        Ok(Some(_device)) => next.run(req).await,
        Ok(None) => unpaired_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("pairing_check_failed: {e}") })),
        )
            .into_response(),
    }
}

fn unpaired_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": "unpaired: this device has no pairing with this ResearchCore deployment — pair it from Ajustes → Puente on the desktop app and enter the token on this surface"
        })),
    )
        .into_response()
}

/// The mobile companion shell (`/m`, Story 6.15): the same built UI the
/// desktop webview loads — the app boots into the mobile companion view
/// when served at this path. The shell carries no data; every read stays
/// behind pairing.
async fn mobile_page(State(state): State<BridgeState>) -> Response {
    let path = state.dist_dir.join("index.html");
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            )],
            bytes,
        )
            .into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            "UI build not found — run `npm run build` (the /m companion ships with the built UI)".to_string(),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApproveBody {
    #[serde(default)]
    force: bool,
}

/// One-tap approve (Story 6.16): the SAME typed merge command the desktop
/// review surface calls, executed by the home writer with surface
/// attribution `mobile`. A stale basis is refused with `basis_stale:`
/// unless `force` — a forced merge records the marker; never a blind merge
/// (AD-13).
async fn remote_approve(
    State(state): State<BridgeState>,
    Path(proposal_id): Path<String>,
    body: Result<Json<ApproveBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let force = body.map(|Json(b)| b.force).unwrap_or(false);
    let proposal_id: Uuid = match proposal_id.parse() {
        Ok(id) => id,
        Err(e) => return bad_request(format!("invalid proposal id `{proposal_id}`: {e}")),
    };
    let c = state.db.0.lock().await;
    let store = EventStore::new(&c);
    match proposals::approve(&store, proposal_id, force, Some("mobile")) {
        Ok(outcome) => Json(outcome).into_response(),
        Err(e) => proposal_error_response(&e),
    }
}

/// One-tap reject (Story 6.16): the same typed rejection command, surface
/// `mobile` — the change never applies; the receipt keeps the decision.
async fn remote_reject(
    State(state): State<BridgeState>,
    Path(proposal_id): Path<String>,
) -> Response {
    let proposal_id: Uuid = match proposal_id.parse() {
        Ok(id) => id,
        Err(e) => return bad_request(format!("invalid proposal id `{proposal_id}`: {e}")),
    };
    let c = state.db.0.lock().await;
    let store = EventStore::new(&c);
    match proposals::reject(&store, proposal_id, Some("mobile")) {
        Ok(proposal) => Json(proposal).into_response(),
        Err(e) => proposal_error_response(&e),
    }
}

fn bad_request(msg: String) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response()
}

/// Map a typed proposal error to an honest HTTP response — the body always
/// carries the stable code (`not_pending:`, `basis_stale:`, `not_found:`),
/// never a bare status (EXPERIENCE.md).
fn proposal_error_response(e: &proposals::ProposalError) -> Response {
    use proposals::ProposalError as E;
    let status = match e {
        E::NotFound(_) | E::TargetNotFound(_) => StatusCode::NOT_FOUND,
        E::NotPending { .. } | E::BasisStale { .. } | E::DigestMismatch(_) => {
            StatusCode::CONFLICT
        }
        E::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({ "error": e.to_string() }))).into_response()
}

// ---------------------------------------------------------------------------
// The registry — one channel at a time
// ---------------------------------------------------------------------------

struct ActiveBridge {
    mode: BridgeMode,
    describe: String,
    abort: tokio::task::AbortHandle,
}

/// The bridge registry: holds the ONE active channel. A second enable while
/// a channel is active is refused with `bridge_already_active:` (AD-7 —
/// one channel, never a second).
pub struct BridgeRegistry {
    inner: Mutex<Option<ActiveBridge>>,
}

impl BridgeRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// The status read: the active mode (or `off`) plus the paired count.
    pub async fn status(&self, db: &Db) -> BridgeStatusView {
        let (mode, describe) = {
            let guard = self.inner.lock().expect("bridge registry poisoned");
            guard
                .as_ref()
                .map(|b| (b.mode, Some(b.describe.clone())))
                .unwrap_or((BridgeMode::Off, None))
        };
        let paired = {
            let c = db.0.lock().await;
            bridge_domain::list_devices(&c).map(|d| d.len()).unwrap_or(0)
        };
        BridgeStatusView {
            mode: mode.as_str().to_string(),
            active: mode != BridgeMode::Off,
            describe,
            paired_devices: paired,
        }
    }

    /// Enable the ONE bridge channel. `mode` picks the adapter; `addr`
    /// overrides the tunnel listen address; `chopflow_url`/`chopflow_token`
    /// carry the ChopFlow deployment's base URL and pairing token. Enabling
    /// `off` disables.
    pub async fn enable(
        &self,
        db: Db,
        mode: BridgeMode,
        addr: Option<String>,
        chopflow_url: Option<String>,
        chopflow_token: Option<String>,
        data_dir: std::path::PathBuf,
    ) -> Result<BridgeStatusView, BridgeError> {
        if mode == BridgeMode::Off {
            self.disable();
            return Ok(self.status(&db).await);
        }
        {
            let guard = self.inner.lock().expect("bridge registry poisoned");
            if let Some(active) = guard.as_ref() {
                return Err(BridgeError::AlreadyActive {
                    active: active.describe.clone(),
                });
            }
        }
        let (describe, abort) = match mode {
            BridgeMode::Tunnel => {
                let raw = addr
                    .or_else(|| std::env::var("RC_BRIDGE_ADDR").ok())
                    .or_else(|| {
                        std::env::var("RC_BRIDGE_PORT")
                            .ok()
                            .map(|p| format!("0.0.0.0:{p}"))
                    })
                    .unwrap_or_else(|| DEFAULT_BRIDGE_ADDR.to_string());
                let addr: SocketAddr = raw
                    .parse()
                    .map_err(|_| BridgeError::InvalidListenAddr { raw: raw.clone() })?;
                let listener = tokio::net::TcpListener::bind(addr)
                    .await
                    .map_err(|e| BridgeError::BindFailed {
                        addr: raw.clone(),
                        source: e,
                    })?;
                let bound = listener.local_addr().map_err(|e| BridgeError::BindFailed {
                    addr: raw.clone(),
                    source: e,
                })?;
                let router =
                    bridge_router(db.clone(), crate::server::dist_dir(), data_dir.clone());
                let task = tokio::spawn(async move {
                    if let Err(e) = axum::serve(listener, router).await {
                        eprintln!("[bridge:tunnel] stopped: {e}");
                    }
                });
                eprintln!(
                    "[bridge:tunnel] listening on {bound} — paired mobile companion at http://{bound}/m"
                );
                (format!("tunnel {bound}"), task.abort_handle())
            }
            BridgeMode::ChopFlow => {
                let base_url = chopflow_url
                    .or_else(|| std::env::var("RC_CHOPFLOW_URL").ok())
                    .filter(|u| !u.trim().is_empty())
                    .ok_or(BridgeError::ChopFlowUnconfigured)?;
                // The deployment's credential is a pairing token, minted by
                // the same user-action pairing flow as a phone (`pair a
                // device`) and pasted here or into RC_CHOPFLOW_TOKEN — the
                // raw token is never stored, so it rides env/explicit arg.
                let token = chopflow_token
                    .or_else(|| std::env::var("RC_CHOPFLOW_TOKEN").ok())
                    .filter(|t| !t.trim().is_empty())
                    .ok_or(BridgeError::ChopFlowUnconfigured)?;
                let client = ChopFlowClient::new(&base_url, &token, "research-core");
                let task = tokio::spawn(chopflow_loop(db.clone(), client.clone()));
                eprintln!(
                    "[bridge:chopflow] connected to {base_url} — polling for remote commands"
                );
                (format!("chopflow {base_url}"), task.abort_handle())
            }
            BridgeMode::Off => unreachable!("handled above"),
        };
        {
            let mut guard = self.inner.lock().expect("bridge registry poisoned");
            *guard = Some(ActiveBridge {
                mode,
                describe: describe.clone(),
                abort,
            });
        }
        Ok(self.status(&db).await)
    }

    /// Disable the active channel (if any). The tunnel stops listening
    /// immediately; the ChopFlow loop stops polling. Disabling an off
    /// bridge is a no-op, never an error.
    pub fn disable(&self) {
        let mut guard = self.inner.lock().expect("bridge registry poisoned");
        if let Some(active) = guard.take() {
            active.abort.abort();
            eprintln!("[bridge] {} disabled — the channel is closed", active.describe);
        }
    }
}

impl Default for BridgeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// The process-wide bridge registry. Managed here (not as Tauri state) so
/// the startup path and the commands share ONE registry — the single
/// channel is a process invariant, not a window's.
static REGISTRY: once_cell::sync::Lazy<BridgeRegistry> =
    once_cell::sync::Lazy::new(BridgeRegistry::new);

/// Enable the one bridge channel (command + startup path).
pub async fn enable(
    db: Db,
    mode: BridgeMode,
    addr: Option<String>,
    chopflow_url: Option<String>,
    chopflow_token: Option<String>,
    data_dir: std::path::PathBuf,
) -> Result<BridgeStatusView, BridgeError> {
    REGISTRY
        .enable(db, mode, addr, chopflow_url, chopflow_token, data_dir)
        .await
}

/// Disable the active channel.
pub fn disable() {
    REGISTRY.disable()
}

/// The bridge status read.
pub async fn status(db: &Db) -> BridgeStatusView {
    REGISTRY.status(db).await
}

/// The startup path: `RC_BRIDGE` names the mode (off | tunnel | chopflow,
/// default OFF — nothing listens beyond the existing localhost view unless
/// enabled). Failures are logged honestly, never fatal — the desktop app
/// does not depend on the bridge to function.
pub fn startup(db: Db, data_dir: std::path::PathBuf) {
    let raw = std::env::var("RC_BRIDGE").unwrap_or_else(|_| "off".to_string());
    let Some(mode) = BridgeMode::parse(&raw) else {
        eprintln!("[bridge] not started: unknown RC_BRIDGE mode `{raw}` — expected off | tunnel | chopflow");
        return;
    };
    if mode == BridgeMode::Off {
        return;
    }
    tauri::async_runtime::spawn(async move {
        if let Err(e) = enable(db, mode, None, None, None, data_dir).await {
            eprintln!("[bridge] not started: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::bridge::pair_device;
    use crate::domain::hypotheses::HypothesisStatus;
    use crate::domain::missions::{Autonomy, MissionCreatedPayload};
    use crate::domain::proposals::propose_transition;
    use crate::eventstore::{EventStore, NewEvent};
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use rusqlite::Connection;
    use std::sync::Arc;
    use tower::ServiceExt;

    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        Db(std::sync::Arc::new(tokio::sync::Mutex::new(conn)))
    }

    fn app(db: Db) -> Router {
        bridge_router(
            db,
            std::path::PathBuf::from("/nonexistent-dist"),
            std::path::PathBuf::from("/nonexistent-data"),
        )
    }

    async fn body_json<T: serde::de::DeserializeOwned>(body: Body) -> T {
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn seed_proposal(db: &Db) -> Uuid {
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        let mission = store
            .append(
                NewEvent::mission_created(MissionCreatedPayload {
                    question: "Does X hold up?".into(),
                    stop_condition: "Stop after $5.".into(),
                    success_criterion: "A blind rater agrees.".into(),
                    autonomy: Autonomy::Watch,
                    spend_ceiling_cents: 500,
                    schedule: "daily-03:00".into(),
                    roles: vec![],
                })
                .unwrap(),
            )
            .unwrap();
        let hyp = store
            .append(NewEvent::hypothesis_created("X holds.", mission.id).unwrap())
            .unwrap();
        let proposal = propose_transition(&store, "run-7", hyp.id, HypothesisStatus::Testing, "run 7 suggests testing")
            .unwrap();
        proposal.id
    }

    /// Pairing is required: an unpaired request is refused with the typed
    /// `unpaired:` error — no open endpoints (NFR-13).
    #[tokio::test]
    async fn unpaired_requests_are_refused_with_the_typed_error() {
        let db = test_db();
        let receipt = {
            let c = db.0.lock().await;
            pair_device(&c, "Pixel 8").unwrap()
        };
        // no token at all
        let res = app(db.clone())
            .oneshot(Request::get("/api/missions").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let v: Value = body_json(res.into_body()).await;
        let err = v["error"].as_str().unwrap();
        assert!(err.starts_with("unpaired:"), "unexpected: {err}");
        // a wrong token is the same refusal — constant-shape
        let res = app(db.clone())
            .oneshot(
                Request::get("/api/missions")
                    .header("x-rc-pairing", "not-the-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        // the paired token reads
        let res = app(db.clone())
            .oneshot(
                Request::get("/api/missions")
                    .header("x-rc-pairing", &receipt.token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        // the /m shell is static — it carries no data and needs no pairing
        let res = app(db)
            .oneshot(Request::get("/m").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "no dist build in tests — honest, not a pairing leak");
    }

    /// Single-writer routing (AD-14): a remote approve goes through the SAME
    /// typed merge command — the merge event's actor is the user, the
    /// surface is attributed `mobile`, and the store holds exactly one
    /// merge.approved appended by the home writer.
    #[tokio::test]
    async fn a_remote_approve_routes_through_the_home_writer_with_surface_attribution() {
        let db = test_db();
        let receipt = {
            let c = db.0.lock().await;
            pair_device(&c, "iPhone").unwrap()
        };
        let proposal_id = seed_proposal(&db).await;
        let res = app(db.clone())
            .oneshot(
                Request::post(format!("/api/proposals/{proposal_id}/approve"))
                    .header("x-rc-pairing", &receipt.token)
                    .header("content-type", "application/json")
                    .body(Body::from("{\"force\":false}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let outcome: ApproveOutcome = body_json(res.into_body()).await;
        assert_eq!(outcome.proposal.status, crate::domain::proposals::ProposalStatus::Merged);
        // the receipt: actor=user, surface=mobile (NFR-13)
        let decided = outcome.proposal.decided.expect("the merge stamped a decision");
        assert_eq!(decided.actor, "user");
        assert_eq!(decided.surface.as_deref(), Some("mobile"));
        // the log: one merge.approved, appended by the home writer
        let c = db.0.lock().await;
        let events = EventStore::new(&c).events_all().unwrap();
        let merges: Vec<_> = events.iter().filter(|e| e.kind == "merge.approved").collect();
        assert_eq!(merges.len(), 1);
        assert_eq!(merges[0].actor, crate::eventstore::Actor::User);
    }

    /// A remote reject is the same typed command; a second decision on a
    /// decided proposal is refused with `not_pending:` — never a re-merge.
    #[tokio::test]
    async fn a_remote_reject_is_typed_and_a_decided_proposal_is_refused() {
        let db = test_db();
        let receipt = {
            let c = db.0.lock().await;
            pair_device(&c, "iPhone").unwrap()
        };
        let proposal_id = seed_proposal(&db).await;
        let res = app(db.clone())
            .oneshot(
                Request::post(format!("/api/proposals/{proposal_id}/reject"))
                    .header("x-rc-pairing", &receipt.token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let proposal: Proposal = body_json(res.into_body()).await;
        assert_eq!(proposal.status, crate::domain::proposals::ProposalStatus::Rejected);
        assert_eq!(proposal.decided.unwrap().surface.as_deref(), Some("mobile"));
        // one-tap applies only to PENDING proposals — a decided one refuses
        let res = app(db.clone())
            .oneshot(
                Request::post(format!("/api/proposals/{proposal_id}/approve"))
                    .header("x-rc-pairing", &receipt.token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
        let v: Value = body_json(res.into_body()).await;
        assert!(v["error"].as_str().unwrap().starts_with("not_pending:"));
    }

    /// An unpaired mutation is refused like an unpaired read — pairing
    /// gates every endpoint, reads and remote commands alike.
    #[tokio::test]
    async fn an_unpaired_remote_command_is_refused() {
        let db = test_db();
        let proposal_id = seed_proposal(&db).await;
        let res = app(db)
            .oneshot(
                Request::post(format!("/api/proposals/{proposal_id}/approve"))
                    .header("content-type", "application/json")
                    .body(Body::from("{\"force\":false}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    /// The remote vocabulary is closed: an unknown command kind is refused
    /// with `invalid_command:` — no remote append path exists.
    #[tokio::test]
    async fn the_remote_command_vocabulary_is_closed() {
        let db = test_db();
        let err = execute_remote_command(
            &db,
            &BridgeCommand {
                id: "c-1".into(),
                kind: "mission.created".into(),
                proposal_id: None,
                force: false,
                question: None,
            },
        )
        .await
        .unwrap_err();
        assert!(err.starts_with("invalid_command:"), "unexpected: {err}");
    }

    /// Both adapters satisfy the same interface contract (NFR-8, NFR-13):
    /// same modes, same summary-only payload, same honest delivery shape —
    /// the tunnel delivers by reachability, ChopFlow through its channel.
    #[tokio::test]
    async fn both_adapters_satisfy_the_same_interface_contract() {
        async fn contract<A: BridgeAdapter>(
            adapter: &A,
            notice: &NotificationItem,
        ) -> (BridgeMode, String, Result<(), String>) {
            let mode = adapter.mode();
            let describe = adapter.describe();
            // deliver is best-effort and honest — Ok or a visible Err
            let delivered = adapter.deliver(notice).await;
            (mode, describe, delivered)
        }
        let notice = NotificationItem {
            kind: "proposal".into(),
            seq: 12,
            ts: chrono::Utc::now(),
            proposal_id: Some(Uuid::new_v4()),
            summary: "pr-12 · hypothesis.status_changed → testing".into(),
            basis_stale: false,
        };
        let (mode, describe, delivered) = contract(&TunnelAdapter, &notice).await;
        assert_eq!(mode, BridgeMode::Tunnel);
        assert!(describe.contains("tunnel"));
        assert!(delivered.is_ok(), "the tunnel delivers by reachability");
        // an unreachable ChopFlow errs honestly — a visible failure, never a
        // silent miss
        let (mode, describe, delivered) = contract(
            &ChopFlowAdapter::new(ChopFlowClient::new(
                "http://127.0.0.1:9",
                "tok",
                "research-core",
            )),
            &notice,
        )
        .await;
        assert_eq!(mode, BridgeMode::ChopFlow);
        assert!(describe.contains("chopflow"));
        assert!(delivered.is_err(), "an unreachable deployment is an honest Err");
    }

    /// One channel at a time (AD-7): a second enable while a channel is
    /// active is refused with `bridge_already_active:`; disable closes it
    /// and a new channel may start.
    #[tokio::test]
    async fn a_second_channel_start_is_refused() {
        let db = test_db();
        let registry = BridgeRegistry::new();
        // tunnel on an ephemeral localhost port — nothing external listens
        let st = registry
            .enable(
                db.clone(),
                BridgeMode::Tunnel,
                Some("127.0.0.1:0".into()),
                None,
                None,
            )
            .await
            .unwrap();
        assert!(st.active);
        assert_eq!(st.mode, "tunnel");
        // a second start — any adapter — is refused with the typed error
        let err = registry
            .enable(
                db.clone(),
                BridgeMode::ChopFlow,
                None,
                Some("http://127.0.0.1:9".into()),
                Some("tok".into()),
            )
            .await
            .unwrap_err();
        assert!(
            err.to_string().starts_with("bridge_already_active:"),
            "unexpected: {err}"
        );
        let err = registry
            .enable(
                db.clone(),
                BridgeMode::Tunnel,
                Some("127.0.0.1:0".into()),
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().starts_with("bridge_already_active:"));
        // disable closes the channel; a new one may start
        registry.disable();
        let st = registry
            .enable(
                db,
                BridgeMode::Tunnel,
                Some("127.0.0.1:0".into()),
                None,
                None,
            )
            .await
            .unwrap();
        assert!(st.active);
        registry.disable();
    }

    /// An unconfigured ChopFlow is an honest error, never a crash
    /// (FR-22.3 spirit) — and the mode vocabulary is typed. Explicit empty
    /// args (never env mutation) keep the test hermetic on any machine.
    #[tokio::test]
    async fn an_unconfigured_chopflow_is_an_honest_error() {
        let db = test_db();
        let registry = BridgeRegistry::new();
        // neither base URL nor token — refused honestly
        let err = registry
            .enable(
                db.clone(),
                BridgeMode::ChopFlow,
                None,
                Some("".into()),
                Some("".into()),
            )
            .await
            .unwrap_err();
        assert!(
            err.to_string().starts_with("chopflow_unconfigured:"),
            "unexpected: {err}"
        );
        // a base URL without a pairing token is equally unconfigured
        let err = registry
            .enable(
                db,
                BridgeMode::ChopFlow,
                None,
                Some("http://127.0.0.1:9".into()),
                Some("".into()),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().starts_with("chopflow_unconfigured:"));
        assert_eq!(BridgeMode::parse("tunnel"), Some(BridgeMode::Tunnel));
        assert_eq!(BridgeMode::parse("chopflow"), Some(BridgeMode::ChopFlow));
        assert_eq!(BridgeMode::parse("off"), Some(BridgeMode::Off));
        assert_eq!(BridgeMode::parse("carrier-pigeon"), None);
    }

    /// The ChopFlow adapter speaks the documented HTTP slice: it polls for
    /// commands, executes them through the home writer (single writer,
    /// AD-14 — same typed command, surface `mobile`), and posts the result.
    #[tokio::test]
    async fn the_chopflow_adapter_executes_remote_commands_through_the_home_writer() {
        use axum::routing::{get as ax_get, post as ax_post};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::atomic::AtomicBool;

        let db = test_db();
        let proposal_id = seed_proposal(&db).await;

        // a local mock ChopFlow deployment speaking the documented slice
        let executed = Arc::new(AtomicUsize::new(0));
        let result_posted = Arc::new(AtomicBool::new(false));
        let executed_for_router = executed.clone();
        let posted_for_router = result_posted.clone();
        let proposal_for_router = proposal_id;
        let mock = Router::new()
            .route(
                "/v1/devices/{device}/commands",
                ax_get(move || async move {
                    if executed_for_router.load(Ordering::SeqCst) == 0 {
                        Json(json!({ "commands": [
                            { "id": "cmd-1", "kind": "approve", "proposalId": proposal_for_router.to_string(), "force": false }
                        ]}))
                    } else {
                        Json(json!({ "commands": [] }))
                    }
                }),
            )
            .route(
                "/v1/devices/{device}/commands/{id}/result",
                ax_post(move |Path((_device, _id)): Path<(String, String)>, body: String| async move {
                    let v: Value = serde_json::from_str(&body).unwrap();
                    assert!(v["ok"].as_bool().unwrap(), "the typed command succeeded");
                    posted_for_router.store(true, Ordering::SeqCst);
                    Json(json!({ "ok": true }))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, mock).await });

        let client = ChopFlowClient::new(&format!("http://{addr}"), "deployment-token", "research-core");
        // one poll cycle: the approve command executes through the home writer
        let n = chopflow_run_once(&db, &client).await.unwrap();
        assert_eq!(n, 1);
        executed.fetch_add(1, Ordering::SeqCst);
        assert!(result_posted.load(Ordering::SeqCst));
        // the merge happened through the typed path — actor=user, surface=mobile
        let c = db.0.lock().await;
        let events = EventStore::new(&c).events_all().unwrap();
        let merges: Vec<_> = events
            .iter()
            .filter(|e| e.kind == "merge.approved")
            .collect();
        assert_eq!(merges.len(), 1);
        let payload: Value = merges[0].payload.clone();
        assert_eq!(payload["surface"].as_str(), Some("mobile"));
        // a second cycle finds nothing pending — idempotent honesty
        drop(c);
        let n = chopflow_run_once(&db, &client).await.unwrap();
        assert_eq!(n, 0);
    }

    /// A failed ChopFlow push is recorded as a visible event — never a
    /// silent miss (FR-9.1 spirit).
    #[tokio::test]
    async fn a_failed_push_is_a_visible_event() {
        let db = test_db();
        {
            let c = db.0.lock().await;
            bridge_domain::record_push_failure(&c, "chopflow", "connection refused");
        }
        let c = db.0.lock().await;
        let events = EventStore::new(&c).events_all().unwrap();
        assert!(events.iter().any(|e| e.kind == "bridge.push_failed"));
    }
}
