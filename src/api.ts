import { invoke } from "@tauri-apps/api/core";
import type { Ref, Chat, Agent, McpServer, Review, Action, Project, Mission, MissionRun, Autonomy, Hypothesis } from "./types";
import { mockApi, mockActive } from "./mock-backend";

// A type alias (not an interface) so it stays assignable to Tauri's
// `InvokeArgs` (Record<string, unknown>) via the implicit index signature.
export type CreateMissionInput = {
  question: string;
  stopCondition: string;
  successCriterion: string;
  autonomy: Autonomy;
  spendCeilingCents: number;
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
  listMissions: async () =>
    (await servedByCore) ? httpJson<Mission[]>("/api/missions") : mockApi.listMissions(),
  getMissionRuns: async (missionId: string) =>
    (await servedByCore)
      ? httpJson<MissionRun[]>(`/api/missions/${missionId}/runs`)
      : mockApi.getMissionRuns(missionId),
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

  // refs
  listRefs: (projectId: string, filter?: string) => invoke<Ref[]>("list_refs", { projectId, filter: filter ?? null }),
  getRef: (id: string) => invoke<Ref>("get_ref", { id }),
  createRef: (r: any) => invoke<Ref>("create_ref", r),
  updateRef: (r: any) => invoke<void>("update_ref", r),
  deleteRef: (id: string) => invoke<void>("delete_ref", { id }),
  searchRefs: (projectId: string, q: string) => invoke<Ref[]>("search_refs", { projectId, q }),
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

  // chats
  listChats: (projectId: string, kind?: string) => invoke<Chat[]>("list_chats", { projectId, kind: kind ?? null }),
  createChat: (projectId: string, kind: string, title: string) => invoke<Chat>("create_chat", { projectId, kind, title }),
  getChat: (id: string) => invoke<Chat>("get_chat", { id }),
  sendMessage: (chatId: string, content: string) => invoke<any>("send_message", { chatId, content }),
  deleteChat: (id: string) => invoke<void>("delete_chat", { id }),

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
    invoke<Mission>("create_mission", m),
  listMissions: () => invoke<Mission[]>("list_missions"),
  getMissionRuns: (missionId: string) => invoke<MissionRun[]>("get_mission_runs", { missionId }),

  // hypotheses (event-sourced: FR-2.2 transitions are audited events,
  // FR-2.3 typed relations are events cause-linked to both endpoints)
  createHypothesis: (statement: string, missionId: string) =>
    invoke<Hypothesis>("create_hypothesis", { statement, missionId }),
  listHypotheses: (missionId: string) => invoke<Hypothesis[]>("list_hypotheses", { missionId }),
  transitionHypothesis: (hypothesisId: string, to: string, basis: string) =>
    invoke<Hypothesis>("transition_hypothesis", { hypothesisId, to, basis }),
  addRelation: (fromHypothesisId: string, toHypothesisId: string, relationKind: string) =>
    invoke<Hypothesis>("add_relation", { fromHypothesisId, toHypothesisId, relationKind }),

  // danger zone — wipe & recreate the database from scratch
  resetDatabase: () => invoke<void>("reset_database"),
};
