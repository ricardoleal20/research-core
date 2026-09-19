// In-memory mock backend. Activates only when the Tauri runtime is absent
// (i.e. the app is served by the Vite dev server in a plain browser). This
// lets the UI boot and be iterated on without the Rust backend. All state is
// ephemeral and resets on reload.
//
// When Tauri is present (real app or `tauri dev`), this module is never used —
// api.ts routes to the real `invoke` calls instead.

import type { Project, Ref, Review, Action, Chat, Agent, McpServer, Message, Mission, MissionRun, Autonomy, Hypothesis, HypothesisStatus, RelationKind, Claim } from "./types";

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

const agents: Agent[] = [
  { id: "a1", key: "rigor", name: "Rigor", description: "Detecta fallos metodológicos y lógicos.", icon: "brain", enabled: 1, kind: "judge" },
  { id: "a2", key: "novelty", name: "Novelty", description: "Evalúa la contribución original.", icon: "spark", enabled: 1, kind: "judge" },
  { id: "a3", key: "clarity", name: "Clarity", description: "Revisa claridad y estructura.", icon: "pen", enabled: 0, kind: "judge" },
];

const delay = (ms = 60) => new Promise<void>((r) => setTimeout(r, ms));

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
  createMission: async (m: { question: string; stopCondition: string; successCriterion: string; autonomy: Autonomy; spendCeilingCents: number }) => {
    await delay();
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
      status: "active",
      spendCents: 0,
      spendState: "ok",
    };
    missions.push(mission);
    return mission;
  },
  listMissions: async () => { await delay(); return [...missions]; },
  getMissionRuns: async (missionId: string) => { await delay(); return [...(missionRuns[missionId] ?? [])]; },

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

  // danger zone
  resetDatabase: async () => { await delay(); },
};
