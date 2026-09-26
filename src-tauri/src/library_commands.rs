// Library shell commands (AD-15a, FR-15, Epic 5): the typed core APIs the
// references view calls. Adds land as `ref.added` events through the domain
// constructors (link paste / manual entry / Zotero import), removal is an
// auditable `ref.removed` event, restore the `ref.restored` un-event — the
// legacy `create_ref`/`delete_ref` relational paths stay dead (AD-16).
// Reads re-fold the library projection (legacy baseline + event log).
//
// Errors lead with stable codes (`already_in_library:`, `invalid_ref:`,
// `not_found:`, `invalid_state:`, `zotero_unreachable:`,
// `zotero_fetch_failed:`, and the resolver's `unsupported_source:` /
// `invalid_url:` / `resolve_failed:`) and stay in code form — bilingual-safe
// by construction (codes are never translated, EXPERIENCE.md).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::Db;
use crate::domain::library::{
    find_duplicate, LibraryProjection, LibraryRef, RefAddedPayload, REF_ADDED, REF_REMOVED,
    REF_RESTORED,
};
use crate::domain::resolver::{resolve_link, ResolvedPaper};
use crate::eventstore::{EventStore, NewEvent, StoredEvent};
use rusqlite::Connection;
use tauri::State;

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// The project an evented ref lands under: the active project, else the
/// first project (the workspace always has its seeded demo project) — a
/// library add never fails on plumbing. (The same resolution the onboarding
/// upsert uses.)
fn ensure_project(conn: &Connection) -> Result<String, String> {
    use rusqlite::OptionalExtension;
    let active: Option<String> = conn
        .query_row(
            "SELECT id FROM projects WHERE is_active = 1 LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    if let Some(id) = active {
        return Ok(id);
    }
    let any: Option<String> = conn
        .query_row(
            "SELECT id FROM projects ORDER BY created_at LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    Ok(any.unwrap_or_default())
}

/// The whole folded library read — the shared read behind the Tauri command
/// and the server shell's read-only route (AD-7).
pub(crate) fn list_refs_inner(
    conn: &Connection,
    project_id: Option<&str>,
    filter: Option<&str>,
) -> Result<Vec<LibraryRef>, String> {
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let mut refs = LibraryProjection::fold(conn, &events).map_err(err)?;
    if let Some(project) = project_id.filter(|p| !p.trim().is_empty()) {
        refs.retain(|r| r.project_id == project);
    }
    match filter {
        Some("active") => refs.retain(|r| !r.removed),
        Some("removed") | Some("archived") => refs.retain(|r| r.removed),
        Some("used") => refs.retain(|r| r.used == 1),
        Some("unused") => refs.retain(|r| r.used == 0),
        Some(f) if f.starts_with("status:") => {
            let st = f.trim_start_matches("status:");
            refs.retain(|r| r.status == st);
        }
        _ => {}
    }
    Ok(refs)
}

/// The folded library state of one ref: Ok(None) = unknown id, Ok(Some)
/// = found with its archived flag. This is the shape the pin validation
/// and the enrich read use.
pub(crate) fn library_state(
    conn: &Connection,
    ref_id: &str,
) -> Result<Option<LibraryRef>, String> {
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let refs = LibraryProjection::fold(conn, &events).map_err(err)?;
    Ok(refs.into_iter().find(|r| r.id == ref_id))
}

/// Append one `ref.added` event (the shared add core): dedup against the
/// folded library (url/doi/zotero key — an honest refusal, never a
/// duplicate), then append and return the folded ref.
fn append_ref_added(
    conn: &Connection,
    payload: RefAddedPayload,
) -> Result<LibraryRef, String> {
    let new_id = payload.ref_id.clone();
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let refs = LibraryProjection::fold(conn, &events).map_err(err)?;
    if let Some(dup) = find_duplicate(&refs, &payload.doi, &payload.url, payload.zotero_item_key.as_deref())
    {
        return Err(format!(
            "already_in_library: `{}` — this reference is already in the library (added via {})",
            dup.title, dup.source
        ));
    }
    let stored = EventStore::new(conn)
        .append(NewEvent::ref_added(payload).map_err(err)?)
        .map_err(err)?;
    let mut refs = LibraryProjection::fold(conn, &[stored]).map_err(err)?;
    Ok(refs.swap_remove(
        refs.iter()
            .position(|r| r.id == new_id)
            .ok_or_else(|| format!("invalid_state: the just-appended ref `{new_id}` did not fold"))?,
    ))
}

/// Add a reference by pasting a link (FR-15.1, multi-source): resolve the
/// link through the SAME shared multi-source resolver the onboarding
/// first-value flow uses (arXiv, DOI/Crossref, PubMed, Semantic Scholar,
/// OpenAlex — typed `unsupported_source:` / `invalid_url:` /
/// `resolve_failed:` before or during the fetch), then append one
/// `ref.added` event (actor=user, source: the resolved source) with the
/// metadata filled from the fetch, carrying the doi and arXiv id. A url/doi
/// already in the library is refused honestly.
#[tauri::command]
pub async fn add_ref_from_link(db: State<'_, Db>, url: String) -> Result<LibraryRef, String> {
    add_ref_from_link_impl(db, url).await
}

/// The v1 command name, kept as an alias: the same door, the same resolver.
#[tauri::command]
pub async fn add_ref_from_arxiv(db: State<'_, Db>, url: String) -> Result<LibraryRef, String> {
    add_ref_from_link_impl(db, url).await
}

/// Shared shell (both command names): resolve (no lock across the network
/// fetch), then append.
async fn add_ref_from_link_impl(
    db: State<'_, Db>,
    url: String,
) -> Result<LibraryRef, String> {
    let meta = resolve_link(&url).await.map_err(err)?;
    let c = db.0.lock().await;
    add_ref_from_link_inner(&c, meta)
}

/// Plain inner (testable): the append half of the link add.
fn add_ref_from_link_inner(conn: &Connection, meta: ResolvedPaper) -> Result<LibraryRef, String> {
    let project = ensure_project(conn)?;
    append_ref_added(
        conn,
        RefAddedPayload {
            ref_id: Uuid::new_v4().to_string(),
            project_id: project,
            title: meta.title,
            authors: meta.authors,
            year: meta.year,
            venue: meta.venue,
            doi: meta.doi.unwrap_or_default(),
            url: meta.url,
            tags: match (&meta.arxiv_id, meta.source.as_str()) {
                (Some(arxiv_id), "arxiv") => format!("arXiv,{arxiv_id}"),
                (_, source) => source.to_string(),
            },
            source: meta.source,
            zotero_item_key: None,
            arxiv_id: meta.arxiv_id,
            abstract_text: meta.abstract_text,
        },
    )
}

/// Add a reference by manual entry (FR-15.3): title required, at least one
/// identifier (doi or url) so the ref is anchorable, everything else
/// optional. Appends one `ref.added` event (source: manual).
#[tauri::command]
pub async fn add_ref_manual(
    db: State<'_, Db>,
    title: String,
    authors: String,
    year: Option<i64>,
    venue: String,
    doi: String,
    url: String,
    tags: String,
) -> Result<LibraryRef, String> {
    let c = db.0.lock().await;
    add_ref_manual_inner(&c, title, authors, year, venue, doi, url, tags)
}

/// Plain inner (testable): validation + the append. Typed codes:
/// `invalid_ref:` for a blank title or an identifier-less entry,
/// `already_in_library:` for a duplicate.
fn add_ref_manual_inner(
    conn: &Connection,
    title: String,
    authors: String,
    year: Option<i64>,
    venue: String,
    doi: String,
    url: String,
    tags: String,
) -> Result<LibraryRef, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(
            "invalid_ref: ref.title must not be empty — a reference is titled (Required to \
             launch / Requerido)"
                .into(),
        );
    }
    let doi = doi.trim().to_string();
    let url = url.trim().to_string();
    if doi.is_empty() && url.is_empty() {
        return Err(
            "invalid_ref: a manual reference carries at least one identifier (doi or url)".into(),
        );
    }
    let project = ensure_project(conn)?;
    append_ref_added(
        conn,
        RefAddedPayload {
            ref_id: Uuid::new_v4().to_string(),
            project_id: project,
            title,
            authors: authors.trim().to_string(),
            year,
            venue: venue.trim().to_string(),
            doi,
            url,
            tags: tags.trim().to_string(),
            source: "manual".into(),
            zotero_item_key: None,
            arxiv_id: None,
            abstract_text: None,
        },
    )
}

/// One Zotero item as imported (FR-15.2) — the local API's item, narrowed
/// to the fields a library ref carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoteroItem {
    /// The Zotero item key — the dedup identity alongside doi/url.
    pub key: String,
    pub title: String,
    pub authors: String,
    pub year: Option<i64>,
    pub venue: String,
    pub doi: String,
    pub url: String,
    pub tags: String,
}

/// The honest import summary (FR-15.2): imported / skipped / failed counts
/// — no item is silently dropped. `skipped_items` names what was skipped
/// and why (duplicates, by identifier).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoteroImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub failed: usize,
    pub refs: Vec<LibraryRef>,
    /// "title — already in the library (doi)" per skipped item.
    pub skipped_items: Vec<String>,
    /// "key — reason" per failed item (unparseable/empty items).
    pub failed_items: Vec<String>,
}

/// Parse the Zotero local API's item list into importable items. Pure —
/// the parse is testable without a connector. Attachments, notes, and
/// annotations are not library refs and are skipped up front.
pub(crate) fn parse_zotero_items(json: &[serde_json::Value]) -> Vec<ZoteroItem> {
    let mut out = Vec::new();
    for item in json {
        let data = match item.get("data").unwrap_or(item) {
            serde_json::Value::Object(m) => m,
            _ => continue,
        };
        let item_type = data
            .get("itemType")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if matches!(item_type, "attachment" | "note" | "annotation") {
            continue;
        }
        let str_field = |k: &str| {
            data.get(k)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .unwrap_or("")
                .to_string()
        };
        let key = item
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let creators = data
            .get("creators")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        let last = c.get("lastName").and_then(|v| v.as_str()).unwrap_or("");
                        let first = c.get("firstName").and_then(|v| v.as_str()).unwrap_or("");
                        let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let full = if !name.is_empty() {
                            name.to_string()
                        } else if !last.is_empty() {
                            format!("{last}{}", if first.is_empty() { String::new() } else { format!(", {first}") })
                        } else {
                            String::new()
                        };
                        if full.is_empty() { None } else { Some(full) }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let year = str_field("date")
            .chars()
            .take(4)
            .collect::<String>()
            .parse::<i64>()
            .ok();
        let tags = data
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.get("tag").and_then(|v| v.as_str()))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        out.push(ZoteroItem {
            key,
            title: str_field("title"),
            authors: creators,
            year,
            venue: str_field("publicationTitle"),
            doi: {
                let d = str_field("DOI");
                if d.is_empty() { str_field("doi") } else { d }
            },
            url: str_field("url"),
            tags,
        });
    }
    out
}

/// The import core (testable): dedup each item (doi/url/zotero key against
/// the folded library AND the items imported in this run — never
/// duplicated), append one `ref.added` (source: zotero, carrying the item
/// key) per new item, and report honest counts. A cancellable run simply
/// stops calling this — events already appended remain (the log is the
/// truth; the partial import stays consistent).
pub(crate) fn import_zotero_items_inner(
    conn: &Connection,
    items: Vec<ZoteroItem>,
) -> Result<ZoteroImportResult, String> {
    let project = ensure_project(conn)?;
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let mut refs = LibraryProjection::fold(conn, &events).map_err(err)?;
    let mut result = ZoteroImportResult {
        imported: 0,
        skipped: 0,
        failed: 0,
        refs: Vec::new(),
        skipped_items: Vec::new(),
        failed_items: Vec::new(),
    };
    let store = EventStore::new(conn);
    for item in items {
        if item.title.trim().is_empty() {
            result.failed += 1;
            result
                .failed_items
                .push(format!("{} — no title", item.key));
            continue;
        }
        let dup = find_duplicate(&refs, &item.doi, &item.url, Some(item.key.as_str()));
        if let Some(dup) = dup {
            result.skipped += 1;
            result
                .skipped_items
                .push(format!("{} — already in the library ({})", dup.title, dup.source));
            continue;
        }
        let payload = RefAddedPayload {
            ref_id: Uuid::new_v4().to_string(),
            project_id: project.clone(),
            title: item.title.clone(),
            authors: item.authors.clone(),
            year: item.year,
            venue: if item.venue.is_empty() { "Zotero".into() } else { item.venue },
            doi: item.doi.clone(),
            url: item.url.clone(),
            tags: if item.tags.is_empty() { "zotero".into() } else { format!("zotero,{}", item.tags) },
            source: "zotero".into(),
            zotero_item_key: Some(item.key.clone()),
            arxiv_id: None,
            abstract_text: None,
        };
        let stored = store
            .append(NewEvent::ref_added(payload.clone()).map_err(err)?)
            .map_err(err)?;
        // Grow the working set so a second item with the same identity is
        // skipped within THIS run too.
        refs.push(LibraryRef {
            id: payload.ref_id.clone(),
            project_id: payload.project_id,
            collection_id: None,
            title: payload.title,
            authors: payload.authors,
            year: payload.year,
            venue: payload.venue,
            doi: payload.doi,
            url: payload.url,
            isbn: String::new(),
            attachment: None,
            status: "unread".into(),
            tags: payload.tags,
            used: 0,
            citation_count: 0,
            created_at: stored.ts.to_rfc3339(),
            source: payload.source,
            zotero_item_key: payload.zotero_item_key,
            arxiv_id: payload.arxiv_id,
            removed: false,
            timeline: Vec::new(),
        });
        result.imported += 1;
        result.refs.push(refs.last().cloned().expect("just pushed"));
    }
    Ok(result)
}

/// The honest down-state error (FR-9.1): the connector probe's reason in
/// code form — the failure is visible, never swallowed.
pub(crate) fn zotero_unreachable_error(reason: &str) -> String {
    format!(
        "zotero_unreachable: {reason} — the Zotero connector is not answering; is Zotero \
         running with the local API enabled? / el conector de Zotero no responde; ¿está \
         Zotero en ejecución con la API local activada?"
    )
}

/// Connect Zotero and import the library's items (FR-15.2): probe the
/// connection first (the FR-9.1 seam — down is an honest error), then
/// fetch the local API's items and import them one `ref.added` each.
#[tauri::command]
pub async fn import_refs_from_zotero(db: State<'_, Db>) -> Result<ZoteroImportResult, String> {
    // Probe before anything is appended — the down state is honest and cheap.
    if let Err(reason) = crate::mcp::probe_connection("zotero").await {
        return Err(zotero_unreachable_error(&reason));
    }
    let json = fetch_zotero_items().await?;
    let items = parse_zotero_items(&json);
    let c = db.0.lock().await;
    import_zotero_items_inner(&c, items)
}

/// Fetch the Zotero local API's item list. The connector answers on its
/// well-known port; a fetch failure is honest (`zotero_fetch_failed:`).
async fn fetch_zotero_items() -> Result<Vec<serde_json::Value>, String> {
    let client = reqwest::Client::builder()
        .user_agent("research-core/0.1")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("zotero_fetch_failed: {e}"))?;
    let url = "http://localhost:23119/api/users/0/items?format=json&limit=100";
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("zotero_fetch_failed: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!(
            "zotero_fetch_failed: the connector answered HTTP {status} — the local API may be \
             disabled in Zotero's settings"
        ));
    }
    resp.json::<Vec<serde_json::Value>>()
        .await
        .map_err(|e| format!("zotero_fetch_failed: {e}"))
}

/// The ref's latest state event (its `ref.added` or latest `ref.restored`)
/// — the cause a `ref.removed` links to (the audit stamp of what was
/// removed). None for a legacy baseline ref (the migration predates the
/// domain — the removal stands on the row itself).
fn latest_state_event(events: &[StoredEvent], ref_id: &str) -> Option<Uuid> {
    events
        .iter()
        .filter(|e| e.kind == REF_ADDED || e.kind == REF_RESTORED)
        .filter(|e| {
            e.payload
                .get("ref_id")
                .and_then(|v| v.as_str())
                .map(|id| id == ref_id)
                .unwrap_or(false)
        })
        .map(|e| e.id)
        .next_back()
}

/// The ref's `ref.removed` event (the one a `ref.restored` undoes) — None
/// when the ref is not currently removed.
fn removal_event(events: &[StoredEvent], ref_id: &str) -> Option<Uuid> {
    events
        .iter()
        .filter(|e| e.kind == REF_REMOVED)
        .filter(|e| {
            e.payload
                .get("ref_id")
                .and_then(|v| v.as_str())
                .map(|id| id == ref_id)
                .unwrap_or(false)
        })
        .map(|e| e.id)
        .next_back()
}

/// Remove a reference (FR-15.4): append one auditable `ref.removed` event —
/// there is no destructive delete, ever. The ref must exist and be active.
#[tauri::command]
pub async fn remove_ref(db: State<'_, Db>, ref_id: String) -> Result<LibraryRef, String> {
    let c = db.0.lock().await;
    remove_ref_inner(&c, &ref_id)
}

/// Plain inner (testable). Typed codes: `not_found:` for an unknown ref,
/// `invalid_state:` for an already-removed one.
pub(crate) fn remove_ref_inner(conn: &Connection, ref_id: &str) -> Result<LibraryRef, String> {
    let ref_id = ref_id.trim();
    let state = library_state(conn, ref_id)?
        .ok_or_else(|| format!("not_found: no reference with id `{ref_id}` in the library"))?;
    if state.removed {
        return Err(format!(
            "invalid_state: the reference `{ref_id}` is already removed — restore it first \
             (FR-15.7)"
        ));
    }
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let cause = latest_state_event(&events, ref_id);
    EventStore::new(conn)
        .append(NewEvent::ref_removed(ref_id, cause).map_err(err)?)
        .map_err(err)?;
    library_state(conn, ref_id)?
        .ok_or_else(|| format!("invalid_state: the removed ref `{ref_id}` did not fold"))
}

/// Restore a removed reference (FR-15.7): the un-event — `ref.restored`
/// cause-linked to the `ref.removed` it undoes. The ref returns to active
/// and pinnable; the "source removed" pin flags clear.
#[tauri::command]
pub async fn restore_ref(db: State<'_, Db>, ref_id: String) -> Result<LibraryRef, String> {
    let c = db.0.lock().await;
    restore_ref_inner(&c, &ref_id)
}

/// Plain inner (testable). Typed codes: `not_found:` for an unknown ref,
/// `invalid_state:` for an active one.
pub(crate) fn restore_ref_inner(conn: &Connection, ref_id: &str) -> Result<LibraryRef, String> {
    let ref_id = ref_id.trim();
    let state = library_state(conn, ref_id)?
        .ok_or_else(|| format!("not_found: no reference with id `{ref_id}` in the library"))?;
    if !state.removed {
        return Err(format!(
            "invalid_state: the reference `{ref_id}` is not removed — nothing to restore"
        ));
    }
    let events = EventStore::new(conn).events_all().map_err(err)?;
    let removed_event = removal_event(&events, ref_id).ok_or_else(|| {
        format!("invalid_state: no {REF_REMOVED} event for `{ref_id}` — the log is inconsistent")
    })?;
    EventStore::new(conn)
        .append(NewEvent::ref_restored(ref_id, removed_event).map_err(err)?)
        .map_err(err)?;
    library_state(conn, ref_id)?
        .ok_or_else(|| format!("invalid_state: the restored ref `{ref_id}` did not fold"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::Db::migrate(&conn).unwrap();
        crate::db::Db::seed(&conn).unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    fn meta(url: &str, doi: &str) -> ResolvedPaper {
        ResolvedPaper {
            source: "arxiv".into(),
            arxiv_id: Some("2401.00001".into()),
            title: "A Fresh Paper".into(),
            authors: "Ngo et al.".into(),
            year: Some(2024),
            venue: "arXiv".into(),
            doi: Some(doi.into()),
            url: url.into(),
            abstract_text: Some("Abstract.".into()),
        }
    }

    #[test]
    fn manual_add_validates_title_and_identifier_then_dedups() {
        let c = test_db();
        // blank title → typed invalid_ref
        let e = add_ref_manual_inner(
            &c, "  ".into(), String::new(), None, String::new(),
            String::new(), String::new(), String::new(),
        )
        .unwrap_err();
        assert!(e.starts_with("invalid_ref:"), "{e}");
        // no identifier → typed invalid_ref
        let e = add_ref_manual_inner(
            &c, "Title".into(), String::new(), None, String::new(),
            "  ".into(), "  ".into(), String::new(),
        )
        .unwrap_err();
        assert!(e.starts_with("invalid_ref:"), "{e}");
        // a valid add lands as a ref.added event and folds back
        let r = add_ref_manual_inner(
            &c, "A Manual Paper".into(), "Doe".into(), Some(2020),
            "Nature".into(), "10.1/manual".into(), "https://x.example".into(),
            "note".into(),
        )
        .unwrap();
        assert_eq!(r.source, "manual");
        assert_eq!(r.title, "A Manual Paper");
        assert!(!r.removed);
        assert_eq!(r.timeline.len(), 1);
        assert_eq!(r.timeline[0].kind, REF_ADDED);
        // the same doi is refused — already in the library
        let e = add_ref_manual_inner(
            &c, "Same Paper Again".into(), String::new(), None, String::new(),
            "10.1/manual".into(), String::new(), String::new(),
        )
        .unwrap_err();
        assert!(e.starts_with("already_in_library:"), "{e}");
    }

    #[test]
    fn arxiv_add_reuses_the_library_dedup_against_legacy_rows() {
        let c = test_db();
        // the seeded legacy "Attention Is All You Need" url → honest refusal
        let e = add_ref_from_link_inner(
            &c,
            meta("https://arxiv.org/abs/1706.03762", "10.48550/arXiv.1706.03762"),
        )
        .unwrap_err();
        assert!(e.starts_with("already_in_library:"), "{e}");
        // a fresh paper appends one ref.added (source: arxiv, tags carry it)
        let r = add_ref_from_link_inner(
            &c,
            meta("https://arxiv.org/abs/2401.00001", "10.48550/arXiv.2401.00001"),
        )
        .unwrap();
        assert_eq!(r.source, "arxiv");
        assert_eq!(r.arxiv_id.as_deref(), Some("2401.00001"));
        assert!(r.tags.contains("arXiv"));
    }

    #[test]
    fn a_resolved_doi_paper_lands_as_a_ref_added_carrying_its_source_and_doi() {
        let c = test_db();
        let r = add_ref_from_link_inner(
            &c,
            ResolvedPaper {
                source: "doi".into(),
                arxiv_id: None,
                title: "Deep learning".into(),
                authors: "LeCun, Yann, Bengio, Yoshua, Hinton, Geoffrey".into(),
                year: Some(2015),
                venue: "Nature".into(),
                doi: Some("10.1038/nature14539".into()),
                url: "https://doi.org/10.1038/nature14539".into(),
                abstract_text: Some("Deep learning allows computational models.".into()),
            },
        )
        .unwrap();
        // the event carries the resolved source + doi (the ref.added fold)
        assert_eq!(r.source, "doi");
        assert_eq!(r.doi, "10.1038/nature14539");
        assert_eq!(r.url, "https://doi.org/10.1038/nature14539");
        assert_eq!(r.arxiv_id, None);
        assert_eq!(r.tags, "doi");
        assert!(!r.removed);
        // the doi is the dedup identity: a second add is refused honestly
        let e = add_ref_from_link_inner(
            &c,
            ResolvedPaper {
                source: "crossref".into(),
                arxiv_id: None,
                title: "Deep learning (again)".into(),
                authors: String::new(),
                year: None,
                venue: String::new(),
                doi: Some("10.1038/nature14539".into()),
                url: "https://doi.org/10.1038/nature14539".into(),
                abstract_text: None,
            },
        )
        .unwrap_err();
        assert!(e.starts_with("already_in_library:"), "{e}");
        // the closed vocabulary accepts every resolved source
        for source in ["pubmed", "s2", "openalex", "crossref"] {
            let r = add_ref_from_link_inner(
                &c,
                ResolvedPaper {
                    source: source.into(),
                    arxiv_id: None,
                    title: format!("Paper from {source}"),
                    authors: String::new(),
                    year: None,
                    venue: String::new(),
                    doi: Some(format!("10.1/{source}")),
                    url: format!("https://example.org/{source}"),
                    abstract_text: None,
                },
            )
            .unwrap();
            assert_eq!(r.source, source);
        }
    }

    #[test]
    fn remove_and_restore_are_auditable_state_transitions() {
        let c = test_db();
        let added = add_ref_manual_inner(
            &c, "To Remove".into(), "A".into(), None, String::new(),
            "10.1/rm".into(), String::new(), String::new(),
        )
        .unwrap();
        // unknown ref → not_found; the ref.removed cause links the add event
        let e = remove_ref_inner(&c, "no-such-ref").unwrap_err();
        assert!(e.starts_with("not_found:"), "{e}");
        let removed = remove_ref_inner(&c, &added.id).unwrap();
        assert!(removed.removed);
        assert_eq!(removed.timeline.len(), 2);
        assert_eq!(removed.timeline[1].kind, REF_REMOVED);
        // double remove → invalid_state
        let e = remove_ref_inner(&c, &added.id).unwrap_err();
        assert!(e.starts_with("invalid_state:"), "{e}");
        // restore on an active ref → invalid_state
        let fresh = add_ref_manual_inner(
            &c, "Active".into(), "B".into(), None, String::new(),
            "10.1/act".into(), String::new(), String::new(),
        )
        .unwrap();
        let e = restore_ref_inner(&c, &fresh.id).unwrap_err();
        assert!(e.starts_with("invalid_state:"), "{e}");
        // restore works and the timeline carries the un-event
        let restored = restore_ref_inner(&c, &added.id).unwrap();
        assert!(!restored.removed);
        assert_eq!(restored.timeline.len(), 3);
        assert_eq!(restored.timeline[2].kind, REF_RESTORED);
        // the ref.restored event is cause-linked to its ref.removed
        let events = EventStore::new(&c).events_all().unwrap();
        let removed_id = events
            .iter()
            .find(|e| e.kind == REF_REMOVED)
            .unwrap()
            .id;
        let restored_ev = events
            .iter()
            .find(|e| e.kind == REF_RESTORED)
            .unwrap();
        assert_eq!(restored_ev.causes, vec![removed_id]);
        // a legacy ref removes and restores through the same events
        let legacy_id: String = c
            .query_row("SELECT id FROM refs LIMIT 1", [], |r| r.get(0))
            .unwrap();
        assert!(remove_ref_inner(&c, &legacy_id).unwrap().removed);
        assert!(!restore_ref_inner(&c, &legacy_id).unwrap().removed);
    }

    #[test]
    fn zotero_items_parse_and_import_with_honest_counts() {
        let c = test_db();
        let json: Vec<serde_json::Value> = serde_json::from_str(
            r#"[
              {"key":"K1","data":{"itemType":"journalArticle","title":"Zotero Paper One",
               "creators":[{"firstName":"Ada","lastName":"Lovelace"}],"date":"2022-03-01",
               "DOI":"10.2/z1","url":"https://z.example/1","publicationTitle":"JMLR",
               "tags":[{"tag":"ml"},{"tag":"theory"}]}},
              {"key":"K2","data":{"itemType":"journalArticle","title":"",
               "creators":[],"date":"2020","DOI":"10.2/z2","url":"https://z.example/2","tags":[]}},
              {"key":"K3","data":{"itemType":"attachment","title":"pdf","tags":[]}},
              {"key":"K4","data":{"itemType":"book","title":"Attention Is All You Need",
               "creators":[{"name":"Vaswani et al."}],"date":"2017",
               "DOI":"10.48550/arXiv.1706.03762","url":"","tags":[]}}
            ]"#,
        )
        .unwrap();
        let items = parse_zotero_items(&json);
        assert_eq!(items.len(), 3, "attachments are not library refs");
        assert_eq!(items[0].authors, "Lovelace, Ada");
        assert_eq!(items[0].year, Some(2022));
        assert_eq!(items[0].tags, "ml,theory");
        assert_eq!(items[2].authors, "Vaswani et al.");

        let result = import_zotero_items_inner(&c, items).unwrap();
        // 1 imported (K1), 1 failed (K2 no title), 1 skipped (K4 = the seeded dup)
        assert_eq!(result.imported, 1);
        assert_eq!(result.failed, 1);
        assert_eq!(result.skipped, 1);
        assert_eq!(result.refs.len(), 1);
        assert_eq!(result.refs[0].source, "zotero");
        assert_eq!(result.refs[0].zotero_item_key.as_deref(), Some("K1"));
        assert_eq!(result.refs[0].venue, "JMLR");
        assert!(result.skipped_items[0].contains("already in the library"));
        assert_eq!(result.failed_items[0], "K2 — no title");

        // re-importing the same items skips everything (dedup by zotero key)
        let json2: Vec<serde_json::Value> = serde_json::from_str(
            r#"[{"key":"K1","data":{"itemType":"journalArticle","title":"Zotero Paper One",
                "creators":[],"date":"2022","DOI":"10.2/z1","url":"https://z.example/1","tags":[]}}]"#,
        )
        .unwrap();
        let again = import_zotero_items_inner(&c, parse_zotero_items(&json2)).unwrap();
        assert_eq!(again.imported, 0);
        assert_eq!(again.skipped, 1);

        // the down-state error is the honest coded form
        assert!(zotero_unreachable_error("unreachable").starts_with("zotero_unreachable:"));
    }

    #[test]
    fn list_refs_inner_filters_by_project_and_archived_state() {
        let c = test_db();
        let all = list_refs_inner(&c, None, None).unwrap();
        assert!(all.len() >= 6);
        let legacy_id: String = c
            .query_row("SELECT id FROM refs LIMIT 1", [], |r| r.get(0))
            .unwrap();
        remove_ref_inner(&c, &legacy_id).unwrap();
        let active = list_refs_inner(&c, None, Some("active")).unwrap();
        assert_eq!(active.len(), all.len() - 1);
        assert!(active.iter().all(|r| !r.removed));
        let archived = list_refs_inner(&c, None, Some("removed")).unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].id, legacy_id);
    }
}
