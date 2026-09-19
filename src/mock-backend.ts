// In-memory mock backend. Activates only when the Tauri runtime is absent
// (i.e. the app is served by the Vite dev server in a plain browser). This
// lets the UI boot and be iterated on without the Rust backend. All state is
// ephemeral and resets on reload.
//
// When Tauri is present (real app or `tauri dev`), this module is never used —
// api.ts routes to the real `invoke` calls instead.

import type { Project, Ref, Review, Action, Chat, Agent, McpServer, Message, Mission, MissionRun, Autonomy, Hypothesis, HypothesisStatus, RelationKind, Claim, FirstValueResult, HypothesisCandidate, RoleConfig, AgentStepResult, Proposal, ApproveOutcome, MorningDigest, DigestRow, TrustStatus, RuntimeState, SpendState, ScopeDial, ScopeCeiling, MissionMeter, TargetMeter, LastRunSpend, RunReceipt, ReceiptRow, Checkpoint, CheckpointsView, RollbackPlan, RollbackOutcome, OrphanedEvent, OrphanedProposal, RollbackRecord } from "./types";

const isTauri =
  typeof window !== "undefined" &&
  (!!((window as any).__TAURI_INTERNALS__) || !!((window as any).__TAURI__));

export const mockActive = !isTauri;

const settings: Record<string, string> = {
  onboarded: "true",
  onboarding_complete: "true",
  tutorial_seen: "true",
  user_name: "Ricardo",
  lang: "es",
  // IA provider
  provider: "openai-compatible",
  model: "gpt-4o-mini",
  base_url: "https://openrouter.ai/api/v1",
  api_key: "sk-mock-key-1234",
  agent_path: "/usr/local/bin/claude",
  // security
  lock_policy: "never",
  lock_idle_min: "15",
  // local-first toggles
  local_first: "true",
  sync_zotero: "false",
  cache_pdfs: "true",
};

const project: Project = {
  id: "p1",
  name: "Optimización de Modelos de Atención",
  folder: "~/research/atencion",
  kind: "thesis-chapter",
  tags: "transformer, attention, NLP",
  color: "#3B5BDB",
  chapter_index: 2,
  chapter_count: 6,
  created_at: "2025-08-01T10:00:00Z",
  updated_at: "2025-09-01T12:00:00Z",
  is_active: 1,
};

const servers: McpServer[] = [
  { id: "m1", name: "arxiv-mcp", transport: "stdio", command: "npx", args: "-y arxiv-mcp", env: "", url: "", tags: "search,papers", connected: 1, created_at: "2025-08-01T10:00:00Z" },
  { id: "m2", name: "semantic-scholar", transport: "http", command: "", args: "", env: "", url: "https://mcp.semanticscholar.org/sse", tags: "search", connected: 0, created_at: "2025-08-02T10:00:00Z" },
];

// In-memory chats + messages so the Asistente / AI Review flows work in-browser.
const chats: Chat[] = [];
const messages: Record<string, Message[]> = {};
let chatSeq = 0;
let msgSeq = 0;
const nowISO = () => "2025-09-01T12:00:00Z";

// In-memory missions so the question box → mission composer flow works in-browser.
const missions: Mission[] = [];
let missionSeq = 0;
// In-memory run lists per mission id (empty until events would reference them).
const missionRuns: Record<string, MissionRun[]> = {};

// In-memory trust state (Story 2.4, FR-5): dials, ceilings, kill switch —
// the mock the dev-browser trust center renders. Mirrors the core's folded
// read model: most-restrictive-wins across scopes, hard ceilings, the
// runtime's kill state.
const mockMissionIds = ["m1-1726732800000", "m2-1726732900000"];
const trustState: {
  runtimeState: RuntimeState;
  killedSeq: number | null;
  globalAutonomy: Autonomy | null;
  missionDials: ScopeDial[];
  targetDials: ScopeDial[];
  globalCeilingCents: number | null;
  missionCeilings: ScopeCeiling[];
  targetCeilings: ScopeCeiling[];
  globalSpendCents: number;
  missionSpend: Record<string, number>;
  targetSpend: Record<string, number>;
  lastRun: LastRunSpend | null;
} = {
  runtimeState: "running",
  killedSeq: null,
  globalAutonomy: "suggest",
  missionDials: [
    { scopeId: mockMissionIds[0], mode: "act_with_receipts" },
    { scopeId: mockMissionIds[1], mode: "watch" },
  ],
  targetDials: [{ scopeId: "cli", mode: "watch" }],
  globalCeilingCents: 2500,
  missionCeilings: [{ scopeId: mockMissionIds[0], ceilingCents: 100 }],
  targetCeilings: [{ scopeId: "openai", ceilingCents: 1500 }],
  globalSpendCents: 1462,
  missionSpend: { [mockMissionIds[0]]: 82, [mockMissionIds[1]]: 0 },
  targetSpend: { openai: 1213, anthropic: 249, cli: 0 },
  lastRun: { runId: "step-42", spendCents: 82, ceilingCents: 100 },
};

/// The mock trust read model: exactly the shape `get_trust_status` folds in
/// the core (mission meters from the seeded missions + the trust state).
function mockTrustStatus(): TrustStatus {
  const meters: MissionMeter[] = missions.length
    ? missions.map((m) => {
        const configured = trustState.missionCeilings.find((c) => c.scopeId === m.id);
        const ceiling = configured ? Math.min(configured.ceilingCents, m.spendCeilingCents) : m.spendCeilingCents;
        const spend = trustState.missionSpend[m.id] ?? m.spendCents;
        const state: SpendState = spend === 0 ? "ok" : ceiling === 0 || spend >= ceiling ? "blocked" : spend * 5 >= ceiling * 4 ? "near" : "ok";
        return { missionId: m.id, question: m.question, spendCents: spend, ceilingCents: ceiling, state, dial: trustState.missionDials.find((d) => d.scopeId === m.id)?.mode ?? m.autonomy };
      })
    : [
        // the seeded two-mission shape (before any mission is created)
        { missionId: mockMissionIds[0], question: "Does retrieval grounding reduce hallucinated citations?", spendCents: 82, ceilingCents: 100, state: "near", dial: "act_with_receipts" },
        { missionId: mockMissionIds[1], question: "Do scaling laws hold for citation density?", spendCents: 0, ceilingCents: 500, state: "ok", dial: "watch" },
      ];
  const targetNames = Array.from(new Set([
    ...trustState.targetDials.map((d) => d.scopeId!),
    ...trustState.targetCeilings.map((c) => c.scopeId!),
    ...Object.keys(trustState.targetSpend),
  ])).sort();
  const targets: TargetMeter[] = targetNames.map((target) => ({
    target,
    spendCents: trustState.targetSpend[target] ?? 0,
    ceilingCents: trustState.targetCeilings.find((c) => c.scopeId === target)?.ceilingCents ?? null,
    dial: trustState.targetDials.find((d) => d.scopeId === target)?.mode ?? null,
  }));
  return {
    runtimeState: trustState.runtimeState,
    killedSeq: trustState.killedSeq,
    globalAutonomy: trustState.globalAutonomy,
    missionDials: trustState.missionDials,
    targetDials: trustState.targetDials,
    globalCeilingCents: trustState.globalCeilingCents,
    missionCeilings: trustState.missionCeilings,
    targetCeilings: trustState.targetCeilings,
    globalSpendCents: trustState.globalSpendCents,
    missions: meters,
    targets,
    lastRun: trustState.lastRun,
    // Story 2.6 (FR-9.1): the connections health line — Zotero down (the
    // seeded outage), arXiv healed, Semantic Scholar healthy.
    connections: [
      { connection: "arxiv", up: true, lastErrorCode: "timeout", lastErrorTs: "2026-09-19T02:44:00Z", lastRestoredTs: "2026-09-19T03:02:00Z" },
      { connection: "semantic_scholar", up: true },
      { connection: "zotero", up: false, lastErrorCode: "unreachable", lastErrorTs: "2026-09-19T03:12:00Z" },
    ],
  };
}

// In-memory hypotheses so the board flows (create, transition, relate) work
// in-browser. Mirrors the event-sourced core: the FR-2.2 transition table is
// enforced here too, and every transition carries an audit stamp.
const hypotheses: Hypothesis[] = [];
let hypSeq = 0;
const allowedNext: Record<HypothesisStatus, HypothesisStatus[]> = {
  proposed: ["testing"],
  testing: ["supported", "refuted"],
  supported: ["revised"],
  refuted: ["revised"],
  revised: ["testing"],
};

// Mock library refs (mirrors the core's seeded refs) so the citation
// pin's reference picker flows in the dev browser.
const mockRefs: Ref[] = [
  { id: "r1", project_id: "p1", collection_id: null, title: "Attention Is All You Need", authors: "Vaswani et al.", year: 2017, venue: "NeurIPS", doi: "10.48550/arXiv.1706.03762", url: "https://arxiv.org/abs/1706.03762", isbn: "", attachment: null, status: "read", tags: "transformer,attention", used: 1, citation_count: 2, created_at: nowISO() },
  { id: "r2", project_id: "p1", collection_id: null, title: "Neural Machine Translation by Jointly Learning to Align and Translate", authors: "Bahdanau et al.", year: 2015, venue: "ICLR", doi: "10.48550/arXiv.1409.0473", url: "https://arxiv.org/abs/1409.0473", isbn: "", attachment: null, status: "read", tags: "attention,NLP", used: 1, citation_count: 1, created_at: nowISO() },
  { id: "r3", project_id: "p1", collection_id: null, title: "Scaling Laws for Neural Language Models", authors: "Kaplan et al.", year: 2020, venue: "arXiv", doi: "10.48550/arXiv.2001.08361", url: "https://arxiv.org/abs/2001.08361", isbn: "", attachment: null, status: "read", tags: "scaling,NLP", used: 1, citation_count: 1, created_at: nowISO() },
  { id: "r4", project_id: "p1", collection_id: null, title: "A Survey on Large Language Models", authors: "Zhao et al.", year: 2023, venue: "arXiv", doi: "10.48550/arXiv.2303.18223", url: "https://arxiv.org/abs/2303.18223", isbn: "", attachment: null, status: "unread", tags: "survey,LLM", used: 0, citation_count: 0, created_at: nowISO() },
];

/** sha-256 hex of an excerpt — the mock mirrors the core's digest so the
 *  pinned pin renders the same mono digest the desktop app shows. */
async function sha256Hex(text: string): Promise<string> {
  const data = new TextEncoder().encode(text);
  const digest = await crypto.subtle.digest("SHA-256", data);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

// In-memory claims (mirrors the event-sourced core): registered claims
// start unpinned (FR-3.4); pinning attaches the AD-5 pin shape with the
// digest computed here, never accepted from the caller.
const claims: Claim[] = [];
let claimSeq = 0;

// In-memory proposals (mirrors the event-sourced core, Story 2.2): agent
// steps emit quarantined proposals — excluded from the board until merged;
// the basis is validated at merge time (basis_stale unless forced), and a
// decided proposal can never be decided again.
const proposals: Proposal[] = [];
let proposalSeq = 0;
let mockEventSeq = 0;

/** The mock's entity-current-seq: the hypothesis's last mutation — its
 *  audit stamp seq (creation, user transition, or applied merge). */
const hypothesisCurrentSeq = (h: Hypothesis): number => h.audit.seq;

// In-memory checkpoints (mirrors the event-sourced core, Story 2.6, AD-1):
// a checkpoint snapshots the mock state at creation; rollback restores the
// snapshot and marks everything created after the checkpoint as superseded
// history — orphaned proposals read as superseded, never hidden.
interface MockSnapshot {
  missions: Mission[];
  hypotheses: Hypothesis[];
  proposals: Proposal[];
}

const seedCpTs = "2026-09-19T03:30:00Z";
const mockCheckpoints: (Checkpoint & { snapshot: MockSnapshot })[] = [
  {
    id: "cp-1-seed", seq: 90, ts: seedCpTs, name: "after onboarding",
    snapshot: { missions: [], hypotheses: [], proposals: [] },
  },
  {
    id: "cp-2-seed", seq: 120, ts: "2026-09-19T06:10:00Z", name: "pre-trial",
    snapshot: { missions: [], hypotheses: [], proposals: [] },
  },
];
const mockRollbacks: RollbackRecord[] = [
  {
    seq: 130, ts: "2026-09-19T08:02:00Z", checkpointId: "cp-1-seed",
    name: "after onboarding", targetSeq: 90, orphanedCount: 3,
  },
];

const cloneSnapshot = (): MockSnapshot => ({
  missions: missions.map((m) => ({ ...m })),
  hypotheses: hypotheses.map((h) => ({ ...h, relations: h.relations.map((r) => ({ ...r })), audit: { ...h.audit } })),
  proposals: proposals.map((p) => ({ ...p, proposedPayload: { ...p.proposedPayload }, decided: p.decided ? { ...p.decided } : null })),
});

const restoreSnapshot = (snap: MockSnapshot): void => {
  missions.length = 0;
  missions.push(...snap.missions);
  hypotheses.length = 0;
  hypotheses.push(...snap.hypotheses);
  proposals.length = 0;
  proposals.push(...snap.proposals);
};

/** The mock's orphaned-listing for one checkpoint: what a rollback would
 *  orphan — every proposal created after it, by name. */
const orphanedFor = (cp: Checkpoint): { events: OrphanedEvent[]; proposals: OrphanedProposal[] } => {
  const events: OrphanedEvent[] = [];
  const orphans: OrphanedProposal[] = [];
  for (const h of hypotheses) {
    if (h.seq > cp.seq && h.audit.seq > cp.seq) {
      events.push({ seq: h.seq, ts: h.ts, kind: "hypothesis.created", actor: "user" });
    }
  }
  for (const p of proposals) {
    if (p.seq > cp.seq && !p.orphanedByRollback) {
      events.push({ seq: p.seq, ts: p.ts, kind: "proposal.created", actor: "agent" });
      orphans.push({
        proposalId: p.id, seq: p.seq,
        targetLabel: p.targetLabel, targetSeq: p.targetSeq,
        proposedTo: p.proposedPayload.to, basis: p.proposedPayload.basis,
      });
    }
  }
  events.sort((a, b) => a.seq - b.seq);
  return { events, proposals: orphans };
};

/** Mirror of the core's propose seam: derive from/basis from the current
 *  mock state — never asserted by the caller. */
function mockPropose(h: Hypothesis, to: HypothesisStatus, basis: string, runId: string): Proposal {
  proposalSeq += 1;
  mockEventSeq += 1;
  const p: Proposal = {
    id: "pr" + proposalSeq + "-" + Date.now(),
    seq: mockEventSeq,
    ts: nowISO(),
    runId,
    missionId: h.missionId,
    targetEntity: h.id,
    targetLabel: h.statement,
    targetSeq: h.seq,
    proposedKind: "hypothesis.status_changed",
    proposedPayload: { from: h.status, to, basis },
    basisSeq: hypothesisCurrentSeq(h),
    basisStale: false,
    status: "pending",
    decided: null,
    supersededBy: null,
  };
  proposals.push(p);
  return p;
}

/** Apply a merged proposal to the mock board — the merge is what applies
 *  the change (the fold's approval-order application, mirrored). */
function mockApply(p: Proposal, decidedSeq: number): void {
  const h = hypotheses.find((x) => x.id === p.targetEntity);
  if (!h) return;
  h.status = p.proposedPayload.to;
  h.audit = { seq: decidedSeq, ts: nowISO(), actor: "agent", basis: p.proposedPayload.basis };
}

/** Upsert a relation chip onto one endpoint (latest event per endpoint
 *  pair wins — editing a relation appends, mirroring the core's fold). */
function upsertMockRelation(
  h: Hypothesis,
  seq: number,
  kind: RelationKind,
  direction: "outgoing" | "incoming",
  other: Hypothesis,
) {
  const existing = h.relations.find(
    (r) => r.direction === direction && r.otherId === other.id,
  );
  if (existing) {
    existing.seq = seq;
    existing.kind = kind;
  } else {
    h.relations.push({
      seq,
      kind,
      direction,
      otherId: other.id,
      otherSeq: other.seq,
      otherStatement: other.statement,
    });
  }
}

// In-memory morning digest (mirrors the event-sourced core, Story 2.3):
// seeded rows so the digest frame shows in the dev browser on first load —
// a finished night, a ceiling-stopped mission, an honest failed row, and
// one dead-run alert. `runNightShiftNow` appends real rows for any mock
// missions on top of the seed.
const seededAt = "2026-09-19T09:04:00Z";
let digestRunSeq = 100;
const seededRows: DigestRow[] = [
  {
    missionId: "m21-seed", missionSeq: 21,
    question: "Does retrieval grounding reduce hallucinated citations?",
    status: "active", runs: 1, finished: 1, failed: 0, failureReason: null,
    ceilingReached: false, proposalsPending: 2, spendCents: 42, ceilingCents: 100,
    receiptSeq: 101, runId: "nightshift-21", lastRunTs: "2026-09-19T03:04:00Z",
  },
  {
    missionId: "m22-seed", missionSeq: 22,
    question: "Does sparse attention hold at long context?",
    status: "active", runs: 1, finished: 1, failed: 0, failureReason: null,
    ceilingReached: true, proposalsPending: 0, spendCents: 100, ceilingCents: 100,
    receiptSeq: 102, runId: "nightshift-22", lastRunTs: "2026-09-19T02:14:00Z",
  },
  {
    missionId: "m24-seed", missionSeq: 24,
    question: "Is linear complexity competitive with quadratic attention?",
    status: "completed", runs: 2, finished: 2, failed: 0, failureReason: null,
    ceilingReached: false, proposalsPending: 0, spendCents: 31, ceilingCents: 100,
    receiptSeq: 103, runId: "nightshift-24", lastRunTs: "2026-09-19T01:44:00Z",
  },
  {
    missionId: "m26-seed", missionSeq: 26,
    question: "Does MoE routing stay stable under distribution shift?",
    status: "failed", runs: 1, finished: 0, failed: 1, failureReason: "provider_error",
    ceilingReached: false, proposalsPending: 0, spendCents: 0, ceilingCents: 100,
    receiptSeq: 104, runId: "nightshift-26", lastRunTs: "2026-09-19T02:58:00Z",
  },
];
const seededDigest: MorningDigest = {
  generatedAt: seededAt,
  outcome: "partial_success",
  spendCents: 173,
  ceilingCents: 400,
  rows: seededRows,
  alerts: [
    { runId: "nightshift-17", missionId: "m26-seed", missionSeq: 26, heartbeatTs: "2026-09-19T02:31:00Z", receiptSeq: 104 },
  ],
  // Story 2.6 (FR-9.1): the seeded connection alert — the Zotero connector
  // is down; arXiv and Semantic Scholar are healthy (label + icon, never
  // color alone).
  connectionAlerts: [
    { connection: "zotero", errorCode: "unreachable", failedTs: "2026-09-19T03:12:00Z" },
  ],
};

// Seeded run receipts (mirrors the event-sourced core, Story 2.5, FR-6.1):
// the frame's flagship ledger — an honest partial run that hit its ceiling —
// plus one per seeded digest row, so the drill-down shows in the dev browser
// on first load. Live mock runs (runNightShiftNow) fold their receipts from
// the run lists at query time — replay = re-query, same as the core.
const seededReceipts: Record<string, RunReceipt> = {
  "nightshift-21": {
    runId: "nightshift-21", missionId: "m21-seed", missionSeq: 21,
    outcome: "failed", ceilingHit: true,
    reason: "cost_ceiling_reached", verdict: null,
    startedTs: "2026-09-19T02:31:04Z", endedTs: "2026-09-19T03:12:58Z", durationSecs: 2514,
    spendCents: 82, ceilingCents: 100, models: ["GLM-5.3"],
    rows: [
      { seq: 101, ts: "2026-09-19T02:31:04Z", kind: "run_start", step: "literature-scan", schedule: "daily-03:00" },
      { seq: 102, ts: "2026-09-19T02:31:19Z", kind: "search", query: "retrieval-augmented generation hallucination" },
      { seq: 103, ts: "2026-09-19T02:33:47Z", kind: "call", provider: "openrouter", model: "GLM-5.3", role: "drafter", inputTokens: 3812, outputTokens: 964, costCents: 9 },
      { seq: 104, ts: "2026-09-19T02:36:02Z", kind: "claim", text: "Passage length conditions the grounding effect." },
      { seq: 105, ts: "2026-09-19T02:39:31Z", kind: "call", provider: "openrouter", model: "GLM-5.3", role: "critic", inputTokens: 4096, outputTokens: 1204, costCents: 12 },
      { seq: 106, ts: "2026-09-19T02:44:10Z", kind: "search", query: "hallucination long-form generation" },
      { seq: 107, ts: "2026-09-19T02:51:26Z", kind: "proposal", proposalId: "pr-107", proposalSeq: 107, to: "revised", status: "pending" },
      { seq: 108, ts: "2026-09-19T03:12:58Z", kind: "refused", scope: "mission", ceilingCents: 100, wouldBeCostCents: 101 },
      { seq: 109, ts: "2026-09-19T03:12:58Z", kind: "run_end", outcome: "failed", reason: "cost_ceiling_reached", verdict: null },
    ],
  },
  "nightshift-22": {
    runId: "nightshift-22", missionId: "m22-seed", missionSeq: 22,
    outcome: "finished", ceilingHit: true,
    reason: null, verdict: "1 scan · sparse attention holds at 32k",
    startedTs: "2026-09-19T02:04:00Z", endedTs: "2026-09-19T02:14:00Z", durationSecs: 600,
    spendCents: 100, ceilingCents: 100, models: ["GLM-5.3"],
    rows: [
      { seq: 112, ts: "2026-09-19T02:04:00Z", kind: "run_start", step: "literature-scan", schedule: "daily-03:00" },
      { seq: 113, ts: "2026-09-19T02:04:12Z", kind: "search", query: "sparse attention long context" },
      { seq: 114, ts: "2026-09-19T02:08:40Z", kind: "call", provider: "openrouter", model: "GLM-5.3", role: "drafter", inputTokens: 2914, outputTokens: 702, costCents: 7 },
      { seq: 115, ts: "2026-09-19T02:13:58Z", kind: "refused", scope: "global", ceilingCents: 100, wouldBeCostCents: 103 },
      { seq: 116, ts: "2026-09-19T02:14:00Z", kind: "run_end", outcome: "finished", reason: null, verdict: "1 scan · sparse attention holds at 32k" },
    ],
  },
  "nightshift-24": {
    runId: "nightshift-24", missionId: "m24-seed", missionSeq: 24,
    outcome: "finished", ceilingHit: false,
    reason: null, verdict: "2 scans · board settled, criterion met",
    startedTs: "2026-09-19T01:31:00Z", endedTs: "2026-09-19T01:44:00Z", durationSecs: 780,
    spendCents: 31, ceilingCents: 100, models: ["GLM-5.3"],
    rows: [
      { seq: 121, ts: "2026-09-19T01:31:00Z", kind: "run_start", step: "literature-scan", schedule: "daily-03:00" },
      { seq: 122, ts: "2026-09-19T01:31:09Z", kind: "search", query: "linear complexity attention quality" },
      { seq: 123, ts: "2026-09-19T01:37:44Z", kind: "call", provider: "openrouter", model: "GLM-5.3", role: "drafter", inputTokens: 2204, outputTokens: 588, costCents: 6 },
      { seq: 124, ts: "2026-09-19T01:41:03Z", kind: "proposal", proposalId: "pr-124", proposalSeq: 124, to: "supported", status: "merged" },
      { seq: 125, ts: "2026-09-19T01:52:30Z", kind: "decision", proposalId: "pr-124", proposalSeq: 124, decision: "merged" },
      { seq: 126, ts: "2026-09-19T01:44:00Z", kind: "run_end", outcome: "finished", reason: null, verdict: "2 scans · board settled, criterion met" },
    ],
  },
  // the dead-run alert's run (FR-9.1): an honest failed receipt — the run
  // died with a stale heartbeat and never reached a provider
  "nightshift-17": {
    runId: "nightshift-17", missionId: "m26-seed", missionSeq: 26,
    outcome: "failed", ceilingHit: false,
    reason: "stale_heartbeat", verdict: null,
    startedTs: "2026-09-19T02:31:00Z", endedTs: "2026-09-19T03:01:00Z", durationSecs: 1800,
    spendCents: 0, ceilingCents: 100, models: [],
    rows: [
      { seq: 131, ts: "2026-09-19T02:31:00Z", kind: "run_start", step: "literature-scan", schedule: "daily-03:00" },
      { seq: 132, ts: "2026-09-19T02:31:08Z", kind: "search", query: "MoE routing distribution shift" },
      { seq: 133, ts: "2026-09-19T03:01:00Z", kind: "run_end", outcome: "failed", reason: "stale_heartbeat", verdict: null },
    ],
  },
  "nightshift-26": {
    runId: "nightshift-26", missionId: "m26-seed", missionSeq: 26,
    outcome: "failed", ceilingHit: false,
    reason: "provider_error", verdict: null,
    startedTs: "2026-09-19T02:58:00Z", endedTs: "2026-09-19T02:58:41Z", durationSecs: 41,
    spendCents: 0, ceilingCents: 100, models: [],
    rows: [
      { seq: 141, ts: "2026-09-19T02:58:00Z", kind: "run_start", step: "literature-scan", schedule: "daily-03:00" },
      { seq: 142, ts: "2026-09-19T02:58:05Z", kind: "search", query: "MoE routing distribution shift" },
      { seq: 143, ts: "2026-09-19T02:58:41Z", kind: "released", reason: "provider_error" },
      { seq: 144, ts: "2026-09-19T02:58:41Z", kind: "run_end", outcome: "failed", reason: "provider_error", verdict: null },
    ],
  },
};

/** Fold a live mock run's receipt from its run list (replay = re-query —
 * mirrors the core's pure fold: run boundaries, the scan's search, the run's
 * quarantined proposals; simulated mock calls cost nothing, so no call rows
 * and zero spend — the honest ledger of what actually happened). */
function mockReceiptFor(runId: string): RunReceipt | null {
  for (const [missionId, runs] of Object.entries(missionRuns)) {
    const started = runs.find((r) => r.kind === "run.started" && r.runId === runId);
    if (!started) continue;
    const mission = missions.find((m) => m.id === missionId);
    const finished = runs.find((r) => r.kind === "run.finished" && r.runId === runId);
    const failed = runs.find((r) => r.kind === "run.failed" && r.runId === runId);
    const terminal = failed ?? finished;
    const runProposals = proposals.filter((p) => p.runId === runId);
    const rows: ReceiptRow[] = [
      { seq: started.seq, ts: started.ts, kind: "run_start", step: "literature-scan", schedule: mission?.schedule ?? "daily-03:00" },
      { seq: started.seq, ts: started.ts, kind: "search", query: mission?.question ?? "" },
      ...runProposals.map((p): ReceiptRow => ({
        seq: p.seq, ts: p.ts, kind: "proposal",
        proposalId: p.id, proposalSeq: p.seq,
        to: p.proposedPayload.to, status: p.status,
      })),
    ];
    if (terminal) {
      rows.push({
        seq: terminal.seq, ts: terminal.ts, kind: "run_end",
        outcome: failed ? "failed" : "finished",
        reason: failed ? ((failed as any).reason ?? "provider_error") : null,
        verdict: finished ? "1 scan · 1 proposal pending" : null,
      });
    }
    const endedTs = terminal?.ts ?? null;
    return {
      runId, missionId, missionSeq: mission?.seq ?? 0,
      outcome: failed ? "failed" : finished ? "finished" : "open",
      ceilingHit: !!mission && mission.spendCents >= mission.spendCeilingCents && mission.spendCeilingCents > 0,
      reason: failed ? ((failed as any).reason ?? "provider_error") : null,
      verdict: finished ? "1 scan · 1 proposal pending" : null,
      startedTs: started.ts, endedTs,
      durationSecs: endedTs ? Math.round((Date.parse(endedTs) - Date.parse(started.ts)) / 1000) : null,
      spendCents: 0, // simulated mock calls cost nothing — nothing was spent
      ceilingCents: mission?.spendCeilingCents ?? 0,
      models: [],
      rows: rows.sort((a, b) => a.seq - b.seq),
    };
  }
  return null;
}

/** The mock digest: the seed plus one live row per active mock mission that
 *  has run (the manual trigger appends them — mirrors the core's fold). */
function currentMockDigest(): MorningDigest {
  const liveRows: DigestRow[] = missions
    .filter((m) => m.status === "active")
    .map((m) => {
      const runs = missionRuns[m.id] ?? [];
      const started = runs.filter((r) => r.kind === "run.started").length;
      if (started === 0) return null;
      const finished = runs.filter((r) => r.kind === "run.finished").length;
      const failedRuns = runs.filter((r) => r.kind === "run.failed");
      const lastStarted = [...runs].reverse().find((r) => r.kind === "run.started");
      const pending = proposals.filter((p) => p.missionId === m.id && p.status === "pending").length;
      const row: DigestRow = {
        missionId: m.id,
        missionSeq: m.seq,
        question: m.question,
        status: m.status,
        runs: started,
        finished,
        failed: failedRuns.length,
        failureReason: failedRuns.length
          ? (failedRuns[failedRuns.length - 1] as any).reason ?? "provider_error"
          : null,
        ceilingReached: m.spendCents >= m.spendCeilingCents && m.spendCeilingCents > 0,
        proposalsPending: pending,
        spendCents: m.spendCents,
        ceilingCents: m.spendCeilingCents,
        receiptSeq: lastStarted?.seq ?? 0,
        runId: lastStarted?.runId ?? "",
        lastRunTs: lastStarted?.ts ?? seededAt,
      };
      return row;
    })
    .filter((r): r is DigestRow => r !== null);
  const rows = [...liveRows, ...seededRows].slice(0, 10);
  const allRuns = rows.reduce((a, r) => ({ s: a.s + r.runs, f: a.f + r.finished, x: a.x + r.failed }), { s: 0, f: 0, x: 0 });
  const outcome = allRuns.s === 0
    ? "no_runs"
    : allRuns.x === 0
      ? "all_finished"
      : allRuns.f === 0
        ? "all_failed"
        : "partial_success";
  return {
    generatedAt: seededAt,
    outcome,
    spendCents: rows.reduce((a, r) => a + r.spendCents, 0),
    ceilingCents: rows.reduce((a, r) => a + r.ceilingCents, 0),
    rows,
    alerts: seededDigest.alerts,
    connectionAlerts: seededDigest.connectionAlerts,
  };
}

const agents: Agent[] = [
  { id: "a1", key: "rigor", name: "Rigor", description: "Detecta fallos metodológicos y lógicos.", icon: "brain", enabled: 1, kind: "judge" },
  { id: "a2", key: "novelty", name: "Novelty", description: "Evalúa la contribución original.", icon: "spark", enabled: 1, kind: "judge" },
  { id: "a3", key: "clarity", name: "Clarity", description: "Revisa claridad y estructura.", icon: "pen", enabled: 0, kind: "judge" },
];

const delay = (ms = 60) => new Promise<void>((r) => setTimeout(r, ms));

// ---------- onboarding (Story 1.9) ----------
// Mirrors the typed core: the same arXiv URL shapes parse to the bare id
// (anything else fails with the same coded `invalid_url:` error before
// anything happens), the paste upserts the paper into the library (matched
// by url, never duplicated), and the simulated provider generates the
// candidates (attributed to "simulated", cost 0 — the mock has no key).

function parseMockArxivUrl(input: string): string {
  const s = input.trim();
  const bad = () => { throw new Error(`invalid_url: \`${s}\` — paste a valid arXiv URL (https://arxiv.org/abs/1706.03762)`); };
  let rest = s;
  if (s.includes("://")) {
    const [scheme, after] = s.split("://", 2) as [string, string];
    if (scheme.toLowerCase() !== "http" && scheme.toLowerCase() !== "https") bad();
    rest = after;
  }
  const lower = rest.toLowerCase();
  if (lower.startsWith("arxiv.org/") || lower.startsWith("www.arxiv.org/") || lower.startsWith("export.arxiv.org/")) {
    // drop the host, keep the path segments: abs/<id> | pdf/<id>(.pdf)
    const [, ...segments] = rest.split("/");
    const [kind, ...idParts] = segments.filter((p) => p !== "");
    if ((kind === "abs" || kind === "pdf") && idParts.length > 0) {
      return idParts.join("/").replace(/\.pdf$/, "");
    }
    bad();
  }
  // bare id: 1706.03762 | 1706.03762v2 | cs/0601011
  if (/^\d{4}\.\d{4,5}(v\d+)?$/.test(rest) || /^[a-z-]+(\.[A-Z]{2})?\/\d{7}$/i.test(rest)) {
    return rest;
  }
  bad();
  return ""; // unreachable
}

// The seeded paper for the design's example paste; any other valid id gets a
// generic-but-honest paper so the whole flow works in the dev browser.
const seedPaper = (arxivId: string) =>
  arxivId === "1706.03762"
    ? {
        title: "Attention Is All You Need",
        authors: "Vaswani et al.",
        year: 2017,
        abstract:
          "The dominant sequence transduction models are based on recurrent or convolutional networks. We propose the Transformer, based solely on attention mechanisms.",
      }
    : {
        title: `arXiv:${arxivId}`,
        authors: "Unknown Author",
        year: 2024,
        abstract: null as string | null,
      };

// The simulated provider's candidates (mirrors the Rust simulator): sensible,
// falsifiable, derived from the paper title in the interface language.
function mockCandidates(title: string, lang: string): { statement: string; confidence: number }[] {
  return lang === "en"
    ? [
        { statement: `The central result of «${title}» replicates under independent evaluation`, confidence: 0.78 },
        { statement: `The method of «${title}» outperforms the baselines it is compared against`, confidence: 0.71 },
        { statement: `The claims of «${title}» hold only within the regimes its authors evaluate`, confidence: 0.65 },
      ]
    : [
        { statement: `El resultado central de «${title}» se replica bajo una evaluación independiente`, confidence: 0.78 },
        { statement: `El método de «${title}» supera a los baselines con los que se compara`, confidence: 0.71 },
        { statement: `Las afirmaciones de «${title}» solo se sostienen dentro de los regímenes que sus autores evalúan`, confidence: 0.65 },
      ];
}

/** The shared first-value flow behind both mock doors (arXiv paste and the
 *  Zotero library stub): upsert the ref, generate the candidates, create the
 *  starter mission, and append the candidates as proposed hypotheses. */
async function mockFirstValue(paper: {
  refId?: string;
  title: string;
  authors: string;
  year: number | null;
  url: string;
  arxivId: string;
}): Promise<FirstValueResult> {
  await delay(450);
  // library upsert — match by url, never duplicate
  let refId = paper.refId;
  if (!refId) {
    const existing = mockRefs.find((r) => r.url === paper.url);
    refId = existing ? existing.id : "r" + (mockRefs.length + 1) + "-" + Date.now();
    if (!existing) {
      mockRefs.push({
        id: refId,
        project_id: "p1",
        collection_id: null,
        title: paper.title,
        authors: paper.authors,
        year: paper.year ?? new Date().getFullYear(),
        venue: "arXiv",
        doi: `10.48550/arXiv.${paper.arxivId}`,
        url: paper.url,
        isbn: "",
        attachment: null,
        status: "unread",
        tags: "arXiv,onboarding",
        used: 0,
        citation_count: 0,
        created_at: nowISO(),
      });
    }
  }
  const lang = settings.lang || "es";
  const drafts = mockCandidates(paper.title, lang);
  const mission = await mockApi.createMission({
    question:
      lang === "en"
        ? `Check the claims in “${paper.title}”`
        : `Comprueba las afirmaciones de «${paper.title}»`,
    stopCondition:
      lang === "en"
        ? "Stop after 3 runs or 20 sources reviewed, whichever comes first."
        : "Detente tras 3 corridas o 20 fuentes revisadas, lo que ocurra primero.",
    successCriterion:
      lang === "en"
        ? "Every surviving candidate has at least 3 pinned sources agreeing at 70%+ confidence, or it is refuted."
        : "Cada candidato que sobreviva tiene al menos 3 anclas de evidencia citadas que coinciden con ≥70 % de confianza, o queda refutado.",
    autonomy: "suggest",
    spendCeilingCents: 100,
  });
  const candidates: HypothesisCandidate[] = [];
  for (const draft of drafts) {
    const h = await mockApi.createHypothesis(draft.statement, mission.id);
    candidates.push({
      hypothesisId: h.id,
      seq: h.seq,
      statement: h.statement,
      status: "proposed",
      confidence: draft.confidence,
      assessingModel: "simulated",
    });
  }
  return {
    receipt: { provider: "simulated", model: "simulated", simulated: true, costCents: 0 },
    paper: {
      refId,
      arxivId: paper.arxivId,
      title: paper.title,
      authors: paper.authors,
      year: paper.year,
      url: paper.url,
    },
    mission,
    candidates,
  };
}

// Agent roles (Story 2.1): the mock mirrors the core's default resolution —
// the drafter runs the configured pair, the critic never shares it (NFR-3),
// and with no key both run simulated. Overrides apply by role name.
function resolveMockRoles(overrides?: RoleConfig[] | null): RoleConfig[] {
  const hasKey = (settings.api_key ?? "").trim() !== "";
  const provider = hasKey ? settings.provider || "simulated" : "simulated";
  const model = hasKey ? settings.model || "simulated" : "simulated";
  const defaults: RoleConfig[] = [
    { name: "drafter", provider, model },
    { name: "critic", provider: "simulated", model: "simulated" },
  ];
  if (!overrides || overrides.length === 0) return defaults;
  const roles = [...defaults];
  for (const o of overrides) {
    const i = roles.findIndex((r) => r.name === o.name);
    if (i >= 0) roles[i] = o;
    else roles.push(o);
  }
  return roles;
}

// The different-model critic rule (NFR-3), exactly as the core enforces it:
// a critic on any drafter's (provider, model) pair is rejected — the
// simulated fallback is exempt (the no-key mock, not an algorithm).
function assertDifferentModelCritics(roles: RoleConfig[]): void {
  const pair = (r: RoleConfig) => `${r.provider.trim().toLowerCase()}+${r.model.trim().toLowerCase()}`;
  const drafters = roles.filter((r) => r.name === "drafter").map(pair);
  for (const critic of roles.filter((r) => r.name === "critic")) {
    if (critic.provider.trim().toLowerCase() === "simulated") continue;
    if (drafters.includes(pair(critic))) {
      throw new Error(
        `same_model_critic: the critic role resolves to ${critic.provider} + ${critic.model} — the same (provider, model) pair as a drafter. ` +
          "Configure a different model for the critic (NFR-3: never one algorithm grading its own homework)",
      );
    }
  }
}

export const mockApi = {
  // projects
  listProjects: async () => { await delay(); return [project]; },
  getActiveProject: async () => { await delay(); return project; },
  setActiveProject: async () => {},
  createProject: async (p: any) => { await delay(); return { ...project, ...p, id: "p" + Date.now() }; },
  updateProject: async () => {},
  deleteProject: async () => {},

  // dashboard
  getDashboard: async () => { await delay(); return { refs_total: 24, refs_used: 11, active_actions: 3, reviews: 2 }; },

  // refs
  listRefs: async () => { await delay(); return [...mockRefs]; },
  getRef: async () => { throw new Error("not in mock"); },
  createRef: async (r: any) => r as Ref,
  updateRef: async () => {},
  deleteRef: async () => {},
  searchRefs: async (_pid: string, q: string) => {
    await delay();
    const needle = q.trim().toLowerCase();
    return mockRefs.filter(
      (r) =>
        !needle ||
        r.title.toLowerCase().includes(needle) ||
        r.authors.toLowerCase().includes(needle) ||
        String(r.year).includes(needle),
    );
  },
  searchRefsExternal: async () => [],
  listCollections: async () => [],

  // reviews
  listReviews: async () => [] as Review[],
  runReview: async () => ({ ok: true }),

  // actions
  listActions: async () => [] as Action[],
  createAction: async (a: any) => a as Action,
  toggleAction: async () => {},
  updateAction: async () => {},
  deleteAction: async () => {},

  // chats
  listChats: async (_pid: string, kind?: string) => {
    await delay();
    return chats.filter((c) => !kind || c.kind === kind);
  },
  createChat: async (pid: string, kind: string, title: string) => {
    await delay();
    const id = "c" + ++chatSeq;
    const c: Chat = {
      id, project_id: pid, kind, title, preview: "",
      created_at: nowISO(), updated_at: nowISO(),
    };
    chats.push(c);
    messages[id] = [];
    return c;
  },
  getChat: async (id: string) => {
    await delay();
    const c = chats.find((x) => x.id === id);
    if (!c) throw new Error("chat not found");
    return { ...c, messages: messages[id] ?? [] };
  },
  sendMessage: async (chatId: string, content: string) => {
    await delay(120);
    const list = messages[chatId] ?? (messages[chatId] = []);
    list.push({ id: "m" + ++msgSeq, chat_id: chatId, role: "user", content, classify_tag: null, meta: null, created_at: nowISO() });
    // Echo a canned agent reply so the thread feels alive.
    list.push({ id: "m" + ++msgSeq, chat_id: chatId, role: "agent", content: "(mock) Entendido. Esta es una respuesta de ejemplo del asistente.", classify_tag: null, meta: null, created_at: nowISO() });
    const c = chats.find((x) => x.id === chatId);
    if (c) { c.preview = content.slice(0, 80); c.updated_at = nowISO(); }
    return { ok: true };
  },
  deleteChat: async (id: string) => {
    const i = chats.findIndex((x) => x.id === id);
    if (i >= 0) chats.splice(i, 1);
    delete messages[id];
  },

  // agents
  listAgents: async () => { await delay(); return agents; },
  toggleAgent: async () => {},

  // mcp
  listMcpServers: async () => { await delay(); return servers; },
  addMcpServer: async (m: any) => { await delay(); return { ...m, id: "m" + Date.now(), connected: 0 } as McpServer; },
  updateMcpServer: async () => {},
  deleteMcpServer: async () => {},
  testMcpServer: async () => ({ ok: true }),
  listMcpTools: async () => [],

  // settings
  getSettings: async () => { await delay(); return { ...settings }; },
  updateSetting: async (key: string, value: string) => { settings[key] = value; },
  setProviderKey: async (key: string) => { settings.api_key = key; },

  // logging + paths
  appLog: async () => {},
  getAppPaths: async () => ({ data_dir: "~/Library/Application Support/Research Core", log_dir: "~/Library/Logs/Research Core" }),
  revealPath: async () => {},
  pickFolder: async () => null,

  // local access key
  verifyKey: async () => true,
  setLockKey: async () => {},
  lockState: async () => ({ policy: "never", idle_min: "", configured: false }),

  // LLM CLI detection
  testCli: async (command: string) => ({ command, path: `/usr/local/bin/${command}` }),

  // missions
  createMission: async (m: { question: string; stopCondition: string; successCriterion: string; autonomy: Autonomy; spendCeilingCents: number; roles?: RoleConfig[] | null }) => {
    await delay();
    // Agent roles (Story 2.1): resolve the layer defaults, apply overrides by
    // name — then enforce the different-model critic rule exactly like the
    // core does (NFR-3), before anything is created.
    const roles = resolveMockRoles(m.roles);
    assertDifferentModelCritics(roles);
    missionSeq += 1;
    const mission: Mission = {
      id: "m" + missionSeq + "-" + Date.now(),
      seq: missionSeq,
      ts: nowISO(),
      question: m.question,
      stopCondition: m.stopCondition,
      successCriterion: m.successCriterion,
      autonomy: m.autonomy,
      spendCeilingCents: m.spendCeilingCents,
      roles,
      schedule: "daily-03:00",
      status: "active",
      spendCents: 0,
      spendState: "ok",
    };
    missions.push(mission);
    return mission;
  },
  listMissions: async () => { await delay(); return [...missions]; },
  getMissionRuns: async (missionId: string) => { await delay(); return [...(missionRuns[missionId] ?? [])]; },
  // run receipts (Story 2.5, FR-6.1): the seeded ledgers plus a live fold
  // for runs the mock Night Shift created — `null` when the run id has no
  // run.started (an honest "no receipt", same as the core's 404)
  getRunReceipt: async (runId: string): Promise<RunReceipt | null> => {
    await delay();
    const seeded = seededReceipts[runId];
    if (seeded) return { ...seeded, rows: seeded.rows.map((r) => ({ ...r })), models: [...seeded.models] };
    return mockReceiptFor(runId);
  },
  // agent steps (Story 2.1): one role step through the mock provider — the
  // dev browser answers with the simulated voice, no spend. Story 2.2: the
  // drafter's step also EMITS a quarantined proposal for the mission's first
  // hypothesis (the AD-3 loop, mirrored — nothing touches the board until a
  // human merges).
  runAgentStep: async (missionId: string, role: string, task: string): Promise<AgentStepResult> => {
    await delay(150);
    const mission = missions.find((x) => x.id === missionId);
    if (!mission) throw new Error(`not_found: no mission with id \`${missionId}\``);
    if (!task.trim()) throw new Error("task must not be empty — an agent step needs something to do");
    const config = mission.roles.find((r) => r.name === role);
    if (!config) throw new Error(`not_found: mission \`${missionId}\` has no role named \`${role}\` — expected drafter | critic`);
    if (config.name === "drafter") {
      const target = hypotheses.find(
        (h) => h.missionId === missionId && allowedNext[h.status].length > 0,
      );
      if (target) {
        mockPropose(
          target,
          allowedNext[target.status][0],
          task.trim(),
          `${config.provider}:${config.name}`,
        );
      }
    }
    return {
      missionId,
      role: config.name,
      provider: config.provider,
      model: config.model,
      content:
        config.name === "critic"
          ? `Crítica (simulada): la tarea «${task.trim()}» avanza la misión, pero las afirmaciones centrales aún carecen de evidencia anclada. Señala qué cita respalda cada afirmación antes de continuar.`
          : `Propuesta (simulada) para «${task.trim()}»: avanzar con una afirmación verificable anclada al tablero, citando las referencias disponibles y marcando la confianza de cada paso.`,
    };
  },

  // hypotheses — mirrors the typed core (FR-2.2 table enforced, relations
  // upserted onto both endpoints with latest-wins semantics)
  createHypothesis: async (statement: string, missionId: string) => {
    await delay();
    if (!statement.trim()) throw new Error("hypothesis.statement must not be empty");
    if (!missions.some((m) => m.id === missionId)) {
      throw new Error(`not_found: no mission with id \`${missionId}\``);
    }
    hypSeq += 1;
    const ts = nowISO();
    const h: Hypothesis = {
      id: "h" + hypSeq + "-" + Date.now(),
      seq: hypSeq,
      ts,
      statement: statement.trim(),
      missionId,
      status: "proposed",
      relations: [],
      audit: { seq: hypSeq, ts, actor: "user", basis: "hypothesis.created" },
    };
    hypotheses.push(h);
    return h;
  },
  listHypotheses: async (missionId: string) => {
    await delay();
    return hypotheses.filter((h) => h.missionId === missionId).map((h) => ({ ...h }));
  },
  transitionHypothesis: async (hypothesisId: string, to: string, basis: string) => {
    await delay();
    const h = hypotheses.find((x) => x.id === hypothesisId);
    if (!h) throw new Error(`not_found: no hypothesis with id \`${hypothesisId}\``);
    if (!allowedNext[h.status].includes(to as HypothesisStatus)) {
      throw new Error(
        `illegal_transition: ${h.status} → ${to} is not a legal hypothesis lifecycle transition (FR-2.2)`,
      );
    }
    if (!basis.trim()) {
      throw new Error("hypothesis.basis must not be empty — every transition names its basis (FR-2.2)");
    }
    h.status = to as HypothesisStatus;
    hypSeq += 1;
    h.audit = { seq: hypSeq, ts: nowISO(), actor: "user", basis: basis.trim() };
    return { ...h };
  },
  addRelation: async (fromHypothesisId: string, toHypothesisId: string, relationKind: string) => {
    await delay();
    const from = hypotheses.find((x) => x.id === fromHypothesisId);
    const to = hypotheses.find((x) => x.id === toHypothesisId);
    if (!from) throw new Error(`not_found: no hypothesis with id \`${fromHypothesisId}\``);
    if (!to) throw new Error(`not_found: no hypothesis with id \`${toHypothesisId}\``);
    if (from.id === to.id) throw new Error("hypothesis.related: a hypothesis cannot relate to itself");
    const kind = (["contradicts", "extends", "specializes", "supports_the_same_claim"] as const)
      .find((k) => k === relationKind);
    if (!kind) {
      throw new Error(`unknown_relation: \`${relationKind}\``);
    }
    hypSeq += 1;
    const seq = hypSeq;
    upsertMockRelation(from, seq, kind, "outgoing", to);
    upsertMockRelation(to, seq, kind, "incoming", from);
    return { ...from };
  },

  // evidence — mirrors the typed core: registered claims start unpinned
  // (FR-3.4), the pin digest is computed here (AD-5), confidence is
  // attributed to the assessing model (FR-3.6), and the ref must exist.
  registerClaim: async (hypothesisId: string, text: string, sourceMessageId: string | null) => {
    await delay();
    if (!text.trim()) throw new Error("claim.text must not be empty — a claim says something");
    if (!hypotheses.some((h) => h.id === hypothesisId)) {
      throw new Error(`not_found: no hypothesis with id \`${hypothesisId}\``);
    }
    claimSeq += 1;
    const claim: Claim = {
      id: "cl" + claimSeq + "-" + Date.now(),
      seq: claimSeq,
      ts: nowISO(),
      hypothesisId,
      text: text.trim(),
      sourceMessageId,
      pinned: false,
      pin: null,
    };
    claims.push(claim);
    return { ...claim };
  },
  pinClaimToCitation: async (
    claimId: string,
    hypothesisId: string,
    refId: string,
    excerpt: string,
    confidence: number,
    assessingModel: string,
  ) => {
    await delay();
    const claim = claims.find((c) => c.id === claimId);
    if (!claim) throw new Error(`not_found: no claim with id \`${claimId}\``);
    const trimmedRef = refId.trim();
    const ref = mockRefs.find((r) => r.id === trimmedRef);
    if (!trimmedRef || !ref) {
      throw new Error(`invalid_ref: \`${trimmedRef}\` — no reference with this id in the library`);
    }
    if (!excerpt.trim()) {
      throw new Error("evidence.excerpt must not be empty — a pin quotes the passage it rests on");
    }
    if (typeof confidence !== "number" || Number.isNaN(confidence) || confidence < 0 || confidence > 1) {
      throw new Error(`invalid confidence \`${confidence}\` — agent-assessed confidence is a number in [0.0, 1.0] (FR-3.6)`);
    }
    if (!assessingModel.trim()) {
      throw new Error("evidence.assessing_model must not be empty — confidence is attributed to the assessing model, never anonymous (FR-3.6)");
    }
    // The digest is computed here, from the excerpt — never trusted from
    // the caller (AD-5).
    const digest = await sha256Hex(excerpt);
    claimSeq += 1;
    claim.pinned = true;
    claim.pin = {
      seq: claimSeq,
      ts: nowISO(),
      claimId: claim.id,
      hypothesisId,
      kind: "citation",
      refId: trimmedRef,
      artifactRef: null,
      excerpt,
      digest,
      confidence,
      assessingModel: assessingModel.trim(),
      refLabel: `${ref.authors} ${ref.year}`,
    };
    return { ...claim };
  },
  // FR-3.3 (Story 1.8): a numerical pin anchors the claim to an artifact
  // (file/figure/table) by artifact_ref + the sha-256 of the pinned
  // content — computed here (AD-5), never trusted from the caller.
  pinClaimToNumerical: async (
    claimId: string,
    hypothesisId: string,
    artifactRef: string,
    content: string,
    confidence: number,
    assessingModel: string,
  ) => {
    await delay();
    const claim = claims.find((c) => c.id === claimId);
    if (!claim) throw new Error(`not_found: no claim with id \`${claimId}\``);
    const trimmedArtifact = artifactRef.trim();
    if (!trimmedArtifact) {
      throw new Error("evidence.artifact_ref must not be empty — a numerical pin names the artifact it anchors to (FR-3.3)");
    }
    if (!content.trim()) {
      throw new Error("evidence.excerpt must not be empty — a pin anchors the content it rests on");
    }
    if (typeof confidence !== "number" || Number.isNaN(confidence) || confidence < 0 || confidence > 1) {
      throw new Error(`invalid confidence \`${confidence}\` — agent-assessed confidence is a number in [0.0, 1.0] (FR-3.6)`);
    }
    if (!assessingModel.trim()) {
      throw new Error("evidence.assessing_model must not be empty — confidence is attributed to the assessing model, never anonymous (FR-3.6)");
    }
    const digest = await sha256Hex(content);
    claimSeq += 1;
    claim.pinned = true;
    claim.pin = {
      seq: claimSeq,
      ts: nowISO(),
      claimId: claim.id,
      hypothesisId,
      kind: "numerical",
      refId: null,
      artifactRef: trimmedArtifact,
      excerpt: content,
      digest,
      confidence,
      assessingModel: assessingModel.trim(),
      refLabel: null,
    };
    return { ...claim };
  },
  listEvidence: async (hypothesisId: string) => {
    await delay();
    return claims
      .filter((c) => c.hypothesisId === hypothesisId)
      .map((c) => ({ ...c, pin: c.pin ? { ...c.pin } : null }));
  },

  // proposals (Story 2.2, AD-3/AD-13) — mirrors the typed core: pending
  // proposals are excluded from the board until a human merges them; the
  // basis is validated at merge time (basis_stale: unless forced, and a
  // forced merge records the marker); conflicting same-basis pending
  // siblings are superseded; a decided proposal can never merge again.
  listProposals: async (missionId: string | null) => {
    await delay();
    return proposals
      .filter((p) => !missionId || p.missionId === missionId)
      .map((p) => ({
        ...p,
        proposedPayload: { ...p.proposedPayload },
        decided: p.decided ? { ...p.decided } : null,
      }));
  },
  approveProposal: async (proposalId: string, force: boolean): Promise<ApproveOutcome> => {
    await delay();
    const p = proposals.find((x) => x.id === proposalId);
    if (!p) throw new Error(`not_found: no proposal with id \`${proposalId}\``);
    if (p.status !== "pending") {
      throw new Error(
        `not_pending: proposal \`${proposalId}\` is \`${p.status}\`, not pending — a decided proposal can never be merged again (AD-13)`,
      );
    }
    const target = hypotheses.find((h) => h.id === p.targetEntity);
    if (!target) {
      throw new Error(`not_found: proposal \`${proposalId}\` targets no known entity in the log`);
    }
    const current = hypothesisCurrentSeq(target);
    const stale = current > p.basisSeq;
    if (stale && !force) {
      throw new Error(
        `basis_stale: proposal \`${proposalId}\` was derived from seq ${p.basisSeq} but the entity has advanced to seq ${current} — force-approve (force: true) to merge past it; the basis-stale marker will be recorded and surfaced`,
      );
    }
    mockEventSeq += 1;
    const decidedSeq = mockEventSeq;
    p.status = "merged";
    p.basisStale = stale;
    p.decided = { seq: decidedSeq, ts: nowISO(), actor: "user" };
    mockApply(p, decidedSeq);
    // conflicting pending siblings: same target, same kind, same basis
    const superseded: Proposal[] = [];
    for (const sib of proposals) {
      if (
        sib.id !== p.id &&
        sib.status === "pending" &&
        sib.targetEntity === p.targetEntity &&
        sib.proposedKind === p.proposedKind &&
        sib.basisSeq === p.basisSeq
      ) {
        mockEventSeq += 1;
        sib.status = "superseded";
        sib.supersededBy = p.id;
        sib.decided = { seq: mockEventSeq, ts: nowISO(), actor: "user" };
        superseded.push({ ...sib, decided: { ...sib.decided! } });
      }
    }
    return {
      proposal: { ...p, proposedPayload: { ...p.proposedPayload }, decided: { ...p.decided! } },
      superseded,
    };
  },
  rejectProposal: async (proposalId: string): Promise<Proposal> => {
    await delay();
    const p = proposals.find((x) => x.id === proposalId);
    if (!p) throw new Error(`not_found: no proposal with id \`${proposalId}\``);
    if (p.status !== "pending") {
      throw new Error(
        `not_pending: proposal \`${proposalId}\` is \`${p.status}\`, not pending — a decided proposal can never be decided again (AD-13)`,
      );
    }
    mockEventSeq += 1;
    p.status = "rejected";
    p.decided = { seq: mockEventSeq, ts: nowISO(), actor: "user" };
    return { ...p, proposedPayload: { ...p.proposedPayload }, decided: { ...p.decided! } };
  },

  // morning digest (Story 2.3) — mirrors the typed core: the seed shows the
  // frame in the dev browser; the manual trigger runs every active mission's
  // scan (a run.started/finished pair + one quarantined proposal — the AD-3
  // loop, mirrored), and schedule changes update the mission in place.
  getMorningDigest: async (): Promise<MorningDigest> => {
    await delay();
    const d = currentMockDigest();
    return { ...d, rows: d.rows.map((r) => ({ ...r })), alerts: d.alerts.map((a) => ({ ...a })) };
  },
  runNightShiftNow: async (): Promise<MorningDigest> => {
    await delay(300);
    for (const mission of missions.filter((m) => m.status === "active")) {
      const runId = `nightshift-${++digestRunSeq}`;
      missionRuns[mission.id] = missionRuns[mission.id] ?? [];
      const push = (kind: string) => {
        mockEventSeq += 1;
        missionRuns[mission.id].push({
          seq: mockEventSeq,
          id: "r" + mockEventSeq + "-" + Date.now(),
          ts: nowISO(),
          kind,
          actor: "system:scheduler",
          // run lifecycle events carry their run id (Story 2.5) — the
          // receipt drill-down's target
          ...(kind.startsWith("run.") ? { runId } : {}),
        });
      };
      push("run.started");
      // the scan's output lands as a quarantined proposal (FR-4.2)
      const target = hypotheses.find(
        (h) => h.missionId === mission.id && allowedNext[h.status].length > 0,
      );
      if (target) {
        mockPropose(
          target,
          allowedNext[target.status][0],
          `escaneo nocturno ${runId}: tres fuentes nuevas anclan la hipótesis central`,
          runId,
        );
      }
      push("run.finished");
    }
    const d = currentMockDigest();
    return { ...d, rows: d.rows.map((r) => ({ ...r })), alerts: d.alerts.map((a) => ({ ...a })) };
  },
  setMissionSchedule: async (missionId: string, schedule: string): Promise<Mission> => {
    await delay();
    const mission = missions.find((m) => m.id === missionId);
    if (!mission) throw new Error(`not_found: no mission with id \`${missionId}\``);
    const trimmed = schedule.trim();
    const valid =
      trimmed.toLowerCase() === "off" ||
      (/^daily-\d{2}:\d{2}$/.test(trimmed) &&
        Number(trimmed.slice(6, 8)) <= 23 &&
        Number(trimmed.slice(9, 11)) <= 59);
    if (!valid) {
      throw new Error(
        "invalid_schedule: `" + trimmed + "` — expected `off` or `daily-HH:MM` (e.g. daily-03:00)",
      );
    }
    mission.schedule = trimmed;
    return { ...mission };
  },

  // onboarding (Story 1.9) — the arXiv paste door and the Zotero library
  // door, mirroring the typed core end to end.
  runFirstValue: async (url: string) => {
    const arxivId = parseMockArxivUrl(url);
    const seed = seedPaper(arxivId);
    return mockFirstValue({
      title: seed.title,
      authors: seed.authors,
      year: seed.year,
      url: `https://arxiv.org/abs/${arxivId}`,
      arxivId,
    });
  },
  runFirstValueFromRef: async (refId: string) => {
    const ref = mockRefs.find((r) => r.id === refId);
    if (!ref) {
      throw new Error(`not_found: no ref with id \`${refId}\` in the library`);
    }
    return mockFirstValue({
      refId: ref.id,
      title: ref.title,
      authors: ref.authors ?? "",
      year: ref.year ?? null,
      url: ref.url ?? "",
      arxivId: (ref.doi ?? "").replace("10.48550/arXiv.", ""),
    });
  },

  // trust center (Story 2.4, FR-5): the read model + the dial, ceiling, and
  // kill-switch mutations, mirroring the core's evented semantics.
  getTrustStatus: async () => { await delay(); return mockTrustStatus(); },
  configureAutonomy: async (scope: string, scopeId: string | null, mode: Autonomy) => {
    await delay();
    if (scope !== "global" && scope !== "mission" && scope !== "target") {
      throw new Error(`unknown scope \`${scope}\` — expected global | mission | target`);
    }
    if (mode !== "watch" && mode !== "suggest" && mode !== "act_with_receipts") {
      throw new Error(`unknown autonomy stop \`${mode}\` — expected watch | suggest | act_with_receipts`);
    }
    if (scope === "global") {
      trustState.globalAutonomy = mode;
    } else if (scopeId) {
      const list = scope === "mission" ? trustState.missionDials : trustState.targetDials;
      const existing = list.find((d) => d.scopeId === scopeId);
      if (existing) existing.mode = mode;
      else list.push({ scopeId, mode });
      list.sort((a, b) => (a.scopeId ?? "").localeCompare(b.scopeId ?? ""));
    } else {
      throw new Error(`scope_id: a ${scope}-scoped setting requires its id`);
    }
    return mockTrustStatus();
  },
  configureCeiling: async (scope: string, scopeId: string | null, ceilingCents: number) => {
    await delay();
    if (scope !== "global" && scope !== "mission" && scope !== "target") {
      throw new Error(`unknown scope \`${scope}\` — expected global | mission | target`);
    }
    if (scope === "global") {
      trustState.globalCeilingCents = ceilingCents;
    } else if (scopeId) {
      const list = scope === "mission" ? trustState.missionCeilings : trustState.targetCeilings;
      const existing = list.find((c) => c.scopeId === scopeId);
      if (existing) existing.ceilingCents = ceilingCents;
      else list.push({ scopeId, ceilingCents });
      list.sort((a, b) => (a.scopeId ?? "").localeCompare(b.scopeId ?? ""));
    } else {
      throw new Error(`scope_id: a ${scope}-scoped setting requires its id`);
    }
    return mockTrustStatus();
  },
  killRuntime: async () => {
    await delay();
    trustState.runtimeState = "killed";
    trustState.killedSeq = 42;
    return mockTrustStatus();
  },
  resumeRuntime: async () => {
    await delay();
    trustState.runtimeState = "running";
    trustState.killedSeq = null;
    return mockTrustStatus();
  },

  // checkpoints (Story 2.6, FR-10.1) — mirrors the event-sourced core: a
  // checkpoint snapshots the state; rollback restores the snapshot and marks
  // post-checkpoint proposals as superseded history (never hidden).
  createCheckpoint: async (name: string): Promise<Checkpoint> => {
    await delay();
    const trimmed = name.trim();
    if (!trimmed) {
      throw new Error("invalid_name: a checkpoint is named — the restore-point list renders names (FR-10.1)");
    }
    mockEventSeq += 1;
    const cp = {
      id: "cp" + mockEventSeq + "-" + Date.now(),
      seq: mockEventSeq,
      ts: nowISO(),
      name: trimmed,
      snapshot: cloneSnapshot(),
    };
    mockCheckpoints.push(cp);
    const { snapshot: _snap, ...view } = cp;
    return view;
  },
  listCheckpoints: async (): Promise<CheckpointsView> => {
    await delay();
    return {
      headSeq: mockEventSeq,
      checkpoints: mockCheckpoints.map(({ snapshot: _s, ...cp }) => ({ ...cp })),
      rollbacks: mockRollbacks.map((r) => ({ ...r })),
    };
  },
  previewRollback: async (checkpointId: string): Promise<RollbackPlan> => {
    await delay();
    const cp = mockCheckpoints.find((c) => c.id === checkpointId);
    if (!cp) throw new Error(`not_found: no checkpoint with id \`${checkpointId}\``);
    const { events, proposals: orphans } = orphanedFor(cp);
    const { snapshot: _s, ...view } = cp;
    return { checkpoint: view, orphanedEvents: events, orphanedProposals: orphans };
  },
  rollbackToCheckpoint: async (checkpointId: string): Promise<RollbackOutcome> => {
    await delay();
    const cp = mockCheckpoints.find((c) => c.id === checkpointId);
    if (!cp) throw new Error(`not_found: no checkpoint with id \`${checkpointId}\``);
    const { events, proposals: orphans } = orphanedFor(cp);
    // execute: restore the snapshot, mark the orphans as superseded history
    restoreSnapshot(cp.snapshot);
    for (const p of proposals) {
      if (p.seq > cp.seq && !p.orphanedByRollback) {
        mockEventSeq += 1;
        p.status = "superseded";
        p.orphanedByRollback = true;
        p.rolledBackSeq = mockEventSeq;
        p.decided = null;
      }
    }
    mockEventSeq += 1;
    const record: RollbackRecord = {
      seq: mockEventSeq, ts: nowISO(), checkpointId: cp.id,
      name: cp.name, targetSeq: cp.seq, orphanedCount: events.length,
    };
    mockRollbacks.push(record);
    return { rollback: record, orphanedEvents: events, orphanedProposals: orphans };
  },

  // danger zone
  resetDatabase: async () => { await delay(); },
};
