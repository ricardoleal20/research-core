// Library domain (FR-15, Epic 5 — references CRUD): the reference manager
// becomes evented. Adds land as `ref.added` events carrying their source
// (arxiv | zotero | manual); removal is an auditable `ref.removed` state
// transition (never a destructive delete — the log stays the single source
// of truth, zero data loss); restore is the un-event `ref.restored`.
//
// The library projection seeds from the legacy relational `refs` table (the
// v1 migration's output — read-only per AD-16) and folds the event log on
// top: legacy rows are the implicit baseline, evented rows apply after
// them, and a `ref.removed` event masks its ref (archived) until a
// `ref.restored` event un-masks it. Replaying the log yields the identical
// library state, archived flags included (NFR-8).
//
// The arXiv fetch adapter is shared with the onboarding first-value flow
// (FR-15.1 reuses the FR-8.1 URL parsing + fetch verbatim): both the
// onboarding paste and the library add resolve through
// `fetch_arxiv_metadata`, so the two doors can never drift.
//
// Errors are coded and bilingual-safe (codes are never translated,
// EXPERIENCE.md): `invalid_url:`, `fetch_failed:`, `already_in_library:`,
// `not_found:`, `invalid_state:`.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::onboarding::{self, OnboardingError};
use crate::eventstore::{Actor, EventError, NewEvent, StoredEvent};

pub const REF_ADDED: &str = "ref.added";
pub const REF_REMOVED: &str = "ref.removed";
pub const REF_RESTORED: &str = "ref.restored";

/// The closed `ref.added` source vocabulary (FR-15.1/15.2/15.3). Legacy
/// baseline rows carry their derived source (`legacy` is never appended).
pub const REF_SOURCES: [&str; 3] = ["arxiv", "zotero", "manual"];

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error(transparent)]
    Onboarding(#[from] OnboardingError),
    #[error(transparent)]
    Event(#[from] EventError),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
}

/// The `ref.added` payload — everything the library fold needs to grow a
/// ref from the log alone. `ref_id` is the ref's identity (a uuid string);
/// the legacy columns the UI renders (`status`, `used`, …) start at their
/// honest defaults and are owned by the projection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefAddedPayload {
    pub ref_id: String,
    pub project_id: String,
    pub title: String,
    #[serde(default)]
    pub authors: String,
    #[serde(default)]
    pub year: Option<i64>,
    #[serde(default)]
    pub venue: String,
    #[serde(default)]
    pub doi: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub tags: String,
    /// arxiv | zotero | manual (the closed vocabulary above).
    pub source: String,
    #[serde(default)]
    pub zotero_item_key: Option<String>,
    #[serde(default)]
    pub arxiv_id: Option<String>,
    #[serde(default)]
    pub abstract_text: Option<String>,
}

/// The `ref.removed` / `ref.restored` payloads — the ref's identity plus
/// nothing: removal is a state transition, not new data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefStatePayload {
    pub ref_id: String,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `ref.added` event comes into
    /// being. Actor is the user — every add is a human action (the Zotero
    /// import appends one event per imported item, actor=user, the human ran
    /// the import). Validates: title non-empty (FR-15.3), source in the
    /// closed vocabulary, ref_id non-empty.
    pub fn ref_added(payload: RefAddedPayload) -> Result<Self, EventError> {
        if payload.title.trim().is_empty() {
            return Err(EventError::Invalid(
                "ref.title must not be empty — a reference is titled (FR-15.3)".into(),
            ));
        }
        if !REF_SOURCES.contains(&payload.source.as_str()) {
            return Err(EventError::Invalid(format!(
                "ref.source `{}` is not one of the closed vocabulary {:?}",
                payload.source, REF_SOURCES
            )));
        }
        if payload.ref_id.trim().is_empty() {
            return Err(EventError::Invalid(
                "ref.ref_id must not be empty — an added ref has its identity".into(),
            ));
        }
        Ok(Self::new(
            REF_ADDED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?)
    }

    /// Typed constructor (AD-15): the auditable remove — a `ref.removed`
    /// event cause-linked to the ref's latest state event (its `ref.added`
    /// or latest `ref.restored`), the audit stamp of what was removed.
    pub fn ref_removed(ref_id: &str, cause: Option<Uuid>) -> Result<Self, EventError> {
        let ref_id = ref_id.trim();
        if ref_id.is_empty() {
            return Err(EventError::Invalid(
                "ref.ref_id must not be empty — a removal names its ref".into(),
            ));
        }
        let payload = RefStatePayload {
            ref_id: ref_id.to_string(),
        };
        let mut event = Self::new(
            REF_REMOVED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?;
        if let Some(cause) = cause {
            event = event.with_causes(vec![cause]);
        }
        Ok(event)
    }

    /// Typed constructor (AD-15): the un-event — `ref.restored` cause-linked
    /// to the `ref.removed` event it undoes (FR-15.7).
    pub fn ref_restored(ref_id: &str, removed_event: Uuid) -> Result<Self, EventError> {
        let ref_id = ref_id.trim();
        if ref_id.is_empty() {
            return Err(EventError::Invalid(
                "ref.ref_id must not be empty — a restore names its ref".into(),
            ));
        }
        let payload = RefStatePayload {
            ref_id: ref_id.to_string(),
        };
        Ok(Self::new(
            REF_RESTORED,
            Actor::User,
            serde_json::to_value(&payload)?,
        )?
        .with_causes(vec![removed_event]))
    }
}

/// One entry of the ref's own mini-timeline — the receipt-voice audit trail
/// the detail drawer renders (mono, seq + ts + actor).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefAuditEntry {
    pub seq: i64,
    pub ts: String,
    /// "user" | "agent:<run_id>" | "system:<component>" — the actor stamp.
    pub actor: String,
    /// ref.added | ref.removed | ref.restored
    pub kind: String,
}

/// The ref as the library projection reads it — the legacy `refs` row shape
/// (snake_case, a drop-in for the UI's `Ref`) plus the evented state: its
/// source, its Zotero/arXiv identity, the archived flag, and the audit
/// timeline. Legacy baseline rows carry an empty timeline (the migration
/// import predates the library domain — an honest empty state).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryRef {
    pub id: String,
    pub project_id: String,
    pub collection_id: Option<String>,
    pub title: String,
    pub authors: String,
    pub year: Option<i64>,
    pub venue: String,
    pub doi: String,
    pub url: String,
    pub isbn: String,
    pub attachment: Option<String>,
    pub status: String,
    pub tags: String,
    pub used: i64,
    pub citation_count: i64,
    pub created_at: String,
    /// arxiv | zotero | manual (evented) or the derived legacy badge source.
    pub source: String,
    #[serde(default)]
    pub zotero_item_key: Option<String>,
    #[serde(default)]
    pub arxiv_id: Option<String>,
    /// FR-15.4/15.5: true while a `ref.removed` event masks the ref —
    /// archived/struck in the library, never pinnable — until restored.
    #[serde(default)]
    pub removed: bool,
    #[serde(default)]
    pub timeline: Vec<RefAuditEntry>,
}

/// The short actor stamp the timeline renders.
pub fn actor_stamp(actor: &Actor) -> String {
    match actor {
        Actor::User => "user".into(),
        Actor::Agent { run_id } => format!("agent:{run_id}"),
        Actor::System { component } => format!(
            "system:{}",
            serde_json::to_value(component)
                .map(|v| v.as_str().unwrap_or("?").to_string())
                .unwrap_or_else(|_| "?".into())
        ),
    }
}

/// The legacy baseline's derived source badge (the pre-evented heuristic the
/// UI rendered): an arXiv DOI badges arXiv, an attachment badges Zotero,
/// everything else badges Manual.
fn derived_source(doi: &str, attachment: Option<&str>) -> String {
    if doi.starts_with("10.48550/arXiv.") {
        "arxiv".into()
    } else if attachment.is_some() {
        "zotero".into()
    } else {
        "manual".into()
    }
}

fn baseline(conn: &Connection) -> Result<Vec<LibraryRef>, rusqlite::Error> {
    // A workspace whose relational schema never existed (fresh test
    // workspaces, event-only stores) has an empty baseline — the fold is
    // event-only there, never an error.
    let has_table: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'refs'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !has_table {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, project_id, collection_id, title, authors, year, venue, doi, url, isbn, \
         attachment, status, tags, used, citation_count, created_at FROM refs",
    )?;
    let rows = stmt.query_map([], |r| {
        let doi: String = r.get(7)?;
        let attachment: Option<String> = r.get(10)?;
        let source = derived_source(&doi, attachment.as_deref());
        Ok(LibraryRef {
            id: r.get(0)?,
            project_id: r.get(1)?,
            collection_id: r.get(2)?,
            title: r.get(3)?,
            authors: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
            year: r.get(5)?,
            venue: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
            doi,
            url: r.get::<_, Option<String>>(8)?.unwrap_or_default(),
            isbn: r.get::<_, Option<String>>(9)?.unwrap_or_default(),
            attachment,
            status: r.get(11)?,
            tags: r.get::<_, Option<String>>(12)?.unwrap_or_default(),
            used: r.get(13)?,
            citation_count: r.get(14)?,
            created_at: r.get(15)?,
            source,
            zotero_item_key: None,
            arxiv_id: None,
            removed: false,
            timeline: Vec::new(),
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// The library projection: the legacy `refs` table as the implicit baseline,
/// the event log folded on top. `ref.added` upserts by id; `ref.removed`
/// masks (archived); `ref.restored` un-masks. State events naming an
/// unknown ref are skipped (the pin fold's convention) — the commands never
/// append them, and a hand-edited log must not brick the whole read.
pub struct LibraryProjection;

impl LibraryProjection {
    pub fn fold(conn: &Connection, events: &[StoredEvent]) -> Result<Vec<LibraryRef>, EventError> {
        let mut refs = baseline(conn)?;
        for event in events {
            match event.kind.as_str() {
                REF_ADDED => {
                    let payload: RefAddedPayload = match serde_json::from_value(event.payload.clone())
                    {
                        Ok(p) => p,
                        Err(_) => {
                            return Err(EventError::Invalid(format!(
                                "corrupt {REF_ADDED} event at seq {}: the payload is not a \
                                 ref.added payload",
                                event.seq
                            )))
                        }
                    };
                    let ref_id = payload.ref_id.clone();
                    if let Some(r) = refs.iter_mut().find(|r| r.id == ref_id) {
                        // A re-add of a known id refreshes the metadata.
                        r.title = payload.title;
                        r.authors = payload.authors;
                        r.year = payload.year;
                        r.venue = payload.venue;
                        r.doi = payload.doi;
                        r.url = payload.url;
                        r.tags = payload.tags;
                        r.source = payload.source;
                        r.zotero_item_key = payload.zotero_item_key;
                        r.arxiv_id = payload.arxiv_id;
                    } else {
                        refs.push(LibraryRef {
                            id: payload.ref_id,
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
                            created_at: event.ts.to_rfc3339(),
                            source: payload.source,
                            zotero_item_key: payload.zotero_item_key,
                            arxiv_id: payload.arxiv_id,
                            removed: false,
                            timeline: Vec::new(),
                        });
                    }
                    if let Some(r) = refs.iter_mut().find(|r| r.id == ref_id) {
                        r.timeline.push(RefAuditEntry {
                            seq: event.seq,
                            ts: event.ts.to_rfc3339(),
                            actor: actor_stamp(&event.actor),
                            kind: event.kind.clone(),
                        });
                    }
                }
                REF_REMOVED | REF_RESTORED => {
                    let payload: RefStatePayload = match serde_json::from_value(event.payload.clone())
                    {
                        Ok(p) => p,
                        Err(_) => {
                            return Err(EventError::Invalid(format!(
                                "corrupt {} event at seq {}: the payload is not a ref state \
                                 payload",
                                event.kind, event.seq
                            )))
                        }
                    };
                    if let Some(r) = refs.iter_mut().find(|r| r.id == payload.ref_id) {
                        r.removed = event.kind == REF_REMOVED;
                        r.timeline.push(RefAuditEntry {
                            seq: event.seq,
                            ts: event.ts.to_rfc3339(),
                            actor: actor_stamp(&event.actor),
                            kind: event.kind.clone(),
                        });
                    }
                }
                _ => {}
            }
        }
        // The legacy read's order (year DESC) so the table renders the same.
        refs.sort_by(|a, b| b.year.cmp(&a.year).then_with(|| a.title.cmp(&b.title)));
        Ok(refs)
    }
}

/// Dedup (FR-15.1/15.2): an existing ref with the same url, doi, or Zotero
/// item key is the same paper — the add is refused, never duplicated.
pub fn find_duplicate<'a>(
    refs: &'a [LibraryRef],
    doi: &str,
    url: &str,
    zotero_item_key: Option<&str>,
) -> Option<&'a LibraryRef> {
    refs.iter().find(|r| {
        (!doi.is_empty() && !r.doi.is_empty() && r.doi == doi)
            || (!url.is_empty() && !r.url.is_empty() && r.url == url)
            || zotero_item_key.is_some_and(|k| r.zotero_item_key.as_deref() == Some(k))
    })
}

/// The arXiv paper's metadata as fetched — the shared adapter's output.
#[derive(Debug, Clone, PartialEq)]
pub struct ArxivMetadata {
    pub arxiv_id: String,
    pub title: String,
    pub authors: String,
    pub year: Option<i64>,
    pub venue: String,
    pub doi: String,
    pub url: String,
    pub abstract_text: Option<String>,
}

/// The shared arXiv fetch adapter (FR-15.1 reusing FR-8.1 verbatim): the
/// same URL parsing (`invalid_url:` before any network) and the same export
/// API fetch behind `run_first_value` — the onboarding paste door and the
/// library add door resolve through this one function, so they can never
/// drift. A data fetch, not an LLM call (AD-9 governs LLM calls only).
pub async fn fetch_arxiv_metadata(url: &str) -> Result<ArxivMetadata, LibraryError> {
    let arxiv_id = onboarding::parse_arxiv_url(url)?;
    let search = crate::mcp::arxiv_fetch(&arxiv_id)
        .await
        .map_err(OnboardingError::FetchFailed)?
        .ok_or_else(|| {
            OnboardingError::FetchFailed(format!("arXiv has no paper with id `{arxiv_id}`"))
        })?;
    Ok(ArxivMetadata {
        arxiv_id,
        title: search.title,
        authors: search.authors,
        year: search.year,
        venue: search.venue,
        doi: search.doi,
        url: search.url,
        abstract_text: search.abstract_text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventstore::EventStore;
    use rusqlite::Connection;

    /// An in-memory migrated + seeded workspace: the demo legacy refs exist
    /// (the baseline), the eventstore is initialized.
    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::Db::migrate(&conn).unwrap();
        crate::db::Db::seed(&conn).unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    fn added(ref_id: &str, url: &str, source: &str) -> NewEvent {
        NewEvent::ref_added(RefAddedPayload {
            ref_id: ref_id.into(),
            project_id: "p1".into(),
            title: "A Manual Paper".into(),
            authors: "Doe et al.".into(),
            year: Some(2024),
            venue: "arXiv".into(),
            doi: String::new(),
            url: url.into(),
            tags: "manual".into(),
            source: source.into(),
            zotero_item_key: None,
            arxiv_id: None,
            abstract_text: None,
        })
        .unwrap()
    }

    #[test]
    fn constructors_validate_their_guarantees() {
        // blank title, unknown source, blank ref_id are refused
        let mut p = RefAddedPayload {
            ref_id: "r".into(),
            project_id: "p1".into(),
            title: "  ".into(),
            authors: String::new(),
            year: None,
            venue: String::new(),
            doi: String::new(),
            url: String::new(),
            tags: String::new(),
            source: "manual".into(),
            zotero_item_key: None,
            arxiv_id: None,
            abstract_text: None,
        };
        assert!(NewEvent::ref_added(p.clone()).is_err());
        p.title = "T".into();
        p.source = "pinterest".into();
        let e = NewEvent::ref_added(p.clone()).unwrap_err().to_string();
        assert!(e.contains("closed vocabulary"), "{e}");
        p.source = "manual".into();
        p.ref_id = "  ".into();
        assert!(NewEvent::ref_added(p).is_err());
        assert!(NewEvent::ref_removed("", None).is_err());
        assert!(NewEvent::ref_restored("", Uuid::new_v4()).is_err());
    }

    #[test]
    fn the_fold_seeds_the_legacy_baseline_and_applies_evented_refs_on_top() {
        let c = conn();
        let store = EventStore::new(&c);
        let baseline_count = LibraryProjection::fold(&c, &[]).unwrap().len();
        assert!(baseline_count >= 6, "the seeded legacy refs are the baseline");

        let stored = store.append(added("ref-new-1", "https://example.org/paper", "manual")).unwrap();
        let refs = LibraryProjection::fold(&c, &[stored]).unwrap();
        assert_eq!(refs.len(), baseline_count + 1);
        let new_ref = refs.iter().find(|r| r.id == "ref-new-1").unwrap();
        assert_eq!(new_ref.source, "manual");
        assert!(!new_ref.removed);
        assert_eq!(new_ref.timeline.len(), 1);
        assert_eq!(new_ref.timeline[0].kind, REF_ADDED);
        assert_eq!(new_ref.timeline[0].actor, "user");

        // legacy rows are present with their derived source badges
        let legacy = refs.iter().find(|r| r.doi == "10.48550/arXiv.1706.03762").unwrap();
        assert_eq!(legacy.source, "arxiv");
        assert!(legacy.timeline.is_empty(), "the migration predates the domain");
    }

    #[test]
    fn removal_masks_restore_unmasks_and_replay_is_identical() {
        let c = conn();
        let store = EventStore::new(&c);
        // an evented ref + a legacy ref
        let add = store.append(added("ref-new-1", "https://example.org/paper", "manual")).unwrap();
        let legacy_id: String = c
            .query_row(
                "SELECT id FROM refs WHERE doi = '10.48550/arXiv.1706.03762'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        let rem1 = store
            .append(NewEvent::ref_removed("ref-new-1", Some(add.id)).unwrap())
            .unwrap();
        let rem2 = store
            .append(NewEvent::ref_removed(&legacy_id, None).unwrap())
            .unwrap();

        let events = store.events_all().unwrap();
        let refs = LibraryProjection::fold(&c, &events).unwrap();
        let evented = refs.iter().find(|r| r.id == "ref-new-1").unwrap();
        assert!(evented.removed, "the evented ref is archived");
        assert_eq!(evented.timeline.len(), 2);
        let legacy = refs.iter().find(|r| r.id == legacy_id).unwrap();
        assert!(legacy.removed, "a removed LEGACY ref is masked by the event");

        // restore both
        store
            .append(NewEvent::ref_restored("ref-new-1", rem1.id).unwrap())
            .unwrap();
        store
            .append(NewEvent::ref_restored(&legacy_id, rem2.id).unwrap())
            .unwrap();
        let events = store.events_all().unwrap();
        let refs = LibraryProjection::fold(&c, &events).unwrap();
        assert!(!refs.iter().find(|r| r.id == "ref-new-1").unwrap().removed);
        assert!(!refs.iter().find(|r| r.id == legacy_id).unwrap().removed);

        // NFR-8: replaying the log yields the identical state, archived
        // flags and timelines included.
        let replay = LibraryProjection::fold(&c, &events).unwrap();
        assert_eq!(refs, replay);
    }

    #[test]
    fn state_events_for_unknown_refs_are_skipped_not_fatal() {
        let c = conn();
        let store = EventStore::new(&c);
        store
            .append(NewEvent::ref_removed("ghost-ref", None).unwrap())
            .unwrap();
        let events = store.events_all().unwrap();
        let refs = LibraryProjection::fold(&c, &events).unwrap();
        assert!(refs.iter().all(|r| !r.removed));
    }

    #[test]
    fn dedup_matches_url_doi_and_zotero_key() {
        let c = conn();
        let refs = LibraryProjection::fold(&c, &[]).unwrap();
        // url match against a seeded legacy ref
        let dup = find_duplicate(&refs, "", "https://arxiv.org/abs/1706.03762", None);
        assert_eq!(dup.unwrap().title, "Attention Is All You Need");
        // doi match
        let dup = find_duplicate(&refs, "10.48550/arXiv.1409.0473", "", None);
        assert!(dup.is_some());
        // no match for a fresh paper
        assert!(find_duplicate(&refs, "10.1/x", "https://fresh.example", None).is_none());
        // zotero key match on an evented zotero ref
        let store = EventStore::new(&c);
        let mut p = RefAddedPayload {
            ref_id: "z1".into(),
            project_id: "p1".into(),
            title: "From Zotero".into(),
            authors: String::new(),
            year: None,
            venue: String::new(),
            doi: String::new(),
            url: String::new(),
            tags: String::new(),
            source: "zotero".into(),
            zotero_item_key: Some("ITEMKEY1".into()),
            arxiv_id: None,
            abstract_text: None,
        };
        let stored = store.append(NewEvent::ref_added(p.clone()).unwrap()).unwrap();
        let refs = LibraryProjection::fold(&c, &[stored]).unwrap();
        assert!(find_duplicate(&refs, "", "", Some("ITEMKEY1")).is_some());
        p.zotero_item_key = None;
        let stored = store.append(NewEvent::ref_added(p).unwrap()).unwrap();
        let refs = LibraryProjection::fold(&c, &[stored]).unwrap();
        // with no key and no doi/url, no duplicate — even the same title
        assert!(find_duplicate(&refs, "", "", None).is_none());
    }
}
