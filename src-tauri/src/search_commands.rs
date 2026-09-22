// Search shell commands (AD-15a, Story 4.1, FR-12.1): the typed core APIs
// the mission's Disclosure view and the assistant call. `run_search` is
// the ONLY search entry (the Night Shift scan goes through the same core
// seam) — it executes the search, appends the `search.run` PRISMA record,
// and returns the results, so the log cannot be skipped. Null results
// return as rows (`resultCount: 0`, `nullResult: true`) — never as
// "nothing happened". `get_search_disclosure` is the read model
// (mission-scoped or workspace-wide), served to the browser too.
//
// Running a search is a mutation (Tauri command path only, AD-14); the
// disclosure is a read. Error strings lead with stable codes
// (`invalid_query:`, `invalid_database:`, `invalid_filters:`,
// `invalid_mission_id:`) and stay in code form — bilingual-safe by
// construction (EXPERIENCE.md).

use rusqlite::Connection;
use serde_json::Value;
use tauri::State;
use uuid::Uuid;

use crate::db::Db;
use crate::domain::search::{
    self, SearchDisclosure, SearchParams, SearchRunOutcome,
};
use crate::eventstore::EventStore;

fn err(e: impl ToString) -> String {
    e.to_string()
}

fn parse_mission(mission_id: Option<&str>) -> Result<Option<Uuid>, String> {
    match mission_id.map(str::trim) {
        None | Some("") => Ok(None),
        Some(raw) => raw
            .parse()
            .map(Some)
            .map_err(|e| format!("invalid_mission_id: `{raw}`: {e}")),
    }
}

/// Run one search (FR-12.1): the single search entry the UI/assistant
/// calls — executes the search, appends the `search.run` event, and
/// returns the results plus the disclosure row that records what ran
/// (query, database, filters, date, count — null results included and
/// marked). Validation failures append nothing.
#[tauri::command]
pub async fn run_search(
    db: State<'_, Db>,
    query: String,
    database: String,
    filters: Option<Value>,
    order: Option<String>,
    first_page: Option<bool>,
    mission_id: Option<String>,
) -> Result<SearchRunOutcome, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err(
            "invalid_query: a search records what was searched — the query cannot be empty (FR-12.1)"
                .into(),
        );
    }
    let database = database.trim().to_string();
    if database.is_empty() {
        return Err(
            "invalid_database: a search records where it ran — name the database (e.g. arxiv, semantic-scholar, web) (FR-12.1)"
                .into(),
        );
    }
    let filters = match filters {
        None | Some(Value::Null) => Default::default(),
        Some(Value::Object(map)) => map.into_iter().collect(),
        Some(other) => {
            return Err(format!(
                "invalid_filters: filters are a JSON map (date ranges, venues) — got {other}"
            ))
        }
    };
    let mission_id = parse_mission(mission_id.as_deref())?;
    let c = db.0.lock().await;
    let store = EventStore::new(&c);
    search::run_search(
        &store,
        &SearchParams {
            query,
            database,
            filters,
            order: order.filter(|o| !o.trim().is_empty()),
            first_page: first_page.unwrap_or(true),
            mission_id,
            // a UI/assistant search runs as the user (no agent run)
            run_id: None,
        },
    )
    .map_err(err)
}

/// The disclosure read model (FR-12.1): every search's PRISMA row in seq
/// order — mission-scoped when a mission id is given, else
/// workspace-wide. Null results render as rows, never hidden.
#[tauri::command]
pub async fn get_search_disclosure(
    db: State<'_, Db>,
    mission_id: Option<String>,
) -> Result<SearchDisclosure, String> {
    let mission = parse_mission(mission_id.as_deref())?;
    let c = db.0.lock().await;
    search_disclosure_inner(&c, mission)
}

/// Plain inner (testable without Tauri state; the server route serves the
/// same read over the shared core): fold the disclosure.
pub(crate) fn search_disclosure_inner(
    conn: &Connection,
    mission: Option<Uuid>,
) -> Result<SearchDisclosure, String> {
    let events = EventStore::new(conn).events_all().map_err(err)?;
    search::search_disclosure(&events, mission).map_err(err)
}
