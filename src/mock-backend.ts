// In-memory mock backend. Activates only when the Tauri runtime is absent
// (i.e. the app is served by the Vite dev server in a plain browser). This
// lets the UI boot and be iterated on without the Rust backend. All state is
// ephemeral and resets on reload.
//
// When Tauri is present (real app or `tauri dev`), this module is never used —
// api.ts routes to the real `invoke` calls instead.

import type { Project, Ref, Review, Action, Chat, Agent, McpServer } from "./types";

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
  listRefs: async () => [] as Ref[],
  getRef: async () => { throw new Error("not in mock"); },
  createRef: async (r: any) => r as Ref,
  updateRef: async () => {},
  deleteRef: async () => {},
  searchRefs: async () => [] as Ref[],
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
  listChats: async () => [] as Chat[],
  createChat: async (c: any) => c as Chat,
  getChat: async () => { throw new Error("not in mock"); },
  sendMessage: async () => ({ ok: true }),
  deleteChat: async () => {},

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

  // danger zone
  resetDatabase: async () => { await delay(); },
};
