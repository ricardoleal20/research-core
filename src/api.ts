import { invoke } from "@tauri-apps/api/core";
import type { Ref, Chat, Agent, McpServer, Review, Action, Project } from "./types";

export const api = {
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
  getDashboard: (project_id: string) => invoke<any>("get_dashboard", { project_id }),

  // refs
  listRefs: (project_id: string, filter?: string) => invoke<Ref[]>("list_refs", { project_id, filter: filter ?? null }),
  getRef: (id: string) => invoke<Ref>("get_ref", { id }),
  createRef: (r: any) => invoke<Ref>("create_ref", r),
  updateRef: (r: any) => invoke<void>("update_ref", r),
  deleteRef: (id: string) => invoke<void>("delete_ref", { id }),
  searchRefs: (project_id: string, q: string) => invoke<Ref[]>("search_refs", { project_id, q }),
  searchRefsExternal: (q: string) => invoke<any[]>("search_refs_external", { q }),
  listCollections: (project_id: string) => invoke<any[]>("list_collections", { project_id }),

  // reviews
  listReviews: (project_id: string) => invoke<Review[]>("list_reviews", { project_id }),
  runReview: (project_id: string, focus: string) => invoke<any>("run_review", { project_id, focus }),

  // actions
  listActions: (project_id: string, done: boolean) => invoke<Action[]>("list_actions", { project_id, done }),
  createAction: (a: any) => invoke<Action>("create_action", a),
  toggleAction: (id: string, done: boolean) => invoke<void>("toggle_action", { id, done }),
  updateAction: (a: any) => invoke<void>("update_action", a),
  deleteAction: (id: string) => invoke<void>("delete_action", { id }),

  // chats
  listChats: (project_id: string, kind?: string) => invoke<Chat[]>("list_chats", { project_id, kind: kind ?? null }),
  createChat: (project_id: string, kind: string, title: string) => invoke<Chat>("create_chat", { project_id, kind, title }),
  getChat: (id: string) => invoke<Chat>("get_chat", { id }),
  sendMessage: (chat_id: string, content: string) => invoke<any>("send_message", { chat_id, content }),
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

  // diagnostics
  diagLog: (message: string) => invoke<void>("diag_log", { message }).catch(() => {}),
};
