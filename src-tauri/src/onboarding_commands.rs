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
use crate::mcp;
use tauri::State;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// Run the first-value flow from a pasted arXiv URL: parse (typed error
/// before any network), fetch the paper's metadata, then the core
/// orchestration — library upsert, candidates through the provider layer,
/// starter mission, candidate hypotheses on the board.
#[tauri::command]
pub async fn run_first_value(
    db: State<'_, Db>,
    url: String,
) -> Result<FirstValueResult, String> {
    let arxiv_id = onboarding::parse_arxiv_url(&url).map_err(err)?;
    let search = mcp::arxiv_fetch(&arxiv_id)
        .await
        .map_err(|e| format!("fetch_failed: {e}"))?
        .ok_or_else(|| {
            format!("fetch_failed: arXiv has no paper with id `{arxiv_id}`")
        })?;
    let paper = Paper {
        title: search.title,
        authors: search.authors,
        year: search.year,
        venue: search.venue,
        doi: search.doi,
        url: search.url,
        arxiv_id,
        abstract_text: search.abstract_text,
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
