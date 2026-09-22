// Search protocol disclosure (FR-12, Story 4.1): every search the app
// performs — the Night Shift scans, anything the UI/assistant runs — goes
// through ONE `run_search` seam that executes the search, appends a
// `search.run` event, and returns the results. The log cannot be skipped
// (AD-1: the log is the truth), and a review's disclosure is a PURE FOLD
// over those events — reproducible, not a slot machine.
//
// Null results are first-class (FR-12.1): a search returning zero appends
// the IDENTICAL event with `result_count: 0`; `null_result` is DERIVED at
// construction (`result_count == 0`), never trusted from the caller — a
// null result is a fact of the count, not an opinion. A review cannot
// render its disclosure as empty when searches occurred: the fold emits
// one row per `search.run`, null rows included and visibly marked.
//
// v1's search execution is the simulated corpus (the same honesty as the
// provider layer's simulated fallback — real databases land with the
// adapter plugins, v0.2.0): an arXiv-flavored static corpus matched by
// query tokens and shaped by the PRISMA filters. The `null_result_count`
// the fold exposes is the seam Story 4.3's readiness gate will count as
// undisclosed null results.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::eventstore::{Actor, EventError, EventStore, NewEvent, StoredEvent};

pub const SEARCH_RUN: &str = "search.run";

/// The Night Shift literature scan's database (Story 4.1): v1's scan runs
/// the simulated arXiv corpus — the event records what actually ran.
pub const DATABASE_ARXIV: &str = "arxiv";

/// The `search.run` payload (FR-12.1): the PRISMA record of one search.
/// `started_at` is the event envelope's `ts` (the store assigns it);
/// `null_result` is derived at construction from `result_count`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchRunPayload {
    /// What was searched — the query as typed/built, never paraphrased.
    pub query: String,
    /// Where it ran — e.g. "arxiv", "semantic-scholar", "zotero", "web".
    pub database: String,
    /// The PRISMA filters as a JSON map — e.g. `from_year`, `to_year`,
    /// `venue`. Empty map when the search ran unfiltered (recorded as
    /// such, never omitted).
    pub filters: BTreeMap<String, Value>,
    /// The ordering the search requested (e.g. "relevance", "date-desc").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
    /// First-page metadata: whether this record is the search's first
    /// page (the PRISMA "first pass" marker).
    pub first_page: bool,
    /// How many results the search returned — `0` for a null result.
    pub result_count: u32,
    /// DERIVED (`result_count == 0`): the constructor overwrites whatever
    /// the caller passed — null results are logged identically, never
    /// hidden (FR-12.1).
    pub null_result: bool,
    /// The mission the search ran for, when mission-scoped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mission_id: Option<Uuid>,
    /// The agent run that performed the search, when run-scoped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

impl NewEvent {
    /// Typed constructor (AD-15): the one way a `search.run` event comes
    /// into being. The actor is the agent run when the search was
    /// run-scoped, else the user; the mission is the cause-link. Fails
    /// loudly on an empty query or database — a search that cannot say
    /// what it searched or where it ran is not a reproducible search.
    pub fn search_run(payload: SearchRunPayload) -> Result<Self, EventError> {
        if payload.query.trim().is_empty() {
            return Err(EventError::Invalid(
                "search.run requires a query — the disclosure renders what was searched (FR-12.1)"
                    .into(),
            ));
        }
        if payload.database.trim().is_empty() {
            return Err(EventError::Invalid(
                "search.run requires a database — a search names where it ran (FR-12.1)".into(),
            ));
        }
        let mut payload = payload;
        // Null results derive from the count — never the caller's word.
        payload.null_result = payload.result_count == 0;
        let actor = match &payload.run_id {
            Some(run_id) => Actor::Agent { run_id: run_id.clone() },
            None => Actor::User,
        };
        let causes = payload.mission_id.into_iter().collect::<Vec<_>>();
        Self::new(
            SEARCH_RUN,
            actor,
            serde_json::to_value(&payload)?,
        )
        .map(|e| e.with_causes(causes))
    }
}

// ---------------------------------------------------------------------------
// The one search seam (FR-12.1): execute, append, return
// ---------------------------------------------------------------------------

/// What one search run takes: the query, the database, the PRISMA filters,
/// the ordering/first-page metadata, and the cause-links.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchParams {
    pub query: String,
    pub database: String,
    pub filters: BTreeMap<String, Value>,
    pub order: Option<String>,
    pub first_page: bool,
    pub mission_id: Option<Uuid>,
    pub run_id: Option<String>,
}

/// One result the simulated corpus returned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub title: String,
    pub authors: String,
    pub year: u32,
    pub venue: String,
    pub url: String,
}

/// What `run_search` returns: the results plus the disclosure row the
/// appended event folds to (the UI renders both — the row IS the record).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRunOutcome {
    pub results: Vec<SearchResult>,
    pub row: SearchDisclosureRow,
}

/// The ONE search entry (FR-12.1): executes the search, appends the
/// `search.run` event with the count it actually returned, and returns
/// both — so the log cannot be skipped. A zero-result search appends the
/// identical event (`result_count: 0`, derived `null_result: true`).
/// Validation failures append nothing.
pub fn run_search(
    store: &EventStore<'_>,
    params: &SearchParams,
) -> Result<SearchRunOutcome, EventError> {
    // Execute first — the count is a fact before it is a record.
    let results = execute_search(params);
    let payload = SearchRunPayload {
        query: params.query.clone(),
        database: params.database.clone(),
        filters: params.filters.clone(),
        order: params.order.clone(),
        first_page: params.first_page,
        result_count: results.len() as u32,
        null_result: false, // derived by the constructor
        mission_id: params.mission_id,
        run_id: params.run_id.clone(),
    };
    let stored = store.append(NewEvent::search_run(payload)?)?;
    Ok(SearchRunOutcome {
        results,
        row: SearchDisclosureRow::of(&stored),
    })
}

// ---------------------------------------------------------------------------
// The simulated corpus (v1): deterministic, offline, arXiv-flavored
// ---------------------------------------------------------------------------

/// How many results a first page carries (the PRISMA first-pass page).
const FIRST_PAGE_SIZE: usize = 10;

/// Query tokens too generic to match on (the corpus matches substance,
/// not syntax).
const STOPWORDS: [&str; 10] = [
    "does", "what", "when", "with", "that", "this", "them", "than", "have", "hold",
];

struct CorpusEntry {
    title: &'static str,
    authors: &'static str,
    year: u32,
    venue: &'static str,
    url: &'static str,
}

/// The v1 simulated corpus: static, seeded, arXiv-flavored. Deterministic
/// by construction — the same query, database, and filters always return
/// the same results, so a logged search replays identically.
static CORPUS: &[CorpusEntry] = &[
    CorpusEntry {
        title: "Attention Is All You Need",
        authors: "Vaswani et al.",
        year: 2017,
        venue: "arxiv",
        url: "https://arxiv.org/abs/1706.03762",
    },
    CorpusEntry {
        title: "Sparse Attention Memory Costs at Long Context",
        authors: "Beltagy et al.",
        year: 2020,
        venue: "arxiv",
        url: "https://arxiv.org/abs/2004.05150",
    },
    CorpusEntry {
        title: "Retrieval-Augmented Generation Reduces Hallucination",
        authors: "Lewis et al.",
        year: 2020,
        venue: "acl",
        url: "https://aclanthology.org/2020.acl-main.612",
    },
    CorpusEntry {
        title: "Grounding Citations in Retrieved Passages",
        authors: "Gao et al.",
        year: 2023,
        venue: "arxiv",
        url: "https://arxiv.org/abs/2305.14627",
    },
    CorpusEntry {
        title: "Lost in the Middle: Context Confuses Language Models",
        authors: "Liu et al.",
        year: 2023,
        venue: "arxiv",
        url: "https://arxiv.org/abs/2307.03172",
    },
    CorpusEntry {
        title: "Scaling Laws for Neural Language Models",
        authors: "Kaplan et al.",
        year: 2020,
        venue: "arxiv",
        url: "https://arxiv.org/abs/2001.08361",
    },
    CorpusEntry {
        title: "Verifying Claims with Non-LLM Fetchers",
        authors: "Chen et al.",
        year: 2024,
        venue: "arxiv",
        url: "https://arxiv.org/abs/2402.14871",
    },
    CorpusEntry {
        title: "kNN-Augmented Language Models",
        authors: "Khandelwal et al.",
        year: 2019,
        venue: "arxiv",
        url: "https://arxiv.org/abs/1911.00172",
    },
];

/// Tokenize a query for corpus matching: lowercase, alphanumeric, no
/// stopwords, no fragments.
fn tokens(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 4 && !STOPWORDS.contains(t))
        .map(str::to_string)
        .collect()
}

/// The simulated search execution: filter the corpus by the PRISMA
/// filters (`from_year`, `to_year`, `venue`), match by query-token
/// overlap, order (relevance by default, `date-desc` by year), and cap
/// the first page. Deterministic: same inputs, same results.
fn execute_search(params: &SearchParams) -> Vec<SearchResult> {
    let from_year = params
        .filters
        .get("from_year")
        .and_then(Value::as_u64)
        .map(|y| y as u32);
    let to_year = params
        .filters
        .get("to_year")
        .and_then(Value::as_u64)
        .map(|y| y as u32);
    let venue = params
        .filters
        .get("venue")
        .and_then(Value::as_str)
        .map(str::to_lowercase);
    let query_tokens = tokens(&params.query);

    let mut hits: Vec<(u32, SearchResult)> = CORPUS
        .iter()
        .filter(|e| from_year.map_or(true, |y| e.year >= y))
        .filter(|e| to_year.map_or(true, |y| e.year <= y))
        .filter(|e| venue.as_deref().map_or(true, |v| e.venue == v))
        .filter_map(|e| {
            let title = e.title.to_lowercase();
            let score = query_tokens
                .iter()
                .filter(|t| title.contains(t.as_str()))
                .count() as u32;
            // a result matches when at least one substantive token hits
            if score == 0 {
                return None;
            }
            Some((
                score,
                SearchResult {
                    title: e.title.into(),
                    authors: e.authors.into(),
                    year: e.year,
                    venue: e.venue.into(),
                    url: e.url.into(),
                },
            ))
        })
        .collect();

    if params.order.as_deref() == Some("date-desc") {
        hits.sort_by(|a, b| b.1.year.cmp(&a.1.year).then(a.1.title.cmp(&b.1.title)));
    } else {
        // relevance: token-overlap score, then year, then title (stable)
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.year.cmp(&a.1.year)).then(a.1.title.cmp(&b.1.title)));
    }
    let mut results: Vec<SearchResult> = hits.into_iter().map(|(_, r)| r).collect();
    if params.first_page {
        results.truncate(FIRST_PAGE_SIZE);
    }
    results
}

// ---------------------------------------------------------------------------
// The disclosure fold (FR-12.1): the PRISMA read model
// ---------------------------------------------------------------------------

/// One disclosure row: the PRISMA fields of one `search.run`, folded from
/// the log. Null results carry `null_result: true` and `result_count: 0`
/// — visibly marked, never hidden.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchDisclosureRow {
    pub seq: i64,
    /// When the search ran — the event envelope's `ts`.
    pub started_at: DateTime<Utc>,
    pub query: String,
    pub database: String,
    pub filters: BTreeMap<String, Value>,
    pub order: Option<String>,
    pub first_page: bool,
    pub result_count: u32,
    pub null_result: bool,
    pub mission_id: Option<Uuid>,
    pub run_id: Option<String>,
}

impl SearchDisclosureRow {
    fn of(event: &StoredEvent) -> Self {
        // The payload was validated at construction (the typed
        // constructor is the only way in, AD-15); a row folds it back.
        let payload: SearchRunPayload = serde_json::from_value(event.payload.clone())
            .expect("search.run payloads are constructor-validated");
        Self {
            seq: event.seq,
            started_at: event.ts,
            query: payload.query,
            database: payload.database,
            filters: payload.filters,
            order: payload.order,
            first_page: payload.first_page,
            result_count: payload.result_count,
            null_result: payload.null_result,
            mission_id: payload.mission_id,
            run_id: payload.run_id,
        }
    }
}

/// The disclosure of one scope (a mission, or the whole workspace): every
/// `search.run` in seq order. `null_result_count` is the seam Story 4.3's
/// readiness gate counts as undisclosed null results (FR-13.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchDisclosure {
    pub rows: Vec<SearchDisclosureRow>,
    pub total: u32,
    pub null_result_count: u32,
}

/// Fold the disclosure (a PURE query over the log, AD-2): one row per
/// `search.run` in the scope, null results included. The invariant
/// (FR-12.1, Story 4.1 AC): when searches occurred, the disclosure
/// CANNOT render as empty — every event folds to exactly one row, so a
/// review with searches (even all-null ones) always discloses them.
pub fn search_disclosure(
    events: &[StoredEvent],
    mission: Option<Uuid>,
) -> Result<SearchDisclosure, EventError> {
    let mut rows = Vec::new();
    for event in events.iter().filter(|e| e.kind == SEARCH_RUN) {
        if let Some(mission_id) = mission {
            let in_mission = event.payload.get("mission_id").and_then(Value::as_str)
                == Some(mission_id.to_string().as_str())
                || event.causes.contains(&mission_id);
            if !in_mission {
                continue;
            }
        }
        rows.push(SearchDisclosureRow::of(event));
    }
    rows.sort_by_key(|r| r.seq);
    let null_result_count = rows.iter().filter(|r| r.null_result).count() as u32;
    Ok(SearchDisclosure {
        total: rows.len() as u32,
        null_result_count,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventstore::EventStore;
    use rusqlite::Connection;
    use serde_json::json;

    /// A fresh store the test's searches append to (the spend/nightshift
    /// test pattern: a local connection the store borrows).
    fn with_store(f: impl FnOnce(&EventStore<'_>)) {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        let store = EventStore::new(&conn);
        f(&store);
    }

    fn payload(query: &str, count: u32) -> SearchRunPayload {
        SearchRunPayload {
            query: query.into(),
            database: DATABASE_ARXIV.into(),
            filters: BTreeMap::new(),
            order: None,
            first_page: true,
            result_count: count,
            null_result: false,
            mission_id: Some(Uuid::new_v4()),
            run_id: Some("nightshift-test-1".into()),
        }
    }

    #[test]
    fn constructor_attributes_the_search_to_the_run_and_causes_the_mission() {
        let mission_id = Uuid::new_v4();
        let mut p = payload("sparse attention at 32k", 2);
        p.mission_id = Some(mission_id);
        p.run_id = Some("nightshift-abc".into());
        let ev = NewEvent::search_run(p).unwrap();
        assert_eq!(ev.kind, "search.run");
        assert_eq!(ev.actor, Actor::Agent { run_id: "nightshift-abc".into() });
        assert_eq!(ev.causes, vec![mission_id]);
        // a user-run search (no run id) attributes to the user
        let mut p = payload("user query", 1);
        p.run_id = None;
        let ev = NewEvent::search_run(p).unwrap();
        assert_eq!(ev.actor, Actor::User);
    }

    #[test]
    fn constructor_derives_the_null_flag_from_the_count_never_the_caller() {
        // zero results => null, even when the caller claims otherwise
        let mut p = payload("zzzz qqqq", 0);
        p.null_result = false;
        let ev = NewEvent::search_run(p).unwrap();
        assert_eq!(ev.payload["result_count"], json!(0));
        assert_eq!(ev.payload["null_result"], json!(true));
        // non-zero => not null, even when the caller claims it
        let mut p = payload("sparse attention", 3);
        p.null_result = true;
        let ev = NewEvent::search_run(p).unwrap();
        assert_eq!(ev.payload["null_result"], json!(false));
    }

    #[test]
    fn constructor_rejects_unreproducible_searches() {
        for (query, database) in [("", "arxiv"), ("  ", "arxiv"), ("q", ""), ("q", "  ")] {
            let mut p = payload(query, 1);
            p.database = database.into();
            let err = NewEvent::search_run(p)
                .expect_err("a search without query+database must fail construction");
            assert!(err.to_string().contains("FR-12.1"), "unexpected: {err}");
        }
    }

    #[test]
    fn run_search_appends_the_event_and_returns_the_results() {
        with_store(|store| {
            let outcome = run_search(
                store,
                &SearchParams {
                    query: "Does sparse attention hold at 32k?".into(),
                    database: DATABASE_ARXIV.into(),
                    filters: BTreeMap::new(),
                    order: Some("relevance".into()),
                    first_page: true,
                    mission_id: Some(Uuid::new_v4()),
                    run_id: Some("nightshift-1".into()),
                },
            )
            .unwrap();
            assert!(!outcome.results.is_empty());
            assert_eq!(outcome.row.result_count as usize, outcome.results.len());
            assert!(!outcome.row.null_result);
            // every result actually mentions something the query asked
            assert!(outcome
                .results
                .iter()
                .any(|r| r.title.to_lowercase().contains("sparse")));
            // the filters shape the results AND ride on the row (PRISMA:
            // filters are part of the record)
            let mut params = SearchParams {
                query: "retrieval".into(),
                database: DATABASE_ARXIV.into(),
                filters: BTreeMap::from([("from_year".into(), json!(2020))]),
                order: None,
                first_page: true,
                mission_id: None,
                run_id: None,
            };
            let filtered = run_search(store, &params).unwrap();
            assert!(!filtered.results.is_empty());
            assert!(filtered.results.iter().all(|r| r.year >= 2020));
            assert_eq!(filtered.row.filters.get("from_year"), Some(&json!(2020)));
            // the venue filter narrows to the venue it names
            params.filters.insert("venue".into(), json!("acl"));
            let venue = run_search(store, &params).unwrap();
            assert!(venue.results.iter().all(|r| r.venue == "acl"));
            // ...and a venue nothing matched is an honest null result
            params.filters.insert("venue".into(), json!("neurips"));
            let null_venue = run_search(store, &params).unwrap();
            assert!(null_venue.results.is_empty());
            assert!(null_venue.row.null_result);
        });
    }

    #[test]
    fn a_null_search_is_logged_identically() {
        with_store(|store| {
            let before = store.head_seq().unwrap();
            let mission_id = Uuid::new_v4();
            let outcome = run_search(
                store,
                &SearchParams {
                    // matches nothing in the corpus — a null result
                    query: "qqqq zzzz wwww".into(),
                    database: DATABASE_ARXIV.into(),
                    filters: BTreeMap::new(),
                    order: None,
                    first_page: true,
                    mission_id: Some(mission_id),
                    run_id: Some("nightshift-null".into()),
                },
            )
            .unwrap();
            assert!(outcome.results.is_empty());
            assert_eq!(outcome.row.result_count, 0);
            assert!(outcome.row.null_result);
            // the event landed — identical shape, zero count
            assert_eq!(store.head_seq().unwrap(), before + 1);
            let stored = store.events_all().unwrap();
            let ev = stored.last().unwrap();
            assert_eq!(ev.kind, SEARCH_RUN);
            assert_eq!(ev.payload["result_count"], json!(0));
            assert_eq!(ev.payload["null_result"], json!(true));
            assert_eq!(ev.payload["query"], json!("qqqq zzzz wwww"));
        });
    }

    #[test]
    fn a_disclosure_cannot_render_empty_when_searches_occurred() {
        // The Story 4.1 AC invariant: a review's disclosure renders its
        // searches — ALL of them, including (and especially) null ones.
        with_store(|store| {
            let mission_id = Uuid::new_v4();
            // a run whose every search came back null
            run_search(
                store,
                &SearchParams {
                    query: "qqqq zzzz".into(),
                    database: DATABASE_ARXIV.into(),
                    filters: BTreeMap::new(),
                    order: None,
                    first_page: true,
                    mission_id: Some(mission_id),
                    run_id: Some("nightshift-all-null".into()),
                },
            )
            .unwrap();
            let events = store.events_all().unwrap();
            let disclosure = search_disclosure(&events, Some(mission_id)).unwrap();
            // searches occurred => rows exist; null rows are visible and counted
            assert!(
                !disclosure.rows.is_empty(),
                "searches occurred — the disclosure cannot render empty"
            );
            assert_eq!(disclosure.total, 1);
            assert_eq!(disclosure.null_result_count, 1);
            assert!(disclosure.rows[0].null_result);
            assert_eq!(disclosure.rows[0].result_count, 0);
            // the workspace-wide fold sees the same searches
            let all = search_disclosure(&events, None).unwrap();
            assert_eq!(all.total, disclosure.total);
        });
    }

    #[test]
    fn the_disclosure_scopes_to_one_mission() {
        with_store(|store| {
            let a = Uuid::new_v4();
            let b = Uuid::new_v4();
            for (mission_id, query) in [(a, "sparse attention"), (b, "qqqq zzzz")] {
                run_search(
                    store,
                    &SearchParams {
                        query: query.into(),
                        database: DATABASE_ARXIV.into(),
                        filters: BTreeMap::new(),
                        order: None,
                        first_page: true,
                        mission_id: Some(mission_id),
                        run_id: Some("nightshift-scope".into()),
                    },
                )
                .unwrap();
            }
            let events = store.events_all().unwrap();
            let for_a = search_disclosure(&events, Some(a)).unwrap();
            assert_eq!(for_a.total, 1);
            assert_eq!(for_a.rows[0].query, "sparse attention");
            let for_b = search_disclosure(&events, Some(b)).unwrap();
            // b's only search is a null one — still disclosed, never dropped
            assert_eq!(for_b.total, 1);
            assert_eq!(for_b.null_result_count, 1);
            let all = search_disclosure(&events, None).unwrap();
            assert_eq!(all.total, 2);
            assert_eq!(all.null_result_count, 1);
        });
    }

    #[test]
    fn rows_render_in_seq_order_with_the_envelope_timestamp() {
        with_store(|store| {
            let mission_id = Uuid::new_v4();
            for query in ["attention", "qqqq zzzz", "retrieval grounding"] {
                run_search(
                    store,
                    &SearchParams {
                        query: query.into(),
                        database: DATABASE_ARXIV.into(),
                        filters: BTreeMap::new(),
                        order: None,
                        first_page: true,
                        mission_id: Some(mission_id),
                        run_id: None,
                    },
                )
                .unwrap();
            }
            let events = store.events_all().unwrap();
            let disclosure = search_disclosure(&events, None).unwrap();
            let seqs: Vec<i64> = disclosure.rows.iter().map(|r| r.seq).collect();
            assert_eq!(seqs, vec![seqs[0], seqs[0] + 1, seqs[0] + 2]);
            // started_at is the envelope ts (the store assigned it)
            for (row, event) in disclosure
                .rows
                .iter()
                .zip(events.iter().filter(|e| e.kind == SEARCH_RUN))
            {
                assert_eq!(row.started_at, event.ts);
            }
        });
    }
}
