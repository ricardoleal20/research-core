// Bridge shell commands (Story 6.14, FR-21.1, AD-15a): the typed surface
// the desktop's bridge settings render. Enable/disable flip the ONE channel
// (a second start is refused with `bridge_already_active:`); pairing is a
// user action minting a one-time token; the status read carries the active
// mode and the paired-devices count. Error strings lead with stable codes
// and stay in code form — bilingual-safe by construction (EXPERIENCE.md).

use crate::bridge::{self, BridgeMode, BridgeStatusView};
use crate::db::Db;
use crate::AppPaths;
use tauri::State;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// The bridge status read: the active channel (or `off`), what it listens
/// on / talks to, and how many devices are paired.
#[tauri::command]
pub async fn bridge_status(db: State<'_, Db>) -> Result<BridgeStatusView, String> {
    Ok(bridge::status(db.inner()).await)
}

/// The notifications read (Story 6.16, FR-21.4): pending quarantine
/// proposals + the digest-ready notice — verdict summaries only, never
/// research content beyond the summary (NFR-13). The desktop bell polls
/// this; the same items push through the active bridge adapter.
#[tauri::command]
pub async fn list_notifications(
    db: State<'_, Db>,
) -> Result<Vec<bridge::NotificationItem>, String> {
    bridge::notifications(db.inner()).await
}

/// Enable the ONE bridge channel (FR-21.1): `mode` is `tunnel` (the
/// default adapter — direct reachability, mobile companion at `/m`) or
/// `chopflow` (first-class, optional). Off by default; a second start while
/// a channel is active is refused with `bridge_already_active:`. The tunnel
/// address defaults to `0.0.0.0:4762` (`RC_BRIDGE_ADDR`/`RC_BRIDGE_PORT`
/// override); ChopFlow needs its deployment base URL + pairing token
/// (`RC_CHOPFLOW_URL`/`RC_CHOPFLOW_TOKEN` or the explicit args).
#[tauri::command]
pub async fn enable_bridge(
    db: State<'_, Db>,
    paths: State<'_, AppPaths>,
    mode: String,
    listen_addr: Option<String>,
    chopflow_url: Option<String>,
    chopflow_token: Option<String>,
) -> Result<BridgeStatusView, String> {
    let mode = BridgeMode::parse(&mode).ok_or_else(|| {
        format!(
            "unknown_mode: `{mode}` — expected off | tunnel | chopflow (FR-21.1)"
        )
    })?;
    bridge::enable(
        db.inner().clone(),
        mode,
        listen_addr,
        chopflow_url,
        chopflow_token,
        paths.data_dir.clone(),
    )
    .await
    .map_err(err)
}

/// Disable the active channel. The tunnel stops listening immediately; the
/// ChopFlow loop stops polling. Disabling an off bridge is a no-op.
#[tauri::command]
pub async fn disable_bridge(db: State<'_, Db>) -> Result<BridgeStatusView, String> {
    bridge::disable();
    Ok(bridge::status(db.inner()).await)
}

/// Pair a device (a user action, NFR-13): mints a one-time token, stores
/// its sha-256, and appends the auditable `bridge.device_paired` event.
/// The receipt's raw token is shown ONCE — the owner enters it on the
/// remote surface; it is never stored and never appears again. Re-pairing a
/// device rotates its token.
#[tauri::command]
pub async fn pair_bridge_device(
    db: State<'_, Db>,
    device_name: String,
) -> Result<crate::domain::bridge::PairingReceipt, String> {
    let c = db.0.lock().await;
    crate::domain::bridge::pair_device(&c, &device_name)
}

/// Unpair a device: its token stops authenticating immediately and the
/// audit trail records the unpair.
#[tauri::command]
pub async fn unpair_bridge_device(
    db: State<'_, Db>,
    device_name: String,
) -> Result<Vec<crate::domain::bridge::PairedDevice>, String> {
    let c = db.0.lock().await;
    crate::domain::bridge::unpair_device(&c, &device_name)
}

/// The visible paired-devices list (fold of the pairing ledger).
#[tauri::command]
pub async fn list_bridge_devices(
    db: State<'_, Db>,
) -> Result<Vec<crate::domain::bridge::PairedDevice>, String> {
    let c = db.0.lock().await;
    crate::domain::bridge::list_devices(&c)
}
