// In-memory mock backend. Activates only when the Tauri runtime is absent
// (i.e. the app is served by the Vite dev server in a plain browser). This
// lets the UI boot and be iterated on without the Rust backend. All state is
// ephemeral and resets on reload.
//
// When Tauri is present (real app or `tauri dev`), this module is never used —
// api.ts routes to the real `invoke` calls instead.

import type { Project, Ref, Review, Action, Chat, ChatAttachment, Agent, McpServer, Message, Mission, MissionRun, Autonomy, Hypothesis, HypothesisStatus, RelationKind, Claim, Skill, FirstValueResult, HypothesisCandidate, RoleConfig, AgentStepResult, Proposal, ApproveOutcome, ProposedPin, ProposedTransition, MorningDigest, DigestRow, TrustStatus, EvidencePin, RuntimeState, SpendState, ScopeDial, ScopeCeiling, MissionMeter, TargetMeter, LastRunSpend, RunReceipt, ReceiptRow, Checkpoint, CheckpointsView, RollbackPlan, RollbackOutcome, OrphanedEvent, OrphanedProposal, RollbackRecord, ExportOutcome, ExportInspect, Job, JobSpec, JobResult, FetchedJobResults, ComputeTargetView, RegisteredAdapter, TargetProbe, SearchDisclosure, SearchDisclosureRow, SearchResult, SearchRunView, ReadinessReport, ReadinessVerdict, ReadinessItem, ReadinessItemKind, ReadinessTrailRow, ZoteroImportResult, DashboardSummary } from "./types";

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

// The AI provider configuration state (Stories 5.7–5.9, FR-17): the mock
// starts HONESTLY unconfigured — the assistant refuses sends until a real
// provider is configured in Ajustes → IA (NFR-11: the mock no longer
// answers the assistant with canned text out of the box).
const aiConfig: {
  mode: string; provider: string; baseUrl: string; model: string;
  hasKey: boolean; cli: string; cliModel: string;
} = { mode: "", provider: "", baseUrl: "", model: "", hasKey: false, cli: "claude", cliModel: "" };

// The curated per-provider model lists (Story 5.9, FR-17.4) — the mock
// mirrors the core's `curated_models` exactly. Custom base URLs: free
// entry (empty list). CLI bridges list exactly ["default"] ("vía CLI").
const CURATED_MODELS: Record<string, string[]> = {
  openai: ["gpt-5.2", "gpt-5-mini", "gpt-4.1", "gpt-4o", "gpt-4o-mini"],
  anthropic: ["claude-opus-4-5", "claude-sonnet-4-5", "claude-haiku-4-5"],
  google: ["gemini-3-pro", "gemini-2-5-pro", "gemini-2-5-flash"],
  openrouter: ["openrouter/auto", "anthropic/claude-sonnet-4.5", "openai/gpt-5.2", "google/gemini-3-pro"],
};

// The mock's honest CLI detection chips: codex and claude simulate as
// present (the seeded dev machine has them); anything else is absent.
const MOCK_CLI_PRESENT = new Set(["codex", "claude"]);

function mockAiConfig() {
  const cliAvailable: Record<string, { path: string } | null> = {};
  for (const name of ["codex", "claude", "opencode"]) {
    cliAvailable[name] = MOCK_CLI_PRESENT.has(name)
      ? { path: `/usr/local/bin/${name}` }
      : null;
  }
  const mode = aiConfig.mode;
  const configured =
    mode === "cli"
      ? !!cliAvailable[aiConfig.cli]
      : mode !== "simulate" &&
        aiConfig.hasKey &&
        (!!aiConfig.baseUrl || CURATED_MODELS[aiConfig.provider] !== undefined);
  const models =
    mode === "cli"
      ? ["default"]
      : CURATED_MODELS[aiConfig.provider] ?? [];
  return {
    mode, provider: aiConfig.provider, baseUrl: aiConfig.baseUrl,
    model: aiConfig.model, hasKey: aiConfig.hasKey,
    cli: aiConfig.cli, cliModel: aiConfig.cliModel,
    cliAvailable, models, configured,
  };
}

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
// Conversation attachments (Story 5.5): the mock keeps them in memory; the
// text the browser read client-side rides the mock reply's context echo.
const chatAttachments: Record<string, ChatAttachment[]> = {};
let attachSeq = 0;
// The curated DEFAULT scientific skill set (Story 5.6, FR-16.6) — mirrors
// the core's seeded registry (same names, same roles, same tool bounds).
const mockSkills: Skill[] = [
  {
    name: "drafter",
    provider: "",
    model: "",
    systemPrompt:
      "Eres el Redactor científico: avanzas el manuscrito — párrafos, related-work, transiciones— siempre anclados en la evidencia fijada del tablero. Cuando propongas texto para el manuscrito, inclúyelo en un bloque citado.",
    tools: ["read_board", "read_library"],
    builtin: true,
  },
  {
    name: "critic",
    provider: "",
    model: "",
    systemPrompt:
      "Eres el Crítico metodológico: evalúas afirmaciones, diseño y evidencia con rigor — señala supuestos débiles, afirmaciones que exceden la evidencia y citas sin anclaje. Nunca redactas texto final; evalúas.",
    tools: ["read_board", "read_library"],
    builtin: true,
  },
  {
    name: "librarian",
    provider: "",
    model: "",
    systemPrompt:
      "Eres el Bibliotecario: buscas y curas literatura — búsquedas dirigidas, síntesis comparativa de referencias, candados de cobertura. Puedes despachar búsquedas; nunca mutas el dominio.",
    tools: ["search", "read_library"],
    builtin: true,
  },
  {
    name: "verifier",
    provider: "",
    model: "",
    systemPrompt:
      "Eres el Verificador: contrastas afirmaciones contra sus fuentes — citas, números, artefactos. Distingues verificación (existencia por código) de confianza (juicio de un modelo); nunca afirmas «verificado» sin código.",
    tools: ["verify", "read_board"],
    builtin: true,
  },
  {
    name: "synthesizer",
    provider: "",
    model: "",
    systemPrompt:
      "Eres el Sintetizador: cruzas hipótesis y evidencia del tablero en hallazgos integradores — patrones, tensiones, vacíos — con trazabilidad a los pines que los sostienen.",
    tools: ["read_board", "read_library"],
    builtin: true,
  },
  {
    name: "note_taker",
    provider: "",
    model: "",
    systemPrompt:
      "Eres el Tomador de notas: registras la sesión de investigación — decisiones, hallazgos, pendientes — en notas concisas y consultables del tablero. Nunca propones mutaciones al dominio.",
    tools: ["read_board"],
    builtin: true,
  },
];
const nowISO = () => "2025-09-01T12:00:00Z";

// In-memory missions so the question box → mission composer flow works in-browser.
const missions: Mission[] = [];
let missionSeq = 0;
// In-memory run lists per mission id (empty until events would reference them).
const missionRuns: Record<string, MissionRun[]> = {};

// In-memory compute targets + jobs (Story 3.2, FR-11.1/11.2/11.4): the
// mock the dev browser's mission card renders. Mirrors the typed core —
// the same freeform-shell rejection before anything lands, the same
// queued → running → terminal lifecycle (terminals always stamped +
// reasoned), advanced here by wall-clock elapsed time since submit.
const mockTargets: ComputeTargetView[] = [
  { name: "local", kind: "local", host: null, allowlisted: null, builtin: true, seq: null, ts: null },
  // a seeded ssh target (Story 3.3): the allowlist editor in Settings has
  // something to work with out of the box — its host starts NOT allowlisted
  {
    name: "gpu-01",
    kind: "ssh",
    host: "gpu-01.lab",
    allowlisted: false,
    builtin: false,
    seq: 1,
    ts: new Date().toISOString(),
  },
  // seeded v0.2.0 kinds (Stories 6.2–6.4): the settings row renders the
  // new chips + their config fields with data to show
  {
    name: "slurm-1",
    kind: "scheduler",
    host: "login.hpc.edu",
    allowlisted: false,
    config: { flavor: "slurm", submitPrefix: "sbatch" },
    builtin: false,
    seq: 2,
    ts: new Date().toISOString(),
  },
  {
    name: "k8s-lab",
    kind: "kubernetes",
    host: null,
    allowlisted: false,
    config: { context: "lab-gpu", namespace: "research", image: "ghcr.io/lab/trainer:latest" },
    builtin: false,
    seq: 3,
    ts: new Date().toISOString(),
  },
  {
    name: "queue-1",
    kind: "chopflow",
    host: null,
    allowlisted: null,
    config: { endpoint: "https://queue.chopflow.dev", queue: "gpu-queue" },
    builtin: false,
    seq: 4,
    ts: new Date().toISOString(),
  },
];

/// The mock allowlist's gate value for a target (mirrors the command
// layer's gate_value): the host ssh/scheduler targets connect to, the
// context a kubernetes target runs on; `None` for kinds with no gate.
function mockGateValue(t: ComputeTargetView): string | null {
  if (t.kind === "ssh" || t.kind === "scheduler") return t.host ?? null;
  if (t.kind === "kubernetes") return t.config?.context ?? null;
  return null;
}

function mockAllowlistedFor(t: ComputeTargetView): boolean | null {
  const gate = mockGateValue(t);
  return gate === null ? null : mockHostAllowlist.includes(gate);
}

const mockRegisteredAdapters: RegisteredAdapter[] = [
  ...["local", "ssh", "scheduler", "kubernetes", "chopflow"].map((kind) => ({
    kind,
    contractVersion: "1",
    builtin: true,
  })),
];
// The mock host allowlist (Story 3.3): hosts ssh targets may connect to;
// hosts outside it are refused before any connection (mirrored).
const mockHostAllowlist: string[] = [];
const mockJobs: Job[] = [];
const mockJobStarts: Record<string, number> = {}; // job id → Date.now() at submit
let mockJobSeq = 0;
let mockJobEventSeq = 0;
// Job timestamps use the real clock (not the fixed nowISO) — the mock
// lifecycle advances with elapsed time, like the real poll loop.
const realNowISO = () => new Date().toISOString();
const MOCK_SHELL_METACHARS = [";", "&", "|", "`", "<", ">", "$", "\n", "\r"];

/// Mirrors the domain's validate_target_config (Stories 6.2–6.4):
/// one-token values, unknown keys refused for the first-party kinds, the
/// required keys present, flavor values validated, endpoints http(s).
function mockValidateConfig(
  kind: string,
  config: Record<string, string>,
  host: string,
): void {
  const allowed: Record<string, string[]> = {
    local: [],
    ssh: [],
    scheduler: ["flavor", "submitPrefix", "pollPrefix", "acctPrefix"],
    kubernetes: ["context", "namespace", "image", "kubectlPrefix"],
    chopflow: ["endpoint", "queue"],
  };
  for (const key of Object.keys(config)) {
    if (allowed[kind] && !allowed[kind].includes(key)) {
      throw new Error(
        `invalid_config: \`${key}\` — kind \`${kind}\` accepts ${allowed[kind].length ? allowed[kind].map((k) => `\`${k}\``).join(" | ") : "no config keys"} (or none)`,
      );
    }
  }
  for (const [key, value] of Object.entries(config)) {
    if (!value.trim() || /\s/.test(value) || value.startsWith("-") || [...value].some((c) => MOCK_SHELL_METACHARS.includes(c))) {
      throw new Error(
        `invalid_config: \`${key}\` — config values are one token each (they become argv elements, never shell strings)`,
      );
    }
  }
  if (kind === "scheduler" && config.flavor && config.flavor !== "slurm" && config.flavor !== "pbs") {
    throw new Error(`invalid_config: \`flavor\` is \`slurm\` or \`pbs\` — got \`${config.flavor}\``);
  }
  if (kind === "kubernetes") {
    for (const required of ["context", "image"]) {
      if (!config[required]) {
        throw new Error(
          `invalid_config: kind \`kubernetes\` requires \`${required}\` — the cluster context the target runs on and the container image its jobs run in`,
        );
      }
    }
  }
  if (kind === "chopflow") {
    const endpoint = config.endpoint ?? "";
    if (!endpoint) {
      throw new Error(
        "invalid_config: kind `chopflow` requires `endpoint` — the ChopFlow queue endpoint the target submits to",
      );
    }
    if (!endpoint.startsWith("http://") && !endpoint.startsWith("https://")) {
      throw new Error(`invalid_config: \`endpoint\` must be an http(s) URL — got \`${endpoint}\``);
    }
  }
  const _ = host;
}

function validateMockSpec(spec: JobSpec): void {
  if (!spec.cmd || !spec.cmd.trim()) {
    throw new Error("invalid_spec: cmd must not be empty — a job names its executable (AD-6)");
  }
  const ch = [...spec.cmd].find((c) => MOCK_SHELL_METACHARS.includes(c));
  if (ch) {
    throw new Error(
      `freeform_shell: cmd \`${spec.cmd}\` carries shell syntax (\`${ch}\`) — the runtime never constructs shell strings; pass arguments in args (AD-6)`,
    );
  }
  for (const key of Object.keys(spec.env ?? {})) {
    if (!key.trim() || key.includes("=") || key.includes("\0")) {
      throw new Error(`invalid_spec: env key \`${key}\` — keys are names: never empty, never \`=\``);
    }
  }
  const r = spec.resources;
  if (r && ((r.cpus != null && r.cpus <= 0) || (r.memoryMb != null && r.memoryMb <= 0))) {
    throw new Error("invalid_spec: resources must be positive — cpus and memoryMb are 1 or more");
  }
  if (spec.workdir != null && !spec.workdir.trim()) {
    throw new Error("invalid_spec: workdir must not be blank when present");
  }
}

// Advance the mock lifecycle by elapsed wall-clock time: queued until
// 700ms after submit, running until 2s, then terminal (finished code 0 —
// `false` demos a reasoned failure). The poll loop, mirrored.
function advanceMockJobs(): void {
  const now = Date.now();
  for (const job of mockJobs) {
    const elapsed = now - (mockJobStarts[job.id] ?? now);
    if (job.phase === "queued" && elapsed >= 700) {
      job.phase = "running";
      job.runningTs = realNowISO();
    }
    if (job.phase === "running" && elapsed >= 2000) {
      job.finishedTs = realNowISO();
      if (job.spec.cmd === "false") {
        job.phase = "failed";
        job.exitCode = 1;
        job.reason = "exit_code_1";
      } else {
        job.phase = "finished";
        job.exitCode = 0;
      }
    }
  }
}

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
  { id: "r1", project_id: "p1", collection_id: null, title: "Attention Is All You Need", authors: "Vaswani et al.", year: 2017, venue: "NeurIPS", doi: "10.48550/arXiv.1706.03762", url: "https://arxiv.org/abs/1706.03762", isbn: "", attachment: null, status: "read", tags: "transformer,attention", used: 1, citation_count: 2, created_at: nowISO(), source: "arxiv", removed: false, timeline: [] },
  { id: "r2", project_id: "p1", collection_id: null, title: "Neural Machine Translation by Jointly Learning to Align and Translate", authors: "Bahdanau et al.", year: 2015, venue: "ICLR", doi: "10.48550/arXiv.1409.0473", url: "https://arxiv.org/abs/1409.0473", isbn: "", attachment: null, status: "read", tags: "attention,NLP", used: 1, citation_count: 1, created_at: nowISO(), source: "arxiv", removed: false, timeline: [] },
  { id: "r3", project_id: "p1", collection_id: null, title: "Scaling Laws for Neural Language Models", authors: "Kaplan et al.", year: 2020, venue: "arXiv", doi: "10.48550/arXiv.2001.08361", url: "https://arxiv.org/abs/2001.08361", isbn: "", attachment: null, status: "read", tags: "scaling,NLP", used: 1, citation_count: 1, created_at: nowISO(), source: "arxiv", removed: false, timeline: [] },
  { id: "r4", project_id: "p1", collection_id: null, title: "A Survey on Large Language Models", authors: "Zhao et al.", year: 2023, venue: "arXiv", doi: "10.48550/arXiv.2303.18223", url: "https://arxiv.org/abs/2303.18223", isbn: "", attachment: null, status: "unread", tags: "survey,LLM", used: 0, citation_count: 0, created_at: nowISO(), source: "arxiv", removed: false, timeline: [] },
];

// The Zotero connection state (FR-9.1 seed): the connector starts DOWN —
// the honest unreachable state the first import attempt surfaces; the
// retry simulates Zotero answering (the deterministic seeded import for
// vite dev).
let zoteroConnectorUp = false;

// The seeded Zotero library the deterministic import pulls in: one item
// duplicates r1 by DOI (the honest "already present" skip), two are new.
const seededZoteroItems: {
  key: string;
  title: string;
  authors: string;
  year: number;
  venue: string;
  doi: string;
  url: string;
  tags: string;
}[] = [
  { key: "ZITEM1", title: "Attention Is All You Need", authors: "Vaswani et al.", year: 2017, venue: "NeurIPS", doi: "10.48550/arXiv.1706.03762", url: "https://arxiv.org/abs/1706.03762", tags: "transformer" },
  { key: "ZITEM2", title: "Mamba: Linear-Time Sequence Modeling with Selective State Spaces", authors: "Gu & Dao", year: 2023, venue: "arXiv", doi: "10.48550/arXiv.2312.00752", url: "https://arxiv.org/abs/2312.00752", tags: "ssm,efficiency" },
  { key: "ZITEM3", title: "Retrieval-Augmented Generation for Knowledge-Intensive NLP Tasks", authors: "Lewis et al.", year: 2020, venue: "NeurIPS", doi: "10.48550/arXiv.2005.11401", url: "https://arxiv.org/abs/2005.11401", tags: "rag,retrieval" },
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

// Story 4.2 mock corpus: the fetchable sources the mock verifier re-reads —
// arXiv abstracts keyed by arXiv id (the demo refs are all arXiv) and
// numerical artifacts keyed by artifact ref. Mirrors the core: the check is
// code, not an LLM call.
const mockSources: Record<string, string> = {
  "1706.03762":
    "Attention Is All You Need. The dominant sequence transduction models are based on recurrent neural networks. We propose the Transformer, a new simple network architecture based solely on attention mechanisms, dispensing with recurrence entirely.",
  "1409.0473":
    "Neural Machine Translation by Jointly Learning to Align and Translate. We propose an extension to the encoder-decoder model which learns to align and translate jointly.",
  "2001.08361":
    "Scaling Laws for Neural Language Models. We study empirical scaling laws for language model performance on the cross-entropy loss.",
  "2303.18223":
    "A Survey on Large Language Models. This paper presents a comprehensive survey on recent advances in large language models.",
};
const mockArtifacts: Record<string, string> = {
  "runs/007/table-3.csv": "accuracy: 0.912, ±0.006, n=5 seeds",
};

/** Whitespace-normalized containment — the documented citation check the
 *  core implements: collapse whitespace runs in both, then contains. */
function excerptAppears(excerpt: string, fetched: string): boolean {
  const norm = (s: string) => s.split(/\s+/).filter(Boolean).join(" ");
  return norm(fetched).includes(norm(excerpt));
}

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

/** Narrow a proposal's intended payload to a transition (Story 3.4: pin
 *  proposals carry the ProposedPin shape instead — callers branch on
 *  proposedKind, mirroring the core's closed vocabulary). */
const asTransition = (p: Proposal): ProposedTransition | null =>
  p.proposedKind === "hypothesis.status_changed"
    ? (p.proposedPayload as ProposedTransition)
    : null;

/** Narrow a proposal's intended payload to a pin candidate. */
const asPin = (p: Proposal): ProposedPin | null =>
  p.proposedKind === "evidence.pinned" ? (p.proposedPayload as ProposedPin) : null;

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

// Recorded mock exports (Story 3.1): folder → the cut its "manifest"
// recorded — the staleness read's state (any rollback after the cut
// stales it, mirroring the core's export_is_stale).
const mockExports = new Map<string, { cutSeq: number }>();

/** The per-entity file list a scope renders — the same shape the core
 *  writes on disk (the mock records the list; the core writes the files). */
function exportFilesFor(scope: string): string[] {
  const slug = (s: string) =>
    s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/(^-|-$)/g, "").slice(0, 40) || "untitled";
  const missionFiles = () => missions.map((m) => `missions/M-${m.seq}-${slug(m.question)}.md`);
  const hypothesisFiles = () => hypotheses.map((h) => `hypotheses/H-${h.seq}-${slug(h.statement)}.md`);
  const evidenceFiles = () => hypotheses.map((h) => `evidence/H-${h.seq}-${slug(h.statement)}.md`);
  const timeline = () => [
    "timeline/events.jsonl",
    "digest/digest.md",
    ...proposals.map((p) => {
      const t = asTransition(p);
      const pin = asPin(p);
      const label = t ? t.to : pin ? (pin.artifact_ref ?? "pin") : "proposal";
      return `proposals/pr-${p.seq}-${slug(label ?? "")}.md`;
    }),
  ];
  const searchLog = () => ["search-log/search-log.md"];
  switch (scope) {
    case "missions": return missionFiles();
    case "hypotheses": return hypothesisFiles();
    case "evidence": return evidenceFiles();
    case "timeline": return timeline();
    case "search_log": return searchLog();
    case "all":
    default:
      return [...missionFiles(), ...hypothesisFiles(), ...evidenceFiles(), ...timeline(), ...searchLog()];
  }
}

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
      const t = asTransition(p);
      orphans.push({
        proposalId: p.id, seq: p.seq,
        targetLabel: p.targetLabel, targetSeq: p.targetSeq,
        proposedTo: t ? t.to : null,
        basis: t ? t.basis : null,
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
 *  the change (the fold's approval-order application, mirrored). A merged
 *  result pin (Story 3.4) pins its anchor claim — never the board. */
function mockApply(p: Proposal, decidedSeq: number): void {
  const h = hypotheses.find((x) => x.id === p.targetEntity);
  if (!h) return;
  const pin = asPin(p);
  if (pin) {
    const claim = claims.find((c) => c.id === pin.claim_id);
    if (!claim) return;
    claim.pinned = true;
    claim.pin = {
      seq: decidedSeq,
      ts: nowISO(),
      claimId: claim.id,
      hypothesisId: pin.hypothesis_id,
      kind: "numerical",
      refId: null,
      artifactRef: pin.artifact_ref ?? null,
      excerpt: pin.excerpt,
      digest: pin.digest,
      confidence: pin.confidence,
      assessingModel: pin.assessing_model,
      refLabel: null,
      // The merge applies a fresh pin — unverified until the verifier runs
      // (Story 4.2); a previous pin's verification never carries over.
      verification: null,
    };
    return;
  }
  const t = asTransition(p);
  if (!t) return;
  h.status = t.to;
  h.audit = { seq: decidedSeq, ts: nowISO(), actor: "agent", basis: t.basis };
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
    // Story 3.4 (FR-11.5): the overnight cluster run — reported with a
    // one-line verdict, its results waiting in quarantine.
    jobsFinished: 1, jobsFailed: 0,
    jobVerdict: { target: "cluster-1", jobId: "3f2a91c4-77b1-4c5e-9a20-8d41c2b6a0f3", failed: false, reason: null },
  },
  {
    missionId: "m22-seed", missionSeq: 22,
    question: "Does sparse attention hold at long context?",
    status: "active", runs: 1, finished: 1, failed: 0, failureReason: null,
    ceilingReached: true, proposalsPending: 0, spendCents: 100, ceilingCents: 100,
    receiptSeq: 102, runId: "nightshift-22", lastRunTs: "2026-09-19T02:14:00Z",
    jobsFinished: 0, jobsFailed: 0, jobVerdict: null,
  },
  {
    missionId: "m24-seed", missionSeq: 24,
    question: "Is linear complexity competitive with quadratic attention?",
    status: "completed", runs: 2, finished: 2, failed: 0, failureReason: null,
    ceilingReached: false, proposalsPending: 0, spendCents: 31, ceilingCents: 100,
    receiptSeq: 103, runId: "nightshift-24", lastRunTs: "2026-09-19T01:44:00Z",
    jobsFinished: 0, jobsFailed: 0, jobVerdict: null,
  },
  {
    missionId: "m26-seed", missionSeq: 26,
    question: "Does MoE routing stay stable under distribution shift?",
    status: "failed", runs: 1, finished: 0, failed: 1, failureReason: "provider_error",
    ceilingReached: false, proposalsPending: 0, spendCents: 0, ceilingCents: 100,
    receiptSeq: 104, runId: "nightshift-26", lastRunTs: "2026-09-19T02:58:00Z",
    jobsFinished: 0, jobsFailed: 0, jobVerdict: null,
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

// Search protocol disclosure (mirrors the event-sourced core, Story 4.1,
// FR-12.1): seeded PRISMA rows for the seeded missions — including the
// honest NULL result (the m21 mission's second search and the failed
// mission's scan found nothing; they still disclose, identically). Live
// searches (runSearch, the mock Night Shift scans) append on top; the
// fold at query time is the disclosure (replay = re-query, same as the
// core's pure fold).
let searchSeq = 150;
const liveSearches: SearchDisclosureRow[] = [];
const seededSearches: SearchDisclosureRow[] = [
  {
    seq: 151, startedAt: "2026-09-19T02:31:19Z",
    query: "retrieval-augmented generation hallucination",
    database: "arxiv", filters: { from_year: 2020 }, order: "relevance", firstPage: true,
    resultCount: 3, nullResult: false, missionId: "m21-seed", runId: "nightshift-21",
  },
  {
    // the honest null: the follow-up search over a narrower corpus —
    // recorded identically, never hidden (FR-12.1)
    seq: 156, startedAt: "2026-09-19T02:44:10Z",
    query: "hallucination long-form generation ablation",
    database: "semantic-scholar", filters: { venue: "neurips" }, order: null, firstPage: true,
    resultCount: 0, nullResult: true, missionId: "m21-seed", runId: "nightshift-21",
  },
  {
    seq: 113, startedAt: "2026-09-19T02:04:12Z",
    query: "Does sparse attention hold at long context?",
    database: "arxiv", filters: {}, order: null, firstPage: true,
    resultCount: 2, nullResult: false, missionId: "m22-seed", runId: "nightshift-22",
  },
  {
    seq: 122, startedAt: "2026-09-19T01:31:09Z",
    query: "Is linear complexity competitive with quadratic attention?",
    database: "arxiv", filters: { from_year: 2019 }, order: "date-desc", firstPage: true,
    resultCount: 1, nullResult: false, missionId: "m24-seed", runId: "nightshift-24",
  },
  {
    // the failed mission's scan searched and found nothing — disclosed
    seq: 132, startedAt: "2026-09-19T02:31:08Z",
    query: "Does MoE routing stay stable under distribution shift?",
    database: "web", filters: {}, order: null, firstPage: true,
    resultCount: 0, nullResult: true, missionId: "m26-seed", runId: "nightshift-17",
  },
];

// Readiness gate (mirrors the event-sourced core, Story 4.3, FR-13): the
// seeded board the dev browser's readiness panel renders — TWO seeds, the
// frame's two variants:
//
// - m21-seed (NOT READY): H-31 still testing, contradicted-by the supported
//   H-32, carrying the frame's three unpinned claims (CLAIMS-2/5/9) — plus
//   the honest null search #156 above, unreckoned while H-31 stays open.
//   The workspace report aggregates these blockers.
// - m24-seed (CLEAN): H-33 supported with two pinned + machine-verified
//   claims — `mockApi.getReadinessReport("m24-seed")` serves the
//   preprint-ready variant with the four-row evidence trail.
//
// The derivation below mirrors the core's fold over the seeded + live
// board (replay = re-query): same blocker categories, same
// info-not-blocker decisions (verified-failed pins, merge-queue pending),
// same trail rows — no scores, only specifics.
const seededReadinessHypotheses: Hypothesis[] = [
  {
    id: "h31-seed", seq: 31, ts: "2026-09-18T22:04:00Z",
    statement: "Retrieval grounding reduces hallucinated citations in long-form generation",
    missionId: "m21-seed", status: "testing",
    relations: [
      { seq: 45, kind: "contradicts", direction: "incoming", otherId: "h32-seed", otherSeq: 32,
        otherStatement: "Grounded generation still hallucinates under distribution shift" },
    ],
    audit: { seq: 44, ts: "2026-09-19T02:50:00Z", actor: "user", basis: "Trial 2 ran; the citations improved but two claims stay unanchored." },
  },
  {
    id: "h32-seed", seq: 32, ts: "2026-09-18T22:06:00Z",
    statement: "Grounded generation still hallucinates under distribution shift",
    missionId: "m21-seed", status: "supported",
    relations: [
      { seq: 45, kind: "contradicts", direction: "outgoing", otherId: "h31-seed", otherSeq: 31,
        otherStatement: "Retrieval grounding reduces hallucinated citations in long-form generation" },
    ],
    audit: { seq: 47, ts: "2026-09-19T03:01:00Z", actor: "user", basis: "The contradiction held across both trials." },
  },
  {
    id: "h33-seed", seq: 33, ts: "2026-09-18T21:40:00Z",
    statement: "Linear-complexity attention stays competitive at long context",
    missionId: "m24-seed", status: "supported",
    relations: [],
    audit: { seq: 46, ts: "2026-09-19T01:52:00Z", actor: "user", basis: "The pinned runs held across five seeds." },
  },
];

// A static verified pin (the digest is inert seed data — the mock never
// re-computes it; the core's constructor guarantees it by construction).
const verifiedPin = (seq: number, claimId: string, hypothesisId: string, refId: string, excerpt: string): EvidencePin => ({
  seq, ts: "2026-09-19T01:50:00Z", claimId, hypothesisId, kind: "citation",
  refId, artifactRef: null, excerpt,
  digest: "3f2a91c477b14c5e9a208d41c2b6a0f33f2a91c477b14c5e9a208d41c2b6a0f3",
  confidence: 0.82, assessingModel: "GLM-5.3", refLabel: "Beltagy et al. 2020",
  verification: { status: "verified", detail: "excerpt_matched", source: "arxiv:2004.05150", ts: "2026-09-19T02:00:00Z" },
});

const seededReadinessClaims: Claim[] = [
  // the frame's three unpinned claims on H-31 (CLAIMS-2/5/9 chips)
  { id: "cl2-seed", seq: 2, ts: "2026-09-18T22:10:00Z", hypothesisId: "h31-seed",
    text: "Grounded citations are copied verbatim 91% of the time",
    sourceMessageId: null, pinned: false, pin: null },
  { id: "cl5-seed", seq: 5, ts: "2026-09-18T22:14:00Z", hypothesisId: "h31-seed",
    text: "Hallucination rate drops below 4% with retrieval grounding",
    sourceMessageId: null, pinned: false, pin: null },
  { id: "cl9-seed", seq: 9, ts: "2026-09-18T22:19:00Z", hypothesisId: "h31-seed",
    text: "Grounding costs under 12% extra latency at 32k context",
    sourceMessageId: null, pinned: false, pin: null },
  // the clean mission's pinned + verified claims
  { id: "cl41-seed", seq: 41, ts: "2026-09-19T01:44:00Z", hypothesisId: "h33-seed",
    text: "Sparse attention matches full attention at 32k context",
    sourceMessageId: null, pinned: true, pin: verifiedPin(60, "cl41-seed", "h33-seed", "ref-sparse", "Sparse attention matches full attention at 32k context, within 0.3 BLEU.") },
  { id: "cl42-seed", seq: 42, ts: "2026-09-19T01:47:00Z", hypothesisId: "h33-seed",
    text: "Memory grows linearly, not quadratically, with context",
    sourceMessageId: null, pinned: true, pin: verifiedPin(61, "cl42-seed", "h33-seed", "ref-sparse", "Memory grows linearly, not quadratically, with context length.") },
];

/** The mock readiness fold (mirrors the core's pure derivation): blockers
 *  each referencing their specific board object, verified-failed pins and
 *  merge-queue pending as info rows (never blockers), the four-row trail. */
function mockReadinessReport(missionId: string | null): ReadinessReport {
  const hyps = [...seededReadinessHypotheses, ...hypotheses];
  const allClaims = [...seededReadinessClaims, ...claims];
  const hypById = new Map(hyps.map((h) => [h.id, h]));
  const inScopeHyps = hyps.filter((h) => !missionId || h.missionId === missionId);
  const inScopeClaims = allClaims.filter((c) => {
    const h = hypById.get(c.hypothesisId);
    return h !== undefined && (!missionId || h.missionId === missionId);
  });
  const inScopeSearches = [...seededSearches, ...liveSearches]
    .filter((r) => !missionId || r.missionId === missionId)
    .sort((a, b) => a.seq - b.seq);

  const blockers: ReadinessItem[] = [];
  const infos: ReadinessItem[] = [];

  // 1. unpinned claims — one blocker per claim, referencing it + its hypothesis
  for (const c of inScopeClaims.filter((c) => !c.pinned)) {
    blockers.push({
      kind: "unpinned_claim", claimId: c.id, claimSeq: c.seq,
      hypothesisId: c.hypothesisId, hypothesisSeq: hypById.get(c.hypothesisId)?.seq ?? null,
      hypothesisStatus: null, searchSeq: null, missionId: null,
      claimTies: [], relationTies: [], pendingCount: 0,
    });
  }

  // 2. load-bearing unresolved / refuted hypotheses (claims or relations on them)
  for (const h of inScopeHyps) {
    const claimTies = inScopeClaims.filter((c) => c.hypothesisId === h.id).map((c) => c.seq);
    const relationTies = h.relations.map((r) => ({
      kind: r.kind, otherSeq: r.otherSeq, incoming: r.direction === "incoming",
    }));
    if (claimTies.length === 0 && relationTies.length === 0) continue;
    if (h.status === "proposed" || h.status === "testing" || h.status === "revised") {
      if (h.status !== "revised") {
        blockers.push({
          kind: "load_bearing_unresolved", claimId: null, claimSeq: null,
          hypothesisId: h.id, hypothesisSeq: h.seq, hypothesisStatus: h.status,
          searchSeq: null, missionId: h.missionId, claimTies, relationTies, pendingCount: 0,
        });
      }
    } else if (h.status === "refuted") {
      blockers.push({
        kind: "load_bearing_refuted", claimId: null, claimSeq: null,
        hypothesisId: h.id, hypothesisSeq: h.seq, hypothesisStatus: h.status,
        searchSeq: null, missionId: h.missionId, claimTies, relationTies, pendingCount: 0,
      });
    }
  }

  // 3. unreckoned null results — a null search blocks while its mission
  //    still carries unresolved hypotheses (the row IS the disclosure; the
  //    reckoning is the board's)
  const unreckoned: number[] = [];
  for (const r of inScopeSearches.filter((r) => r.nullResult)) {
    const missionUnresolved = r.missionId !== null
      && hyps.some((h) => h.missionId === r.missionId
        && (h.status === "proposed" || h.status === "testing" || h.status === "revised"));
    if (missionUnresolved) {
      unreckoned.push(r.seq);
      blockers.push({
        kind: "unreckoned_null_result", claimId: null, claimSeq: null,
        hypothesisId: null, hypothesisSeq: null, hypothesisStatus: null,
        searchSeq: r.seq, missionId: r.missionId, claimTies: [], relationTies: [], pendingCount: 0,
      });
    }
  }

  // infos (never blockers): verified-failed pins + merge-queue pending
  for (const c of inScopeClaims.filter((c) => c.pinned && c.pin?.verification?.status === "failed")) {
    infos.push({
      kind: "pin_verification_failed", claimId: c.id, claimSeq: c.seq,
      hypothesisId: c.hypothesisId, hypothesisSeq: hypById.get(c.hypothesisId)?.seq ?? null,
      hypothesisStatus: null, searchSeq: null, missionId: null,
      claimTies: [], relationTies: [], pendingCount: 0,
    });
  }
  const pendingProposals = proposals.filter((p) =>
    p.status === "pending" && (!missionId || p.missionId === missionId));
  if (pendingProposals.length > 0) {
    infos.push({
      kind: "merge_queue_pending", claimId: null, claimSeq: null,
      hypothesisId: null, hypothesisSeq: null, hypothesisStatus: null,
      searchSeq: null, missionId: missionId, claimTies: [], relationTies: [],
      pendingCount: pendingProposals.length,
    });
  }

  // the trail: what was checked, the counts, the objects
  const pinnedClaims = inScopeClaims.filter((c) => c.pinned);
  const resolvedHyps = inScopeHyps.filter((h) => h.status === "supported" || h.status === "refuted");
  const nullRows = inScopeSearches.filter((r) => r.nullResult);
  const decided = proposals
    .filter((p) => p.decided !== null)
    .sort((a, b) => (b.decided?.seq ?? b.seq) - (a.decided?.seq ?? a.seq));
  const trail: ReadinessTrailRow[] = [
    {
      kind: "claims_pinned", total: inScopeClaims.length, clean: pinnedClaims.length,
      verified: pinnedClaims.filter((c) => c.pin?.verification?.status === "verified").length,
      pending: 0, refs: pinnedClaims.map((c) => `CLAIMS-${c.seq}`),
    },
    {
      kind: "hypotheses_resolved", total: inScopeHyps.length, clean: resolvedHyps.length,
      verified: 0, pending: 0, refs: resolvedHyps.map((h) => `H-${h.seq}`),
    },
    {
      kind: "nulls_disclosed", total: nullRows.length, clean: nullRows.length - unreckoned.length,
      verified: 0, pending: 0, refs: nullRows.map((r) => `#${r.seq}`),
    },
    {
      kind: "merge_queue", total: 0, clean: 0, verified: 0, pending: pendingProposals.length,
      refs: pendingProposals.length > 0
        ? pendingProposals.map((p) => `pr-${p.seq}`)
        : decided.slice(0, 1).map((p) => `pr-${p.seq}`),
    },
  ];

  return {
    scope: missionId,
    verdict: blockers.length === 0 ? "ready" : "not_ready",
    blockers, infos, trail,
  };
}

/** The mock corpus (mirrors the core's simulated arXiv corpus): a query
 *  with no substantive token (≥ 4 chars, not a stopword) matches nothing
 *  — an honest null result, logged identically. */
const MOCK_STOPWORDS = new Set([
  "does", "what", "when", "with", "that", "this", "them", "than", "have", "hold",
]);
const MOCK_CORPUS: SearchResult[] = [
  { title: "Attention Is All You Need", authors: "Vaswani et al.", year: 2017, venue: "arxiv", url: "https://arxiv.org/abs/1706.03762" },
  { title: "Sparse Attention Memory Costs at Long Context", authors: "Beltagy et al.", year: 2020, venue: "arxiv", url: "https://arxiv.org/abs/2004.05150" },
  { title: "Retrieval-Augmented Generation Reduces Hallucination", authors: "Lewis et al.", year: 2020, venue: "acl", url: "https://aclanthology.org/2020.acl-main.612" },
  { title: "Grounding Citations in Retrieved Passages", authors: "Gao et al.", year: 2023, venue: "arxiv", url: "https://arxiv.org/abs/2305.14627" },
  { title: "Lost in the Middle: Context Confuses Language Models", authors: "Liu et al.", year: 2023, venue: "arxiv", url: "https://arxiv.org/abs/2307.03172" },
  { title: "Scaling Laws for Neural Language Models", authors: "Kaplan et al.", year: 2020, venue: "arxiv", url: "https://arxiv.org/abs/2001.08361" },
  { title: "Verifying Claims with Non-LLM Fetchers", authors: "Chen et al.", year: 2024, venue: "arxiv", url: "https://arxiv.org/abs/2402.14871" },
  { title: "kNN-Augmented Language Models", authors: "Khandelwal et al.", year: 2019, venue: "arxiv", url: "https://arxiv.org/abs/1911.00172" },
];

function mockSearchResults(query: string): SearchResult[] {
  const tokens = query
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter((t) => t.length >= 4 && !MOCK_STOPWORDS.has(t));
  if (tokens.length === 0) return [];
  return MOCK_CORPUS.filter((r) => tokens.some((t) => r.title.toLowerCase().includes(t)));
}

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
        to: asTransition(p)?.to ?? "", status: p.status,
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
      // Remote job completions (Story 3.4): the mock counts its finished /
      // failed jobs per mission, latest verdict last.
      const jobs = mockJobs.filter((j) => j.missionId === m.id);
      const jobsFinished = jobs.filter((j) => j.phase === "finished").length;
      const failedJobs = jobs.filter((j) => j.phase === "failed");
      // the latest completed job (mockJobs is append-ordered)
      const latestJob = jobs.filter((j) => j.phase === "finished" || j.phase === "failed").pop();
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
        jobsFinished,
        jobsFailed: failedJobs.length,
        jobVerdict: latestJob
          ? {
              target: latestJob.target,
              jobId: latestJob.id,
              failed: latestJob.phase === "failed",
              reason: latestJob.reason ?? null,
            }
          : null,
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
        source: "arxiv",
        removed: false,
        arxiv_id: paper.arxivId,
        timeline: [],
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
  listRefs: async (_projectId?: string, filter?: string | null) => {
    await delay();
    let refs = [...mockRefs];
    if (filter === "active") refs = refs.filter((r) => !r.removed);
    else if (filter === "removed" || filter === "archived") refs = refs.filter((r) => r.removed);
    return refs;
  },
  getRef: async () => { throw new Error("not in mock"); },
  createRef: async (r: any) => r as Ref,
  updateRef: async () => {},
  deleteRef: async () => {},

  // ---- Evented references CRUD (FR-15, Epic 5) — the mock mirrors the
  // core's domain/library: adds append ref.added semantics (dedup by
  // url/doi/zotero key with the honest already_in_library refusal),
  // removal is the auditable archived state (never destructive), restore
  // is the un-event, and a removed ref is never pinnable.
  addRefFromArxiv: async (url: string): Promise<Ref> => {
    await delay(300);
    const arxivId = parseMockArxivUrl(url);
    const seed = seedPaper(arxivId);
    const urlNorm = `https://arxiv.org/abs/${arxivId}`;
    const doi = `10.48550/arXiv.${arxivId}`;
    const dup = mockRefs.find((r) => r.url === urlNorm || r.doi === doi);
    if (dup) {
      throw new Error(
        `already_in_library: \`${dup.title}\` — this reference is already in the library (added via ${dup.source ?? "arxiv"})`,
      );
    }
    mockEventSeq += 1;
    const ref: Ref = {
      id: "r" + (mockRefs.length + 1) + "-" + Date.now(),
      project_id: "p1",
      collection_id: null,
      title: seed.title,
      authors: seed.authors,
      year: seed.year ?? new Date().getFullYear(),
      venue: "arXiv",
      doi,
      url: urlNorm,
      isbn: "",
      attachment: null,
      status: "unread",
      tags: `arXiv,${arxivId}`,
      used: 0,
      citation_count: 0,
      created_at: nowISO(),
      source: "arxiv",
      removed: false,
      arxiv_id: arxivId,
      timeline: [{ seq: mockEventSeq, ts: nowISO(), actor: "user", kind: "ref.added" }],
    };
    mockRefs.push(ref);
    return { ...ref, timeline: (ref.timeline ?? []).map((e) => ({ ...e })) };
  },
  addRefManual: async (
    title: string,
    authors: string,
    year: number | null,
    venue: string,
    doi: string,
    url: string,
    tags: string,
  ): Promise<Ref> => {
    await delay();
    const trimmedTitle = title.trim();
    if (!trimmedTitle) {
      throw new Error(
        "invalid_ref: ref.title must not be empty — a reference is titled (Required to launch / Requerido)",
      );
    }
    const trimmedDoi = doi.trim();
    const trimmedUrl = url.trim();
    if (!trimmedDoi && !trimmedUrl) {
      throw new Error("invalid_ref: a manual reference carries at least one identifier (doi or url)");
    }
    const dup = mockRefs.find(
      (r) =>
        (trimmedDoi && r.doi === trimmedDoi) || (trimmedUrl && r.url === trimmedUrl),
    );
    if (dup) {
      throw new Error(
        `already_in_library: \`${dup.title}\` — this reference is already in the library (added via ${dup.source ?? "manual"})`,
      );
    }
    mockEventSeq += 1;
    const ref: Ref = {
      id: "r" + (mockRefs.length + 1) + "-" + Date.now(),
      project_id: "p1",
      collection_id: null,
      title: trimmedTitle,
      authors: authors.trim(),
      year: year ?? new Date().getFullYear(),
      venue: venue.trim(),
      doi: trimmedDoi,
      url: trimmedUrl,
      isbn: "",
      attachment: null,
      status: "unread",
      tags: tags.trim(),
      used: 0,
      citation_count: 0,
      created_at: nowISO(),
      source: "manual",
      removed: false,
      timeline: [{ seq: mockEventSeq, ts: nowISO(), actor: "user", kind: "ref.added" }],
    };
    mockRefs.push(ref);
    return { ...ref, timeline: (ref.timeline ?? []).map((e) => ({ ...e })) };
  },
  // The seeded Zotero connection is DOWN (the FR-9.1 seed) — the first
  // import attempt surfaces it honestly; the retry simulates the connector
  // answering (the deterministic seeded import for vite dev).
  importRefsFromZotero: async (): Promise<ZoteroImportResult> => {
    await delay(400);
    if (!zoteroConnectorUp) {
      zoteroConnectorUp = true;
      throw new Error(
        "zotero_unreachable: unreachable — the Zotero connector is not answering; is Zotero running with the local API enabled? / el conector de Zotero no responde; ¿está Zotero en ejecución con la API local activada?",
      );
    }
    const result: ZoteroImportResult = {
      imported: 0,
      skipped: 0,
      failed: 0,
      refs: [],
      skippedItems: [],
      failedItems: [],
    };
    for (const item of seededZoteroItems) {
      const title = item.title.trim();
      if (!title) {
        result.failed += 1;
        result.failedItems.push(`${item.key} — no title`);
        continue;
      }
      const dup = mockRefs.find(
        (r) =>
          (item.doi && r.doi === item.doi) ||
          (item.url && r.url === item.url) ||
          r.zotero_item_key === item.key,
      );
      if (dup) {
        result.skipped += 1;
        result.skippedItems.push(
          `${dup.title} — already in the library (${dup.source ?? "zotero"})`,
        );
        continue;
      }
      mockEventSeq += 1;
      const ref: Ref = {
        id: "r" + (mockRefs.length + 1) + "-" + Date.now(),
        project_id: "p1",
        collection_id: null,
        title,
        authors: item.authors,
        year: item.year,
        venue: item.venue,
        doi: item.doi,
        url: item.url,
        isbn: "",
        attachment: null,
        status: "unread",
        tags: `zotero${item.tags ? "," + item.tags : ""}`,
        used: 0,
        citation_count: 0,
        created_at: nowISO(),
        source: "zotero",
        removed: false,
        zotero_item_key: item.key,
        timeline: [{ seq: mockEventSeq, ts: nowISO(), actor: "user", kind: "ref.added" }],
      };
      mockRefs.push(ref);
      result.imported += 1;
      result.refs.push({ ...ref, timeline: (ref.timeline ?? []).map((e) => ({ ...e })) });
    }
    return result;
  },
  removeRef: async (refId: string): Promise<Ref> => {
    await delay();
    const ref = mockRefs.find((r) => r.id === refId);
    if (!ref) throw new Error(`not_found: no reference with id \`${refId}\` in the library`);
    if (ref.removed) {
      throw new Error(
        `invalid_state: the reference \`${refId}\` is already removed — restore it first (FR-15.7)`,
      );
    }
    mockEventSeq += 1;
    ref.removed = true;
    ref.timeline = [...(ref.timeline ?? []), { seq: mockEventSeq, ts: nowISO(), actor: "user", kind: "ref.removed" }];
    return { ...ref, timeline: (ref.timeline ?? []).map((e) => ({ ...e })) };
  },
  restoreRef: async (refId: string): Promise<Ref> => {
    await delay();
    const ref = mockRefs.find((r) => r.id === refId);
    if (!ref) throw new Error(`not_found: no reference with id \`${refId}\` in the library`);
    if (!ref.removed) {
      throw new Error(
        `invalid_state: the reference \`${refId}\` is not removed — nothing to restore`,
      );
    }
    mockEventSeq += 1;
    ref.removed = false;
    ref.timeline = [...(ref.timeline ?? []), { seq: mockEventSeq, ts: nowISO(), actor: "user", kind: "ref.restored" }];
    return { ...ref, timeline: (ref.timeline ?? []).map((e) => ({ ...e })) };
  },
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

  // chats — mirrors the typed core's scoped surface (Stories 5.4–5.6):
  // mission scope + skill per conversation, messages carrying the scope,
  // attachments as chips, and the honest context echo on every reply.
  listChats: async (_pid: string, kind?: string) => {
    await delay();
    return chats.filter((c) => !kind || c.kind === kind).map((c) => ({ ...c }));
  },
  createChat: async (pid: string, kind: string, title: string, missionId?: string | null, skill?: string | null, model?: string | null) => {
    await delay();
    if (missionId && !missions.some((m) => m.id === missionId)) {
      throw new Error(`unknown mission \`${missionId}\` — a conversation can only scope to a mission that exists`);
    }
    const skillName = (skill || "").trim();
    if (skillName && !mockSkills.some((s) => s.name === skillName)) {
      throw new Error(`unknown skill \`${skillName}\` — a conversation can only run a registered skill`);
    }
    const id = "c" + ++chatSeq;
    const c: Chat = {
      id, project_id: pid, kind, title, preview: "",
      mission_id: missionId || null,
      skill: skillName || null,
      model: (model || "").trim() || null,
      created_at: nowISO(), updated_at: nowISO(),
    };
    chats.push(c);
    messages[id] = [];
    chatAttachments[id] = [];
    return { ...c };
  },
  getChat: async (id: string) => {
    await delay();
    const c = chats.find((x) => x.id === id);
    if (!c) throw new Error("chat not found");
    return {
      ...c,
      messages: (messages[id] ?? []).map((m) => ({ ...m })),
      attachments: (chatAttachments[id] ?? []).map((a) => ({ ...a })),
    };
  },
  sendMessage: async (chatId: string, content: string) => {
    await delay(120);
    const c = chats.find((x) => x.id === chatId);
    if (!c) throw new Error("chat not found");
    // The assistant honesty rule (Story 5.7, NFR-11): with NO real provider
    // configured, assistant sends are refused with the same typed error the
    // core returns — the mock no longer answers the assistant with canned
    // text. Other kinds (review) keep the simulated guarantee (FR-17.5).
    if (c.kind !== "review" && !mockAiConfig().configured) {
      throw new Error(
        "no_provider_configured: the assistant needs a real provider (an API provider or a CLI bridge) — configure one in Ajustes → IA / el asistente necesita un proveedor real (un proveedor de API o un puente CLI) — configura uno en Ajustes → IA",
      );
    }
    const list = messages[chatId] ?? (messages[chatId] = []);
    list.push({ id: "m" + ++msgSeq, chat_id: chatId, role: "user", content, classify_tag: null, meta: null, mission_id: c.mission_id, created_at: nowISO() });
    // The honest context echo (mirrors the core's simulated provider): the
    // scoped context this send carried — board context, skill, attachments —
    // is echoed back so vite dev demonstrates the wiring.
    const echo: string[] = [];
    if (c.mission_id) {
      const mission = missions.find((m) => m.id === c.mission_id);
      if (mission) {
        const hyps = hypotheses.filter((h) => h.missionId === mission.id);
        echo.push(`contexto: tablero M-${mission.seq} sincronizado (${hyps.length} hipótesis)`);
      }
    }
    if (c.skill) echo.push(`skill: ${c.skill}`);
    const attachCount = (chatAttachments[chatId] ?? []).length;
    if (attachCount > 0) echo.push(`adjuntos: ${attachCount}`);
    const base = "(mock) Entendido. Esta es una respuesta de ejemplo del asistente.";
    const reply = echo.length ? `${base}\n\n— ${echo.join(" · ")}` : base;
    // The reply carries its attribution (Story 5.7/5.9): the provider +
    // model that produced it — the message-render idiom shows it.
    const ai = mockAiConfig();
    const provider = ai.mode === "cli" ? `cli/${ai.cli}` : ai.provider || "mock";
    const model = c.model || ai.model || (ai.mode === "cli" ? "default" : "mock-model");
    const meta = JSON.stringify({ provider, model });
    list.push({ id: "m" + ++msgSeq, chat_id: chatId, role: "agent", content: reply, classify_tag: null, meta, mission_id: c.mission_id, created_at: nowISO() });
    c.preview = content.slice(0, 80);
    c.updated_at = nowISO();
    return { ok: true };
  },
  setChatScope: async (chatId: string, missionId: string | null) => {
    await delay();
    const c = chats.find((x) => x.id === chatId);
    if (!c) throw new Error(`chat \`${chatId}\` not found`);
    if (missionId && !missions.some((m) => m.id === missionId)) {
      throw new Error(`unknown mission \`${missionId}\` — a conversation can only scope to a mission that exists`);
    }
    c.mission_id = missionId || null;
    c.updated_at = nowISO();
    return { ...c };
  },
  setChatSkill: async (chatId: string, skill: string | null) => {
    await delay();
    const c = chats.find((x) => x.id === chatId);
    if (!c) throw new Error(`chat \`${chatId}\` not found`);
    const skillName = (skill || "").trim();
    if (skillName && !mockSkills.some((s) => s.name === skillName)) {
      throw new Error(`unknown skill \`${skillName}\` — a conversation can only run a registered skill`);
    }
    c.skill = skillName || null;
    c.updated_at = nowISO();
    return { ...c };
  },
  // The per-conversation model choice (Story 5.9, FR-17.4): per-chat and
  // history-preserving — earlier messages keep the attribution they were
  // produced with.
  setChatModel: async (chatId: string, model: string | null) => {
    await delay();
    const c = chats.find((x) => x.id === chatId);
    if (!c) throw new Error(`chat \`${chatId}\` not found`);
    c.model = (model || "").trim() || null;
    c.updated_at = nowISO();
    return { ...c };
  },
  // Skills (Story 5.6): the seeded six + user-added — a skill is data.
  listSkills: async (): Promise<Skill[]> => {
    await delay();
    return mockSkills.map((s) => ({ ...s, tools: [...s.tools] }));
  },
  addChatAttachments: async (
    chatId: string,
    files: { name: string; path?: string | null; content?: string | null }[],
  ) => {
    await delay();
    const c = chats.find((x) => x.id === chatId);
    if (!c) throw new Error(`chat \`${chatId}\` not found`);
    const list = chatAttachments[chatId] ?? (chatAttachments[chatId] = []);
    const attached: ChatAttachment[] = [];
    const refused: { name: string; reason: string }[] = [];
    for (const f of files) {
      const lower = f.name.toLowerCase();
      // classify exactly like the core: extension + honest content check
      let kind: ChatAttachment["kind"] = "binary";
      if (lower.endsWith(".md") || lower.endsWith(".txt") || lower.endsWith(".tex")) kind = "text";
      else if (lower.endsWith(".pdf")) kind = "pdf";
      if (kind === "text" && typeof f.content !== "string") {
        refused.push({ name: f.name, reason: "`" + f.name + "`: archivo de texto ilegible / unreadable text file" });
        continue;
      }
      let included = false;
      let note = "";
      if (kind === "text") included = true;
      else if (kind === "pdf") note = "pdf-extraction-unavailable (el texto PDF se extrae en la app de escritorio)";
      else note = "unsupported-inline v1";
      const a: ChatAttachment = {
        id: "at" + ++attachSeq + "-" + Date.now(),
        name: f.name,
        kind,
        digest: "mock-" + attachSeq,
        size_bytes: (f.content ?? "").length,
        included,
        truncated: false,
        note,
      };
      list.push(a);
      attached.push(a);
    }
    return { attached, refused };
  },
  removeChatAttachment: async (chatId: string, attachmentId: string) => {
    await delay();
    const list = chatAttachments[chatId] ?? [];
    const i = list.findIndex((a) => a.id === attachmentId);
    if (i >= 0) list.splice(i, 1);
    return list.map((a) => ({ ...a }));
  },
  deleteChat: async (id: string) => {
    const i = chats.findIndex((x) => x.id === id);
    if (i >= 0) chats.splice(i, 1);
    delete messages[id];
    delete chatAttachments[id];
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
  // CLI detection chips (Story 5.8): honest in the mock — codex and claude
  // simulate as present, anything else is absent (never a dead spawn).
  testCli: async (command: string) =>
    MOCK_CLI_PRESENT.has(command.trim())
      ? { command, path: `/usr/local/bin/${command}` }
      : { command, path: null },
  // The AI provider configuration (Stories 5.7–5.9): the read the
  // assistant's unconfigured state and Ajustes → IA render (never the key —
  // only its presence), the configure/switch mutations, and the connection
  // test (which doubles as the live model list for the picker).
  getAiConfig: async () => {
    await delay();
    return mockAiConfig();
  },
  configureAiProvider: async (provider: string, baseUrl: string, model: string, apiKey: string) => {
    await delay();
    aiConfig.mode = "provider";
    aiConfig.provider = provider.trim();
    aiConfig.baseUrl = baseUrl.trim();
    aiConfig.model = model.trim();
    if (apiKey.trim()) aiConfig.hasKey = true;
    return mockAiConfig();
  },
  useCliBridge: async (cli: string) => {
    await delay();
    const name = cli.trim() || "claude";
    if (!MOCK_CLI_PRESENT.has(name)) {
      throw new Error(
        `cli_unavailable: the \`${name}\` CLI was not found on PATH — install it first / el CLI \`${name}\` no está en PATH`,
      );
    }
    aiConfig.mode = "cli";
    aiConfig.cli = name;
    return mockAiConfig();
  },
  testProviderConnection: async () => {
    await delay(400);
    if (!aiConfig.hasKey) {
      return {
        ok: false,
        models: [] as string[],
        error: `provider \`${aiConfig.provider || "?"}\` has no API key stored — save one first / no hay clave guardada`,
      };
    }
    const models = CURATED_MODELS[aiConfig.provider] ?? [];
    if (!models.length) {
      return {
        ok: false,
        models: [] as string[],
        error: "the provider answered but listed no models / el proveedor respondió sin modelos",
      };
    }
    return { ok: true, models, error: null };
  },
  listProviderModels: async (provider: string) => {
    await delay();
    return [...(CURATED_MODELS[provider.trim()] ?? [])];
  },

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
  // The dashboard's one aggregated read (Story 5.10, FR-18.1): the same
  // composition the core folds — every widget over the mock's own reads,
  // read-only by construction (derives from the live mock state + the
  // seeded night, appends nothing).
  getDashboardSummary: async (): Promise<DashboardSummary> => {
    await delay();
    // recent receipts: the seeded night's ledgers + any live run receipts,
    // newest start first, capped at five (mirrors the core's fold)
    const candidates: { runId: string; startedTs: string }[] =
      Object.values(seededReceipts).map((r) => ({ runId: r.runId, startedTs: r.startedTs }));
    for (const runs of Object.values(missionRuns)) {
      for (const r of runs) {
        if (r.kind === "run.started" && r.runId) {
          candidates.push({ runId: r.runId, startedTs: r.ts });
        }
      }
    }
    const seen = new Set<string>();
    const recentReceipts = candidates
      .filter((c) => (seen.has(c.runId) ? false : seen.add(c.runId)))
      .sort((a, b) => (a.startedTs < b.startedTs ? 1 : -1))
      .slice(0, 5)
      .map((c) => seededReceipts[c.runId] ?? mockReceiptFor(c.runId))
      .filter((r): r is RunReceipt => r !== null)
      .map((r) => ({ ...r, rows: r.rows.map((x) => ({ ...x })), models: [...r.models] }));
    const d = currentMockDigest();
    return {
      missions: [...missions],
      // the seeded night's board + the live board — the same hypotheses the
      // readiness fold sees (replay = re-query)
      hypotheses: [...seededReadinessHypotheses, ...hypotheses].map((h) => ({ ...h })),
      digest: { ...d, rows: d.rows.map((r) => ({ ...r })), alerts: d.alerts.map((a) => ({ ...a })) },
      trust: mockTrustStatus(),
      recentReceipts,
      readiness: mockReadinessReport(null),
    };
  },
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
  // Search protocol disclosure (Story 4.1, FR-12.1): the ONE search entry
  // — executes the search, records the PRISMA row, returns the results.
  // Null results record identically (resultCount 0, nullResult true) —
  // never a hidden nothing.
  runSearch: async (
    query: string,
    database: string,
    filters: Record<string, unknown> | null,
    order: string | null,
    firstPage: boolean,
    missionId: string | null,
  ): Promise<SearchRunView> => {
    await delay(150);
    const q = query.trim();
    if (!q) throw new Error("invalid_query: a search records what was searched — the query cannot be empty (FR-12.1)");
    const db = database.trim();
    if (!db) throw new Error("invalid_database: a search records where it ran — name the database (FR-12.1)");
    const results = mockSearchResults(q);
    const row: SearchDisclosureRow = {
      seq: ++searchSeq,
      startedAt: nowISO(),
      query: q,
      database: db,
      filters: filters ?? {},
      order: order && order.trim() ? order.trim() : null,
      firstPage,
      resultCount: results.length,
      nullResult: results.length === 0,
      missionId,
      runId: null, // a UI/assistant search runs as the user
    };
    liveSearches.push(row);
    return { results: results.map((r) => ({ ...r })), row: { ...row } };
  },
  // The disclosure read model (FR-12.1): every search's PRISMA row in seq
  // order — mission-scoped when an id is given, else workspace-wide.
  getSearchDisclosure: async (missionId: string | null): Promise<SearchDisclosure> => {
    await delay();
    const rows = [...seededSearches, ...liveSearches]
      .filter((r) => (missionId ? r.missionId === missionId : true))
      .sort((a, b) => a.seq - b.seq)
      .map((r) => ({ ...r }));
    return {
      rows,
      total: rows.length,
      nullResultCount: rows.filter((r) => r.nullResult).length,
    };
  },
  // Readiness gate (Story 4.3, FR-13.1/13.2): the preprint-tier report —
  // the same pure derivation the core folds (replay = re-query). The
  // workspace report aggregates the seeded dirty board; the seeded clean
  // mission serves the preprint-ready variant.
  getReadinessReport: async (missionId: string | null): Promise<ReadinessReport> => {
    await delay();
    return mockReadinessReport(missionId);
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
    // FR-15.5 (Epic 5): a removed ref is never pinnable — the typed
    // refusal, mirrored from the core's pin validation.
    if (ref.removed) {
      throw new Error(
        `ref_removed: \`${trimmedRef}\` — this reference was removed from the library and cannot be pinned (restore it first, FR-15.5)`,
      );
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
    // the caller (AD-5). Re-pinning: a previous verification never carries
    // over to the new excerpt — it stays visible as stale (Story 4.2).
    const digest = await sha256Hex(excerpt);
    const carriedStale = claim.pin?.verification
      ? { ...claim.pin.verification, status: "stale" as const }
      : null;
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
      refRemoved: false,
      verification: carriedStale,
    };
    return { ...claim, pin: { ...claim.pin } };
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
    const carriedStale = claim.pin?.verification
      ? { ...claim.pin.verification, status: "stale" as const }
      : null;
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
      verification: carriedStale,
    };
    return { ...claim };
  },
  listEvidence: async (hypothesisId: string) => {
    await delay();
    return claims
      .filter((c) => c.hypothesisId === hypothesisId)
      .map((c) => ({
        ...c,
        // FR-15.6: the pin stays pinned; the "source removed" flag reads
        // the ref's CURRENT archived state (it clears on restore).
        pin: c.pin
          ? {
              ...c.pin,
              refRemoved: c.pin.refId
                ? (mockRefs.find((r) => r.id === c.pin!.refId)?.removed ?? false)
                : false,
            }
          : null,
      }));
  },
  // Pin verification (Story 4.2, FR-14.1): the mock mirrors the core's
  // no-LLM check — re-read each pin's source from the seeded corpus and
  // set the latest verification on the pin. Failures mark visibly and
  // never delete the pin; re-verification overwrites (latest wins).
  runPinVerification: async (hypothesisId: string | null) => {
    await delay();
    for (const claim of claims) {
      if (hypothesisId && claim.hypothesisId !== hypothesisId) continue;
      const pin = claim.pin;
      if (!claim.pinned || !pin) continue; // unpinned — nothing to verify
      let status: "verified" | "failed";
      let detail: string;
      let source: string;
      if (pin.kind === "citation") {
        const ref = mockRefs.find((r) => r.id === pin.refId);
        // Source resolution mirrors the core: the arXiv id from the DOI or
        // URL when there is one, else the ref's URL.
        const doiArxiv = ref?.doi.match(/^10\.48550\/arXiv\.(.+)$/)?.[1] ?? "";
        const urlArxiv = ref?.url.match(/arxiv\.org\/(?:abs|pdf)\/([^/.]+)/)?.[1] ?? "";
        const arxivId = doiArxiv || urlArxiv;
        if (arxivId) {
          source = `arxiv:${arxivId}`;
          const text = mockSources[arxivId];
          detail = text === undefined ? "fetch_error" : excerptAppears(pin.excerpt, text) ? "excerpt_matched" : "not_found";
          status = detail === "excerpt_matched" ? "verified" : "failed";
        } else if (ref?.url) {
          source = ref.url;
          // The mock has no corpus for non-arXiv URLs — an honest fetch
          // error, like the core against an unreachable source.
          detail = "fetch_error";
          status = "failed";
        } else {
          source = pin.refId ?? "";
          detail = "no_source";
          status = "failed";
        }
      } else {
        source = pin.artifactRef ?? "";
        const content = mockArtifacts[source];
        if (content === undefined) {
          detail = "artifact_missing";
          status = "failed";
        } else if ((await sha256Hex(content)) === pin.digest) {
          detail = "digest_ok";
          status = "verified";
        } else if (excerptAppears(pin.excerpt, content)) {
          detail = "excerpt_matched";
          status = "verified";
        } else {
          detail = "artifact_changed";
          status = "failed";
        }
      }
      pin.verification = { status, detail, source, ts: nowISO() };
    }
    return claims
      .filter((c) => !hypothesisId || c.hypothesisId === hypothesisId)
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
      // Story 4.1 (FR-12.1): the scan's literature search runs through
      // the ONE search seam — the PRISMA row lands inside the run before
      // anything else, null results included and logged identically.
      const results = mockSearchResults(mission.question);
      searchSeq += 1;
      liveSearches.push({
        seq: searchSeq,
        startedAt: nowISO(),
        query: mission.question,
        database: "arxiv",
        filters: {},
        order: null,
        firstPage: true,
        resultCount: results.length,
        nullResult: results.length === 0,
        missionId: mission.id,
        runId,
      });
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

  // compute targets + jobs (Story 3.2, FR-11.1/11.2/11.4) — mirrors the
  // typed core: the same freeform-shell rejection before anything lands,
  // the same queued → running → terminal lifecycle with stamped +
  // reasoned terminals, advanced by elapsed time on every read.
  listComputeTargets: async (): Promise<ComputeTargetView[]> => {
    await delay();
    return mockTargets.map((t) => ({ ...t, allowlisted: mockAllowlistedFor(t) }));
  },
  getRegisteredAdapters: async (): Promise<RegisteredAdapter[]> => {
    await delay();
    return mockRegisteredAdapters.map((a) => ({ ...a }));
  },
  probeComputeTarget: async (name: string): Promise<TargetProbe> => {
    await delay();
    const target = mockTargets.find((t) => t.name === name);
    if (!target) {
      throw new Error(`unknown_target: \`${name}\` — no compute target with that name`);
    }
    if (target.kind === "local") {
      return { status: "ok", detail: "local machine — always reachable" };
    }
    if (target.kind === "chopflow") {
      return { status: "ok", detail: `queue answered (${target.config?.endpoint ?? "?"})` };
    }
    return {
      status: mockAllowlistedFor(target) === false ? "unreachable" : "ok",
      detail:
        mockAllowlistedFor(target) === false
          ? `${mockGateValue(target)} not allowlisted — add it in Settings → Compute targets`
          : `${target.kind} answered (${mockGateValue(target) ?? "local"})`,
    };
  },
  getHostAllowlist: async (): Promise<string[]> => {
    await delay();
    return [...mockHostAllowlist];
  },
  setHostAllowlist: async (hosts: string[]): Promise<string[]> => {
    await delay();
    for (const host of hosts) {
      const trimmed = host.trim();
      if (!trimmed || /\s/.test(trimmed) || trimmed.startsWith("-") || [...trimmed].some((c) => MOCK_SHELL_METACHARS.includes(c))) {
        throw new Error(
          `invalid_host: \`${host}\` — the allowlist holds one-token hosts (e.g. gpu-01.lab, user@10.0.0.4)`,
        );
      }
    }
    mockHostAllowlist.length = 0;
    mockHostAllowlist.push(...hosts.map((h) => h.trim()));
    return [...mockHostAllowlist];
  },
  declareComputeTarget: async (
    name: string,
    kind: string,
    host?: string | null,
    config?: Record<string, string>,
  ): Promise<ComputeTargetView[]> => {
    await delay();
    const trimmedName = name.trim();
    const trimmedKind = kind.trim();
    const trimmedHost = (host ?? "").trim();
    const trimmedConfig = config ?? {};
    if (!mockRegisteredAdapters.some((a) => a.kind === trimmedKind)) {
      throw new Error(
        `unknown_kind: \`${trimmedKind}\` — no adapter of that kind is registered (${mockRegisteredAdapters.map((a) => a.kind).join(" | ")})`,
      );
    }
    if (!/^[a-z0-9]([a-z0-9-]*[a-z0-9])?$/.test(trimmedName)) {
      throw new Error(
        "invalid_name: `" + trimmedName + "` — expected lowercase letters, digits and dashes (e.g. laptop, cluster-1)",
      );
    }
    if (trimmedKind === "ssh" && !trimmedHost) {
      throw new Error(
        "target.declared requires a host for kind `ssh` — the target names the machine it connects to",
      );
    }
    if (trimmedKind !== "ssh" && trimmedKind !== "scheduler" && trimmedHost) {
      throw new Error(
        `invalid_host: a \`${trimmedKind}\` target carries no host — only ssh and scheduler targets name the machine they connect to`,
      );
    }
    if (trimmedHost && (/\s/.test(trimmedHost) || trimmedHost.startsWith("-") || [...trimmedHost].some((c) => MOCK_SHELL_METACHARS.includes(c)))) {
      throw new Error(
        `invalid_host: \`${trimmedHost}\` — a host is one token (it becomes one argv element of ssh)`,
      );
    }
    mockValidateConfig(trimmedKind, trimmedConfig, trimmedHost);
    if (!mockTargets.some((t) => t.name === trimmedName)) {
      const view: ComputeTargetView = {
        name: trimmedName,
        kind: trimmedKind,
        host: trimmedKind === "ssh" || trimmedKind === "scheduler" ? (trimmedHost || null) : null,
        allowlisted: null,
        config: Object.keys(trimmedConfig).length ? { ...trimmedConfig } : undefined,
        builtin: false,
        seq: ++mockJobEventSeq,
        ts: realNowISO(),
      };
      view.allowlisted = mockAllowlistedFor(view);
      mockTargets.push(view);
    }
    return mockTargets.map((t) => ({ ...t, allowlisted: mockAllowlistedFor(t) }));
  },
  submitJob: async (missionId: string, target: string, spec: JobSpec): Promise<Job> => {
    await delay();
    const mission = missions.find((m) => m.id === missionId);
    if (!mission) throw new Error(`not_found: no mission with id \`${missionId}\``);
    validateMockSpec(spec);
    if (!mockTargets.some((t) => t.name === target)) {
      throw new Error(`unknown_target: \`${target}\` — declared targets: ${mockTargets.map((t) => t.name).join(" | ")}`);
    }
    // The allowlist gate (Story 3.3, mirrored): an ssh target's host must
    // be allowlisted BEFORE any connection is attempted.
    const sshTarget = mockTargets.find((t) => t.name === target && t.kind === "ssh");
    if (sshTarget && !mockHostAllowlist.includes(sshTarget.host ?? "")) {
      throw new Error(
        `host_not_allowed: \`${sshTarget.host}\` — hosts outside the allowlist are refused before any connection is attempted (allowlist: ${mockHostAllowlist.length ? mockHostAllowlist.join(" | ") : "empty — add hosts in Settings → Compute targets"})`,
      );
    }
    mockJobSeq += 1;
    mockJobEventSeq += 1;
    const id = "job-" + mockJobSeq + "-" + Date.now();
    const job: Job = {
      id,
      seq: mockJobEventSeq,
      ts: realNowISO(),
      missionId,
      target,
      handle: "h-" + id,
      spec: {
        cmd: spec.cmd,
        args: [...(spec.args ?? [])],
        env: { ...(spec.env ?? {}) },
        ...(spec.resources ? { resources: { ...spec.resources } } : {}),
        ...(spec.workdir != null && spec.workdir !== "" ? { workdir: spec.workdir } : {}),
      },
      phase: "queued",
      exitCode: null,
      reason: null,
      runningTs: null,
      finishedTs: null,
    };
    mockJobs.push(job);
    mockJobStarts[job.id] = Date.now();
    // the submission surfaces in the mission's runs drill-down (receipt voice)
    missionRuns[missionId] = missionRuns[missionId] ?? [];
    missionRuns[missionId].push({
      seq: ++mockJobEventSeq,
      id: "r" + mockJobEventSeq + "-" + Date.now(),
      ts: realNowISO(),
      kind: "job.submitted",
      actor: "user",
    });
    advanceMockJobs();
    return { ...job };
  },
  pollJobs: async (missionId: string): Promise<Job[]> => {
    await delay();
    if (!missions.some((m) => m.id === missionId)) {
      throw new Error(`not_found: no mission with id \`${missionId}\``);
    }
    advanceMockJobs();
    return mockJobs.filter((j) => j.missionId === missionId).map((j) => ({ ...j }));
  },
  fetchJob: async (jobId: string): Promise<JobResult> => {
    await delay();
    advanceMockJobs();
    const job = mockJobs.find((j) => j.id === jobId);
    if (!job) throw new Error(`not_found: no job with id \`${jobId}\``);
    if (job.phase === "queued" || job.phase === "running") {
      throw new Error(`job_not_terminal: the job is still ${job.phase} — fetch waits for it to end`);
    }
    const argv = [job.spec.cmd, ...(job.spec.args ?? [])].join(" ");
    if (job.phase === "failed") {
      return { code: job.exitCode ?? null, stdout: "", stderr: `[mock] ${argv} failed: ${job.reason}` };
    }
    return {
      code: 0,
      stdout: `[mock] ${argv}\n[mock] completed on target \`${job.target}\` — 2.0s, exit 0`,
      stderr: "",
    };
  },
  // Fetch results → quarantined evidence (Story 3.4, FR-11.5, AD-3/AD-5):
  // mirrors the typed core — the finished job's captured stdout becomes ONE
  // proposal per meaningful artifact whose intended payload is a numerical
  // pin (artifact_ref + sha-256 digest computed here, never from the
  // caller), anchored to an UNPINNED claim on the mission's first
  // hypothesis. Idempotent: a second fetch appends nothing. The merge pins.
  fetchJobResults: async (jobId: string): Promise<FetchedJobResults> => {
    await delay();
    advanceMockJobs();
    const job = mockJobs.find((j) => j.id === jobId);
    if (!job) throw new Error(`not_found: no job with id \`${jobId}\``);
    if (job.phase !== "finished") {
      throw new Error(
        `job_not_finished: the job is ${job.phase} — results land as evidence proposals only when a job finishes`,
      );
    }
    const argv = [job.spec.cmd, ...(job.spec.args ?? [])].join(" ");
    const results: JobResult = {
      code: 0,
      stdout: `[mock] ${argv}\n[mock] accuracy: 0.912, ±0.006, n=5 seeds — completed on \`${job.target}\``,
      stderr: "",
    };
    // idempotency: this job's result proposals already exist
    const existing = proposals.filter(
      (p) => p.proposedKind === "evidence.pinned" && p.runId === `fetch-${job.id}`,
    );
    if (existing.length > 0) {
      return {
        job: { ...job },
        results,
        proposals: existing.map((p) => ({ ...p, proposedPayload: { ...p.proposedPayload } })),
        created: 0,
      };
    }
    // the v1 targeting rule: the mission's FIRST hypothesis
    const hyp = hypotheses.find((h) => h.missionId === job.missionId);
    if (!hyp) {
      throw new Error(
        "no_hypothesis: the mission has no hypothesis to pin results onto — a result pin targets the mission's first hypothesis (v1)",
      );
    }
    const artifactRef = `jobs/${job.id}/stdout`;
    const digest = await sha256Hex(results.stdout);
    // the anchor claim (AD-5 — a pin attaches to a claim): registered
    // UNPINNED; it renders amber until the merge pins it (FR-3.4)
    claimSeq += 1;
    mockEventSeq += 1;
    const claim: Claim = {
      id: "cl" + claimSeq + "-" + Date.now(),
      seq: mockEventSeq,
      ts: nowISO(),
      hypothesisId: hyp.id,
      text: `Result artifact \`${artifactRef}\` from job e-${job.seq} on \`${job.target}\``,
      sourceMessageId: null,
      pinned: false,
      pin: null,
    };
    claims.push(claim);
    // the pin-candidate proposal — awaiting merge like every other proposal
    proposalSeq += 1;
    mockEventSeq += 1;
    const p: Proposal = {
      id: "pr" + proposalSeq + "-" + Date.now(),
      seq: mockEventSeq,
      ts: nowISO(),
      runId: `fetch-${job.id}`,
      missionId: job.missionId,
      targetEntity: hyp.id,
      targetLabel: hyp.statement,
      targetSeq: hyp.seq,
      proposedKind: "evidence.pinned",
      proposedPayload: {
        claim_id: claim.id,
        hypothesis_id: hyp.id,
        kind: "numerical",
        ref_id: null,
        artifact_ref: artifactRef,
        excerpt: results.stdout,
        digest,
        confidence: 0.5,
        assessing_model: settings.model || "simulated",
      },
      basisSeq: hypothesisCurrentSeq(hyp),
      basisStale: false,
      status: "pending",
      decided: null,
      supersededBy: null,
    };
    proposals.push(p);
    // the proposal surfaces in the mission's runs drill-down (receipt voice)
    missionRuns[job.missionId] = missionRuns[job.missionId] ?? [];
    missionRuns[job.missionId].push({
      seq: p.seq,
      id: "r" + p.seq + "-" + Date.now(),
      ts: nowISO(),
      kind: "proposal.created",
      actor: "agent",
    });
    return {
      job: { ...job },
      results,
      proposals: [{ ...p, proposedPayload: { ...p.proposedPayload } }],
      created: 1,
    };
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
    // keep the orphaned proposal OBJECTS: after the restore the mock arrays
    // hold only the checkpoint state — the orphans re-enter as superseded
    // history (never hidden, EXPERIENCE.md)
    const orphanObjects = proposals.filter((p) => p.seq > cp.seq && !p.orphanedByRollback);
    // execute: restore the board + missions to the checkpoint state...
    restoreSnapshot(cp.snapshot);
    // ...and the orphaned proposals land in the quarantine as SUPERSEDED:
    // excluded from every projection, listed as history
    for (const p of orphanObjects) {
      mockEventSeq += 1;
      p.status = "superseded";
      p.orphanedByRollback = true;
      p.rolledBackSeq = mockEventSeq;
      p.decided = null; // the merge never happened in the restored read model
      proposals.push(p);
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

  // open export (Story 3.1, FR-7.1/7.2): renders the mock's read model at
  // ONE cut (the head at render start) into a recorded folder — the same
  // shape the core's export_workspace returns. The browser mock cannot
  // write files, so the "folder" is a recorded manifest; the staleness
  // read mirrors the core's export_is_stale signal honestly (any rollback
  // appended after the recorded cut stales it).
  exportWorkspace: async (dir: string, scope: string): Promise<ExportOutcome> => {
    await delay();
    const trimmed = dir.trim();
    if (!trimmed) throw new Error("invalid_dir: an export names the folder it writes");
    const cut = mockEventSeq;
    const previous = mockExports.get(trimmed);
    const staleNotice =
      previous && mockRollbacks.some((r) => r.seq > previous.cutSeq)
        ? {
            previousCut: previous.cutSeq,
            rollbackSeq: mockRollbacks.find((r) => r.seq > previous.cutSeq)!.seq,
            rollbackTs: mockRollbacks.find((r) => r.seq > previous.cutSeq)!.ts,
          }
        : null;
    const files = exportFilesFor(scope);
    mockExports.set(trimmed, { cutSeq: cut });
    return {
      dir: trimmed,
      manifest: {
        cutSeq: cut,
        cutTs: nowISO(),
        renderedTs: nowISO(),
        scope,
        appVersion: "0.1.0",
        fileCount: files.length + 1,
        staleNotice,
      },
      files,
    };
  },

  inspectExport: async (dir: string): Promise<ExportInspect | null> => {
    await delay();
    const previous = mockExports.get(dir.trim());
    if (!previous) return null;
    const rollback = mockRollbacks.find((r) => r.seq > previous.cutSeq);
    return {
      cutSeq: previous.cutSeq,
      stale: !!rollback,
      rollbackSeq: rollback ? rollback.seq : null,
    };
  },
};
