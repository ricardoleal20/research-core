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
  created_at: string;
  updated_at: string;
  messages?: Message[];
}

export interface Message {
  id: string;
  chat_id: string;
  role: string; // user | agent
  content: string;
  classify_tag: string | null;
  meta: string | null;
  created_at: string;
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

export type MissionStatus = "active" | "awaiting_review" | "completed" | "stopped" | "failed";

// DESIGN.md components.spend-meter: ok under 80% of ceiling, near at 80%+,
// blocked at/over the hard ceiling (dispatch refused past it, AD-10).
export type SpendState = "ok" | "near" | "blocked";

// One agent role of a mission's runtime config (Story 2.1, NFR-3): a named
// role bound to a (provider, model) pair — the core rejects a config whose
// critic shares a drafter's pair (same_model_critic).
export type AgentRoleName = "drafter" | "critic";

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
// (the intended event's payload, already validated by its own constructor).
export interface ProposedTransition {
  from: HypothesisStatus;
  to: HypothesisStatus;
  basis: string; // the agent's justification for the change
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
  proposedKind: string; // "hypothesis.status_changed"
  proposedPayload: ProposedTransition;
  basisSeq: number; // the seq of the entity state the proposal derives from (AD-13)
  basisStale: boolean; // pending: entity advanced past the basis (warning variant); merged: the forced-past-stale marker
  status: ProposalStatus;
  decided: ProposalDecision | null;
  supersededBy: string | null; // the proposal whose merge superseded this one
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
  lastRunTs: string; // ISO-8601 UTC
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
