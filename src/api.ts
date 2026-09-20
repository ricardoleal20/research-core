import { invoke } from "@tauri-apps/api/core";
import type { Ref, Chat, ChatAttachment, Agent, McpServer, Review, Action, Project, Mission, MissionRun, Autonomy, Hypothesis, Claim, FirstValueResult, RoleConfig, AgentStepResult, Proposal, ApproveOutcome, MorningDigest, TrustStatus, RunReceipt, Checkpoint, CheckpointsView, RollbackPlan, RollbackOutcome, ExportOutcome, ExportInspect, Job, JobSpec, JobResult, FetchedJobResults, ComputeTargetView, SearchDisclosure, SearchRunView, ReadinessReport, ZoteroImportResult, Skill } from "./types";
import { mockApi, mockActive } from "./mock-backend";

// One attachment as picked, shaped for both transports: the desktop sends
// the picked {name, path} pairs (the core reads + classifies + stores
// locally); the pure-vite browser mock sends the client-read {name,
// content} pairs instead. Never both.
export type AttachmentPick = { name: string; path?: string | null; content?: string | null };

// A type alias (not an interface) so it stays assignable to Tauri's
// `InvokeArgs` (Record<string, unknown>) via the implicit index signature.
export type CreateMissionInput = {
  question: string;
  stopCondition: string;
  successCriterion: string;
  autonomy: Autonomy;
  spendCeilingCents: number;
  // Optional agent-role overrides (Story 2.1): when unset, the core resolves
  // the layer's defaults (drafter on the configured pair, critic never the
  // same pair — NFR-3).
  roles?: RoleConfig[] | null;
};

// Plain-browser transport (AD-7): when this page is served by the in-process
// server embedded in the desktop app, missions reads go to the same-origin
// read-only API over the shared core — the same data the desktop webview
// renders, one writer (AD-14). When no server answers (pure `vite` dev), the
// in-memory mock responds as before. The server exposes no mutations in v1:
// creating a mission from the served browser view is refused with a clear
// message — mutations stay on the Tauri command path.
const servedByCore: Promise<boolean> = fetch("/api/missions")
  .then((r) => r.ok)
  .catch(() => false);

async function httpJson<T>(path: string): Promise<T> {
  const r = await fetch(path);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json() as Promise<T>;
}

const browserApi = {
  ...mockApi,
  // Chat intelligence (Stories 5.4–5.6): the skills list is a read (the
  // served view renders the chat header's selector over the same-origin
  // API); every send, scope/skill change, and attachment mutation is
  // refused in the served read-only view (AD-14) — the pure-vite mock
  // keeps all of it working in-browser.
  listSkills: async (): Promise<Skill[]> => {
    if (await servedByCore) return httpJson<Skill[]>("/api/skills");
    return mockApi.listSkills();
  },
  createChat: async (
    _projectId: string,
    _kind: string,
    _title: string,
    _missionId?: string | null,
    _skill?: string | null,
  ) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage chats from the desktop app / " +
          "Vista de solo lectura — gestiona los chats desde la app de escritorio",
      );
    }
    return mockApi.createChat(_projectId, _kind, _title, _missionId, _skill);
  },
  sendMessage: async (_chatId: string, _content: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — send messages from the desktop app / " +
          "Vista de solo lectura — envía mensajes desde la app de escritorio",
      );
    }
    return mockApi.sendMessage(_chatId, _content);
  },
  setChatScope: async (_chatId: string, _missionId: string | null) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage chats from the desktop app / " +
          "Vista de solo lectura — gestiona los chats desde la app de escritorio",
      );
    }
    return mockApi.setChatScope(_chatId, _missionId);
  },
  setChatSkill: async (_chatId: string, _skill: string | null) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage chats from the desktop app / " +
          "Vista de solo lectura — gestiona los chats desde la app de escritorio",
      );
    }
    return mockApi.setChatSkill(_chatId, _skill);
  },
  addChatAttachments: async (
    _chatId: string,
    _files: AttachmentPick[],
  ) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — attach files from the desktop app / " +
          "Vista de solo lectura — adjunta archivos desde la app de escritorio",
      );
    }
    return mockApi.addChatAttachments(_chatId, _files);
  },
  removeChatAttachment: async (_chatId: string, _attachmentId: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage attachments from the desktop app / " +
          "Vista de solo lectura — gestiona los adjuntos desde la app de escritorio",
      );
    }
    return mockApi.removeChatAttachment(_chatId, _attachmentId);
  },
  // The references library (FR-15, Epic 5): the read goes to the
  // same-origin read-only API over the shared core (the evented library
  // fold — legacy baseline + ref.added/removed/restored); every mutation
  // below is refused in the served read-only browser view (AD-14) and
  // served by the in-memory mock in plain `vite` dev.
  listRefs: async (projectId: string, filter?: string | null) => {
    if (await servedByCore) {
      const params = new URLSearchParams({ projectId });
      if (filter) params.set("filter", filter);
      return httpJson<Ref[]>(`/api/refs?${params.toString()}`);
    }
    return mockApi.listRefs(projectId, filter);
  },
  addRefFromArxiv: async (_url: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage the library from the desktop app / " +
          "Vista de solo lectura — gestiona la biblioteca desde la app de escritorio",
      );
    }
    return mockApi.addRefFromArxiv(_url);
  },
  addRefManual: async (
    _title: string,
    _authors: string,
    _year: number | null,
    _venue: string,
    _doi: string,
    _url: string,
    _tags: string,
  ) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage the library from the desktop app / " +
          "Vista de solo lectura — gestiona la biblioteca desde la app de escritorio",
      );
    }
    return mockApi.addRefManual(_title, _authors, _year, _venue, _doi, _url, _tags);
  },
  importRefsFromZotero: async () => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage the library from the desktop app / " +
          "Vista de solo lectura — gestiona la biblioteca desde la app de escritorio",
      );
    }
    return mockApi.importRefsFromZotero();
  },
  removeRef: async (_refId: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage the library from the desktop app / " +
          "Vista de solo lectura — gestiona la biblioteca desde la app de escritorio",
      );
    }
    return mockApi.removeRef(_refId);
  },
  restoreRef: async (_refId: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage the library from the desktop app / " +
          "Vista de solo lectura — gestiona la biblioteca desde la app de escritorio",
      );
    }
    return mockApi.restoreRef(_refId);
  },
  listMissions: async () =>
    (await servedByCore) ? httpJson<Mission[]>("/api/missions") : mockApi.listMissions(),
  getMissionRuns: async (missionId: string) =>
    (await servedByCore)
      ? httpJson<MissionRun[]>(`/api/missions/${missionId}/runs`)
      : mockApi.getMissionRuns(missionId),
  // Compute jobs (Story 3.2, FR-11.4): the mission card's jobs area reads
  // the last observed lifecycle over the same-origin API (polling and
  // submitting stay on the Tauri command path — mutations, AD-14).
  pollJobs: async (missionId: string) => {
    if (await servedByCore) {
      return httpJson<Job[]>(`/api/missions/${missionId}/jobs`);
    }
    return mockApi.pollJobs(missionId);
  },
  fetchJob: async (jobId: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — fetch results from the desktop app / " +
          "Vista de solo lectura — obtén resultados desde la app de escritorio",
      );
    }
    return mockApi.fetchJob(jobId);
  },
  // Fetch results → quarantined evidence (Story 3.4, FR-11.5): a mutation —
  // the served browser view refuses it; the read-only per-job proposal list
  // is available at /api/jobs/:id/result-proposals via listProposals.
  fetchJobResults: async (jobId: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — fetch results from the desktop app / " +
          "Vista de solo lectura — obtén resultados desde la app de escritorio",
      );
    }
    return mockApi.fetchJobResults(jobId);
  },
  listComputeTargets: async () => {
    if (await servedByCore) return httpJson<ComputeTargetView[]>("/api/targets");
    return mockApi.listComputeTargets();
  },
  // The host allowlist (Story 3.3): reads go over the same-origin API;
  // edits stay on the Tauri command path (mutations, AD-14).
  getHostAllowlist: async () => {
    if (await servedByCore) return httpJson<string[]>("/api/host-allowlist");
    return mockApi.getHostAllowlist();
  },
  setHostAllowlist: async (_hosts: string[]) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — edit the allowlist from the desktop app / " +
          "Vista de solo lectura — edita la lista desde la app de escritorio",
      );
    }
    return mockApi.setHostAllowlist(_hosts);
  },
  submitJob: async (_missionId: string, _target: string, _spec: JobSpec) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — submit jobs from the desktop app / " +
          "Vista de solo lectura — envía trabajos desde la app de escritorio",
      );
    }
    return mockApi.submitJob(_missionId, _target, _spec);
  },
  declareComputeTarget: async (_name: string, _kind: string, _host?: string | null) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — declare targets from the desktop app / " +
          "Vista de solo lectura — declara destinos desde la app de escritorio",
      );
    }
    return mockApi.declareComputeTarget(_name, _kind, _host);
  },
  // Run receipts (Story 2.5, FR-6.1): the drill-down's audit ledger — a read
  // over the same-origin API (the served browser view replays the identical
  // ledger the desktop webview does); unknown runs are an honest 404.
  getRunReceipt: async (runId: string) => {
    if (await servedByCore) {
      const r = await fetch(`/api/runs/${encodeURIComponent(runId)}/receipt`);
      if (r.status === 404) return null;
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      return (await r.json()) as RunReceipt;
    }
    return mockApi.getRunReceipt(runId);
  },
  // Search protocol disclosure (Story 4.1, FR-12.1): the disclosure read
  // goes to the same-origin read-only API over the shared core; running a
  // search is a mutation — the served browser view refuses it.
  getSearchDisclosure: async (missionId: string | null) => {
    if (await servedByCore) {
      return missionId
        ? httpJson<SearchDisclosure>(`/api/missions/${missionId}/search-disclosure`)
        : httpJson<SearchDisclosure>("/api/search-disclosure");
    }
    return mockApi.getSearchDisclosure(missionId);
  },
  runSearch: async (
    _query: string,
    _database: string,
    _filters: Record<string, unknown> | null,
    _order: string | null,
    _firstPage: boolean,
    _missionId: string | null,
  ) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — run searches from the desktop app / " +
          "Vista de solo lectura — ejecuta búsquedas desde la app de escritorio",
      );
    }
    return mockApi.runSearch(_query, _database, _filters, _order, _firstPage, _missionId);
  },
  createMission: async (m: CreateMissionInput) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — launch missions from the desktop app / " +
          "Vista de solo lectura — lanza misiones desde la app de escritorio"
      );
    }
    return mockApi.createMission(m);
  },
  listHypotheses: async (missionId: string) =>
    (await servedByCore)
      ? httpJson<Hypothesis[]>(`/api/missions/${missionId}/hypotheses`)
      : mockApi.listHypotheses(missionId),
  createHypothesis: async (statement: string, missionId: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage hypotheses from the desktop app / " +
          "Vista de solo lectura — gestiona hipótesis desde la app de escritorio"
      );
    }
    return mockApi.createHypothesis(statement, missionId);
  },
  transitionHypothesis: async (hypothesisId: string, to: string, basis: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage hypotheses from the desktop app / " +
          "Vista de solo lectura — gestiona hipótesis desde la app de escritorio"
      );
    }
    return mockApi.transitionHypothesis(hypothesisId, to, basis);
  },
  addRelation: async (
    fromHypothesisId: string,
    toHypothesisId: string,
    relationKind: string,
  ) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage hypotheses from the desktop app / " +
          "Vista de solo lectura — gestiona hipótesis desde la app de escritorio"
      );
    }
    return mockApi.addRelation(fromHypothesisId, toHypothesisId, relationKind);
  },
  listEvidence: async (hypothesisId: string) =>
    (await servedByCore)
      ? httpJson<Claim[]>(`/api/hypotheses/${hypothesisId}/evidence`)
      : mockApi.listEvidence(hypothesisId),
  registerClaim: async (hypothesisId: string, text: string, sourceMessageId: string | null) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage evidence from the desktop app / " +
          "Vista de solo lectura — gestiona la evidencia desde la app de escritorio"
      );
    }
    return mockApi.registerClaim(hypothesisId, text, sourceMessageId);
  },
  pinClaimToCitation: async (
    claimId: string,
    hypothesisId: string,
    refId: string,
    excerpt: string,
    confidence: number,
    assessingModel: string,
  ) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage evidence from the desktop app / " +
          "Vista de solo lectura — gestiona la evidencia desde la app de escritorio"
      );
    }
    return mockApi.pinClaimToCitation(claimId, hypothesisId, refId, excerpt, confidence, assessingModel);
  },
  pinClaimToNumerical: async (
    claimId: string,
    hypothesisId: string,
    artifactRef: string,
    content: string,
    confidence: number,
    assessingModel: string,
  ) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage evidence from the desktop app / " +
          "Vista de solo lectura — gestiona la evidencia desde la app de escritorio"
      );
    }
    return mockApi.pinClaimToNumerical(claimId, hypothesisId, artifactRef, content, confidence, assessingModel);
  },
  // Pin verification (Story 4.2, FR-14.1): a mutation — the served browser
  // view refuses it; the read-only verification status rides the evidence
  // read (`/api/hypotheses/:id/evidence`).
  runPinVerification: async (hypothesisId: string | null) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — run verification from the desktop app / " +
          "Vista de solo lectura — ejecuta la verificación desde la app de escritorio"
      );
    }
    return mockApi.runPinVerification(hypothesisId);
  },
  // Onboarding (Story 1.9, FR-8.1): the first-value flow is a mutation —
  // the served browser view refuses it like every other write.
  runFirstValue: async (url: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — run the first value from the desktop app / " +
          "Vista de solo lectura — genera el primer valor desde la app de escritorio"
      );
    }
    return mockApi.runFirstValue(url);
  },
  runFirstValueFromRef: async (refId: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — run the first value from the desktop app / " +
          "Vista de solo lectura — genera el primer valor desde la app de escritorio"
      );
    }
    return mockApi.runFirstValueFromRef(refId);
  },
  // Agent steps (Story 2.1): a role step dispatches through the provider
  // layer — a mutation, refused by the served browser view.
  runAgentStep: async (missionId: string, role: string, task: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — run agent steps from the desktop app / " +
          "Vista de solo lectura — ejecuta pasos de agente desde la app de escritorio"
      );
    }
    return mockApi.runAgentStep(missionId, role, task);
  },
  // Morning Digest (Story 2.3, FR-4.4): the read goes to the same-origin
  // read-only API over the shared core; triggering the Night Shift and
  // changing schedules are mutations — the served browser view refuses
  // them.
  getMorningDigest: async () => {
    if (await servedByCore) {
      return httpJson<MorningDigest>("/api/digest");
    }
    return mockApi.getMorningDigest();
  },
  runNightShiftNow: async () => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — run the Night Shift from the desktop app / " +
          "Vista de solo lectura — ejecuta el Turno Nocturno desde la app de escritorio"
      );
    }
    return mockApi.runNightShiftNow();
  },
  setMissionSchedule: async (_missionId: string, _schedule: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage schedules from the desktop app / " +
          "Vista de solo lectura — gestiona los horarios desde la app de escritorio"
      );
    }
    return mockApi.setMissionSchedule(_missionId, _schedule);
  },
  // Trust center (Story 2.4, FR-5): the status read goes to the same-origin
  // read-only API over the shared core; dial, ceiling, and kill-switch
  // changes are mutations — the served browser view refuses them.
  getTrustStatus: async () => {
    if (await servedByCore) {
      return httpJson<TrustStatus>("/api/trust");
    }
    return mockApi.getTrustStatus();
  },
  configureAutonomy: async (_scope: string, _scopeId: string | null, _mode: Autonomy) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage trust from the desktop app / " +
          "Vista de solo lectura — gestiona la confianza desde la app de escritorio"
      );
    }
    return mockApi.configureAutonomy(_scope, _scopeId, _mode);
  },
  configureCeiling: async (_scope: string, _scopeId: string | null, _ceilingCents: number) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage trust from the desktop app / " +
          "Vista de solo lectura — gestiona la confianza desde la app de escritorio"
      );
    }
    return mockApi.configureCeiling(_scope, _scopeId, _ceilingCents);
  },
  killRuntime: async () => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — the kill switch stays in the desktop app / " +
          "Vista de solo lectura — el interruptor queda en la app de escritorio"
      );
    }
    return mockApi.killRuntime();
  },
  resumeRuntime: async () => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — the kill switch stays in the desktop app / " +
          "Vista de solo lectura — el interruptor queda en la app de escritorio"
      );
    }
    return mockApi.resumeRuntime();
  },
  // Proposals (Story 2.2, AD-3/AD-13): reads go to the same-origin
  // read-only API over the shared core; merging and rejecting are
  // mutations — the served browser view refuses them.
  listProposals: async (missionId: string | null) => {
    if (await servedByCore) {
      return missionId
        ? httpJson<Proposal[]>(`/api/missions/${missionId}/proposals`)
        : httpJson<Proposal[]>("/api/proposals");
    }
    return mockApi.listProposals(missionId);
  },
  approveProposal: async (_proposalId: string, _force: boolean) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — review proposals from the desktop app / " +
          "Vista de solo lectura — revisa las propuestas desde la app de escritorio"
      );
    }
    return mockApi.approveProposal(_proposalId, _force);
  },
  rejectProposal: async (_proposalId: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — review proposals from the desktop app / " +
          "Vista de solo lectura — revisa las propuestas desde la app de escritorio"
      );
    }
    return mockApi.rejectProposal(_proposalId);
  },
  // Checkpoints (Story 2.6, FR-10.1): the restore-point list goes to the
  // same-origin read-only API over the shared core; creating, previewing,
  // and rolling back are mutations — the served browser view refuses them.
  listCheckpoints: async () => {
    if (await servedByCore) {
      return httpJson<CheckpointsView>("/api/checkpoints");
    }
    return mockApi.listCheckpoints();
  },
  createCheckpoint: async (_name: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage checkpoints from the desktop app / " +
          "Vista de solo lectura — gestiona los puntos de control desde la app de escritorio",
      );
    }
    return mockApi.createCheckpoint(_name);
  },
  previewRollback: async (_checkpointId: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage checkpoints from the desktop app / " +
          "Vista de solo lectura — gestiona los puntos de control desde la app de escritorio",
      );
    }
    return mockApi.previewRollback(_checkpointId);
  },
  // Open export (Story 3.1, FR-7.1/7.2): rendering writes files — a desktop
  // action, refused in the served read-only view (single writer, AD-14);
  // the inspect read is desktop-only too (it reads the local folder).
  exportWorkspace: async (dir: string, scope: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — export from the desktop app / " +
          "Vista de solo lectura — exporta desde la app de escritorio",
      );
    }
    return mockApi.exportWorkspace(dir, scope);
  },
  inspectExport: async (dir: string) => {
    if (await servedByCore) return null;
    return mockApi.inspectExport(dir);
  },
  rollbackToCheckpoint: async (_checkpointId: string) => {
    if (await servedByCore) {
      throw new Error(
        "Read-only view — manage checkpoints from the desktop app / " +
          "Vista de solo lectura — gestiona los puntos de control desde la app de escritorio",
      );
    }
    return mockApi.rollbackToCheckpoint(_checkpointId);
  },
};

// When the Tauri runtime is absent (plain browser via `vite`), the browser
// transport above takes over; inside the Tauri app, the real `invoke` calls.
export const api = mockActive ? browserApi : {
  // projects
  listProjects: () => invoke<Project[]>("list_projects"),
  getActiveProject: () => invoke<Project | null>("get_active_project"),
  setActiveProject: (id: string) => invoke<void>("set_active_project", { id }),
  createProject: (p: { name: string; folder: string; kind: string; tags: string; color: string }) =>
    invoke<Project>("create_project", p),
  updateProject: (p: { id: string; name: string; folder: string; tags: string }) =>
    invoke<void>("update_project", p),
  deleteProject: (id: string) => invoke<void>("delete_project", { id }),

  // dashboard
  getDashboard: (projectId: string) => invoke<any>("get_dashboard", { projectId }),

  // refs — the read re-folds the evented library projection (legacy
  // baseline + ref.* events, FR-15); the legacy create_ref/update_ref/
  // delete_ref relational commands stay dead (AD-16) — the mutations are
  // the evented pair below.
  listRefs: (projectId: string, filter?: string | null) =>
    invoke<Ref[]>("list_refs", { projectId, filter: filter ?? null }),
  getRef: (id: string) => invoke<Ref>("get_ref", { id }),
  createRef: (r: any) => invoke<Ref>("create_ref", r),
  updateRef: (r: any) => invoke<void>("update_ref", r),
  deleteRef: (id: string) => invoke<void>("delete_ref", { id }),
  searchRefs: (projectId: string, q: string) => invoke<Ref[]>("search_refs", { projectId, q }),
  // evented references CRUD (FR-15, Epic 5): arXiv paste (the shared
  // fetch adapter behind the onboarding first-value flow), manual entry
  // (title + one identifier), the deduped Zotero import, and the
  // auditable remove/restore pair.
  addRefFromArxiv: (url: string) => invoke<Ref>("add_ref_from_arxiv", { url }),
  addRefManual: (
    title: string,
    authors: string,
    year: number | null,
    venue: string,
    doi: string,
    url: string,
    tags: string,
  ) => invoke<Ref>("add_ref_manual", { title, authors, year, venue, doi, url, tags }),
  importRefsFromZotero: () => invoke<ZoteroImportResult>("import_refs_from_zotero"),
  removeRef: (refId: string) => invoke<Ref>("remove_ref", { refId }),
  restoreRef: (refId: string) => invoke<Ref>("restore_ref", { refId }),
  searchRefsExternal: (q: string) => invoke<any[]>("search_refs_external", { q }),
  listCollections: (projectId: string) => invoke<any[]>("list_collections", { projectId }),

  // reviews
  listReviews: (projectId: string) => invoke<Review[]>("list_reviews", { projectId }),
  runReview: (projectId: string, focus: string) => invoke<any>("run_review", { projectId, focus }),

  // actions
  listActions: (projectId: string, done: boolean) => invoke<Action[]>("list_actions", { projectId, done }),
  createAction: (a: any) => invoke<Action>("create_action", a),
  toggleAction: (id: string, done: boolean) => invoke<void>("toggle_action", { id, done }),
  updateAction: (a: any) => invoke<void>("update_action", a),
  deleteAction: (id: string) => invoke<void>("delete_action", { id }),

  // chats — the scoped surface (Stories 5.4–5.6): mission scope + skill per
  // conversation, messages carrying the scope, attachments as digest-stored
  // refs; every binding movement is evented in the core (chat.scoped /
  // chat.skill_set / chat.attachment_*)
  listChats: (projectId: string, kind?: string) => invoke<Chat[]>("list_chats", { projectId, kind: kind ?? null }),
  createChat: (
    projectId: string,
    kind: string,
    title: string,
    missionId?: string | null,
    skill?: string | null,
  ) =>
    invoke<Chat>("create_chat", {
      projectId,
      kind,
      title,
      missionId: missionId ?? null,
      skill: skill ?? null,
    }),
  getChat: (id: string) => invoke<Chat>("get_chat", { id }),
  sendMessage: (chatId: string, content: string) => invoke<any>("send_message", { chatId, content }),
  deleteChat: (id: string) => invoke<void>("delete_chat", { id }),
  setChatScope: (chatId: string, missionId: string | null) =>
    invoke<Chat>("set_chat_scope", { chatId, missionId }),
  setChatSkill: (chatId: string, skill: string | null) =>
    invoke<Chat>("set_chat_skill", { chatId, skill }),
  // skills (Story 5.6): the registry is data — the curated six plus
  // user-added; addSkill grows the pool without code changes
  listSkills: () => invoke<Skill[]>("list_skills"),
  addSkill: (name: string, provider: string, model: string, systemPrompt: string, tools: string[]) =>
    invoke<Skill>("add_skill", { name, provider, model, systemPrompt, tools }),
  // attachments (Story 5.5): the desktop file picker + the attach/remove
  // pair; the core reads, classifies (text/pdf/binary), digests, and
  // stores locally — content leaves only inside the provider call (NFR-12)
  pickAttachmentFiles: () =>
    invoke<{ name: string; path: string }[]>("pick_attachment_files"),
  addChatAttachments: (chatId: string, files: { name: string; path: string }[]) =>
    invoke<{ attached: ChatAttachment[]; refused: { name: string; reason: string }[] }>(
      "add_chat_attachments",
      { chatId, files },
    ),
  removeChatAttachment: (chatId: string, attachmentId: string) =>
    invoke<ChatAttachment[]>("remove_chat_attachment", { chatId, attachmentId }),

  // agents
  listAgents: () => invoke<Agent[]>("list_agents"),
  toggleAgent: (id: string, enabled: boolean) => invoke<void>("toggle_agent", { id, enabled }),

  // mcp
  listMcpServers: () => invoke<McpServer[]>("list_mcp_servers"),
  addMcpServer: (m: any) => invoke<McpServer>("add_mcp_server", m),
  updateMcpServer: (m: any) => invoke<void>("update_mcp_server", m),
  deleteMcpServer: (id: string) => invoke<void>("delete_mcp_server", { id }),
  testMcpServer: (id: string) => invoke<any>("test_mcp_server", { id }),
  listMcpTools: () => invoke<any>("list_mcp_tools"),

  // settings
  getSettings: () => invoke<Record<string, string>>("get_settings"),
  updateSetting: (key: string, value: string) => invoke<void>("update_setting", { key, value }),
  setProviderKey: (key: string) => invoke<void>("set_provider_key", { key }),

  // logging + paths
  appLog: (message: string) => invoke<void>("app_log", { message }).catch(() => {}),
  getAppPaths: () => invoke<{ data_dir: string; log_dir: string }>("get_app_paths"),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
  pickFolder: () => invoke<string | null>("pick_folder"),

  // local access key (lock policy)
  verifyKey: (key: string) => invoke<boolean>("verify_key", { key }),
  setLockKey: (key: string) => invoke<void>("set_lock_key", { key }),
  lockState: () => invoke<{ policy: string; idle_min: string; configured: boolean }>("lock_state"),

  // LLM CLI detection
  testCli: (command: string) => invoke<{ command: string; path: string | null }>("test_cli", { command }),

  // missions (event-sourced: create appends mission.created, list folds the projection)
  createMission: (m: CreateMissionInput) =>
    invoke<Mission>("create_mission", { ...m, roles: m.roles ?? null }),
  listMissions: () => invoke<Mission[]>("list_missions"),
  getMissionRuns: (missionId: string) => invoke<MissionRun[]>("get_mission_runs", { missionId }),
  // run receipts (Story 2.5, FR-6.1): one run's ordered audit ledger — a
  // pure query over the log (replay = re-query)
  getRunReceipt: (runId: string) => invoke<RunReceipt | null>("get_run_receipt", { runId }),
  // search protocol disclosure (event-sourced, Story 4.1, FR-12.1): the
  // ONE search entry — executes, appends the search.run PRISMA record,
  // returns results (nulls logged identically); the disclosure is the
  // pure fold the mission's Divulgación section renders
  runSearch: (
    query: string,
    database: string,
    filters: Record<string, unknown> | null,
    order: string | null,
    firstPage: boolean,
    missionId: string | null,
  ) =>
    invoke<SearchRunView>("run_search", {
      query,
      database,
      filters: filters ?? null,
      order: order ?? null,
      firstPage,
      missionId: missionId ?? null,
    }),
  getSearchDisclosure: (missionId: string | null) =>
    invoke<SearchDisclosure>("get_search_disclosure", { missionId }),
  // Readiness gate (Story 4.3, FR-13): the preprint-tier report — a pure
  // fold over the log (replay = re-query); missionId scopes it to one
  // mission's board, null asks the whole workspace
  getReadinessReport: (missionId: string | null) =>
    invoke<ReadinessReport>("get_readiness_report", { missionId }),
  // agent steps (Story 2.1): one role step through the provider layer —
  // spend recorded role-tagged, result returned to the caller
  runAgentStep: (missionId: string, role: string, task: string) =>
    invoke<AgentStepResult>("run_agent_step", { missionId, role, task }),

  // hypotheses (event-sourced: FR-2.2 transitions are audited events,
  // FR-2.3 typed relations are events cause-linked to both endpoints)
  createHypothesis: (statement: string, missionId: string) =>
    invoke<Hypothesis>("create_hypothesis", { statement, missionId }),
  listHypotheses: (missionId: string) => invoke<Hypothesis[]>("list_hypotheses", { missionId }),
  transitionHypothesis: (hypothesisId: string, to: string, basis: string) =>
    invoke<Hypothesis>("transition_hypothesis", { hypothesisId, to, basis }),
  addRelation: (fromHypothesisId: string, toHypothesisId: string, relationKind: string) =>
    invoke<Hypothesis>("add_relation", { fromHypothesisId, toHypothesisId, relationKind }),

  // evidence pins (event-sourced, Story 1.7: claim.registered +
  // evidence.pinned; the digest is computed in the core, never sent)
  registerClaim: (hypothesisId: string, text: string, sourceMessageId: string | null) =>
    invoke<Claim>("register_claim", { hypothesisId, text, sourceMessageId }),
  pinClaimToCitation: (
    claimId: string,
    hypothesisId: string,
    refId: string,
    excerpt: string,
    confidence: number,
    assessingModel: string,
  ) =>
    invoke<Claim>("pin_claim_to_citation", {
      claimId,
      hypothesisId,
      refId,
      excerpt,
      confidence,
      assessingModel,
    }),
  pinClaimToNumerical: (
    claimId: string,
    hypothesisId: string,
    artifactRef: string,
    content: string,
    confidence: number,
    assessingModel: string,
  ) =>
    invoke<Claim>("pin_claim_to_numerical", {
      claimId,
      hypothesisId,
      artifactRef,
      content,
      confidence,
      assessingModel,
    }),
  listEvidence: (hypothesisId: string) =>
    invoke<Claim[]>("list_evidence", { hypothesisId }),
  // Pin verification (Story 4.2, FR-14.1): re-check every pin of the scope
  // against its source with NO LLM call — one evidence.verified event per
  // pin (actor=system/verifier); the returned claims carry the latest
  // verification on every pin.
  runPinVerification: (hypothesisId: string | null) =>
    invoke<Claim[]>("run_pin_verification", { hypothesisId }),

  // morning digest (event-sourced, Story 2.3): the Night Shift result —
  // the manual trigger runs every active mission's scan now; schedule
  // changes append mission.scheduled events
  getMorningDigest: () => invoke<MorningDigest>("get_morning_digest"),
  runNightShiftNow: () => invoke<MorningDigest>("run_night_shift_now"),
  setMissionSchedule: (missionId: string, schedule: string) =>
    invoke<Mission>("set_mission_schedule", { missionId, schedule }),

  // proposals (event-sourced, Story 2.2: agent changes land as quarantined
  // proposals — excluded from projections until a human merges them, AD-3;
  // the basis is validated at merge time, AD-13)
  listProposals: (missionId: string | null) =>
    invoke<Proposal[]>("list_proposals", { missionId }),
  approveProposal: (proposalId: string, force: boolean) =>
    invoke<ApproveOutcome>("approve_proposal", { proposalId, force }),
  rejectProposal: (proposalId: string) =>
    invoke<Proposal>("reject_proposal", { proposalId }),

  // onboarding — the sixty-second first value (Story 1.9, FR-8.1): one
  // arXiv paste (or one library ref via the Zotero connector stub) becomes a
  // starter mission + hypothesis candidates through the provider layer
  runFirstValue: (url: string) => invoke<FirstValueResult>("run_first_value", { url }),
  runFirstValueFromRef: (refId: string) =>
    invoke<FirstValueResult>("run_first_value_from_ref", { refId }),

  // trust center (event-sourced, Story 2.4, FR-5): the status read + the
  // dial, ceiling, and kill-switch mutations (the Tauri command path, AD-14)
  getTrustStatus: () => invoke<TrustStatus>("get_trust_status"),
  configureAutonomy: (scope: string, scopeId: string | null, mode: Autonomy) =>
    invoke<TrustStatus>("configure_autonomy", { scope, scopeId, mode }),
  configureCeiling: (scope: string, scopeId: string | null, ceilingCents: number) =>
    invoke<TrustStatus>("configure_ceiling", { scope, scopeId, ceilingCents }),
  killRuntime: () => invoke<TrustStatus>("kill_runtime"),
  resumeRuntime: () => invoke<TrustStatus>("resume_runtime"),

  // checkpoints (event-sourced, Story 2.6, FR-10.1): restore points are
  // checkpoint.created events carrying the log head; rollback appends
  // checkpoint.rolled_back — history is never rewritten, the shared fold
  // cursor does the returning
  createCheckpoint: (name: string) => invoke<Checkpoint>("create_checkpoint", { name }),
  listCheckpoints: () => invoke<CheckpointsView>("list_checkpoints"),
  previewRollback: (checkpointId: string) =>
    invoke<RollbackPlan>("preview_rollback", { checkpointId }),
  rollbackToCheckpoint: (checkpointId: string) =>
    invoke<RollbackOutcome>("rollback_to_checkpoint", { checkpointId }),

  // open export (event-sourced, Story 3.1, FR-7.1/7.2): renders every
  // requested scope's fold at ONE named seq cut into open git-friendly
  // files; inspect reads an existing export folder's cut + staleness
  // (Story 2.6's signal) for the composer's warning state
  exportWorkspace: (dir: string, scope: string) =>
    invoke<ExportOutcome>("export_workspace", { dir, scope }),
  inspectExport: (dir: string) =>
    invoke<ExportInspect | null>("inspect_export", { dir }),

  // compute targets + jobs (event-sourced, Story 3.2/3.3,
  // FR-11.1/11.2/11.4, AD-6): specs are typed JSON validated before
  // submit — the runtime executes argv directly and never constructs
  // shell strings; the lifecycle (submit → monitor → terminal) is evented,
  // terminals always stamped + reasoned, and every terminal job attributes
  // its usage to its target (target.spend_recorded, AD-10). SSH targets
  // (Story 3.3) carry a host; hosts outside the allowlist are refused
  // before any connection is attempted.
  listComputeTargets: () => invoke<ComputeTargetView[]>("list_compute_targets"),
  declareComputeTarget: (name: string, kind: string, host?: string | null) =>
    invoke<ComputeTargetView[]>("declare_compute_target", { name, kind, host }),
  getHostAllowlist: () => invoke<string[]>("get_host_allowlist"),
  setHostAllowlist: (hosts: string[]) =>
    invoke<string[]>("set_host_allowlist", { hosts }),
  submitJob: (missionId: string, target: string, spec: JobSpec) =>
    invoke<Job>("submit_job", { missionId, target, spec }),
  pollJobs: (missionId: string) => invoke<Job[]>("poll_jobs", { missionId }),
  fetchJob: (jobId: string) => invoke<JobResult>("fetch_job", { jobId }),
  // Fetch results → quarantined evidence (Story 3.4, FR-11.5, AD-3/AD-5):
  // each meaningful artifact lands as a proposal whose intended payload is a
  // numerical pin (artifact_ref + sha-256 digest computed in the core);
  // idempotent — a second fetch appends nothing. The merge pins it.
  fetchJobResults: (jobId: string) =>
    invoke<FetchedJobResults>("fetch_job_results", { jobId }),

  // danger zone — wipe & recreate the database from scratch
  resetDatabase: () => invoke<void>("reset_database"),
};
