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

export interface Mission {
  id: string; // the mission.created event id
  seq: number; // store-assigned seq of the creation event
  ts: string; // ISO-8601 UTC
  question: string;
  stopCondition: string;
  successCriterion: string;
  autonomy: Autonomy;
  spendCeilingCents: number;
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
