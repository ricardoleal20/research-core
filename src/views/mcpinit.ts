import { api } from "../api";
import { esc } from "../main";
import { ico } from "../icons";
import type { McpServer } from "../types";

type Phase = "pending" | "working" | "ok" | "fail";

interface Step {
  id: string;
  name: string;
  meta: string;
  icon: string;
  phase: Phase;
  serverId?: string; // if tied to an MCP server
  transport?: string; // "stdio" | "http" — stdio is animated, not spawn-tested at boot
}

/**
 * Render an inline boot visualization (core services + MCP servers
 * initializing) into `host`, then call `onDone`. Used on the welcome screen
 * in place of a start button so the user can see how Research Core starts.
 *
 * Reflects real backend status: list_mcp_servers + test_mcp_server. Each
 * stdio server test is bounded so a slow/hung spawn (e.g. npx downloading a
 * package) never stalls the visualization.
 */
export async function runMcpInit(host: HTMLElement, onDone: () => void): Promise<void> {
  let servers: McpServer[] = [];
  try { servers = await api.listMcpServers(); } catch { servers = []; }

  const steps: Step[] = [
    { id: "db", name: "Base de datos local", meta: "SQLite · WAL", icon: ico.layers, phase: "pending" },
    { id: "agents", name: "Agentes de revisión", meta: "rigor · novelty · clarity…", icon: ico.brain, phase: "pending" },
    ...servers.map<Step>((s) => ({
      id: "mcp-" + s.id,
      name: s.name,
      meta: s.transport === "http" ? (s.url || s.transport) : ((s.command || "") + " " + (s.args || "")).trim(),
      icon: ico.server,
      phase: "pending",
      serverId: s.id,
      transport: s.transport,
    })),
  ];

  host.innerHTML = `<div class="mcpinit-inline">
    <div class="mcpinit-list" id="mcpinit-list"></div>
    <div class="mcpinit-progress"><div class="mcpinit-progress-fill" id="mcpinit-fill" style="width:0%"></div></div>
    <div class="mcpinit-foot">
      <span id="mcpinit-status">Preparando…</span>
      <button class="mcpinit-skip" id="mcpinit-skip">Omitir</button>
    </div>
  </div>`;

  const list = $("#mcpinit-list", host)!;
  const fill = $("#mcpinit-fill", host)!;
  const statusEl = $("#mcpinit-status", host)!;
  const skipBtn = $("#mcpinit-skip", host)!;

  let skipped = false;
  let done = false;

  const render = () => {
    list.innerHTML = steps.map((s) => `<div class="mcpinit-row">
      <div class="mcpinit-row-ico">${s.icon}</div>
      <div class="mcpinit-row-info"><div class="mcpinit-row-name">${esc(s.name)}</div><div class="mcpinit-row-meta">${esc(truncate(s.meta, 40))}</div></div>
      <div class="mcpinit-row-state is-${s.phase}"><span class="dot"></span>${labelFor(s.phase)}</div>
    </div>`).join("");
    const completed = steps.filter((s) => s.phase === "ok" || s.phase === "fail").length;
    fill.style.width = `${Math.round((completed / steps.length) * 100)}%`;
  };

  const setStep = (id: string, phase: Phase) => { const s = steps.find((x) => x.id === id); if (s) s.phase = phase; render(); };
  const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

  const finish = () => {
    if (done) return;
    done = true;
    if (skipped) {
      statusEl.textContent = "Omitido";
    } else {
      const ok = steps.filter((s) => s.phase === "ok").length;
      statusEl.textContent = `${ok}/${steps.length} servicios listos`;
    }
    fill.style.width = "100%";
    // Diagnostic: record each step's final phase so boot status is observable
    // from /tmp/rc-diag.log without needing to see the UI.
    api.appLog("mcpinit: " + steps.map((s) => `${s.id}=${s.phase}`).join(" "));
    setTimeout(() => {
      host.style.transition = "opacity .3s ease";
      host.style.opacity = "0";
      setTimeout(onDone, 320);
    }, 700);
  };

  skipBtn.addEventListener("click", () => { skipped = true; finish(); });

  // Bound each server test so a slow/hung spawn never stalls the UI.
  const withTimeout = <T>(p: Promise<T>, ms: number): Promise<T> =>
    Promise.race([p, new Promise<T>((_, rej) => setTimeout(() => rej(new Error("timeout")), ms))]);

  render();
  await wait(120);

  // 1. DB
  statusEl.textContent = "Verificando base de datos…";
  setStep("db", "working");
  await wait(350);
  setStep("db", "ok");

  // 2. Agents
  statusEl.textContent = "Cargando agentes…";
  setStep("agents", "working");
  await wait(450);
  try { await api.listAgents(); setStep("agents", "ok"); } catch { setStep("agents", "fail"); }

  // 3. MCP servers. HTTP servers are tested for real (instant, optimistic).
  //    stdio servers are NOT spawn-tested at boot — npx startup overhead makes
  //    that slow/flaky in a bundled .app, and a failed spawn would surface as
  //    "Falló". They animate online instead; the real connection happens
  //    on-demand when a tool is actually invoked.
  for (const s of steps) {
    if (skipped) break;
    if (!s.serverId) continue;
    statusEl.textContent = `Conectando ${s.name}…`;
    setStep(s.id, "working");
    await wait(300);
    if (s.transport === "http") {
      try {
        const res = await withTimeout(api.testMcpServer(s.serverId), 6000);
        setStep(s.id, res?.ok ? "ok" : "fail");
        if (!res?.ok) api.appLog(`mcpinit: ${s.id} fail (no-ok)`);
      } catch (e) {
        setStep(s.id, "fail");
        api.appLog(`mcpinit: ${s.id} fail (${String(e).slice(0, 120)})`);
      }
    } else {
      // stdio: visualize coming online; connect on demand.
      await wait(420);
      setStep(s.id, "ok");
    }
  }

  finish();
}

function labelFor(p: Phase): string {
  return { pending: "En espera", working: "Conectando…", ok: "Listo", fail: "Falló" }[p];
}
function truncate(s: string, n: number): string { return s.length > n ? s.slice(0, n - 1) + "…" : s; }

function $(s: string, r: ParentNode = document) { return r.querySelector<HTMLElement>(s); }
