// Verifier shell commands (AD-15a, Story 4.2, FR-14.1): the runtime job
// that re-checks every pin against its source WITH NO LLM CALL. For each
// pinned claim the verifier resolves where the pin's content lives (the
// refs table's DOI/arXiv-id/URL for citations; the `artifact_ref` grammar
// — a local file path or a `jobs/<id>/stdout` ref — for numerical pins),
// re-fetches/re-reads it, and appends one `evidence.verified` event
// (actor=system/verifier, AD-15). Fetching is data work: it goes through
// the research adapters (the same arXiv fetch onboarding uses) and plain
// HTTP/fs — NEVER through the provider layer (AD-9 governs LLM calls
// only), and the NO-LLM invariant test below armed-proves it.
//
// Failures are visible, never deleting: a failed check marks the pin (the
// fold renders the error chip + reason) and the pin stays in place.
// Re-verification appends again — the latest event wins. Error strings
// lead with stable codes and stay in code form — bilingual-safe by
// construction (EXPERIENCE.md).

use std::collections::HashMap;

use rusqlite::Connection;
use tauri::State;
use uuid::Uuid;

use crate::db::Db;
use crate::domain::evidence::{Claim, EvidenceProjection, PinKind};
use crate::domain::verifier::{
    verify_citation, verify_numerical, FetchFuture, FetchResult, PinSource, PinSourceFetcher,
    VerificationOutcome, DETAIL_NO_SOURCE,
};
use crate::eventstore::{EventStore, NewEvent};

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// One pin scheduled for verification — everything the check needs,
/// resolved under one lock so the fetches run lock-free.
struct PinCheck {
    claim_id: Uuid,
    hypothesis_id: Uuid,
    /// The pin event's seq — the identity the result reports on. Re-checked
    /// before appending: a pin re-pinned mid-run is skipped (its result
    /// would target text that is no longer pinned).
    pin_seq: i64,
    kind: PinKind,
    excerpt: String,
    digest: String,
    /// Where the content can be re-read from; None = the ref names no
    /// fetchable source (an honest `no_source` failure, never a guess).
    source: Option<PinSource>,
    /// What was consulted, for the event payload: `arxiv:<id>`, a URL, or
    /// an artifact ref.
    label: String,
}

/// Resolve a citation pin's fetchable source from the refs table: the
/// arXiv id (from the ref's URL or its DOI) when there is one, else the
/// ref's own URL, else `https://doi.org/<doi>`. A ref with neither reads
/// `no_source` — visible, never guessed.
fn resolve_citation_source(
    conn: &Connection,
    ref_id: &str,
) -> Result<(Option<PinSource>, String), String> {
    let row = conn.query_row(
        "SELECT doi, url FROM refs WHERE id = ?1",
        [ref_id],
        |r| {
            let doi: Option<String> = r.get(0)?;
            let url: Option<String> = r.get(1)?;
            Ok((doi, url))
        },
    );
    let (doi, url) = match row {
        Ok(pair) => pair,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            // The ref row is gone from the library — the pin still cites it;
            // report against the ref id, honestly unsourced.
            return Ok((None, ref_id.to_string()));
        }
        Err(e) => return Err(err(e)),
    };
    let doi = doi.unwrap_or_default();
    let url = url.unwrap_or_default();
    // arXiv by URL: https://arxiv.org/abs/<id> (or /pdf/<id>).
    let arxiv_from_url = url
        .trim()
        .split_once("arxiv.org/")
        .and_then(|(_, rest)| {
            let id = rest
                .strip_prefix("abs/")
                .or_else(|| rest.strip_prefix("pdf/"))?;
            let id = id.trim().trim_end_matches(".pdf");
            (!id.is_empty()).then(|| id.to_string())
        });
    // arXiv by DOI: 10.48550/arXiv.<id>.
    let arxiv_from_doi = doi
        .trim()
        .strip_prefix("10.48550/arXiv.")
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    if let Some(id) = arxiv_from_url.or(arxiv_from_doi) {
        return Ok((Some(PinSource::Arxiv { id: id.clone() }), format!("arxiv:{id}")));
    }
    if !url.trim().is_empty() {
        let url = url.trim().to_string();
        return Ok((Some(PinSource::Url { url: url.clone() }), url));
    }
    if !doi.trim().is_empty() {
        let canonical = format!("https://doi.org/{}", doi.trim());
        return Ok((
            Some(PinSource::Url { url: canonical.clone() }),
            canonical,
        ));
    }
    Ok((None, ref_id.to_string()))
}

/// Resolve a numerical pin's artifact by the `artifact_ref` grammar:
/// `jobs/<id>/stdout` re-fetches the compute job's results through the
/// target adapter (the same seam `fetch_job` uses); anything else is a
/// local file path (absolute, or relative to the working directory).
fn resolve_artifact_source(artifact_ref: &str) -> (Option<PinSource>, String) {
    if let Some(id) = artifact_ref
        .trim()
        .strip_prefix("jobs/")
        .and_then(|rest| rest.strip_suffix("/stdout"))
    {
        if let Ok(job_id) = id.parse::<Uuid>() {
            // Canonical label: the job id in simple form, like the fetch
            // path that created these artifacts writes them.
            return (
                Some(PinSource::JobStdout { job_id }),
                format!("jobs/{}/stdout", job_id.simple()),
            );
        }
    }
    (
        Some(PinSource::File { path: artifact_ref.trim().to_string() }),
        artifact_ref.trim().to_string(),
    )
}

/// The real fetcher: the research adapters + plain HTTP/fs, the same data
/// paths onboarding and the jobs shell already use — deliberately NOT the
/// provider layer. There is no path from here to an LLM call.
struct CoreFetcher {
    db: Db,
}

impl PinSourceFetcher for CoreFetcher {
    fn fetch<'a>(&'a self, source: &'a PinSource) -> FetchFuture<'a> {
        Box::pin(async move {
            match source {
                // The same public-export arXiv fetch the onboarding paste
                // flow uses (Story 1.9) — title + abstract are the fetched
                // content a citation excerpt is checked against.
                PinSource::Arxiv { id } => match crate::mcp::arxiv_fetch(id).await {
                    Ok(Some(paper)) => Ok(match paper.abstract_text {
                        Some(abstract_text) if !abstract_text.trim().is_empty() => {
                            format!("{} {}", paper.title, abstract_text)
                        }
                        _ => paper.title,
                    }),
                    Ok(None) => Err(format!("not_found: no arXiv paper with id `{id}`")),
                    Err(e) => Err(e),
                },
                // A plain URL (the ref's URL or a canonical DOI link;
                // redirects followed). The body text is the fetched
                // content — publisher HTML included, so an excerpt that
                // only matches after HTML unwrapping reads not_found, an
                // honest failure rather than a guess.
                PinSource::Url { url } => {
                    let client = reqwest::Client::builder()
                        .user_agent("research-core/0.1")
                        .timeout(std::time::Duration::from_secs(20))
                        .build()
                        .map_err(|e| e.to_string())?;
                    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
                    let status = resp.status();
                    let text = resp.text().await.map_err(|e| e.to_string())?;
                    if !status.is_success() {
                        return Err(format!("http {status}"));
                    }
                    Ok(text)
                }
                // A local artifact file.
                PinSource::File { path } => {
                    std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))
                }
                // A compute job's stdout — re-fetched through the same
                // `fetch_job` seam the results flow uses (Story 3.4).
                PinSource::JobStdout { job_id } => {
                    let results = crate::jobs_commands::fetch_job_inner(&self.db, *job_id)
                        .await
                        .map_err(err)?;
                    Ok(results.stdout)
                }
            }
        })
    }
}

/// Run pin verification over a scope (Story 4.2, FR-14.1): re-check every
/// pinned claim's pin against its source and append one
/// `evidence.verified` event per check — actor=system/verifier, NO LLM
/// call anywhere on the path. `hypothesis_id` scopes the run (None =
/// workspace-wide). Returns the re-folded claims of the scope with the
/// latest verification status on every pin.
#[tauri::command]
pub async fn run_pin_verification(
    db: State<'_, Db>,
    hypothesis_id: Option<String>,
) -> Result<Vec<Claim>, String> {
    let scope = match hypothesis_id.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => Some(
            raw.parse()
                .map_err(|e| format!("invalid hypothesis id `{raw}`: {e}"))?,
        ),
    };
    let fetcher = CoreFetcher { db: db.inner().clone() };
    run_pin_verification_inner(db.inner(), scope, &fetcher).await
}

/// Plain inner (testable without Tauri state; the fetcher is injected so
/// tests seed a fetchable corpus). Three phases, the repo's lock
/// discipline: resolve under one lock, fetch lock-free, append under one
/// lock with the pin identity re-checked (a pin re-pinned mid-run is
/// skipped — its result would target text no longer pinned).
pub(crate) async fn run_pin_verification_inner(
    db: &Db,
    scope: Option<Uuid>,
    fetcher: &dyn PinSourceFetcher,
) -> Result<Vec<Claim>, String> {
    // Phase 1: fold + resolve every pin's source under one lock.
    let checks: Vec<PinCheck> = {
        let conn = db.0.lock().await;
        let events = EventStore::new(&conn).events_all().map_err(err)?;
        let claims = EvidenceProjection::fold(&events).map_err(err)?;
        let mut checks = Vec::new();
        for claim in claims.iter().filter(|c| scope.is_none_or(|s| c.hypothesis_id == s)) {
            let Some(pin) = claim.pin.as_ref() else {
                continue; // unpinned — nothing to verify (FR-3.4 already flags it)
            };
            let (source, label) = match pin.kind {
                PinKind::Citation => {
                    resolve_citation_source(&conn, pin.ref_id.as_deref().unwrap_or(""))?
                }
                PinKind::Numerical => {
                    resolve_artifact_source(pin.artifact_ref.as_deref().unwrap_or(""))
                }
            };
            checks.push(PinCheck {
                claim_id: claim.id,
                hypothesis_id: claim.hypothesis_id,
                pin_seq: pin.seq,
                kind: pin.kind,
                excerpt: pin.excerpt.clone(),
                digest: pin.digest.clone(),
                source,
                label,
            });
        }
        checks
    };

    // Phase 2: fetch every source, lock-free (network + fs + adapters).
    let mut fetched: Vec<(&PinCheck, FetchResult)> = Vec::with_capacity(checks.len());
    for check in &checks {
        let result = match &check.source {
            Some(source) => fetcher.fetch(source).await,
            // The ref names no fetchable source — an honest no_source
            // failure, appended like every other result.
            None => Err("no_source".into()),
        };
        fetched.push((check, result));
    }

    // Phase 3: append one event per check, the pin identity re-checked.
    {
        let conn = db.0.lock().await;
        let store = EventStore::new(&conn);
        let events = store.events_all().map_err(err)?;
        let current: HashMap<Uuid, i64> = EvidenceProjection::fold(&events)
            .map_err(err)?
            .into_iter()
            .filter_map(|c| c.pin.map(|p| (c.id, p.seq)))
            .collect();
        for (check, result) in fetched {
            if current.get(&check.claim_id) != Some(&check.pin_seq) {
                continue; // re-pinned mid-run — skip, never a stale result
            }
            let (outcome, detail) = match (&check.source, check.kind) {
                // The ref names no fetchable source — its own honest code,
                // not a generic fetch error.
                (None, _) => (VerificationOutcome::Failed, DETAIL_NO_SOURCE),
                (Some(_), PinKind::Citation) => verify_citation(&check.excerpt, &result),
                (Some(_), PinKind::Numerical) => {
                    verify_numerical(&check.excerpt, &check.digest, &result)
                }
            };
            store
                .append(
                    NewEvent::evidence_verified(
                        check.claim_id,
                        check.hypothesis_id,
                        check.pin_seq,
                        outcome,
                        detail,
                        &check.label,
                    )
                    .map_err(err)?,
                )
                .map_err(err)?;
        }
    }

    // The re-folded read: the claims of the scope, latest status on every
    // pin — exactly what the UI renders after the run.
    let conn = db.0.lock().await;
    let events = EventStore::new(&conn).events_all().map_err(err)?;
    let claims = EvidenceProjection::fold(&events).map_err(err)?;
    Ok(claims
        .into_iter()
        .filter(|c| scope.is_none_or(|s| c.hypothesis_id == s))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::evidence::{excerpt_digest, VerificationStatus};
    use crate::domain::missions::{Autonomy, MissionCreatedPayload};
    use crate::domain::verifier::{
        DETAIL_ARTIFACT_CHANGED, DETAIL_ARTIFACT_MISSING, DETAIL_DIGEST_OK, DETAIL_EXCERPT_MATCHED,
        DETAIL_FETCH_ERROR, DETAIL_NOT_FOUND, DETAIL_NO_SOURCE, EVIDENCE_VERIFIED,
    };
    use crate::eventstore::NewEvent as RawNewEvent;
    use crate::eventstore::StoredEvent;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    /// The `evidence.verified` events of a log, in seq order — the run's
    /// own receipt (tests assert on it; the fold remains the UI's read
    /// path).
    fn verified_events(events: &[StoredEvent]) -> Vec<StoredEvent> {
        events
            .iter()
            .filter(|e| e.kind == EVIDENCE_VERIFIED)
            .cloned()
            .collect()
    }

    /// A seeded fetchable corpus: deterministic content per source. The
    /// NO-LLM test wraps it to record every fetch.
    struct SeededFetcher {
        arxiv: HashMap<String, String>,
        urls: HashMap<String, String>,
        files: HashMap<String, String>,
        jobs: HashMap<Uuid, String>,
        fetches: AtomicUsize,
    }

    impl SeededFetcher {
        fn new() -> Self {
            Self {
                arxiv: HashMap::new(),
                urls: HashMap::new(),
                files: HashMap::new(),
                jobs: HashMap::new(),
                fetches: AtomicUsize::new(0),
            }
        }

        fn fetch_count(&self) -> usize {
            self.fetches.load(Ordering::SeqCst)
        }
    }

    impl PinSourceFetcher for SeededFetcher {
        fn fetch<'a>(&'a self, source: &'a PinSource) -> FetchFuture<'a> {
            self.fetches.fetch_add(1, Ordering::SeqCst);
            let result = match source {
                PinSource::Arxiv { id } => match self.arxiv.get(id) {
                    Some(text) => Ok(text.clone()),
                    None => Err(format!("not_found: no arXiv paper with id `{id}`")),
                },
                PinSource::Url { url } => match self.urls.get(url) {
                    Some(text) => Ok(text.clone()),
                    None => Err(format!("unreachable: {url}")),
                },
                PinSource::File { path } => match self.files.get(path) {
                    Some(text) => Ok(text.clone()),
                    None => Err(format!("read {path}: no such file")),
                },
                PinSource::JobStdout { job_id } => match self.jobs.get(job_id) {
                    Some(text) => Ok(text.clone()),
                    None => Err(format!("not_found: no job with id `{job_id}`")),
                },
            };
            Box::pin(async move { result })
        }
    }

    fn test_db() -> (Db, std::path::PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("rc-verifier-cmd-test-{}.sqlite", uuid::Uuid::new_v4()));
        let db = Db::open(&path).unwrap();
        (db, path)
    }

    fn cleanup(path: &std::path::PathBuf) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    }

    /// Seed one mission + hypothesis; returns the hypothesis id (async —
    /// these tests run under tokio).
    async fn seed_hypothesis(db: &Db) -> Uuid {
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        let mission = store
            .append(RawNewEvent::mission_created(MissionCreatedPayload {
                question: "Does X hold up?".into(),
                stop_condition: "Stop after $5.".into(),
                success_criterion: "A blind rater agrees.".into(),
                autonomy: Autonomy::Suggest,
                spend_ceiling_cents: 500,
                schedule: "daily-03:00".into(),
                roles: vec![],
            })
            .unwrap())
            .unwrap();
        store
            .append(RawNewEvent::hypothesis_created("X holds.", mission.id).unwrap())
            .unwrap()
            .id
    }

    /// Insert one library ref with a URL (idempotent on the id — tests
    /// reuse one ref id across hypotheses). The project is the seeded demo
    /// project, so the refs-table foreign key holds.
    fn insert_ref(c: &Connection, id: &str, url: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        c.execute(
            "INSERT OR REPLACE INTO refs(id,project_id,title,authors,year,venue,doi,url,status,used,citation_count,created_at)
             VALUES(?1,'proj-tesis-cap2','A paper','Vaswani et al.',2017,'arXiv','',?2,'unread',0,0,?3)",
            rusqlite::params![id, url, now],
        )
        .unwrap();
    }

    /// Register a claim + pin it to a citation whose ref fetches `url`.
    async fn pinned_citation(db: &Db, h: Uuid, url: &str, excerpt: &str) -> Uuid {
        let c = db.0.lock().await;
        insert_ref(&c, "ref-verif", url);
        let store = EventStore::new(&c);
        let claim = store
            .append(RawNewEvent::claim_registered("A cited claim.", h, None).unwrap())
            .unwrap();
        store
            .append(
                RawNewEvent::evidence_pinned_citation(
                    claim.id,
                    h,
                    "ref-verif",
                    excerpt,
                    0.82,
                    "GLM-5.3",
                )
                .unwrap(),
            )
            .unwrap();
        claim.id
    }

    async fn verification_of(db: &Db, claim_id: Uuid) -> Option<(VerificationStatus, String, String)> {
        let c = db.0.lock().await;
        let events = EventStore::new(&c).events_all().unwrap();
        EvidenceProjection::fold(&events)
            .unwrap()
            .into_iter()
            .find(|cl| cl.id == claim_id)
            .and_then(|cl| {
                cl.pin.and_then(|p| {
                    p.verification
                        .map(|v| (v.status, v.detail, v.source))
                })
            })
    }

    /// A citation pin verifies when the fetched source contains the
    /// excerpt (seeded corpus) — one `evidence.verified` event, actor
    /// system/verifier, verified/excerpt_matched.
    #[tokio::test]
    async fn a_citation_pin_verifies_when_the_fetched_source_contains_the_excerpt() {
        let (db, path) = test_db();
        let h = seed_hypothesis(&db).await;
        let excerpt = "Attention dispenses with recurrence entirely.";
        let claim = pinned_citation(&db, h, "https://example.org/attention", excerpt).await;
        let mut fetcher = SeededFetcher::new();
        fetcher.urls.insert(
            "https://example.org/attention".into(),
            format!("Title.\nAbstract: {excerpt}\nEnd."),
        );

        let claims = run_pin_verification_inner(&db, Some(h), &fetcher).await.unwrap();
        assert_eq!(claims.len(), 1);
        assert!(claims[0].pinned, "the pin stays");
        let v = claims[0].pin.as_ref().unwrap().verification.as_ref().unwrap();
        assert_eq!(v.status, VerificationStatus::Verified);
        assert_eq!(v.detail, DETAIL_EXCERPT_MATCHED);
        assert_eq!(v.source, "https://example.org/attention");

        // The event itself: actor system/verifier, cause-linked.
        let c = db.0.lock().await;
        let events = EventStore::new(&c).events_all().unwrap();
        drop(c);
        let [event] = verified_events(&events).try_into().ok().expect("one event");
        assert_eq!(
            event.actor,
            crate::eventstore::Actor::System {
                component: crate::eventstore::SystemComponent::Verifier
            }
        );
        assert!(event.causes.contains(&claim));
        assert_eq!(event.payload["outcome"], serde_json::json!("verified"));
        assert_eq!(event.payload["detail"], serde_json::json!(DETAIL_EXCERPT_MATCHED));
        cleanup(&path);
    }

    /// An arXiv ref (by URL or DOI) fetches through the arXiv adapter
    /// seam: label `arxiv:<id>`, content = title + abstract.
    #[tokio::test]
    async fn an_arxiv_ref_verifies_through_the_arxiv_seam() {
        let (db, path) = test_db();
        let h = seed_hypothesis(&db).await;
        let excerpt = "The stacked hourglassconvolutional network.";
        {
            let c = db.0.lock().await;
            insert_ref(&c, "ref-arxiv", "https://arxiv.org/abs/1706.03762");
            let store = EventStore::new(&c);
            let claim = store
                .append(RawNewEvent::claim_registered("A cited claim.", h, None).unwrap())
                .unwrap();
            store
                .append(
                    RawNewEvent::evidence_pinned_citation(
                        claim.id,
                        h,
                        "ref-arxiv",
                        excerpt,
                        0.8,
                        "GLM-5.3",
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let mut fetcher = SeededFetcher::new();
        fetcher.arxiv.insert(
            "1706.03762".into(),
            format!("Attention Is All You Need. {excerpt}"),
        );
        let claims = run_pin_verification_inner(&db, None, &fetcher).await.unwrap();
        let v = claims[0].pin.as_ref().unwrap().verification.as_ref().unwrap();
        assert_eq!(v.status, VerificationStatus::Verified);
        assert_eq!(v.source, "arxiv:1706.03762");
        cleanup(&path);
    }

    /// A numerical pin verifies when the artifact re-reads to the pinned
    /// content (digest recomputes), and FAILS — visibly, undeleted — when
    /// the artifact changed or is missing.
    #[tokio::test]
    async fn a_numerical_pin_verifies_by_digest_and_fails_when_the_artifact_changed() {
        let (db, path) = test_db();
        let h = seed_hypothesis(&db).await;
        let content = "accuracy: 0.912, ±0.006, n=5 seeds";
        let claim = {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let claim = store
                .append(RawNewEvent::claim_registered("Run 7 hit 91.2%.", h, None).unwrap())
                .unwrap();
            store
                .append(
                    RawNewEvent::evidence_pinned_numerical(
                        claim.id,
                        h,
                        "runs/007/table-3.csv",
                        content,
                        0.91,
                        "GLM-5.3",
                    )
                    .unwrap(),
                )
                .unwrap();
            claim.id
        };
        // Digest recomputes over the re-read artifact → verified.
        let mut fetcher = SeededFetcher::new();
        fetcher.files.insert("runs/007/table-3.csv".into(), content.into());
        let claims = run_pin_verification_inner(&db, Some(h), &fetcher).await.unwrap();
        let v = claims[0].pin.as_ref().unwrap().verification.as_ref().unwrap();
        assert_eq!(v.status, VerificationStatus::Verified);
        assert_eq!(v.detail, DETAIL_DIGEST_OK);

        // The artifact changed (the pinned values are gone) → failed,
        // artifact_changed — and the pin is NOT deleted.
        let mut changed = SeededFetcher::new();
        changed
            .files
            .insert("runs/007/table-3.csv".into(), "accuracy: 0.901, n=3 seeds".into());
        let claims = run_pin_verification_inner(&db, Some(h), &changed).await.unwrap();
        assert!(claims[0].pinned, "a failed verification never deletes the pin");
        let v = claims[0].pin.as_ref().unwrap().verification.as_ref().unwrap();
        assert_eq!(v.status, VerificationStatus::Failed);
        assert_eq!(v.detail, DETAIL_ARTIFACT_CHANGED);

        // The artifact is missing → failed, artifact_missing; still pinned.
        let missing = SeededFetcher::new();
        let claims = run_pin_verification_inner(&db, Some(h), &missing).await.unwrap();
        assert!(claims[0].pinned);
        let v = claims[0].pin.as_ref().unwrap().verification.as_ref().unwrap();
        assert_eq!(v.status, VerificationStatus::Failed);
        assert_eq!(v.detail, DETAIL_ARTIFACT_MISSING);
        assert_eq!(v.source, "runs/007/table-3.csv");
        let _ = claim;
        cleanup(&path);
    }

    /// A `jobs/<id>/stdout` artifact re-reads through the job seam: the
    /// verifier re-fetches the job's results and the digest recomputes.
    #[tokio::test]
    async fn a_job_stdout_artifact_verifies_through_the_job_seam() {
        let (db, path) = test_db();
        let h = seed_hypothesis(&db).await;
        let stdout = "accuracy: 0.912, n=5";
        let job_id = Uuid::new_v4();
        {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let claim = store
                .append(RawNewEvent::claim_registered("Run hit 91.2%.", h, None).unwrap())
                .unwrap();
            store
                .append(
                    RawNewEvent::evidence_pinned_numerical(
                        claim.id,
                        h,
                        &format!("jobs/{}/stdout", job_id.simple()),
                        stdout,
                        0.9,
                        "GLM-5.3",
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let mut fetcher = SeededFetcher::new();
        fetcher.jobs.insert(job_id, stdout.into());
        let claims = run_pin_verification_inner(&db, None, &fetcher).await.unwrap();
        let v = claims[0].pin.as_ref().unwrap().verification.as_ref().unwrap();
        assert_eq!(v.status, VerificationStatus::Verified);
        assert_eq!(v.detail, DETAIL_DIGEST_OK);
        assert_eq!(v.source, format!("jobs/{}/stdout", job_id.simple()));
        cleanup(&path);
    }

    /// Failure marks visibly, never deletes: an excerpt absent from the
    /// fetched source reads failed/not_found; a ref with no fetchable
    /// source reads failed/no_source; a fetch error reads
    /// failed/fetch_error — the pin stays in every case.
    #[tokio::test]
    async fn failures_are_visible_and_never_delete_the_pin() {
        let (db, path) = test_db();
        let h = seed_hypothesis(&db).await;
        // Present ref, absent excerpt.
        let absent = pinned_citation(&db, h, "https://example.org/paper", "A passage it never contained.").await;
        // A ref with neither doi nor url.
        let unsourced = {
            let c = db.0.lock().await;
            insert_ref(&c, "ref-bare", "");
            let store = EventStore::new(&c);
            let claim = store
                .append(RawNewEvent::claim_registered("A bare claim.", h, None).unwrap())
                .unwrap();
            store
                .append(
                    RawNewEvent::evidence_pinned_citation(
                        claim.id,
                        h,
                        "ref-bare",
                        "Any excerpt.",
                        0.5,
                        "GLM-5.3",
                    )
                    .unwrap(),
                )
                .unwrap();
            claim.id
        };
        let mut fetcher = SeededFetcher::new();
        fetcher
            .urls
            .insert("https://example.org/paper".into(), "Entirely different text.".into());
        let claims = run_pin_verification_inner(&db, Some(h), &fetcher).await.unwrap();
        assert_eq!(claims.len(), 2);
        for claim in &claims {
            assert!(claim.pinned, "a failed verification never deletes the pin");
        }
        let absent_read = claims.iter().find(|c| c.id == absent).unwrap();
        let v = absent_read.pin.as_ref().unwrap().verification.as_ref().unwrap();
        assert_eq!((v.status, v.detail.as_str()), (VerificationStatus::Failed, DETAIL_NOT_FOUND));
        let unsourced_read = claims.iter().find(|c| c.id == unsourced).unwrap();
        let v = unsourced_read.pin.as_ref().unwrap().verification.as_ref().unwrap();
        assert_eq!((v.status, v.detail.as_str()), (VerificationStatus::Failed, DETAIL_NO_SOURCE));

        // A fetch error (the corpus entry vanishes) → failed/fetch_error.
        let unreachable = SeededFetcher::new();
        let claims = run_pin_verification_inner(&db, Some(h), &unreachable).await.unwrap();
        let v = claims
            .iter()
            .find(|c| c.id == absent)
            .unwrap()
            .pin
            .as_ref()
            .unwrap()
            .verification
            .as_ref()
            .unwrap();
        assert_eq!((v.status, v.detail.as_str()), (VerificationStatus::Failed, DETAIL_FETCH_ERROR));
        cleanup(&path);
    }

    /// Re-verification flips failed→verified: the latest event wins, and a
    /// pin with no event yet renders unverified until the first run.
    #[tokio::test]
    async fn reverify_flips_failed_to_verified_and_unverified_until_first_run() {
        let (db, path) = test_db();
        let h = seed_hypothesis(&db).await;
        let excerpt = "Attention dispenses with recurrence entirely.";
        let claim = pinned_citation(&db, h, "https://example.org/attention", excerpt).await;

        // Before any run: unverified (None) — distinct from confidence.
        let v = verification_of(&db, claim).await;
        assert_eq!(v, None);

        // First run: the source is unreachable → failed/fetch_error.
        let down = SeededFetcher::new();
        run_pin_verification_inner(&db, Some(h), &down).await.unwrap();
        let (status, detail, _) = verification_of(&db, claim).await.unwrap();
        assert_eq!((status, detail.as_str()), (VerificationStatus::Failed, DETAIL_FETCH_ERROR));

        // Second run: the source is back and contains the excerpt → the
        // latest event wins: verified.
        let mut up = SeededFetcher::new();
        up.urls.insert(
            "https://example.org/attention".into(),
            format!("Title. {excerpt}"),
        );
        run_pin_verification_inner(&db, Some(h), &up).await.unwrap();
        let (status, detail, _) = verification_of(&db, claim).await.unwrap();
        assert_eq!((status, detail.as_str()), (VerificationStatus::Verified, DETAIL_EXCERPT_MATCHED));

        // Two events in the log — nothing was rewritten (append-only).
        let c = db.0.lock().await;
        let events = EventStore::new(&c).events_all().unwrap();
        drop(c);
        assert_eq!(verified_events(&events).len(), 2);
        cleanup(&path);
    }

    /// The scope filter: a workspace-wide run (None) covers every
    /// hypothesis; a scoped run touches only its own claims.
    #[tokio::test]
    async fn the_scope_governs_which_pins_verify() {
        let (db, path) = test_db();
        let h1 = seed_hypothesis(&db).await;
        let h2 = {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let mission = store
                .append(RawNewEvent::mission_created(MissionCreatedPayload {
                    question: "Second mission?".into(),
                    stop_condition: "Stop.".into(),
                    success_criterion: "Done.".into(),
                    autonomy: Autonomy::Suggest,
                    spend_ceiling_cents: 500,
                    schedule: "daily-03:00".into(),
                    roles: vec![],
                })
                .unwrap())
                .unwrap();
            store
                .append(RawNewEvent::hypothesis_created("Y holds.", mission.id).unwrap())
                .unwrap()
                .id
        };
        let excerpt = "Attention dispenses with recurrence entirely.";
        pinned_citation(&db, h1, "https://example.org/attention", excerpt).await;
        pinned_citation(&db, h2, "https://example.org/attention", excerpt).await;
        let mut fetcher = SeededFetcher::new();
        fetcher.urls.insert(
            "https://example.org/attention".into(),
            format!("Title. {excerpt}"),
        );
        // Scoped to h1: only h1's claim comes back (and only h1's pin got
        // an event).
        let claims = run_pin_verification_inner(&db, Some(h1), &fetcher).await.unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].hypothesis_id, h1);
        assert!(claims[0].pin.as_ref().unwrap().verification.is_some());
        let c = db.0.lock().await;
        let events = EventStore::new(&c).events_all().unwrap();
        drop(c);
        assert_eq!(verified_events(&events).len(), 1);
        // Workspace-wide: the second run covers both (h1 re-verifies, h2
        // verifies for the first time).
        let claims = run_pin_verification_inner(&db, None, &fetcher).await.unwrap();
        assert_eq!(claims.len(), 2);
        assert!(claims.iter().all(|c| c
            .pin
            .as_ref()
            .and_then(|p| p.verification.as_ref().map(|v| v.status == VerificationStatus::Verified))
            .unwrap_or(false)));
        cleanup(&path);
    }

    /// The NO-LLM invariant (Story 4.2's one rule): the verify path never
    /// touches the provider layer. The workspace is ARMED with a real
    /// remote provider whose endpoint is a local tripwire listener — any
    /// LLM call would connect and be recorded — and every real call
    /// appends `spend.recorded` (AD-10). The run completes, verifies a
    /// pin, the tripwire never fires, and no spend event lands: existence
    /// by code, not by a model's word.
    #[tokio::test]
    async fn the_verifier_never_touches_the_provider_layer() {
        use crate::db::set_setting;

        let (db, path) = test_db();
        let h = seed_hypothesis(&db).await;
        let excerpt = "Attention dispenses with recurrence entirely.";
        let claim = pinned_citation(&db, h, "https://example.org/attention", excerpt).await;

        // Arm the tripwire: a local listener standing in for the provider
        // endpoint. Any HTTP an LLM call would make lands here and flips
        // the flag. The acceptor waits for exactly one connection — the
        // test releases it with its own connection AFTER the assertions.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let fired = Arc::new(AtomicBool::new(false));
        let fired_thread = fired.clone();
        let acceptor = std::thread::spawn(move || {
            if let Ok((_stream, _)) = listener.accept() {
                fired_thread.store(true, Ordering::SeqCst);
            }
        });
        {
            let c = db.0.lock().await;
            // llm_mode=provider + a base_url pointing at the tripwire +
            // a legacy key row: `ProviderLayer::resolve` on this workspace
            // yields a REAL remote provider — armed and never called.
            set_setting(&c, "llm_mode", "provider").unwrap();
            set_setting(&c, "provider", "custom").unwrap();
            set_setting(&c, "base_url", &format!("http://{addr}")).unwrap();
            set_setting(&c, "api_key", "tripwire-key").unwrap();
            set_setting(&c, "model", "tripwire-model").unwrap();
        }

        let mut fetcher = SeededFetcher::new();
        fetcher.urls.insert(
            "https://example.org/attention".into(),
            format!("Title. {excerpt}"),
        );
        let claims = run_pin_verification_inner(&db, Some(h), &fetcher).await.unwrap();
        let v = claims[0].pin.as_ref().unwrap().verification.as_ref().unwrap();
        assert_eq!(v.status, VerificationStatus::Verified, "verified by code");
        assert_eq!(fetcher.fetch_count(), 1, "exactly one data fetch");

        // The invariant: the armed provider never fired, and no real call
        // recorded spend — an LLM call would have done one or the other.
        assert!(
            !fired.load(Ordering::SeqCst),
            "the verifier made an LLM call to the armed provider endpoint — forbidden (Story 4.2)"
        );
        let c = db.0.lock().await;
        let events = EventStore::new(&c).events_all().unwrap();
        drop(c);
        assert!(
            !events.iter().any(|e| e.kind == "spend.recorded"),
            "a real provider call would have appended spend.recorded (AD-10) — none did"
        );
        // Release the acceptor with the test's own connection — the flag
        // flipping NOW is this release, not the verifier.
        std::net::TcpStream::connect(addr).unwrap();
        acceptor.join().unwrap();
        assert!(fired.load(Ordering::SeqCst), "the release connection was seen");
        let _ = claim;
        cleanup(&path);
    }

    /// Confidence vs verification separation, end to end (FR-3.6 + FR-14.1):
    /// after a verified run the pin still carries its agent-assessed
    /// confidence attributed to the assessing model — two separate axes,
    /// "verified" never drives the confidence label.
    #[tokio::test]
    async fn confidence_and_verification_stay_separate_axes_end_to_end() {
        let (db, path) = test_db();
        let h = seed_hypothesis(&db).await;
        let excerpt = "Attention dispenses with recurrence entirely.";
        let claim = pinned_citation(&db, h, "https://example.org/attention", excerpt).await;
        let mut fetcher = SeededFetcher::new();
        fetcher.urls.insert(
            "https://example.org/attention".into(),
            format!("Title. {excerpt}"),
        );
        let claims = run_pin_verification_inner(&db, Some(h), &fetcher).await.unwrap();
        let pin = claims[0].pin.as_ref().unwrap();
        // The machine axis.
        assert_eq!(pin.verification.as_ref().unwrap().status, VerificationStatus::Verified);
        // The agent axis — untouched by the run.
        assert_eq!(pin.confidence, 0.82);
        assert_eq!(pin.assessing_model, "GLM-5.3");
        assert_eq!(pin.digest, excerpt_digest(excerpt));
        let _ = claim;
        cleanup(&path);
    }
}
