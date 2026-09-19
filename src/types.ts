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

export interface Mission {
  id: string; // the mission.created event id
  seq: number; // store-assigned seq of the creation event
  ts: string; // ISO-8601 UTC
  question: string;
  stopCondition: string;
  successCriterion: string;
  autonomy: Autonomy;
  spendCeilingCents: number;
}
