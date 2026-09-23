// Journal Fit Finder shell (Story 6.12, FR-19.3): an LLM task through the
// provider layer (AD-9) that ranks candidate venues from the BUNDLED local
// dataset for the manuscript/board context — every rationale claim pinned
// to evidence where it cites a real board object, unpinned references
// flagged (FR-3.4), the ranking advisory and EVENTED with model + provider
// attribution (FR-19.3, AD-10), and the choice delivered as a REVIEWABLE
// PROPOSAL — approving it creates the submission mission (Story 6.13,
// FR-19.4); rejecting it leaves everything unchanged. No autonomous
// submission action exists anywhere (PRD §10). Simulated-provider graceful
// when unconfigured (the support engine's seam). Errors lead with stable
// codes; typed errors for unconfigured/empty contexts.

use rusqlite::Connection;
use tauri::State;
use uuid::Uuid;

use crate::adapters::providers::{ChatRequest, Message, ProviderLayer, ProviderSettings};
use crate::db::Db;
use crate::domain::evidence::EvidenceProjection;
use crate::domain::hypotheses::{HypothesesProjection, HypothesisStatus};
use crate::domain::journals::{self, FitCandidate};
use crate::domain::manuscript::ManuscriptsProjection;
use crate::domain::missions::RoleConfig;
use crate::domain::nightshift::{RUN_FINISHED, RUN_STARTED};
use crate::domain::submissions::SubmissionCreatedPayload;
use crate::eventstore::{EventStore, NewEvent, StoredEvent};
use crate::trust::{reserve_and_chat, CallPlan};

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// The fit role's provider resolution (Story 6.10's precedence): the
/// configured LOCAL CLI first when complete, then the configured BYOK
/// provider, then the simulated layer — the app works keyless and the
/// ranking never dead-spawns (NFR-14).
fn fit_role(settings: &ProviderSettings) -> RoleConfig {
    if settings.mode.trim() == "cli" {
        let model = settings.cli_model.trim().to_string();
        if !model.is_empty() {
            return RoleConfig {
                name: "journal-fit".into(),
                provider: "cli".into(),
                model,
            };
        }
    } else if !settings.is_simulated() {
        let (provider, model) = (settings.name.trim().to_string(), settings.model.trim().to_string());
        if !provider.is_empty() && !model.is_empty() {
            return RoleConfig {
                name: "journal-fit".into(),
                provider,
                model,
            };
        }
    }
    RoleConfig {
        name: "journal-fit".into(),
        provider: "simulated".into(),
        model: "simulated".into(),
    }
}

/// The board context the fit derives from — the evidence the ranking's
/// rationale will cite (the Evidence Ledger discipline: a rationale claim
/// citing H-{n}/CLAIMS-{n} is pinned when the object exists).
#[derive(Debug)]
pub(crate) struct FitContext {
    pub mission_id: Option<Uuid>,
    pub description: String,
    /// The resolved H-{n}/CLAIMS-{n} labels of the scope — rationale
    /// references validated against them.
    pub board_labels: Vec<String>,
    pub manuscript_note: Option<String>,
}

/// Build the fit's context from the board + manuscript read models. An
/// EMPTY context (no mission question, no hypotheses, no manuscript) is
/// the typed `empty_context:` error — a ranking needs something to rank
/// against.
pub(crate) fn build_fit_context(
    events: &[StoredEvent],
    mission: Option<Uuid>,
    manuscript_note: Option<String>,
) -> Result<FitContext, String> {
    let hyps = HypothesesProjection::fold(events).map_err(err)?;
    let claims = EvidenceProjection::fold(events).map_err(err)?;
    let hyp_mission: std::collections::HashMap<Uuid, Uuid> =
        hyps.iter().map(|h| (h.id, h.mission_id)).collect();
    let in_scope_h: Vec<&crate::domain::hypotheses::Hypothesis> = hyps
        .iter()
        .filter(|h| mission.is_none_or(|m| h.mission_id == m))
        .collect();
    let in_scope_c: Vec<&crate::domain::evidence::Claim> = claims
        .iter()
        .filter(|c| {
            hyp_mission
                .get(&c.hypothesis_id)
                .is_some_and(|m| mission.is_none_or(|s| *m == s))
        })
        .collect();
    let mut board_lines: Vec<String> = Vec::new();
    let mut board_labels: Vec<String> = Vec::new();
    for h in &in_scope_h {
        board_labels.push(format!("H-{}", h.seq));
        board_lines.push(format!(
            "H-{} [{}] {}",
            h.seq,
            status_code(h.status),
            h.statement
        ));
    }
    for c in in_scope_c.iter().take(12) {
        board_labels.push(format!("CLAIMS-{}", c.seq));
        board_lines.push(format!(
            "CLAIMS-{} (pinned: {}) {}",
            c.seq,
            if c.pinned { "yes" } else { "no" },
            c.text
        ));
    }
    if board_lines.is_empty() && manuscript_note.is_none() && mission.is_none() {
        return Err(
            "empty_context: no mission question, no board objects, and no manuscript — a fit \
             ranking needs a manuscript and/or a mission topic to rank against"
                .to_string(),
        );
    }
    let description = if board_lines.is_empty() {
        format!("[no board objects in scope]{}", manuscript_note.as_deref().unwrap_or(""))
    } else {
        format!("{}\n{}", board_lines.join("\n"), manuscript_note.as_deref().unwrap_or(""))
    };
    Ok(FitContext {
        mission_id: mission,
        description,
        board_labels,
        manuscript_note,
    })
}

fn status_code(s: HypothesisStatus) -> &'static str {
    match s {
        HypothesisStatus::Proposed => "proposed",
        HypothesisStatus::Testing => "testing",
        HypothesisStatus::Supported => "supported",
        HypothesisStatus::Refuted => "refuted",
        HypothesisStatus::Revised => "revised",
    }
}

/// The ranking prompt (Spanish — the app's working language; the venue
/// records are DATA inline, no cloud fetch for the list the model ranks):
/// one line per venue, strict code form, so the parse is exact.
pub(crate) fn fit_prompt(fit_context: &FitContext) -> String {
    let mut venue_lines = String::new();
    for v in journals::venues() {
        venue_lines.push_str(&format!(
            "- {id} | {name} | scope: {scope}\n",
            id = v.id,
            name = v.name,
            scope = v.scope.join(", ")
        ));
    }
    format!(
        "Eres un asesor de publicaciones científicas. La base del tablero del investigador es:\n\
         ---\n{}\n---\n\
         Elige las 3 revistas MÁS adecuadas de ESTA lista (solo esta lista, nunca inventes):\n\
         {}\n\
         Responde EXACTAMENTE con una línea por revista, sin texto extra:\n\
         `venue_id | puntuación 0-100 | razón de una línea citando H-n / CLAIMS-n / etiquetas de scope`\n\
         Razones sin cita (p. ej. «encaja por su alcance») se permiten solo si no existe un objeto \
         del tablero que la respalde; si citas un H-n o CLAIMS-n debe existir de verdad.",
        fit_context.description,
        venue_lines
    )
}

/// One parsed candidate line, before board validation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParsedCandidate {
    pub venue_id: String,
    pub score: u8,
    pub rationale: String,
}

/// Parse the model's reply: each non-empty line `venue_id | score | rationale`.
/// A line that does not parse is not a candidate (the honest-skip rule —
/// never scraped from prose). Venue ids outside the bundled dataset are
/// dropped the same rule (the ranking is over the bundled data, NFR-1).
pub(crate) fn parse_fit_lines(reply: &str) -> Vec<ParsedCandidate> {
    let mut out = Vec::new();
    for line in reply.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let parts: Vec<&str> = line.splitn(3, '|').map(str::trim).collect();
        if parts.len() != 3 {
            continue;
        }
        let Ok(score) = parts[1].parse::<u8>() else {
            continue;
        };
        if score > 100 {
            continue;
        }
        if journals::venue(parts[0]).is_none() {
            continue; // not a bundled dataset venue — not a candidate
        }
        out.push(ParsedCandidate {
            venue_id: parts[0].to_string(),
            score,
            rationale: parts[2].to_string(),
        });
    }
    out
}

/// Validate a candidate's rationale against the board: every `H-{n}` /
/// `CLAIMS-{n}` token that exists in `board_labels` is a pinned citation;
/// one that resolves nowhere is flagged (FR-3.4: unpinned rationale claims
/// are flagged, never silent).
pub(crate) fn validate_candidate(
    parsed: &ParsedCandidate,
    board_labels: &[String],
) -> FitCandidate {
    let mut refs = Vec::new();
    let mut unverified_refs = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for tok in tokenize_refs(&parsed.rationale) {
        if !seen.insert(tok.clone()) {
            continue;
        }
        if board_labels.iter().any(|b| b == &tok) {
            refs.push(tok);
        } else {
            unverified_refs.push(tok);
        }
    }
    refs.sort();
    unverified_refs.sort();
    FitCandidate {
        venue_id: parsed.venue_id.clone(),
        score: parsed.score,
        rationale: parsed.rationale.clone(),
        refs,
        unverified_refs,
    }
}

/// The `H-{n}` / `CLAIMS-{n}` tokens of a rationale line.
pub(crate) fn tokenize_refs(rationale: &str) -> Vec<String> {
    let mut out = Vec::new();
    for pat in ["H-", "CLAIMS-"] {
        let mut from = 0usize;
        while let Some(found) = rationale[from..].find(pat) {
            let start = from + found + pat.len();
            let digits: String = rationale[start..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if !digits.is_empty() {
                out.push(format!("{pat}{digits}"));
            }
            from = start + digits.len();
        }
    }
    out
}

/// One fit run's result (the Fit Finder surface's read): the evented
/// ranking with attribution + the reviewable proposal, or the honest
/// skips (nothing parsed, nothing to rank).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalFitResult {
    pub candidates: Vec<FitCandidate>,
    pub provider: String,
    pub model: String,
    /// The reviewable "choose the top venue" proposal (pending); None
    /// when nothing parsed or the ranking is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<crate::domain::proposals::Proposal>,
}

/// Run the Journal Fit Finder (Story 6.12, FR-19.3): build the board
/// context, ask the provider layer for a ranked shortlist over the
/// bundled dataset, event the ranking (attributed), and propose the top
/// venue's submission mission — advisory end to end: approving the
/// proposal is a human's merge, rejecting changes nothing.
#[tauri::command]
pub async fn run_journal_fit(
    db: State<'_, Db>,
    mission_id: Option<String>,
) -> Result<JournalFitResult, String> {
    let mission = match mission_id.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => Some(
            raw.parse()
                .map_err(|e| format!("invalid_mission_id: `{raw}`: {e}"))?,
        ),
    };
    run_journal_fit_inner(&db, mission).await
}

pub(crate) async fn run_journal_fit_inner(
    db: &Db,
    mission: Option<Uuid>,
) -> Result<JournalFitResult, String> {
    // Phase 1: fold + resolve the context and the provider under one lock.
    let (context, layer, role, events_head) = {
        let conn = db.0.lock().await;
        let store = EventStore::new(&conn);
        let events = store.events_all().map_err(err)?;
        // The manuscript note (the .tex repo IS the manuscript — the main
        // file and the abstract, from the same scans the gate uses).
        let manuscript_note: Option<String> = {
            let ms = ManuscriptsProjection::fold(&events)
                .map_err(err)?
                .into_iter()
                .filter(|m| mission.is_none_or(|id| m.mission_id == id))
                .last();
            ms.map(|m| {
                let stats = journals::build_venue_stats(&m);
                let abstract_line = stats
                    .abstract_words
                    .map(|w| format!(", abstract {w} palabras"))
                    .unwrap_or_default();
                format!("[manuscrito: main={}{abstract_line}]", m.main_file)
            })
        };
        let context = build_fit_context(&events, mission, manuscript_note)?;
        let settings = ProviderSettings::load(&conn);
        let role = fit_role(&settings);
        let layer = ProviderLayer::for_role(db, &conn, &role).map_err(|e| e.to_string())?;
        let head = store.events_all().map_err(err)?.last().map(|e| e.seq).unwrap_or(0);
        (context, layer, role, head)
    };

    // Phase 2: dispatch lock-free (the trust dispatch: kill switch, dial,
    // ceiling, spend), through the provider layer.
    let run_id = format!("journal-fit-{}", Uuid::new_v4().simple());
    {
        let conn = db.0.lock().await;
        if let Some(m) = mission {
            // A mission-scoped fit is a run the receipts drill-down can
            // find.
            EventStore::new(&conn)
                .append(NewEvent::run_started(&run_id, m, "manual", "journal-fit").map_err(err)?)
                .map_err(err)?;
        }
    }
    let plan = CallPlan {
        run_id: run_id.clone(),
        target: layer.name().to_string(),
        model: role.model.clone(),
        mission_id: mission,
        estimate_cents: crate::trust::estimate_cents(&layer, &role.model),
        autonomous: false,
    };
    let req = ChatRequest::new(vec![
        Message::system(
            "Eres un asesor de publicaciones científicas: recomiendas revistas de la lista \
             dada, con razones citadas al tablero, y NUNCA inventas revistas fuera de la lista.",
        ),
        Message::user(fit_prompt(&context)),
    ])
    .with_model(&role.model)
    .with_role("journal-fit");
    let req = match mission {
        Some(m) => req.for_mission(m),
        None => req,
    };
    let resp = reserve_and_chat(db, &layer, req, plan).await.map_err(err)?;
    let parsed = parse_fit_lines(&resp.content);
    let candidates: Vec<FitCandidate> = parsed
        .iter()
        .map(|p| validate_candidate(p, &context.board_labels))
        .collect();

    // Phase 3: append the evented ranking + the reviewable proposal under
    // one lock.
    let proposal = {
        let conn = db.0.lock().await;
        let store = EventStore::new(&conn);
        let event = NewEvent::journal_fit_completed(
            &run_id,
            mission,
            &layer.name(),
            &role.model,
            &candidates,
        )
        .map_err(err)?;
        let stored = store.append(event).map_err(err)?;
        // The reviewable choice: the top candidate's submission mission,
        // as a proposal (AD-3). Approving it creates the checklist — the
        // merge is the human's choice, never the agent's (PRD §10).
        let mut proposal = None;
        if let Some(top) = candidates.first() {
            if let Some(v) = journals::venue(&top.venue_id) {
                let intended = NewEvent::submission_created(SubmissionCreatedPayload {
                    question: format!("Envío a {}", v.name),
                    stop_condition: "submission-ready".into(),
                    success_criterion: "todos los elementos del checklist marcados".into(),
                    venue_id: v.id.to_string(),
                    source_mission_id: mission,
                })
                .map_err(err)?;
                let p = store
                    .append(
                        NewEvent::proposal_created(
                            &run_id,
                            &intended,
                            stored.id,
                            stored.seq,
                            stored.id,
                        )
                        .map_err(err)?,
                    )
                    .map_err(err)?;
                let folded = crate::domain::proposals::ProposalsProjection::fold(
                    &store.events_all().map_err(err)?,
                )
                .map_err(err)?;
                proposal = folded.into_iter().find(|x| x.id == p.id);
            }
        }
        if let Some(m) = mission {
            store
                .append(
                    NewEvent::run_finished(
                        &run_id,
                        m,
                        &format!(
                            "fit: {} ranked · {} proposed",
                            candidates.len(),
                            proposal.is_some() as u32
                        ),
                        proposal.is_some() as u32,
                    )
                    .map_err(err)?,
                )
                .map_err(err)?;
        }
        proposal
    };
    // A fit that proposed nothing still recorded the evented ranking — the
    // advisory read (latest_fit) never dead-spawns a second ranking.
    let _ = events_head;
    Ok(JournalFitResult {
        candidates,
        provider: layer.name().to_string(),
        model: role.model,
        proposal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        EventStore::init(&conn).unwrap();
        conn
    }

    #[test]
    fn parse_fit_lines_reads_the_strict_code_form() {
        let reply = "siam-jsc | 92 | mejor encaje por numerical-analysis H-4\n\
                     garbage line without pipes\n\
                     physrev-e | 71 | scope coincidente\n\
                     acm-toms | 105 | fuera de rango\n\
                     not-a-venue | 80 | inventada\n\
                     jcp-comp | 65 | motivos CLAIMS-2 sin comillas\n";
        let parsed = parse_fit_lines(reply);
        let ids: Vec<&str> = parsed.iter().map(|p| p.venue_id.as_str()).collect();
        assert_eq!(
            ids,
            ["siam-jsc", "physrev-e", "jcp-comp"],
            "unparseable/out-of-range/dataset-foreign lines are not candidates"
        );
        assert_eq!(parsed[0].score, 92);
    }

    #[test]
    fn rationale_references_are_validated_against_the_ledger() {
        let board = vec!["H-4".to_string(), "CLAIMS-2".to_string()];
        let parsed = ParsedCandidate {
            venue_id: "siam-jsc".into(),
            score: 92,
            rationale: "encaja por numerical-analysis y H-4; cita H-99 erróneamente".into(),
        };
        let c = validate_candidate(&parsed, &board);
        assert_eq!(c.refs, ["H-4"]);
        assert_eq!(c.unverified_refs, ["H-99"], "unpinned rationale claims are flagged");
        assert_eq!(c.venue_id, "siam-jsc");
        // dedupe
        let parsed2 = ParsedCandidate {
            venue_id: "siam-jsc".into(),
            score: 80,
            rationale: "H-4 H-4 H-4".into(),
        };
        assert_eq!(validate_candidate(&parsed2, &board).refs, ["H-4"]);
    }

    #[test]
    fn a_boardless_scope_without_a_manuscript_is_a_typed_empty_context() {
        let conn = mem_conn();
        let events = EventStore::new(&conn).events_all().unwrap();
        let e = build_fit_context(&events, None, None).unwrap_err();
        assert!(e.starts_with("empty_context:"), "stable code first: {e}");
        // a manuscript note alone is not empty
        let context = build_fit_context(&events, None, Some("[manuscrito: main=main.tex]".into()))
            .unwrap();
        assert!(context.description.contains("main.tex"));
    }

    #[test]
    fn the_topic_alone_is_a_rankable_context() {
        let conn = mem_conn();
        {
            let store = EventStore::new(&conn);
            let mission = store
                .append(
                    NewEvent::mission_created(crate::domain::missions::MissionCreatedPayload {
                        question: "Do stiff ODE solvers scale?".into(),
                        stop_condition: "Stop after $5.".into(),
                        success_criterion: "A blind rater agrees.".into(),
                        autonomy: crate::domain::missions::Autonomy::Suggest,
                        spend_ceiling_cents: 500,
                        schedule: "daily-03:00".into(),
                        roles: vec![],
                    })
                    .unwrap(),
                )
                .unwrap()
                .id;
            let h = store
                .append(NewEvent::hypothesis_created("X holds.", mission).unwrap())
                .unwrap();
            let _ = h;
            let events = store.events_all().unwrap();
            let context = build_fit_context(&events, Some(mission), None).unwrap();
            assert!(context.board_labels.iter().any(|b| b.starts_with("H-")));
        }
    }

    #[test]
    fn the_fit_event_builds_through_its_typed_constructor() {
        let conn = mem_conn();
        let store = EventStore::new(&conn);
        let cands = vec![FitCandidate {
            venue_id: "siam-jsc".into(),
            score: 92,
            rationale: "H-4".into(),
            refs: vec!["H-4".into()],
            unverified_refs: vec![],
        }];
        let ev = NewEvent::journal_fit_completed("fit-1", None, "simulated", "simulated", &cands)
            .unwrap();
        assert_eq!(ev.kind, "journal.fit_completed");
        // a foreign venue is refused
        let bad = vec![FitCandidate {
            venue_id: "not-a-venue".into(),
            score: 80,
            rationale: "x".into(),
            refs: vec![],
            unverified_refs: vec![],
        }];
        let e = NewEvent::journal_fit_completed("fit-1", None, "simulated", "simulated", &bad)
            .unwrap_err()
            .to_string();
        assert!(e.contains("unknown_venue:"));

        let stored = store.append(ev).unwrap();
        // the scope keyed matches the run (the surface's read)
        let latest = journals::latest_fit(&store.events_all().unwrap(), None)
            .unwrap()
            .expect("the fit run is recorded");
        assert_eq!(latest.run_id, "fit-1");
        assert_eq!(latest.candidates[0].venue_id, "siam-jsc");
        assert_eq!(latest.seq, stored.seq);
        assert!(journals::latest_fit(&store.events_all().unwrap(), Some(Uuid::new_v4()))
            .unwrap()
            .is_none());
    }

    /// The end-to-end keyless path: the simulated layer answers, the run
    /// is evented, the top venue's submission mission is PROPOSED (not
    /// applied — the quarantine holds until the human merges, AD-3).
    #[tokio::test]
    async fn the_simulated_fit_is_advisory_and_quarantined() {
        let mut path = std::env::temp_dir();
        path.push(format!("rc-fit-test-{}.sqlite", uuid::Uuid::new_v4()));
        let db = crate::db::Db::open(&path).unwrap();
        let mission = {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let m = store
                .append(
                    NewEvent::mission_created(crate::domain::missions::MissionCreatedPayload {
                        question: "Do stiff ODE solvers scale?".into(),
                        stop_condition: "Stop after $5.".into(),
                        success_criterion: "A blind rater agrees.".into(),
                        autonomy: crate::domain::missions::Autonomy::Suggest,
                        spend_ceiling_cents: 500,
                        schedule: "daily-03:00".into(),
                        roles: vec![],
                    })
                    .unwrap(),
                )
                .unwrap()
                .id;
            store
                .append(NewEvent::hypothesis_created("X holds.", m).unwrap())
                .unwrap();
            m
        };
        // The run owns its locks — call it unguarded, then read the log
        // under its own lock.
        let result = run_journal_fit_inner(&db, Some(mission)).await.unwrap();
        assert_eq!(result.provider, "simulated");
        // the simulated answer is quoted in the prompt — the parse still
        // yields candidates from the dataset (the simulated layer's reply
        // mirrors the code form)
        assert!(!result.candidates.is_empty(), "simulated provides a ranked list");
        assert!(result.candidates.iter().all(|c| journals::venue(&c.venue_id).is_some()));
        // advisory: a submission proposal sits in quarantine, and NOTHING
        // was applied (no submission mission exists yet)
        let proposal = result.proposal.as_ref().expect("the choice is proposed");
        assert_eq!(proposal.status, crate::domain::proposals::ProposalStatus::Pending);
        let submissions = {
            let c = db.0.lock().await;
            crate::domain::submissions::SubmissionProjection::fold(
                &EventStore::new(&c).events_all().unwrap(),
            )
            .unwrap()
        };
        assert!(submissions.is_empty(), "nothing auto-applied (AD-3)");
        assert_eq!(
            proposal.proposed_payload["venue_id"],
            serde_json::json!(result.candidates[0].venue_id),
            "the proposal chooses the top candidate"
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    }
}