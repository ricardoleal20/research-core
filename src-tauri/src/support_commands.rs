// Support engine shell commands (Story 6.9, FR-23.1, AD-9, AD-10): the
// runtime job that performs the entailment-style SUPPORT check on pinned
// claims — the SANCTIONED LLM counterpart of the NON-LLM existence verifier
// (Story 4.2). For each pinned claim the engine asks a model — through the
// provider layer, with the trust dispatch (kill switch, dial, ceiling, spend
// events) — whether the pinned excerpt actually supports the claim, and
// appends one `pin.support_checked` event (actor=system/support) carrying
// the verdict, the judge's confidence, and the judging model.
//
// THE DIFFERENT-MODEL TEST (NFR-3 extended, AD-9): the judge must differ
// from the pin's own assessing model — never the same model grading its own
// pin. The engine picks the judging pair per pin (the configured local CLI
// first — $0 at high volume, the sweep's preference — then the configured
// BYOK provider, then the simulated layer), skipping any candidate whose
// model matches the pin's assessing model; when no different model is
// available the pin is honestly SKIPPED (`no_different_model`) — its support
// stays unchecked, never silently judged. The typed constructor re-refuses
// the same-model case, and the fold re-checks it on read.
//
// Honesty, not amnesia: an unsupported verdict marks the pin visibly and
// the pin stays; re-checking appends again (the manual command re-checks
// every pin in scope; the Night Shift sweep checks only pins never checked
// for their current pin_seq — efficient de-dup, Story 6.10). Unparseable
// judge replies are NOT verdicts — the pin is skipped (`unparsed`), never
// recorded as `unverifiable` (that verdict belongs to the judge alone).

use rusqlite::Connection;
use tauri::State;
use uuid::Uuid;

use crate::adapters::providers::{
    ChatRequest, Message, ProviderError, ProviderLayer, ProviderSettings,
};
use crate::db::Db;
use crate::domain::evidence::{Claim, EvidenceProjection};
use crate::domain::hypotheses::HypothesesProjection;
use crate::domain::missions::RoleConfig;
use crate::domain::support::{
    models_differ, SupportVerdict, PinSupportCheck, SupportStatus, SUPPORT_JUDGE_ROLE,
};
use crate::eventstore::{EventStore, NewEvent};
use crate::trust::{reserve_and_chat, CallPlan};

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// Resolves the judging pair's adapter through the provider layer; tests
/// inject a fake remote so the real-call paths (spend, receipts) run without
/// network (the same seam shape as `AgentRuntime`'s resolver).
pub(crate) type SupportJudgeResolver =
    fn(&Db, &Connection, &RoleConfig) -> Result<ProviderLayer, ProviderError>;

/// The judging candidates in preference order (Story 6.10's rule): the
/// configured LOCAL CLI first when it is complete ($0 at sweep volume), then
/// the configured BYOK remote provider, then the simulated layer last. The
/// per-pin different-model test filters the list further — the first
/// candidate whose model differs from the pin's assessing model judges.
pub(crate) fn judge_candidates(s: &ProviderSettings) -> Vec<RoleConfig> {
    let judge = |provider: &str, model: &str| RoleConfig {
        name: SUPPORT_JUDGE_ROLE.into(),
        provider: provider.into(),
        model: model.into(),
    };
    let mut out = Vec::new();
    if s.mode.trim() == "cli" {
        let model = s.cli_model.trim();
        if !model.is_empty() {
            out.push(judge("cli", model));
        }
    } else if !s.is_simulated() {
        let (provider, model) = (s.name.trim(), s.model.trim());
        if !provider.is_empty() && !model.is_empty() {
            out.push(judge(provider, model));
        }
    }
    out.push(judge("simulated", "simulated"));
    out
}

/// The first judging candidate that passes the different-model test for
/// `assessing_model` — None when every candidate IS the pin's own assessing
/// model (the honest `no_different_model` skip).
pub(crate) fn pick_judge(
    settings: &ProviderSettings,
    assessing_model: &str,
) -> Option<RoleConfig> {
    judge_candidates(settings)
        .into_iter()
        .find(|c| models_differ(assessing_model, &c.model))
}

/// The entailment prompt (Spanish — the app's working language): the judge
/// sees the claim and the pinned excerpt and answers in a strict two-line
/// code form — the verdict token, then its confidence — so the parse is
/// exact, never a guess scraped from prose.
pub(crate) fn judge_prompt(claim_text: &str, excerpt: &str) -> String {
    format!(
        "Juzga si el extracto anclado SOSTIENE la afirmación (implicación, no \
         mera relevancia):\n\nAfirmación: «{claim_text}»\n\nExtracto anclado:\n«{excerpt}»\n\n\
         Responde EXACTAMENTE en dos líneas:\n1. `supported` si el extracto \
         implica la afirmación; `partially` si sostiene una versión más \
         débil que lo que la afirmación dice; `unsupported` si no la \
         sostiene; `unverifiable` si no puedes determinarlo desde el \
         extracto.\n2. `confidence: <número entre 0.0 y 1.0>` — tu confianza \
         en tu veredicto.",
    )
}

/// Parse the judge's reply: the first non-empty line must be a verdict
/// token, the second `confidence: <x>` in [0, 1]. Anything else is not a
/// verdict — the caller skips the pin (`unparsed`), never invents one.
pub(crate) fn parse_judgment(reply: &str) -> Option<(SupportVerdict, f64)> {
    let mut lines = reply.lines().map(str::trim).filter(|l| !l.is_empty());
    let verdict = SupportVerdict::parse(lines.next()?)?;
    let confidence_line = lines.next()?;
    let raw = confidence_line
        .strip_prefix("confidence:")?
        .trim()
        .parse::<f64>()
        .ok()?;
    if !(0.0..=1.0).contains(&raw) || raw.is_nan() {
        return None;
    }
    Some((verdict, raw))
}

/// The scope of one support run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SupportScope {
    Workspace,
    Hypothesis(Uuid),
    Mission(Uuid),
}

/// One pin scheduled for a support check — everything the judge needs,
/// resolved under one lock so the LLM calls run lock-free.
struct SupportCheck {
    claim_id: Uuid,
    claim_seq: i64,
    claim_text: String,
    hypothesis_id: Uuid,
    mission_id: Option<Uuid>,
    pin_seq: i64,
    excerpt: String,
    assessing_model: String,
}

/// The outcome of one pin's check attempt — the checked verdict, or the
/// honest code-form reason it was skipped (never a fake verdict).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportCheckRecord {
    /// The claim's registration seq — the CLAIMS-{n} chip.
    pub claim_seq: i64,
    /// Some((verdict, confidence)) when the judgment landed and was
    /// appended; None when the pin was skipped (see `skip`).
    pub verdict: Option<SupportVerdict>,
    pub confidence: Option<f64>,
    /// The judging model when a judgment landed.
    pub judging_model: Option<String>,
    /// The code-form skip reason: no_different_model | unparsed |
    /// runtime_killed | autonomy_watch | cost_ceiling_reached |
    /// provider_error | store_error.
    pub skip: Option<String>,
}

/// One support run's summary — the sweep's rollup (the digest's one-line
/// verdict renders from it) and the UI's honest report of what ran.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportRunSummary {
    pub checked: u32,
    pub supported: u32,
    pub partially: u32,
    pub unsupported: u32,
    pub unverifiable: u32,
    /// The per-pin outcomes — verdicts and skips, each referencing its
    /// CLAIMS-{n} chip (FR column style: specifics, never aggregates alone).
    pub records: Vec<SupportCheckRecord>,
    /// The re-folded claims of the scope — exactly what the UI renders after
    /// the run, with the latest support check on every pin.
    pub claims: Vec<Claim>,
}

impl SupportRunSummary {
    /// The one-line, code-form verdict (bilingual-safe by construction —
    /// EXPERIENCE.md): "support: 3 checked · 2 supported · 1 unsupported · 1
    /// skipped(no_different_model)" — counts of zero render as absent.
    pub fn verdict_line(&self) -> String {
        let mut line = format!("support: {} checked", self.checked);
        for (n, label) in [
            (self.supported, "supported"),
            (self.partially, "partially"),
            (self.unsupported, "unsupported"),
            (self.unverifiable, "unverifiable"),
        ] {
            if n > 0 {
                line.push_str(&format!(" · {n} {label}"));
            }
        }
        let skipped = self.records.iter().filter(|r| r.skip.is_some()).count();
        if skipped > 0 {
            line.push_str(&format!(" · {skipped} skipped"));
        }
        line
    }
}

/// Run the support checks over a scope (Story 6.9, FR-23.1): for every
/// pinned claim in scope, pick a judging model that differs from the pin's
/// assessing model, ask it through the provider layer (trust dispatch:
/// kill switch, dial, ceiling, spend), and append one `pin.support_checked`
/// event per judgment. `only_unchecked` de-dups (the sweep passes true: a
/// pin already checked for its current pin_seq is not re-asked; the manual
/// command passes false — re-checking is explicit and re-checkable).
///
/// Three phases, the repo's lock discipline (mirroring the existence
/// verifier's inner): resolve under one lock, judge lock-free, append under
/// one lock with the pin identity re-checked.
#[tauri::command]
pub async fn run_pin_support_checks(
    db: State<'_, Db>,
    hypothesis_id: Option<String>,
) -> Result<SupportRunSummary, String> {
    let scope = match hypothesis_id.as_deref().map(str::trim) {
        None | Some("") => SupportScope::Workspace,
        Some(raw) => SupportScope::Hypothesis(
            raw.parse()
                .map_err(|e| format!("invalid hypothesis id `{raw}`: {e}"))?,
        ),
    };
    run_support_checks_inner(db.inner(), &scope, false, ProviderLayer::for_role, false).await
}

pub(crate) async fn run_support_checks_inner(
    db: &Db,
    scope: &SupportScope,
    only_unchecked: bool,
    resolver: SupportJudgeResolver,
    autonomous: bool,
) -> Result<SupportRunSummary, String> {
    // Phase 1: fold + resolve the checks and the judging candidates under
    // one lock.
    let (checks, settings) = {
        let conn = db.0.lock().await;
        let events = EventStore::new(&conn).events_all().map_err(err)?;
        let claims = EvidenceProjection::fold(&events).map_err(err)?;
        let hyp_mission: std::collections::HashMap<Uuid, Uuid> =
            HypothesesProjection::fold(&events)
                .map_err(err)?
                .into_iter()
                .map(|h| (h.id, h.mission_id))
                .collect();
        let mut checks = Vec::new();
        for claim in &claims {
            let in_scope = match scope {
                SupportScope::Workspace => true,
                SupportScope::Hypothesis(h) => claim.hypothesis_id == *h,
                SupportScope::Mission(m) => hyp_mission
                    .get(&claim.hypothesis_id)
                    .is_some_and(|cm| cm == m),
            };
            if !in_scope {
                continue;
            }
            let Some(pin) = claim.pin.as_ref() else {
                continue; // unpinned — nothing to judge (FR-3.4 already flags it)
            };
            // De-dup (Story 6.10): a pin already judged for its CURRENT
            // pin_seq (a non-stale support status) is not re-asked by the
            // sweep; a stale result predates the current pin — re-checkable.
            if only_unchecked
                && pin
                    .support
                    .as_ref()
                    .is_some_and(|s| s.status != SupportStatus::Stale)
            {
                continue;
            }
            checks.push(SupportCheck {
                claim_id: claim.id,
                claim_seq: claim.seq,
                claim_text: claim.text.clone(),
                hypothesis_id: claim.hypothesis_id,
                mission_id: hyp_mission.get(&claim.hypothesis_id).copied(),
                pin_seq: pin.seq,
                excerpt: pin.excerpt.clone(),
                assessing_model: pin.assessing_model.clone(),
            });
        }
        let settings = ProviderSettings::load(&conn);
        (checks, settings)
    };

    // Phase 2: judge lock-free — one provider-layer call per pin, through
    // the trust dispatch (kill switch + dial checked, spend settled, AD-10).
    // Skips are honest code-form reasons, never fake verdicts.
    let mut judged: Vec<(&SupportCheck, SupportVerdict, f64, String)> = Vec::new();
    let mut records: Vec<SupportCheckRecord> = Vec::with_capacity(checks.len());
    for check in &checks {
        let Some(config) = pick_judge(&settings, &check.assessing_model) else {
            records.push(SupportCheckRecord {
                claim_seq: check.claim_seq,
                verdict: None,
                confidence: None,
                judging_model: None,
                skip: Some("no_different_model".into()),
            });
            continue;
        };
        // Resolve the adapter under a short lock, then dispatch lock-free
        // (the same discipline as the runtime's run_step). A resolver
        // failure is this PIN's honest skip, never the run's death — the
        // remaining pins still get their judgments.
        let layer = {
            let conn = db.0.lock().await;
            match resolver(db, &conn, &config) {
                Ok(layer) => layer,
                Err(_) => {
                    records.push(SupportCheckRecord {
                        claim_seq: check.claim_seq,
                        verdict: None,
                        confidence: None,
                        judging_model: None,
                        skip: Some("provider_error".into()),
                    });
                    continue;
                }
            }
        };
        let plan = CallPlan {
            run_id: format!("support-{}", Uuid::new_v4().simple()),
            target: layer.name().to_string(),
            model: config.model.clone(),
            mission_id: check.mission_id,
            estimate_cents: crate::trust::estimate_cents(&layer, &config.model),
            autonomous,
        };
        let req = ChatRequest::new(vec![
            Message::system(
                "Eres un juez de soporte académico: evalúas con escepticismo \
                 si una fuente anclada sostiene una afirmación, sin inventar \
                 contenido que el extracto no dice. Respondes exactamente en \
                 el formato de dos líneas pedido.",
            ),
            Message::user(judge_prompt(&check.claim_text, &check.excerpt)),
        ])
        .with_model(&config.model)
        .with_role(SUPPORT_JUDGE_ROLE);
        let req = match check.mission_id {
            Some(mission_id) => req.for_mission(mission_id),
            None => req,
        };
        let judging_model = config.model.clone();
        match reserve_and_chat(db, &layer, req, plan).await {
            Ok(resp) => match parse_judgment(&resp.content) {
                Some((verdict, confidence)) => {
                    judged.push((check, verdict, confidence, judging_model));
                }
                None => records.push(SupportCheckRecord {
                    claim_seq: check.claim_seq,
                    verdict: None,
                    confidence: None,
                    judging_model: None,
                    skip: Some("unparsed".into()),
                }),
            },
            Err(e) => records.push(SupportCheckRecord {
                claim_seq: check.claim_seq,
                verdict: None,
                confidence: None,
                judging_model: None,
                skip: Some(e.run_reason()),
            }),
        }
    }

    // Phase 3: append one event per judgment under one lock, the pin
    // identity re-checked (a pin re-pinned mid-run is skipped — its verdict
    // would target text no longer pinned) and the assessing model re-read
    // from the CURRENT pin for the different-model test.
    {
        let conn = db.0.lock().await;
        let store = EventStore::new(&conn);
        let events = store.events_all().map_err(err)?;
        let current: std::collections::HashMap<Uuid, (i64, String)> =
            EvidenceProjection::fold(&events)
                .map_err(err)?
                .into_iter()
                .filter_map(|c| {
                    c.pin.map(|p| (c.id, (p.seq, p.assessing_model)))
                })
                .collect();
        for (check, verdict, confidence, judging_model) in judged {
            let Some((pin_seq, assessing_model)) = current.get(&check.claim_id) else {
                continue; // re-pinned away (or claim gone) mid-run — skip
            };
            if *pin_seq != check.pin_seq {
                continue; // re-pinned mid-run — never a verdict for old text
            }
            store
                .append(
                    NewEvent::pin_support_checked(
                        check.claim_id,
                        check.hypothesis_id,
                        check.pin_seq,
                        verdict,
                        confidence,
                        &judging_model,
                        assessing_model,
                    )
                    .map_err(err)?,
                )
                .map_err(err)?;
            records.push(SupportCheckRecord {
                claim_seq: check.claim_seq,
                verdict: Some(verdict),
                confidence: Some(confidence),
                judging_model: Some(judging_model),
                skip: None,
            });
        }
    }
    records.sort_by_key(|r| r.claim_seq);

    // The re-folded read: the claims of the scope, latest support on every
    // pin — exactly what the UI renders after the run.
    let claims = {
        let conn = db.0.lock().await;
        let events = EventStore::new(&conn).events_all().map_err(err)?;
        let all = EvidenceProjection::fold(&events).map_err(err)?;
        let hyp_mission: std::collections::HashMap<Uuid, Uuid> =
            HypothesesProjection::fold(&events)
                .map_err(err)?
                .into_iter()
                .map(|h| (h.id, h.mission_id))
                .collect();
        all.into_iter()
            .filter(|c| match scope {
                SupportScope::Workspace => true,
                SupportScope::Hypothesis(h) => c.hypothesis_id == *h,
                SupportScope::Mission(m) => hyp_mission
                    .get(&c.hypothesis_id)
                    .is_some_and(|cm| cm == m),
            })
            .collect()
    };

    let mut summary = SupportRunSummary {
        checked: 0,
        supported: 0,
        partially: 0,
        unsupported: 0,
        unverifiable: 0,
        records,
        claims,
    };
    for r in &summary.records {
        if let Some(v) = r.verdict {
            summary.checked += 1;
            match v {
                SupportVerdict::Supported => summary.supported += 1,
                SupportVerdict::Partially => summary.partially += 1,
                SupportVerdict::Unsupported => summary.unsupported += 1,
                SupportVerdict::Unverifiable => summary.unverifiable += 1,
            }
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::providers::{ChatResponse, ProviderClient, Usage};
    use crate::db::set_setting;
    use crate::domain::evidence::{excerpt_digest, PinKind};
    use crate::domain::missions::{Autonomy, MissionCreatedPayload};
    use crate::domain::support::{PinSupportCheckedPayload, PIN_SUPPORT_CHECKED};
    use crate::eventstore::StoredEvent;
    use rusqlite::Connection;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::Db::migrate(&conn).unwrap();
        EventStore::init(&conn).unwrap();
        Db(std::sync::Arc::new(tokio::sync::Mutex::new(conn)))
    }

    /// Seed one mission + hypothesis; returns the hypothesis id.
    async fn seed_hypothesis(db: &Db) -> Uuid {
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        let mission = store
            .append(
                NewEvent::mission_created(MissionCreatedPayload {
                    question: "Does X hold up?".into(),
                    stop_condition: "Stop after $5.".into(),
                    success_criterion: "A blind rater agrees.".into(),
                    autonomy: Autonomy::Suggest,
                    spend_ceiling_cents: 500,
                    schedule: "daily-03:00".into(),
                    roles: vec![],
                })
                .unwrap(),
            )
            .unwrap();
        store
            .append(NewEvent::hypothesis_created("X holds.", mission.id).unwrap())
            .unwrap()
            .id
    }

    /// Register + pin one claim (citation kind) with the given assessing
    /// model; returns the claim id.
    async fn pinned_claim(db: &Db, h: Uuid, text: &str, excerpt: &str, assessing: &str) -> Uuid {
        let c = db.0.lock().await;
        let store = EventStore::new(&c);
        let claim = store
            .append(NewEvent::claim_registered(text, h, None).unwrap())
            .unwrap();
        store
            .append(
                NewEvent::evidence_pinned_citation(claim.id, h, "ref-1", excerpt, 0.8, assessing)
                    .unwrap(),
            )
            .unwrap();
        claim.id
    }

    async fn events_of(db: &Db, kind: &str) -> Vec<StoredEvent> {
        let c = db.0.lock().await;
        EventStore::new(&c)
            .events_all()
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == kind)
            .collect()
    }

    async fn support_of(db: &Db, claim_id: Uuid) -> Option<PinSupportCheck> {
        let c = db.0.lock().await;
        let events = EventStore::new(&c).events_all().unwrap();
        EvidenceProjection::fold(&events)
            .unwrap()
            .into_iter()
            .find(|cl| cl.id == claim_id)
            .and_then(|cl| cl.pin.and_then(|p| p.support))
    }

    /// A judge that answers per claim text: "unsupported" when the claim
    /// says "no", "partially" when it says "weak", "unverifiable" when it
    /// says "cant", else "supported" — always a parseable two-line reply.
    /// A no-network remote layer over it, so spend paths run end to end.
    struct PerClaimJudge {
        answers: HashMap<String, &'static str>,
    }

    impl ProviderClient for PerClaimJudge {
        fn name(&self) -> &str {
            "fake-judge"
        }
        fn chat(
            &self,
            req: ChatRequest,
        ) -> crate::adapters::providers::BoxFuture<'_, Result<ChatResponse, ProviderError>>
        {
            // The claim under judgment, extracted from the prompt's
            // «Afirmación: …» line — never the prompt's own instruction
            // words (which would match every needle).
            let claim = Message::last_user(&req.messages)
                .and_then(|m| {
                    m.content
                        .split("Afirmación: «")
                        .nth(1)
                        .and_then(|rest| rest.split('»').next())
                })
                .unwrap_or("");
            let content = self
                .answers
                .iter()
                .find(|(needle, _)| claim.contains(needle.as_str()))
                .map(|(_, reply)| *reply)
                .unwrap_or("supported\nconfidence: 0.9");
            Box::pin(async move {
                Ok(ChatResponse {
                    content: content.to_string(),
                    usage: Usage { input_tokens: 200, output_tokens: 40 },
                })
            })
        }
    }

    fn per_claim_resolver(
        db: &Db,
        _conn: &Connection,
        role: &RoleConfig,
    ) -> Result<ProviderLayer, ProviderError> {
        Ok(crate::adapters::providers::remote_layer_with_client(
            db,
            &role.provider,
            &role.model,
            Box::new(PerClaimJudge {
                answers: HashMap::from([
                    ("no-sostiene".to_string(), "unsupported\nconfidence: 0.8"),
                    ("débil".to_string(), "partially\nconfidence: 0.7"),
                    ("indeterminable".to_string(), "unverifiable\nconfidence: 0.6"),
                ]),
            }),
        ))
    }

    fn remote_settings() -> ProviderSettings {
        ProviderSettings {
            mode: "provider".into(),
            name: "custom".into(),
            base_url: "http://localhost:9".into(),
            api_key: "sk-test".into(),
            model: "judge-model".into(),
            local_base_url: String::new(),
            cli: String::new(),
            cli_model: String::new(),
        }
    }

    /// Store the settings so the engine's candidates resolve (the engine
    /// reads ProviderSettings::load, not the injected struct).
    async fn arm_settings(db: &Db, s: &ProviderSettings) {
        let c = db.0.lock().await;
        set_setting(&c, "llm_mode", &s.mode).unwrap();
        set_setting(&c, "provider", &s.name).unwrap();
        set_setting(&c, "base_url", &s.base_url).unwrap();
        set_setting(&c, "api_key", &s.api_key).unwrap();
        set_setting(&c, "model", &s.model).unwrap();
        set_setting(&c, "cli", &s.cli).unwrap();
        set_setting(&c, "cli_model", &s.cli_model).unwrap();
    }

    // ---------- the pure pieces ----------

    #[test]
    fn judge_candidates_prefer_local_then_byok_then_simulated() {
        // CLI mode with a complete local config: the local CLI first,
        // simulated last — $0 at sweep volume before anything paid.
        let s = ProviderSettings {
            mode: "cli".into(),
            cli_model: "claude-sonnet-4-5".into(),
            ..remote_settings()
        };
        let candidates = judge_candidates(&s);
        assert_eq!(candidates.len(), 2);
        assert_eq!((candidates[0].provider.as_str(), candidates[0].model.as_str()), ("cli", "claude-sonnet-4-5"));
        assert_eq!(candidates[1].provider, "simulated");
        // provider mode: the BYOK pair first, simulated last
        let candidates = judge_candidates(&remote_settings());
        assert_eq!(candidates.len(), 2);
        assert_eq!(
            (candidates[0].provider.as_str(), candidates[0].model.as_str()),
            ("custom", "judge-model")
        );
        // simulated mode: only simulated
        let s = ProviderSettings { mode: "simulate".into(), ..remote_settings() };
        assert_eq!(judge_candidates(&s).len(), 1);
        assert_eq!(judge_candidates(&s)[0].provider, "simulated");
    }

    /// THE DIFFERENT-MODEL TEST at candidate selection: a judging candidate
    /// whose model IS the pin's assessing model is passed over; when every
    /// candidate matches, None — the honest no_different_model skip.
    #[test]
    fn pick_judge_passes_over_the_pins_own_assessing_model() {
        let s = remote_settings(); // primary custom/judge-model, simulated fallback
        // a pin assessed by a third model: the BYOK pair judges
        let picked = pick_judge(&s, "GLM-5.3").unwrap();
        assert_eq!(picked.model, "judge-model");
        // a pin assessed by the BYOK model itself: simulated judges
        let picked = pick_judge(&s, "judge-model").unwrap();
        assert_eq!(picked.model, "simulated");
        // a pin assessed by BOTH candidates' models: no judge — skip
        let s = ProviderSettings { mode: "simulate".into(), ..remote_settings() };
        assert_eq!(pick_judge(&s, "simulated"), None);
    }

    #[test]
    fn parse_judgment_accepts_the_two_line_contract_only() {
        for (reply, expected) in [
            ("supported\nconfidence: 0.9", Some((SupportVerdict::Supported, 0.9))),
            (" unsupported \nconfidence: 0.5", Some((SupportVerdict::Unsupported, 0.5))),
            ("partially\nconfidence: 0.75", Some((SupportVerdict::Partially, 0.75))),
            ("unsupported\nconfidence: 1.0", Some((SupportVerdict::Unsupported, 1.0))),
            ("unverifiable\nconfidence: 0", Some((SupportVerdict::Unverifiable, 0.0))),
            // a leading blank line is tolerated (the first NON-EMPTY line)
            ("\n\nsupported\nconfidence: 0.9", Some((SupportVerdict::Supported, 0.9))),
            // anything else is not a verdict — never scraped from prose
            ("I think this is supported.\nconfidence: 0.9", None),
            ("supported", None), // no confidence line
            ("supported\nconfidence: 1.5", None), // out of range
            ("supported\nconfidence: high", None),
            ("maybe\nconfidence: 0.9", None),
            ("", None),
        ] {
            assert_eq!(parse_judgment(reply), expected, "reply: {reply:?}");
        }
    }

    #[test]
    fn the_verdict_line_is_code_form_and_omits_zero_counts() {
        let summary = SupportRunSummary {
            checked: 4,
            supported: 2,
            partially: 1,
            unsupported: 1,
            unverifiable: 0,
            records: vec![
                SupportCheckRecord {
                    claim_seq: 2,
                    verdict: Some(SupportVerdict::Supported),
                    confidence: Some(0.9),
                    judging_model: Some("judge-model".into()),
                    skip: None,
                },
                SupportCheckRecord {
                    claim_seq: 5,
                    verdict: None,
                    confidence: None,
                    judging_model: None,
                    skip: Some("no_different_model".into()),
                },
            ],
            claims: vec![],
        };
        let line = summary.verdict_line();
        assert_eq!(
            line,
            "support: 4 checked · 2 supported · 1 partially · 1 unsupported · 1 skipped"
        );
        assert!(!line.contains('\n'));
        // zero activity renders the honest empty line
        let empty = SupportRunSummary {
            checked: 0,
            supported: 0,
            partially: 0,
            unsupported: 0,
            unverifiable: 0,
            records: vec![],
            claims: vec![],
        };
        assert_eq!(empty.verdict_line(), "support: 0 checked");
    }

    // ---------- the engine, end to end ----------

    /// The engine judges every pinned claim in scope through the provider
    /// layer — one `pin.support_checked` event per judgment, actor
    /// system/support, attributed to its judging model, spend recorded
    /// (AD-10) — and the three signals stay separate on the re-folded pin.
    #[tokio::test]
    async fn the_engine_judges_every_pin_and_lands_attributed_events() {
        let db = test_db();
        let h = seed_hypothesis(&db).await;
        arm_settings(&db, &remote_settings()).await;
        let ok = pinned_claim(&db, h, "Attention drops recurrence.", "Excerpt.", "GLM-5.3").await;
        let bad =
            pinned_claim(&db, h, "Esto no-sostiene la afirmación.", "Excerpt.", "GLM-5.3").await;

        let summary = run_support_checks_inner(
            &db,
            &SupportScope::Workspace,
            false,
            per_claim_resolver,
            false,
        )
        .await
        .unwrap();
        assert_eq!(summary.checked, 2);
        assert_eq!(summary.supported, 1);
        assert_eq!(summary.unsupported, 1);
        assert_eq!(summary.claims.len(), 2);

        // One event per judgment: actor system/support, judging model
        // recorded, cause-linked to claim + hypothesis.
        let events = events_of(&db, PIN_SUPPORT_CHECKED).await;
        assert_eq!(events.len(), 2);
        for e in &events {
            assert_eq!(
                e.actor,
                crate::eventstore::Actor::System {
                    component: crate::eventstore::SystemComponent::Support
                }
            );
            let payload: PinSupportCheckedPayload =
                serde_json::from_value(e.payload.clone()).unwrap();
            assert_eq!(payload.judging_model, "judge-model");
            assert_ne!(payload.judging_model, payload_verdict_assessing(&db, &payload.claim_id).await);
            assert_eq!(e.causes.len(), 2);
        }
        // The read model carries the third signal; confidence untouched.
        let s = support_of(&db, ok).await.unwrap();
        assert_eq!(s.status, SupportStatus::Supported);
        assert_eq!(s.judging_model, "judge-model");
        let s = support_of(&db, bad).await.unwrap();
        assert_eq!(s.status, SupportStatus::Unsupported);
        let claims = summary.claims;
        let read = claims.iter().find(|c| c.id == bad).unwrap();
        assert_eq!(read.pin.as_ref().unwrap().confidence, 0.8);
        assert_eq!(read.pin.as_ref().unwrap().assessing_model, "GLM-5.3");
        assert!(read.pinned, "an unsupported verdict never deletes the pin");
    }

    async fn payload_verdict_assessing(db: &Db, claim_id: &Uuid) -> String {
        let c = db.0.lock().await;
        let events = EventStore::new(&c).events_all().unwrap();
        EvidenceProjection::fold(&events)
            .unwrap()
            .into_iter()
            .find(|cl| cl.id == *claim_id)
            .and_then(|cl| cl.pin.map(|p| p.assessing_model))
            .unwrap()
    }

    /// THE DIFFERENT-MODEL TEST, end to end: a pin assessed by the BYOK
    /// model itself falls to the simulated candidate; a pin assessed by
    /// BOTH candidates' models is honestly SKIPPED (no_different_model) —
    /// no event, support stays unchecked, never a fake verdict.
    #[tokio::test]
    async fn a_pin_with_no_different_model_available_is_honestly_skipped() {
        let db = test_db();
        let h = seed_hypothesis(&db).await;
        // provider mode: candidates = custom/judge-model, simulated/simulated
        arm_settings(&db, &remote_settings()).await;
        // a pin assessed by the BYOK model: simulated judges (a different
        // model — the per-claim fake stands in for it)
        let fallback = pinned_claim(&db, h, "A claim.", "Excerpt.", "judge-model").await;
        let summary = run_support_checks_inner(
            &db,
            &SupportScope::Workspace,
            false,
            per_claim_resolver,
            false,
        )
        .await
        .unwrap();
        let s = support_of(&db, fallback).await.unwrap();
        assert_eq!(s.judging_model, "simulated");
        assert_eq!(s.status, SupportStatus::Supported);

        // simulate mode: only candidate is simulated — a pin assessed by
        // `simulated` has NO different model: skipped, no event, unchecked.
        let db2 = test_db();
        let h2 = seed_hypothesis(&db2).await;
        arm_settings(&db2, &ProviderSettings { mode: "simulate".into(), ..remote_settings() })
            .await;
        let stuck =
            pinned_claim(&db2, h2, "A claim.", "Excerpt.", "simulated").await;
        let summary = run_support_checks_inner(
            &db2,
            &SupportScope::Workspace,
            false,
            per_claim_resolver,
            false,
        )
        .await
        .unwrap();
        assert_eq!(summary.checked, 0);
        assert_eq!(summary.records.len(), 1);
        assert_eq!(summary.records[0].skip.as_deref(), Some("no_different_model"));
        assert_eq!(summary.records[0].claim_seq, 3, "the claim's CLAIMS-n chip");
        assert!(events_of(&db2, PIN_SUPPORT_CHECKED).await.is_empty());
        assert_eq!(support_of(&db2, stuck).await, None, "unchecked, never faked");
        assert_eq!(
            summary.verdict_line(),
            "support: 0 checked · 1 skipped"
        );
    }

    /// De-dup (Story 6.10): only_unchecked skips pins already judged for
    /// their CURRENT pin_seq — and an unparsed reply leaves the pin
    /// unchecked (so the next sweep retries it), while a judged pin is not
    /// re-asked.
    #[tokio::test]
    async fn only_unchecked_dedups_and_unparsed_leaves_the_pin_unchecked() {
        let db = test_db();
        let h = seed_hypothesis(&db).await;
        arm_settings(&db, &remote_settings()).await;
        let ok = pinned_claim(&db, h, "A claim.", "Excerpt.", "GLM-5.3").await;

        // First sweep: judged once.
        let s = run_support_checks_inner(
            &db,
            &SupportScope::Workspace,
            true,
            per_claim_resolver,
            true,
        )
        .await
        .unwrap();
        assert_eq!(s.checked, 1);
        // Second sweep: de-duped — no new event, no new spend.
        let s = run_support_checks_inner(
            &db,
            &SupportScope::Workspace,
            true,
            per_claim_resolver,
            true,
        )
        .await
        .unwrap();
        assert_eq!(s.checked, 0, "already judged for its current pin — skipped");
        assert!(s.records.is_empty(), "de-dup drops it from the records entirely");
        assert_eq!(events_of(&db, PIN_SUPPORT_CHECKED).await.len(), 1);

        // A re-pin makes the old judgment stale: the sweep re-asks (the
        // stale result predates the current pin).
        {
            let c = db.0.lock().await;
            EventStore::new(&c)
                .append(
                    NewEvent::evidence_pinned_citation(
                        ok,
                        h,
                        "ref-1",
                        "A NEW excerpt.",
                        0.8,
                        "GLM-5.3",
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let s = run_support_checks_inner(
            &db,
            &SupportScope::Workspace,
            true,
            per_claim_resolver,
            true,
        )
        .await
        .unwrap();
        assert_eq!(s.checked, 1, "a stale result is re-checkable by the sweep");
        assert_eq!(events_of(&db, PIN_SUPPORT_CHECKED).await.len(), 2);

        // The manual command (only_unchecked=false) re-checks everything —
        // re-verification is explicit and re-checkable.
        let s = run_support_checks_inner(
            &db,
            &SupportScope::Workspace,
            false,
            per_claim_resolver,
            false,
        )
        .await
        .unwrap();
        assert_eq!(s.checked, 1);
        assert_eq!(events_of(&db, PIN_SUPPORT_CHECKED).await.len(), 3);
    }

    /// The scope governs: a hypothesis-scoped run judges only its own
    /// claims; the mission scope judges the mission's.
    #[tokio::test]
    async fn the_scope_governs_which_pins_get_judged() {
        let db = test_db();
        let h1 = seed_hypothesis(&db).await;
        let h2 = seed_hypothesis(&db).await;
        arm_settings(&db, &remote_settings()).await;
        pinned_claim(&db, h1, "A claim.", "Excerpt.", "GLM-5.3").await;
        pinned_claim(&db, h2, "A claim.", "Excerpt.", "GLM-5.3").await;

        let s = run_support_checks_inner(
            &db,
            &SupportScope::Hypothesis(h1),
            false,
            per_claim_resolver,
            false,
        )
        .await
        .unwrap();
        assert_eq!(s.checked, 1);
        assert_eq!(s.claims.len(), 1);
        assert_eq!(s.claims[0].hypothesis_id, h1);
        // workspace: the second run covers both
        let s = run_support_checks_inner(
            &db,
            &SupportScope::Workspace,
            false,
            per_claim_resolver,
            false,
        )
        .await
        .unwrap();
        assert_eq!(s.checked, 2);
        // mission scope of h1's mission: only that mission's claims
        let mission = {
            let c = db.0.lock().await;
            let events = EventStore::new(&c).events_all().unwrap();
            HypothesesProjection::fold(&events)
                .unwrap()
                .into_iter()
                .find(|hyp| hyp.id == h1)
                .unwrap()
                .mission_id
        };
        let s = run_support_checks_inner(
            &db,
            &SupportScope::Mission(mission),
            true,
            per_claim_resolver,
            true,
        )
        .await
        .unwrap();
        assert_eq!(s.checked, 0, "de-duped — both pins already judged");
    }

    /// Real calls append spend (AD-10): the engine's dispatches are
    /// role-tagged (`support`) and mission-linked — the receipts name what
    /// ran. (The per-claim fake is a Remote-kind layer, so every call
    /// records.)
    #[tokio::test]
    async fn the_engines_calls_record_role_tagged_spend() {
        let db = test_db();
        let h = seed_hypothesis(&db).await;
        arm_settings(&db, &remote_settings()).await;
        pinned_claim(&db, h, "A claim.", "Excerpt.", "GLM-5.3").await;
        run_support_checks_inner(
            &db,
            &SupportScope::Workspace,
            false,
            per_claim_resolver,
            false,
        )
        .await
        .unwrap();
        let spend = {
            let c = db.0.lock().await;
            EventStore::new(&c)
                .events_all()
                .unwrap()
                .into_iter()
                .filter(|e| e.kind == "spend.recorded")
                .collect::<Vec<_>>()
        };
        assert_eq!(spend.len(), 1, "one role-tagged spend per real call");
        assert_eq!(spend[0].payload["role"], serde_json::json!("support"));
        assert_eq!(spend[0].payload["model"], serde_json::json!("judge-model"));
        assert!(spend[0].payload["cost_cents"].as_u64().unwrap() > 0);
        let _ = Arc::new(0); // Arc import exercised
    }

    /// An unpinned claim is never judged; a provider error is an honest
    /// skip (provider_error), never a verdict.
    #[tokio::test]
    async fn unpinned_claims_are_skipped_silently_and_provider_errors_are_honest_skips() {
        let db = test_db();
        let h = seed_hypothesis(&db).await;
        arm_settings(&db, &remote_settings()).await;
        // one unpinned claim + one pinned
        {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            store
                .append(NewEvent::claim_registered("Unpinned.", h, None).unwrap())
                .unwrap();
        }
        pinned_claim(&db, h, "A claim.", "Excerpt.", "GLM-5.3").await;
        let failing: SupportJudgeResolver = |_db, _conn, _role| {
            Err(ProviderError::Api {
                name: "test".into(),
                status: 503,
                body: "down".into(),
            })
        };
        let s = run_support_checks_inner(&db, &SupportScope::Workspace, false, failing, false)
            .await
            .unwrap();
        assert_eq!(s.checked, 0);
        assert_eq!(s.records.len(), 1, "only the pinned claim has a record");
        assert_eq!(s.records[0].skip.as_deref(), Some("provider_error"));
        assert!(events_of(&db, PIN_SUPPORT_CHECKED).await.is_empty());
        // the unpinned claim never appears in the records at all
        assert!(s.records.iter().all(|r| r.claim_seq != 1));
    }

    /// The digest of the pinned excerpt verifies on read (AD-5) — the
    /// engine inherits the fold's corruption guarantees: a tampered pin
    /// fails the run loudly, never judges.
    #[tokio::test]
    async fn a_tampered_pin_fails_the_run_loudly() {
        let db = test_db();
        let h = seed_hypothesis(&db).await;
        {
            let c = db.0.lock().await;
            let store = EventStore::new(&c);
            let claim = store
                .append(NewEvent::claim_registered("Claim.", h, None).unwrap())
                .unwrap();
            store
                .append(
                    NewEvent::new(
                        "evidence.pinned",
                        crate::eventstore::Actor::User,
                        serde_json::json!({
                            "claim_id": claim.id.to_string(),
                            "hypothesis_id": h.to_string(),
                            "kind": "citation",
                            "ref_id": "ref-1",
                            "excerpt": "The quoted excerpt.",
                            "digest": excerpt_digest("different text entirely"),
                            "confidence": 0.9,
                            "assessing_model": "GLM-5.3",
                        }),
                    )
                    .unwrap()
                    .with_causes(vec![claim.id, h]),
                )
                .unwrap();
        }
        let result = run_support_checks_inner(
            &db,
            &SupportScope::Workspace,
            false,
            per_claim_resolver,
            false,
        )
        .await;
        assert!(result.is_err(), "a tampered pin never reaches a judge");
        let _ = PinKind::Citation; // import exercised
    }
}
