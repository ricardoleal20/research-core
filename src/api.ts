import { invoke } from "@tauri-apps/api/core";
import type { Ref, Chat, Agent, McpServer, Review, Action, Project, Mission, Autonomy } from "./types";
import { mockApi, mockActive } from "./mock-backend";

// When the Tauri runtime is absent (plain browser via `vite`), fall back to an
// in-memory mock so the UI can boot and be iterated on without the Rust backend.
export const api = mockActive ? mockApi : {
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
  createMission: (m: { question: string; stopCondition: string; successCriterion: string; autonomy: Autonomy; spendCeilingCents: number }) =>
    invoke<Mission>("create_mission", m),
  listMissions: () => invoke<Mission[]>("list_missions"),

  // danger zone — wipe & recreate the database from scratch
  resetDatabase: () => invoke<void>("reset_database"),
};
