// Onboarding shell commands (AD-15a, FR-8.1 — the sixty-second first value):
// the two doors of the first-run surface. `run_first_value` takes a pasted
// arXiv URL — parsed (typed `invalid_url:` before any fetch), fetched via the
// arXiv export adapter, then orchestrated in the core. `run_first_value_from_ref`
// is the Zotero door: the connector's minimal stub — the already-migrated
// library is the connector's output, and a chosen ref becomes the paper.
//
// The candidate generation flows through the provider layer (AD-9), resolved
// from settings + keychain — the simulated fallback fires when no key is
// configured, so first value needs zero configuration. Errors are coded and
// bilingual-safe (codes are never translated).

use crate::adapters::providers::ProviderLayer;
use crate::db::Db;
use crate::domain::onboarding::{self, FirstValueResult, Paper};
use tauri::State;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// Run the first-value flow from a pasted arXiv URL: parse (typed error
/// before any network), fetch the paper's metadata through the SHARED
/// arXiv adapter (the same `fetch_arxiv_metadata` the library add uses,
/// FR-15.1 — the two doors can never drift), then the core orchestration —
/// library upsert, candidates through the provider layer, starter mission,
/// candidate hypotheses on the board.
#[tauri::command]
pub async fn run_first_value(
    db: State<'_, Db>,
    url: String,
) -> Result<FirstValueResult, String> {
    let meta = crate::domain::library::fetch_arxiv_metadata(&url)
        .await
        .map_err(err)?;
    let paper = Paper {
        title: meta.title,
        authors: meta.authors,
        year: meta.year,
        venue: meta.venue,
        doi: meta.doi,
        url: meta.url,
        arxiv_id: meta.arxiv_id,
        abstract_text: meta.abstract_text,
    };
    orchestrate(&db, paper).await
}

/// Run the first-value flow from a library ref — the Zotero door. The Zotero
/// connector is a minimal stub in v1: the migrated library is its output, and
/// this command turns a chosen ref into the paper the flow generates from.
#[tauri::command]
pub async fn run_first_value_from_ref(
    db: State<'_, Db>,
    ref_id: String,
) -> Result<FirstValueResult, String> {
    let paper = {
        let conn = db.0.lock().await;
        onboarding::paper_from_ref(&conn, &ref_id).map_err(err)?
    };
    orchestrate(&db, paper).await
}

/// Resolve the provider layer (settings + keychain — the simulated fallback
/// when no key), then run the core orchestration. The connection lock is
/// never held across the provider call (the layer locks internally). Error
/// Display forms lead with their codes — `invalid_url:` / `fetch_failed:` /
/// `generation_failed:` / `not_found:` reach the bilingual UI verbatim.
async fn orchestrate(db: &Db, paper: Paper) -> Result<FirstValueResult, String> {
    let layer = {
        let conn = db.0.lock().await;
        ProviderLayer::resolve(db, &conn).map_err(err)?
    };
    onboarding::run_first_value(db, &layer, paper).await.map_err(err)
}
