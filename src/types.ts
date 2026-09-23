export interface Project {
  id: string;
  name: string;
  folder: string;
  kind: string;
  tags: string;
  color: string;
  chapter_index: number;
  chapter_count: number;
  created_at: string;
  updated_at: string;
  is_active: number;
}

export interface Ref {
  id: string;
  project_id: string;
  collection_id: string | null;
  title: string;
  authors: string;
  year: number;
  venue: string;
  doi: string;
  url: string;
  isbn: string;
  attachment: string | null;
  status: string; // unread | reading | read | reviewed
  tags: string;
  used: number; // 0 | 1
  citation_count: number;
  created_at: string;
  usages?: Usage[];
  // Evented library state (FR-15, Epic 5): the source of the ref.added
  // event (arxiv | zotero | manual, derived for legacy baseline rows), the
  // archived flag while a ref.removed event masks the ref, the Zotero/arXiv
  // identity, and the ref's own mini-timeline (receipt voice).
  source?: string;
  removed?: boolean;
  zotero_item_key?: string | null;
  arxiv_id?: string | null;
  timeline?: RefAuditEntry[];
}

// One entry of a ref's own mini-timeline — the audit trail the detail
// drawer renders (mono, seq + ts + actor).
export interface RefAuditEntry {
  seq: number;
  ts: string;
  actor: string; // "user" | "agent:<run_id>" | "system:<component>"
  kind: string; // ref.added | ref.removed | ref.restored
}

// The honest Zotero import summary (FR-15.2): imported / skipped / failed
// counts — no item is silently dropped.
export interface ZoteroImportResult {
  imported: number;
  skipped: number;
  failed: number;
  refs: Ref[];
  skippedItems: string[];
  failedItems: string[];
}

export interface Usage {
  id: string;
  ref_id: string;
  project_id: string;
  location: string;
  context: string;
  created_at: string;
}

export interface Review {
  id: string;
  project_id: string;
  number: number;
  score: number;
  focus: string;
  dims: any[];
  verdict: string;
  created_at: string;
  findings?: Finding[];
}

export interface Finding {
  id: string;
  review_id: string;
  severity: string;
  location: string;
  text: string;
}

export interface Action {
  id: string;
  project_id: string;
  code: string;
  title: string;
  description: string;
  priority: string; // high | med | low
  origin: string; // manual | asistente | review
  due: string;
  location: string;
  done: number;
  done_at: string | null;
  source_review_id: string | null;
  source_finding: string;
  notes: string;
  created_at: string;
}

export interface Chat {
  id: string;
  project_id: string;
  kind: string; // asistente | review
  title: string;
  preview: string;
  mission_id: string | null; // Story 5.4 (FR-16.4): the mission scope; null = General
  skill: string | null; // Story 5.6 (FR-16.8): the per-conversation skill; null = plain persona
  model: string | null; // Story 5.9 (FR-17.4): the chosen model; null = the provider default
  created_at: string;
  updated_at: string;
  messages?: Message[];
  attachments?: ChatAttachment[]; // Story 5.5: the folded active attachment chips
}

export interface Message {
  id: string;
  chat_id: string;
  role: string; // user | agent
  content: string;
  classify_tag: string | null;
  meta: string | null;
  mission_id: string | null; // Story 5.4: the scope the message was sent under
  created_at: string;
}

// One conversation attachment (Story 5.5, FR-16.1–16.3): stored by its
// digest-addressed ref; text/pdf are included as provider context, binary
// is flagged "unsupported-inline v1" — never silently dropped.
export interface ChatAttachment {
  id: string;
  name: string;
  kind: "text" | "pdf" | "binary";
  digest: string;
  size_bytes: number;
  included: boolean;
  truncated: boolean;
  note: string; // "" | "unsupported-inline v1" | "pdf-extraction-failed" | …
}

export interface Agent {
  id: string;
  key: string;
  name: string;
  description: string;
  icon: string;
  enabled: number;
  kind: string; // judge | orchestrator
}

export interface McpServer {
  id: string;
  name: string;
  transport: string; // stdio | http
  command: string;
  args: string;
  env: string;
  url: string;
  tags: string;
  connected: number;
  created_at: string;
}

// Missions (event-sourced read model, camelCase wire form per Tauri 2)
export type Autonomy = "watch" | "suggest" | "act_with_receipts";

export type MissionStatus = "draft" | "active" | "awaiting_review" | "completed" | "stopped" | "failed";

// DESIGN.md components.spend-meter: ok under 80% of ceiling, near at 80%+,
// blocked at/over the hard ceiling (dispatch refused past it, AD-10).
export type SpendState = "ok" | "near" | "blocked";

// One agent role of a mission's runtime config (Story 2.1, NFR-3): a named
// role bound to a (provider, model) pair — the core rejects a config whose
// critic shares a drafter's pair (same_model_critic). Story 5.6 (FR-16.7)
// grows the vocabulary with the six scientific skill roles; NFR-3's
// different-model rule stays mission-scoped — skill roles are per-chat.
export type AgentRoleName =
  | "drafter"
  | "critic"
  | "librarian"
  | "verifier"
  | "synthesizer"
  | "note_taker";

// A skill definition (Story 5.6, FR-16.6–16.8): data, not code — a name,
// a RoleConfig-shaped (provider, model) pair (empty = the configured
// layer's model, exactly as the default drafter resolves), a system-prompt
// role, and an allowed tool set from the closed vocabulary. The curated
// six ship pre-installed; the registry is extensible (add_skill).
export interface Skill {
  name: AgentRoleName;
  provider: string; // "" = the configured layer; "simulated" | "cli" | BYOK
  model: string; // "" = the configured layer's model
  systemPrompt: string;
  tools: string[]; // closed vocabulary: search | read_board | read_library | verify
  builtin: boolean;
}

export interface RoleConfig {
  name: AgentRoleName; // the role's name is its identity
  provider: string; // provider-layer name ("simulated" | "cli" | BYOK name)
  model: string; // the model the role runs on
}

export interface Mission {
  id: string; // the mission.created event id
  seq: number; // store-assigned seq of the creation event
  ts: string; // ISO-8601 UTC
  question: string;
  stopCondition: string;
  successCriterion: string;
  autonomy: Autonomy;
  spendCeilingCents: number;
  roles: RoleConfig[]; // the mission's agent-role config (Story 2.1)
  schedule: string; // Night Shift schedule (Story 2.3): "off" | "daily-HH:MM"
  status: MissionStatus; // derived by the fold from events referencing the mission
  spendCents: number; // total spend.recorded cost against the ceiling
  spendState: SpendState; // the spend meter's state
}

// One entry of a mission's run list (FR-1.3): any log event referencing the
// mission — the basic drill-down the missions home renders.
export interface MissionRun {
  seq: number;
  id: string;
  ts: string;
  kind: string; // raw event kind, rendered in mono (receipt voice)
  actor: string; // "user" | "agent" | "system:<component>"
  role?: string; // the agent role a role-scoped event is attributed to
  runId?: string; // the run the event belongs to (Story 2.5) — the receipt drill-down's target
  detail?: string; // the terminal reason a job.failed row renders (Stories 6.2–6.4, AD-12)
}

// One completed agent step (Story 2.1): which role ran, on which
// provider+model, and what it produced.
export interface AgentStepResult {
  missionId: string;
  role: AgentRoleName;
  provider: string;
  model: string;
  content: string;
}

// Hypotheses (event-sourced read model, Story 1.5): lifecycle is
// machine-enforced (FR-2.2) — proposed → testing → supported/refuted,
// supported/refuted → revised, revised → testing; every transition is an
// event with an audit stamp. Typed relations render as chips (FR-2.3).
export type HypothesisStatus = "proposed" | "testing" | "supported" | "refuted" | "revised";

// The typed-relation vocabulary (FR-2.3): contradicts | extends |
// specializes | supports_the_same_claim.
export type RelationKind = "contradicts" | "extends" | "specializes" | "supports_the_same_claim";

// A relation chip's direction from its card's perspective:
// outgoing reads "⟶ contradicts H-7"; incoming reads "⟵ contradicted-by H-3".
export type RelationDirection = "outgoing" | "incoming";

// The audit stamp of the last lifecycle event (FR-2.2): actor, ts, basis.
export interface AuditStamp {
  seq: number;
  ts: string;
  actor: string; // "user" | "agent" | "system:<component>"
  basis: string; // transition basis; the event kind on creation
}

// One typed-relation chip on a hypothesis card (FR-2.3) — chips only,
// never a graph canvas.
export interface RelationChip {
  seq: number; // the relation event's seq (latest per endpoint pair wins)
  kind: RelationKind;
  direction: RelationDirection;
  otherId: string; // the other endpoint's creation event id
  otherSeq: number; // the other endpoint's creation seq — the H-n label
  otherStatement: string;
}

export interface Hypothesis {
  id: string; // the hypothesis.created event id
  seq: number; // creation event seq — the H-n label derives from it
  ts: string; // ISO-8601 UTC
  statement: string;
  missionId: string;
  status: HypothesisStatus; // derived by the fold from status_changed events
  relations: RelationChip[]; // derived: typed relations, both directions
  audit: AuditStamp; // derived: the last lifecycle event's stamp
}

// Evidence pins (event-sourced read model, Story 1.7): an AI-generated
// claim attached to a hypothesis; a claim without an evidence.pinned
// event reads as UNPINNED (FR-3.4) and renders the amber chip.
export type PinKind = "citation" | "numerical";

// One evidence pin (AD-5 shape): the source it anchors to (a library ref
// for citations, an artifact_ref for numerical pins — FR-3.3), the pinned
// content (`excerpt` — the quoted passage or the values), the sha-256
// digest of that content (computed at construction — never trusted from
// callers), and the agent-assessed confidence attributed to the assessing
// model (FR-3.6) — never "verified".
// The LATEST machine verification of one pin (Story 4.2, AD-15): a
// projection from `evidence.verified` events (actor=system/verifier, never
// an LLM). `status` is the display vocabulary — verified | failed come from
// the event; stale is fold-derived (the result predates the current pin,
// re-verifiable). null = unverified — a state entirely distinct from the
// agent-assessed confidence: verification is existence by code, confidence
// is a named model's judgment (FR-3.6).
export type VerificationStatus = "verified" | "failed" | "stale";

export interface PinVerification {
  status: VerificationStatus;
  detail: string; // machine code: excerpt_matched / digest_ok / not_found / artifact_changed / artifact_missing / fetch_error / no_source
  source: string; // what was consulted: arxiv:<id>, a URL, or an artifact ref
  ts: string;
}

// The support check (Story 6.9, FR-23.1/23.2) — the THIRD signal on a pin:
// an entailment-style faithfulness judgment by an LLM (actor system/support)
// through the provider layer, attributed to its judging model. Never
// conflated with confidence (the assessing model's own judgment) or
// verification (existence by code): three signals, three chips. The judging
// model always DIFFERS from the pin's assessing model (NFR-3 extended —
// never the same model grading its own pin).
export type SupportVerdict = "supported" | "partially" | "unsupported" | "unverifiable";

// The read model's display vocabulary: the event's verdict, plus `stale`
// (fold-derived — the result predates the current pin, re-checkable).
export type SupportStatus = SupportVerdict | "stale";

export interface PinSupportCheck {
  status: SupportStatus;
  confidence: number; // the judge's confidence in its verdict, 0.0–1.0
  judgingModel: string; // rendered mono with the result — attribution
  ts: string; // visibly dated — never silently assumed fresh
}

// One pin's check attempt from a support run (Story 6.9): the landed
// verdict, or the honest code-form skip reason (no_different_model |
// unparsed | provider_error | runtime_killed | autonomy_watch |
// cost_ceiling_reached) — never a fake verdict.
export interface SupportCheckRecord {
  claimSeq: number; // the CLAIMS-n chip
  verdict: SupportVerdict | null;
  confidence: number | null;
  judgingModel: string | null;
  skip: string | null;
}

// One support run's summary (Story 6.9): the rollup the digest's one-line
// verdict renders from, the per-pin records (specifics, never aggregates
// alone), and the re-folded claims of the scope.
export interface SupportRunSummary {
  checked: number;
  supported: number;
  partially: number;
  unsupported: number;
  unverifiable: number;
  records: SupportCheckRecord[];
  claims: Claim[];
}

export interface EvidencePin {
  seq: number; // the evidence.pinned event's seq (latest per claim wins)
  ts: string;
  claimId: string;
  hypothesisId: string;
  kind: PinKind;
  refId: string | null; // the library ref a citation pin cites; null for numerical
  artifactRef: string | null; // the artifact a numerical pin anchors to; null for citations
  excerpt: string;
  digest: string; // sha-256 hex of the pinned content
  confidence: number; // 0.0–1.0, agent-assessed
  assessingModel: string; // e.g. "GLM-5.3" — the confidence's attribution
  refLabel: string | null; // author-year from the library (citation pins only)
  refRemoved?: boolean | null; // FR-15.6: the pinned ref was removed — "source removed" flag
  verification: PinVerification | null; // latest machine verification; null = unverified
  support?: PinSupportCheck | null; // latest support check (6.9); null/absent = unchecked
}

export interface Claim {
  id: string; // the claim.registered event id
  seq: number;
  ts: string;
  hypothesisId: string;
  text: string;
  sourceMessageId: string | null; // optional assistant-message provenance
  pinned: boolean; // FR-3.4: false until an evidence.pinned event lands
  pin: EvidencePin | null;
}

// Proposals / quarantine (event-sourced read model, Story 2.2, AD-3/AD-13):
// an agent-intended change, EXCLUDED from projections until a human merges
// it. The lifecycle is pending → merged | rejected | superseded | voided —
// a decided proposal can never be decided again.
export type ProposalStatus = "pending" | "merged" | "rejected" | "superseded" | "voided";

// The receipt stamp of the event that decided a proposal.
export interface ProposalDecision {
  seq: number;
  ts: string;
  actor: string; // "user" | "agent:<run_id>" | "system:<component>"
}

// The proposed change — closed vocabulary v1: hypothesis status transitions
// (the intended event's payload, already validated by its own constructor),
// and evidence pins (Story 3.4, FR-11.5): a fetched job result proposes a
// numerical pin candidate (artifact_ref + sha-256 digest computed at
// proposal time, AD-5) awaiting merge.
export interface ProposedTransition {
  from: HypothesisStatus;
  to: HypothesisStatus;
  basis: string; // the agent's justification for the change
}

// The intended numerical evidence.pinned payload of a result proposal
// (Story 3.4, AD-5) — snake_case on the wire (the log's payload convention).
export interface ProposedPin {
  claim_id: string;
  hypothesis_id: string;
  kind: "citation" | "numerical";
  ref_id?: string | null;
  artifact_ref?: string | null; // the artifact the pin anchors to (numerical)
  excerpt: string; // the pinned content (the captured output)
  digest: string; // sha-256 of the excerpt — computed at proposal time
  confidence: number; // agent-assessed, attributed (FR-3.6)
  assessing_model: string;
}

export interface Proposal {
  id: string; // the proposal.created event id
  seq: number;
  ts: string;
  runId: string; // the proposing agent run
  missionId: string | null; // the target hypothesis's mission; null for unknown targets
  targetEntity: string; // the hypothesis the proposal intends to change
  targetLabel: string | null; // the hypothesis statement — the card's label
  targetSeq: number | null; // the H-n label seq
  proposedKind: string; // "hypothesis.status_changed" | "evidence.pinned"
  proposedPayload: ProposedTransition | ProposedPin; // discriminated by proposedKind
  basisSeq: number; // the seq of the entity state the proposal derives from (AD-13)
  basisStale: boolean; // pending: entity advanced past the basis (warning variant); merged: the forced-past-stale marker
  status: ProposalStatus;
  decided: ProposalDecision | null;
  supersededBy: string | null; // the proposal whose merge superseded this one
  // Story 2.6 (AD-1): true when a rollback orphaned this proposal's creation
  // — it renders as superseded history in the quarantine view (never hidden,
  // EXPERIENCE.md) and can never be merged. rolledBackSeq names the
  // rollback event (the "superseded by rollback e-{seq}" stamp).
  orphanedByRollback?: boolean;
  rolledBackSeq?: number;
}

// What a merge produced: the merged proposal and the pending siblings the
// merge superseded (surfaced so nothing disappears quietly).
export interface ApproveOutcome {
  proposal: Proposal;
  superseded: Proposal[];
}

// Morning Digest (event-sourced read model, Story 2.3, FR-4.4): a pure
// projection over the last night's runs — the Night Shift result, skimmable
// in ninety seconds. Delivered even when runs failed (FR-4.3): a failed run
// is an honest row carrying its reason, never a missing one.

// The run-outcome badge of the digest header: everything finished, some
// failed, everything failed, or nothing ran.
export type DigestOutcome = "no_runs" | "all_finished" | "partial_success" | "all_failed";

// One digest row: one mission's night, structured so the view composes its
// bilingual one-line verdict (the row renders at most two lines — the
// verdict plus its translation).
export interface DigestRow {
  missionId: string;
  missionSeq: number; // the M-n label derives from it
  question: string;
  status: MissionStatus; // the lifecycle chip
  runs: number; // runs in the window
  finished: number;
  failed: number;
  failureReason: string | null; // code-form reason when a run failed (FR-4.3)
  ceilingReached: boolean; // spend at the ceiling — Story 2.4's seam
  proposalsPending: number; // quarantined proposals awaiting review (FR-4.2)
  spendCents: number; // the mission's folded spend vs ceiling
  ceilingCents: number;
  receiptSeq: number; // the latest run.started seq — the receipts link
  runId: string; // the latest run's id (Story 2.5) — the receipt drill-down's target
  lastRunTs: string; // ISO-8601 UTC
  // Remote job completions in the window (Story 3.4, FR-11.5): the digest
  // reports them with one-line verdicts — a jobs-only night still earns its
  // row.
  jobsFinished: number;
  jobsFailed: number;
  jobVerdict: DigestJobVerdict | null; // the latest completed job's verdict
  // Support checks the night swept (Story 6.10, FR-23.3): the one-line
  // verdict names both counts — the unsupported count never buried.
  supportChecks: number;
  supportUnsupported: number;
}

// One remote job completion's verdict (Story 3.4): which target, which job,
// finished or failed (with the reason — no job ends silently, AD-12).
export interface DigestJobVerdict {
  target: string; // the named compute target
  jobId: string; // the job's id — rendered short
  failed: boolean;
  reason?: string | null; // code-form reason (present iff failed)
}

// The dead-man-switch alert row (FR-9.1 hook): a run that died with a stale
// heartbeat. Rendered distinct from (and counted separately of) the rows.
export interface DigestAlert {
  runId: string;
  missionId: string; // the receipts link's anchor
  missionSeq: number;
  heartbeatTs: string; // the heartbeat the run died at
  receiptSeq: number;
}

export interface MorningDigest {
  generatedAt: string; // ISO-8601 UTC
  outcome: DigestOutcome;
  spendCents: number; // aggregate of the rows' folded spend
  ceilingCents: number;
  rows: DigestRow[]; // <= 10, newest night first
  alerts: DigestAlert[]; // dead-run alert rows
  connectionAlerts: ConnectionAlert[]; // research connections down (Story 2.6, FR-9.1)
}

// A connection alert row (Story 2.6, FR-9.1): a research connection (Zotero,
// arXiv, Semantic Scholar) whose latest state is down — rendered with label
// + icon, never color alone.
export interface ConnectionAlert {
  connection: string;
  errorCode: string; // code-form reason (bilingual-safe)
  failedTs: string; // ISO-8601 UTC
}

// Onboarding — the sixty-second first value (Story 1.9, FR-8.1): one pasted
// arXiv URL (or one library ref through the Zotero connector stub) becomes a
// starter mission plus three hypothesis candidates, through the provider
// layer (the simulated fallback fires when no key is configured).

// The generation receipt (mono, honesty): who generated, with which model,
// and what it cost. Simulated generation says so and costs nothing.
export interface GenerationReceipt {
  provider: string; // spend-attribution name, e.g. "openrouter" | "simulated"
  model: string; // the assessing model the candidates' confidence is attributed to
  simulated: boolean; // true when the simulated fallback answered (no key)
  costCents: number; // 0 for simulated/CLI
}

// The paper the flow worked from — the library ref id above all (the paste
// upserted it; the Zotero path matched it).
export interface PaperInfo {
  refId: string;
  arxivId: string;
  title: string;
  authors: string;
  year: number | null;
  url: string;
}

// One hypothesis candidate: the created hypothesis (its board identity —
// candidates ARE proposed hypotheses) plus the generation-time confidence,
// attributed to the assessing model (Story 1.7 conventions).
export interface HypothesisCandidate {
  hypothesisId: string;
  seq: number; // creation event seq — the H-n label derives from it
  statement: string;
  status: HypothesisStatus; // always "proposed" at generation time
  confidence: number; // 0.0–1.0, the generator's self-assessed score
  assessingModel: string;
}

// Everything the result moment renders: the receipt, the paper, the starter
// mission, and the candidates.
export interface FirstValueResult {
  receipt: GenerationReceipt;
  paper: PaperInfo;
  mission: Mission;
  candidates: HypothesisCandidate[];
}

// ---- Trust center (Story 2.4, FR-5): dials, ceilings, kill switch ----

// The runtime's kill state (AD-15e): while "killed" is the latest
// runtime-state event by seq, every dispatch is refused.
export type RuntimeState = "running" | "killed";

// One scope's autonomy dial setting: the mission id or target name (null for
// the global scope).
export interface ScopeDial {
  scopeId: string | null;
  mode: Autonomy;
}

// One scope's spend ceiling setting, in cents.
export interface ScopeCeiling {
  scopeId: string | null;
  ceilingCents: number;
}

// One mission's spend meter: current spend vs the effective (most
// restrictive) ceiling.
export interface MissionMeter {
  missionId: string;
  question: string;
  spendCents: number;
  ceilingCents: number;
  state: SpendState;
  dial: Autonomy; // the mission's effective dial (creation ∩ configured)
}

// One compute target's spend meter (null ceiling = unbounded).
export interface TargetMeter {
  target: string;
  spendCents: number;
  ceilingCents: number | null;
  dial: Autonomy | null; // the target's configured dial (null = unconfigured)
}

// The last run's spend line ("last run: 82¢ of 100¢").
export interface LastRunSpend {
  runId: string;
  spendCents: number;
  ceilingCents: number | null;
}

// Everything the trust center renders (FR-5): runtime state, effective dials
// and ceilings at every scope, spend vs ceiling per scope, and the last
// run's spend line — the read model over the trust events.
export interface TrustStatus {
  runtimeState: RuntimeState;
  killedSeq: number | null;
  globalAutonomy: Autonomy | null;
  missionDials: ScopeDial[];
  targetDials: ScopeDial[];
  globalCeilingCents: number | null;
  missionCeilings: ScopeCeiling[];
  targetCeilings: ScopeCeiling[];
  globalSpendCents: number;
  missions: MissionMeter[];
  targets: TargetMeter[];
  lastRun: LastRunSpend | null;
  // The research connections' health (Story 2.6, FR-9.1): the trust center's
  // health line — latest failure per connection, label + icon, never color
  // alone. Empty until a connection has been probed.
  connections?: ConnectionHealth[];
}

// One research connection's health as read from the log (Story 2.6): up /
// down with the latest failure's code + timestamp and the latest restore.
export interface ConnectionHealth {
  connection: string;
  up: boolean;
  lastErrorCode?: string | null;
  lastErrorTs?: string | null;
  lastRestoredTs?: string | null;
}

// ---- Run Timeline Receipts (Story 2.5, FR-6): the drill-down audit ledger ----

// The run's terminal state for the outcome chip.
export type RunOutcome = "finished" | "failed" | "open";

// The ledger row vocabulary (the receipt frame's chips): run start, search,
// provider call, claim, merge proposal (quarantined), the human's decision
// on one, the honest ceiling refusal, a released reservation, the run's end.
// The `kind` tag discriminates the wire form; every row carries its audit
// seq (the e-{seq} ref) and timestamp.
export type ReceiptRow =
  | { seq: number; ts: string; kind: "run_start"; step: string; schedule: string }
  | { seq: number; ts: string; kind: "search"; query: string }
  | {
      seq: number;
      ts: string;
      kind: "call";
      provider: string;
      model: string;
      role?: string;
      inputTokens: number;
      outputTokens: number;
      costCents: number;
    }
  | { seq: number; ts: string; kind: "claim"; text: string }
  | {
      seq: number;
      ts: string;
      kind: "proposal";
      proposalId: string;
      proposalSeq: number; // the pr-n label
      to: string; // the transition the proposal intends
      status: string; // pending | merged | rejected | superseded | voided
    }
  | {
      seq: number;
      ts: string;
      kind: "decision";
      proposalId: string;
      proposalSeq: number;
      decision: string; // merged | rejected | superseded
    }
  | {
      seq: number;
      ts: string;
      kind: "refused"; // the honest "dispatch refused" alert row
      scope: string;
      ceilingCents: number;
      wouldBeCostCents: number;
    }
  | { seq: number; ts: string; kind: "released"; reason: string }
  | {
      seq: number;
      ts: string;
      kind: "run_end";
      outcome: RunOutcome;
      reason?: string | null;
      verdict?: string | null;
    };

// One run's receipt (FR-6.1): the meta header — outcome chip, duration,
// spend vs ceiling ("82¢ of 100¢"), the models it called — plus the ordered
// ledger. A pure projection (AD-2): re-querying replays the identical
// ledger. Reachable ONLY as drill-down from missions and the digest
// (FR-6.2) — never a parallel surface.
export interface RunReceipt {
  runId: string;
  missionId: string;
  missionSeq: number; // the M-n breadcrumb label
  outcome: RunOutcome;
  ceilingHit: boolean; // a ceiling refused one of the run's dispatches
  reason: string | null; // code-form failure reason
  verdict: string | null; // the finished run's one-liner
  startedTs: string;
  endedTs: string | null;
  durationSecs: number | null;
  spendCents: number;
  ceilingCents: number;
  models: string[]; // the models the run called, first-call order
  rows: ReceiptRow[]; // seq order — the run's every logged atom
}

// ---- Search protocol disclosure (Story 4.1, FR-12.1: PRISMA-style) ----

// One disclosure row: the PRISMA record of one search the app performed —
// query, database, filters, date, result count. Null results are rows
// (`resultCount: 0`, `nullResult: true`), visibly marked, never hidden —
// a search that found nothing still happened and still discloses.
export interface SearchDisclosureRow {
  seq: number; // the search.run event's seq (the e-{seq} ref)
  startedAt: string; // ISO-8601 UTC — the event envelope's ts
  query: string;
  database: string; // e.g. "arxiv", "semantic-scholar", "zotero", "web"
  filters: Record<string, unknown>; // date ranges, venues — JSON map
  order: string | null; // e.g. "relevance", "date-desc"
  firstPage: boolean; // the PRISMA first-pass marker
  resultCount: number;
  nullResult: boolean; // derived: resultCount === 0
  missionId: string | null;
  runId: string | null; // the agent run that searched, when run-scoped
}

// The disclosure of one scope (a mission, or the whole workspace): every
// search in seq order. `nullResultCount` is the seam the readiness gate
// (Story 4.3) counts as undisclosed null results.
export interface SearchDisclosure {
  rows: SearchDisclosureRow[];
  total: number;
  nullResultCount: number;
}

// One result the search returned (the v1 simulated corpus).
export interface SearchResult {
  title: string;
  authors: string;
  year: number;
  venue: string;
  url: string;
}

// What run_search returns: the results plus the disclosure row that
// records what ran (the row IS the log's record of the search).
export interface SearchRunView {
  results: SearchResult[];
  row: SearchDisclosureRow;
}

// ---- Checkpoints & rollback (Story 2.6, FR-10.1, AD-1) ----

// One restore point as read from the log: the user named the log head at
// creation; rolling back returns the read model to that seq.
export interface Checkpoint {
  id: string; // the checkpoint.created event id
  seq: number; // the log head at creation — where a rollback returns to
  ts: string; // ISO-8601 UTC
  name: string;
}

// One rollback in the log — the history the checkpoint control lists under
// its restore points (never hidden, EXPERIENCE.md).
export interface RollbackRecord {
  seq: number; // the checkpoint.rolled_back event's seq
  ts: string;
  checkpointId: string;
  name: string;
  targetSeq: number;
  orphanedCount: number;
}

// Everything the checkpoint control renders (FR-10.1): the current head,
// the restore points, and the rollback history.
export interface CheckpointsView {
  headSeq: number;
  checkpoints: Checkpoint[];
  rollbacks: RollbackRecord[];
}

// One orphaned event, as the rollback confirmation lists it — the superseded
// history (never hidden, never summarized away).
export interface OrphanedEvent {
  seq: number;
  ts: string;
  kind: string; // raw event kind, rendered in mono (receipt voice)
  actor: string; // "user" | "agent" | "system:<component>"
}

// One orphaned PROPOSAL, by name — the confirmation's non-negotiable content
// (EXPERIENCE.md: the rollback confirmation names every orphaned proposal).
export interface OrphanedProposal {
  proposalId: string;
  seq: number;
  targetLabel: string | null; // the target hypothesis's statement — the NAME
  targetSeq: number | null; // the H-n label seq
  proposedTo: string | null; // the transition the proposal intended
  basis: string | null; // the agent's one-line justification
}

// What a rollback WOULD orphan right now — the confirmation dialog's read.
export interface RollbackPlan {
  checkpoint: Checkpoint;
  orphanedEvents: OrphanedEvent[];
  orphanedProposals: OrphanedProposal[];
}

// What a rollback DID orphan — the outcome the post-rollback view renders.
export interface RollbackOutcome {
  rollback: RollbackRecord;
  orphanedEvents: OrphanedEvent[];
  orphanedProposals: OrphanedProposal[];
}

// --- Open export (Story 3.1, FR-7.1/7.2, AD-11) -------------------------

// Why the previous export at a folder is stale: its cut, and the rollback
// appended after it that changed what that cut means (both cuts render in
// mono on the manifest, EXPERIENCE.md).
export interface StaleNotice {
  previousCut: number;
  rollbackSeq: number;
  rollbackTs: string;
}

// The parsed export manifest — the summary the result moment renders.
export interface ExportManifest {
  cutSeq: number; // the single named seq cut every file rendered at
  cutTs: string | null; // null for the empty log's e-0 beginning
  renderedTs: string;
  scope: string;
  appVersion: string;
  fileCount: number;
  staleNotice: StaleNotice | null; // set when this render supersedes a stale export
}

// What export_workspace did: where it wrote, the manifest summary, and
// every file it rendered (paths relative to the folder).
export interface ExportOutcome {
  dir: string;
  manifest: ExportManifest;
  files: string[];
}

// The "at open" read of an existing export folder: the cut its manifest
// records, and whether a rollback after that cut has staled it (Story
// 2.6's signal) — the composer's stale-warning state.
export interface ExportInspect {
  cutSeq: number;
  stale: boolean;
  rollbackSeq: number | null;
}

// --- Compute targets + jobs (Story 3.2, FR-11.1/11.2/11.4, AD-6) ---------

// The resource request a spec may carry (optional; Local records but cannot
// enforce it — no cgroups on a laptop; the SSH adapter forwards it).
export interface JobResources {
  cpus?: number;
  memoryMb?: number;
}

// The structured job spec (AD-6): typed fields ONLY — the composer renders
// them as a live JSON preview and never offers a freeform shell box
// (EXPERIENCE.md). cmd is a single executable: shell syntax is rejected
// before submit, on both sides of the wire.
export interface JobSpec {
  cmd: string;
  args: string[];
  env: Record<string, string>;
  resources?: JobResources | null;
  workdir?: string | null;
}

// The job lifecycle the mission card renders (FR-11.4): queued → running →
// terminal; every terminal carries a timestamp and (on failure) a reason.
export type JobPhase = "queued" | "running" | "finished" | "failed";

// A compute job as read from the log — the mission card's jobs area.
export interface Job {
  id: string; // the job.submitted event id
  seq: number;
  ts: string; // when the job was submitted
  missionId: string;
  target: string; // the named compute target ("local" | a declared name)
  handle: string; // the target adapter's process handle (fetch addresses it)
  spec: JobSpec;
  phase: JobPhase;
  exitCode?: number | null;
  reason?: string | null; // why a failed job failed (spawn error, exit code, signal)
  runningTs?: string | null;
  finishedTs?: string | null; // the terminal stamp — always present once terminal
}

// A fetched job's captured results (FR-11.5's fetch; the quarantine flow
// is Story 3.4 — here they surface on the job row).
export interface JobResult {
  code: number | null;
  stdout: string;
  stderr: string;
}

// What fetch_job_results produced (Story 3.4, FR-11.5): the captured output
// and the quarantined proposals it landed as — one per meaningful artifact,
// each a numerical pin candidate awaiting merge (AD-3; auto-pinning is
// v0.2.0). `created` is 0 on an idempotent re-fetch.
export interface FetchedJobResults {
  job: Job;
  results: JobResult;
  proposals: Proposal[];
  created: number;
}

// One compute target as the mission card's target row renders it: a name
// and the adapter kind behind it ("local" | "ssh" | "scheduler" |
// "kubernetes" | "chopflow"), plus — for ssh/scheduler targets (Stories
// 3.3, 6.2) — the host it connects to and whether that gate value is on
// the workspace allowlist, and — for the v0.2.0 kinds (Stories 6.2–6.4)
// — the per-kind config map.
export interface ComputeTargetView {
  name: string;
  kind: string;
  host?: string | null; // the host an ssh/scheduler target connects to
  allowlisted?: boolean | null; // the gate value on the allowlist? (null when no gate)
  config?: Record<string, string>; // per-kind settings (flavor/prefixes, context/ns/image, endpoint/queue)
  builtin: boolean; // the built-in "local" needs no target.declared event
  seq?: number | null;
  ts?: string | null;
}

// One registered adapter kind (Story 6.5, NFR-15): what the settings row
// lists — community adapters appear exactly as first-party ones do.
export interface RegisteredAdapter {
  kind: string;
  contractVersion: string; // the adapter contract version it was validated against
  builtin: boolean; // ships with ResearchCore vs. registered from outside
}

// A target probe's outcome (the settings row's discovery/unreachable
// state, per adapter — Story 6.2's probe): `ok` carries the adapter's own
// detail line, `unreachable` its typed reason, `unsupported` for kinds
// with no probe.
export interface TargetProbe {
  status: "ok" | "unreachable" | "unsupported";
  detail: string;
}

// ---- Readiness gate (Story 4.3, FR-13.1/13.2 — the final PRD story) ----

// The gate's verdict: derived, never stored. `ready` renders
// "preprint-ready"; `not_ready` renders its blockers. The three-state
// presentation (not ready / near / ready) derives on the read side from
// blockers + infos: near = 0 blockers but ≥1 advisory info row — states,
// never scores (FR-13.1).
export type ReadinessVerdict = "ready" | "not_ready";

// One blocking or advisory item. Every item references EXACTLY ONE specific
// board object (FR-13.1): the claim (CLAIMS-{seq}), the hypothesis (H-{seq}
// with its lifecycle + the ties that make it load-bearing), or the search
// log row (#{seq} — the search.run event IS the disclosure, FR-12.1).
export type ReadinessItemKind =
  | "unpinned_claim" // a claim with no evidence.pinned event (FR-3.4)
  | "load_bearing_unresolved" // things rest on it; still proposed/testing
  | "load_bearing_refuted" // things rest on it; refuted
  | "unreckoned_null_result" // null search; its mission still unresolved
  | "pin_verification_failed" // INFO: pinned, but the machine check failed
  // INFO (Story 6.10, FR-23.3): the gate consumes the THIRD signal —
  // support verdicts, never blockers (the pin stays; honesty, not amnesia).
  | "pin_unsupported" // support says the citing source does not hold the claim up
  | "pin_partially_supported" // support says the claim asserts more than its source
  | "support_unchecked" // unverified support after N sweeps — visibly to-verify
  | "merge_queue_pending" // INFO: quarantine is not board state (AD-3)
  // manuscript scope (Story 6.8, FR-20.5): the paper cannot quietly outrun
  // the evidence — each flag references the hypothesis card AND the
  // manuscript location (file + line + marker)
  | "manuscript_hypothesis_unresolved" // \hyp cites a testing/proposed hypothesis
  | "manuscript_hypothesis_refuted" // \hyp cites a refuted hypothesis
  | "manuscript_claim_unpinned" // \claim cites a claim without an evidence pin
  | "manuscript_claim_unlinked" // a marker that resolves to no board object
  | "manuscript_unreadable"; // INFO: the manuscript dir could not be read (the check could not run)

// One typed relation tie on a load-blocking hypothesis: the kind, the other
// endpoint's H-{n}, and whether it reads incoming ("contradicted-by H-3")
// or outgoing ("contradicts H-3") on this hypothesis.
export interface ReadinessRelationTie {
  kind: RelationKind;
  otherSeq: number;
  incoming: boolean;
}

export interface ReadinessItem {
  kind: ReadinessItemKind;
  claimId: string | null; // the blocking claim's id (CLAIMS-{claimSeq})
  claimSeq: number | null;
  hypothesisId: string | null; // the load-bearing hypothesis (H-{hypothesisSeq})
  hypothesisSeq: number | null;
  hypothesisStatus: HypothesisStatus | null;
  searchSeq: number | null; // the null search's log row (#{searchSeq})
  missionId: string | null;
  claimTies: number[]; // CLAIMS-{n} resting on the hypothesis
  relationTies: ReadinessRelationTie[];
  pendingCount: number; // the merge-queue info row's count
  // manuscript scope (Story 6.8): the flag's manuscript location — the
  // file (repo-relative), the 1-based line, and the raw marker text
  manuscriptFile?: string | null;
  manuscriptLine?: number | null;
  marker?: string | null;
}

// One evidence-trail row — the clean board's justification: what was
// checked, the counts, and the objects that satisfied it (EXPERIENCE.md
// Flow 2: "every cleared item still listed beside the object that
// satisfied it"). Four rows, matching the readiness frame.
export type ReadinessTrailKind =
  | "claims_pinned" // N/N claims pinned (+ how many machine-verified)
  | "hypotheses_resolved" // N/N resolved (supported or refuted)
  | "nulls_disclosed" // N/N null-result searches reckoned with
  | "merge_queue" // pending count; cites the last decided proposal when clean
  // manuscript scope (Story 6.8): N/N board markers linked clean — present
  // only when a manuscript is registered (progressive disclosure)
  | "manuscript_consistency";

export interface ReadinessTrailRow {
  kind: ReadinessTrailKind;
  total: number; // what the check covered (0 for the merge-queue row)
  clean: number; // what passed (0 for the merge-queue row)
  verified: number; // claims row only: pins with a verified machine check
  pending: number; // merge-queue row only: pending proposals in scope
  refs: string[]; // the objects the row cites: "CLAIMS-2", "H-4", "#12", "pr-1042"
}

// The readiness report of one scope (a mission, or the whole workspace):
// a PURE derived view over the log — blockers, advisory infos, and the
// trail. No readiness state exists anywhere; asking again re-folds.
export interface ReadinessReport {
  scope: string | null; // the mission id the report ran for; null = workspace
  verdict: ReadinessVerdict;
  blockers: ReadinessItem[];
  infos: ReadinessItem[];
  trail: ReadinessTrailRow[];
}

// The dashboard's aggregated read (Story 5.10, FR-18.1): every widget's
// data in ONE fold over the log — a transport envelope over the existing
// read models (missions, boards, digest, trust, receipts, readiness),
// never a new domain model. A dashboard render never appends events.
export interface DashboardSummary {
  missions: Mission[]; // widget 1 (FR-18.2): status counts + active shortlist
  hypotheses: Hypothesis[]; // widget 2 (FR-18.3): every board, across missions
  digest: MorningDigest; // widget 3 (FR-18.4): the digest teaser's source
  trust: TrustStatus; // widget 4 (FR-18.5): spend-vs-ceiling meters
  recentReceipts: RunReceipt[]; // widget 5 (FR-18.6): last N runs, newest first
  readiness: ReadinessReport; // widget 6 (FR-18.7): workspace-scoped verdict
}

// The AI provider configuration (Stories 5.7–5.9 + 6.1, FR-17/FR-24): what
// Ajustes → IA and the assistant surface render. The key itself NEVER
// crosses this boundary — only its presence (NFR-10: keychain-only
// credentials).
export interface AiConfig {
  mode: string; // "" | "simulate" | "cli" | "provider" | "local"
  provider: string; // openai | anthropic | google | openrouter | custom | local | …
  baseUrl: string;
  model: string;
  hasKey: boolean;
  cli: string; // the CLI bridge binary ("claude" when unset)
  cliModel: string;
  cliAvailable: Record<string, { path: string } | null>; // honest per-binary detection
  local: AiLocalStatus; // the local provider's honest detection (Story 6.1)
  models: string[]; // the picker's list (curated; [] = free entry; CLI = ["default"]; local = live /api/tags)
  configured: boolean; // a REAL provider is configured — the assistant's gate (a reachable local endpoint counts, FR-24.2)
}

// The local provider's honest detection status (Story 6.1, FR-24.1/NFR-14):
// reachable + its installed models, or the honest unreachable reason —
// never a dead spawn, never an invented list.
export interface AiLocalStatus {
  baseUrl: string; // the configured local endpoint (Ollama default when unset)
  reachable: boolean;
  models: string[]; // the live /api/tags list — flows into every picker
  error: string | null;
}

// One test-connection run (Story 5.7): ok + the live model list (the
// picker refreshes from it), or the honest error string.
export interface AiConnectionTest {
  ok: boolean;
  models: string[];
  error: string | null;
}

// ---- Manuscript (Stories 6.6–6.8, FR-20) ----
// The .tex repo IS the manuscript (FR-20.1): a mission registers a .tex
// project directory on disk; the event log references it, never copies it.

// One mission's registered manuscript: the registration receipt (seq/ts),
// the ABSOLUTE .tex project dir on disk, and the repo-relative main file
// to compile.
export interface Manuscript {
  missionId: string;
  seq: number;
  ts: string;
  dir: string;
  mainFile: string;
}

// One .tex source as the file list renders it: the repo-relative path,
// the word count, and the board-link marker counts (`\hyp{…}` / `\claim{…}`
// — the FR-20.4 convention agent diffs insert).
export interface TexFileScan {
  path: string;
  words: number;
  hypMarkers: number;
  claimMarkers: number;
  bytes: number;
}

// The detected LaTeX toolchain (FR-20.2): null is the honest
// `tex_not_found` state — never a fake render (NFR-9).
export interface ToolchainView {
  name: string;
  path: string;
  configured: boolean; // the tex_compiler setting named it (vs. the search)
}

// The compile outcome: "ok" produced a PDF, "error" ran and failed (the
// log tail is the receipt), "tex_not_found" is the honest missing
// toolchain state with the install hint.
export type CompileOutcome = "ok" | "error" | "tex_not_found";

export interface CompileView {
  seq: number;
  ts: string;
  outcome: CompileOutcome;
  tool: string | null;
  logTail: string;
  pdfUrl: string | null; // the served PDF url (the side pane's <object>)
}

// The manuscript surface's one read (progressive disclosure: the surface
// renders only when one exists — null until registered).
export interface ManuscriptView {
  manuscript: Manuscript;
  files: TexFileScan[];
  toolchain: ToolchainView | null;
  lastCompile: CompileView | null;
}

// One manuscript file's content (the editing surface's read/write).
export interface ManuscriptFileView {
  path: string;
  content: string;
}

// One before/after hunk of an agent's proposed LaTeX edit (Story 6.7,
// FR-20.3): the exact text replaced and the text replacing it — never a
// freeform overwrite.
export interface ManuscriptDiffHunk {
  before: string;
  after: string;
}

// One quarantined LaTeX diff (Story 6.7): an agent-actor proposal excluded
// from the manuscript until the human merges it (AD-3). The basis carries
// the file's content digest + the log seq it derived from; a merge whose
// file has advanced past its basis is refused with `basis_stale:` unless
// force-approved (the marker is recorded and surfaced).
export interface ManuscriptDiffProposal {
  id: string;
  seq: number;
  ts: string;
  runId: string;
  missionId: string;
  file: string;
  hunks: ManuscriptDiffHunk[];
  basisDigest: string;
  basisSeq: number;
  basisStale: boolean; // derived: the file changed since the proposal
  status: "pending" | "merged" | "rejected";
  decided: { seq: number; ts: string; actor: string } | null;
  backupPath: string | null; // the pre-merge file backup (merge safety)
  note: string;
}

// The bridge status read (Story 6.14, FR-21.1): the ONE channel's state —
// mode (off | tunnel | chopflow), what it listens on / talks to, and the
// paired-devices count.
export interface BridgeStatusView {
  mode: string; // "off" | "tunnel" | "chopflow"
  active: boolean;
  describe: string | null;
  pairedDevices: number;
}

// A paired device as read from the pairing ledger (Story 6.14): the audit
// fold of bridge.device_paired / bridge.device_unpaired events.
export interface PairedDevice {
  device: string;
  fingerprint: string; // first 8 hex of the token's sha-256 — recognizable, never authenticating
  pairedSeq: number;
  pairedTs: string;
}

// What pairing hands the owner ONCE (Story 6.14): the raw token is shown
// one time and never stored anywhere.
export interface PairingReceipt {
  device: string;
  token: string;
  fingerprint: string;
}

// A verdict-summary notification (Story 6.16, FR-21.4, NFR-13): summaries
// in code form only — never research content beyond the summary.
export interface NotificationItem {
  kind: string; // "proposal" | "digest"
  seq: number;
  ts: string;
  proposalId: string | null;
  summary: string;
  basisStale: boolean;
}
