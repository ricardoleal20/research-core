import { api } from "../api";
import { state, el, esc, toast } from "../main";
import { ico } from "../icons";
import { showContentSkeleton } from "../skeleton";
import type { Agent, McpServer } from "../types";

const TRANSPORT_ICONS: Record<string, string> = { stdio: ico.server, http: ico.server };

export async function renderStatus(view: HTMLElement) {
  const p = state.active!;
  // Skeleton while MCP servers, agents and project status load.
  const wait = showContentSkeleton(view, 3);
  await wait();

  view.innerHTML = `<div class="status-body">
    <section class="pane status-section">
      <div class="ss-head"><h3>${ico.server} Servidores MCP</h3>
        <span class="ss-sub" id="mcp-sub">cargando…</span>
        <button class="btn btn-primary" id="add-mcp" style="margin-left:auto">${ico.plus} Añadir servidor</button></div>
      <div class="mcp-grid" id="mcp-grid"></div>
    </section>
    <section class="pane status-section">
      <div class="ss-head"><h3>${ico.brain} Agentes</h3>
        <span class="ss-sub">configura qué jueces participan en la revisión</span></div>
      <div class="agents-grid" id="agents-grid"></div>
    </section>
    <section class="pane status-section">
      <div class="ss-head"><h3>${ico.layers} Estado del proyecto</h3></div>
      <div class="proj-status" id="proj-status"></div>
    </section>
  </div>`;
  await loadMcp();
  await loadAgents();
  await loadProjStatus(p.id);
  $("#add-mcp")!.addEventListener("click", () => openMcpForm());
}

async function loadMcp() {
  const servers = await api.listMcpServers();
  $("#mcp-sub")!.textContent = `${servers.filter((s) => s.connected).length}/${servers.length} conectados`;
  const grid = $("#mcp-grid")!;
  if (!servers.length) { grid.innerHTML = `<p style="color:var(--muted)">Sin servidores MCP.</p>`; return; }
  grid.innerHTML = servers.map(mcpHtml).join("");
  $$("#mcp-grid .mcp-card").forEach((card) => {
    const id = card.dataset.id!;
    $(".mcp-test", card)!.addEventListener("click", async (e) => {
      e.stopPropagation();
      const btn = e.currentTarget as HTMLElement;
      btn.textContent = "Conectando…";
      try {
        const res = await api.testMcpServer(id);
        toast(res.ok ? "Servidor conectado" : "Falló: " + (res.error ?? "ver logs"));
        await loadMcp();
      } catch (err) { toast("Error: " + err); btn.textContent = "Probar"; }
    });
    card.addEventListener("click", () => openMcpForm(servers.find((s) => s.id === id)));
  });
}

function mcpHtml(s: McpServer) {
  return `<article class="mcp-card ${s.connected ? "is-connected" : ""}" data-id="${s.id}">
    <div class="mcp-top">
      <span class="mcp-ico">${TRANSPORT_ICONS[s.transport] ?? ico.server}</span>
      <div class="mcp-info"><div class="mcp-name">${esc(s.name)}</div><div class="mcp-transport">${esc(s.transport)} · ${esc(s.tags || "")}</div></div>
      <span class="status-pill ${s.connected ? "sp-read" : "sp-unread"}"><span class="dot"></span>${s.connected ? "Conectado" : "Desconectado"}</span>
    </div>
    <div class="mcp-cmd mono">${esc(s.command)} ${esc(s.args)}</div>
    <div class="mcp-actions">
      <button class="btn btn-secondary btn-sm mcp-test">Probar</button>
    </div>
  </article>`;
}

function openMcpForm(s?: McpServer) {
  const overlay = el(`<div class="modal-overlay" id="mcp-overlay"><div class="modal" style="width:540px">
    <div class="modal-head"><div class="modal-title">${s ? "Editar servidor MCP" : "Nuevo servidor MCP"}</div></div>
    <div class="modal-body">
      <div class="field-row">
        <div class="field"><label>Nombre</label><input id="mc-name" type="text" value="${s ? esc(s.name) : ""}"></div>
        <div class="field"><label>Transporte</label><select id="mc-transport">
          <option value="stdio" ${s?.transport === "stdio" ? "selected" : ""}>stdio</option>
          <option value="http" ${s?.transport === "http" ? "selected" : ""}>http</option>
        </select></div>
      </div>
      <div class="field"><label>Comando</label><input id="mc-command" type="text" class="mono" value="${s ? esc(s.command) : ""}" placeholder="npx"></div>
      <div class="field"><label>Argumentos (espacio)</label><input id="mc-args" type="text" class="mono" value="${s ? esc(s.args) : ""}" placeholder="-y @modelcontextprotocol/server-filesystem /path"></div>
      <div class="field"><label>URL (si http)</label><input id="mc-url" type="text" class="mono" value="${s ? esc(s.url) : ""}"></div>
      <div class="field-row">
        <div class="field"><label>Variables de entorno (KEY=val, coma)</label><input id="mc-env" type="text" class="mono" value="${s ? esc(s.env) : ""}"></div>
        <div class="field"><label>Etiquetas (coma)</label><input id="mc-tags" type="text" value="${s ? esc(s.tags) : ""}"></div>
      </div>
    </div>
    <div class="modal-foot">
      <button class="btn btn-secondary" data-action="close">Cancelar</button>
      ${s ? `<button class="btn btn-ghost" id="mc-del" style="margin-right:auto">Eliminar</button>` : ""}
      <button class="btn btn-primary" id="mc-save">Guardar</button>
    </div>
  </div></div>`);
  $(".app-window")!.appendChild(overlay);
  const close = () => overlay.remove();
  $("[data-action='close']", overlay)!.addEventListener("click", close);
  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  $("#mc-save")!.addEventListener("click", async () => {
    const data = {
      name: val("mc-name"), transport: val("mc-transport"), command: val("mc-command"),
      args: val("mc-args"), url: val("mc-url"), env: val("mc-env"), tags: val("mc-tags"),
    };
    try {
      if (s) { await api.updateMcpServer({ id: s.id, ...data }); toast("Servidor actualizado"); }
      else { await api.addMcpServer(data); toast("Servidor añadido"); }
      close();
      await loadMcp();
    } catch (e) { toast("Error: " + e); }
  });
  $("#mc-del")?.addEventListener("click", async () => {
    await api.deleteMcpServer(s!.id);
    toast("Servidor eliminado");
    close();
    await loadMcp();
  });
}

async function loadAgents() {
  const agents = await api.listAgents();
  $("#agents-grid")!.innerHTML = agents.map((a) => agentCardHtml(a)).join("");
  $$("#agents-grid .agent-toggle").forEach((b) =>
    b.addEventListener("click", async () => {
      const id = b.dataset.id!;
      const enabled = !b.classList.contains("is-on");
      await api.toggleAgent(id, enabled);
      b.classList.toggle("is-on", enabled);
      b.setAttribute("aria-checked", String(enabled));
      b.closest(".ag-card")!.classList.toggle("is-enabled", enabled);
    }));
}

function agentCardHtml(a: Agent) {
  return `<div class="ag-card ${a.enabled ? "is-enabled" : ""}">
    <div class="ag-head"><span class="ag-key mono">${esc(a.key)}</span>
      <button class="agent-toggle ${a.enabled ? "is-on" : ""}" data-id="${a.id}" role="switch" aria-checked="${!!a.enabled}"><span class="knob"></span></button></div>
    <div class="ag-name">${esc(a.name)}</div>
    <div class="ag-desc">${esc(a.description)}</div>
  </div>`;
}

async function loadProjStatus(pid: string) {
  const d = await api.getDashboard(pid);
  $("#proj-status")!.innerHTML = `
    <div class="ps-row"><span class="ps-k">Total referencias</span><span class="ps-v">${d.refs_total ?? 0}</span></div>
    <div class="ps-row"><span class="ps-k">Referencias usadas</span><span class="ps-v">${d.refs_used ?? 0}</span></div>
    <div class="ps-row"><span class="ps-k">Revisiones AI</span><span class="ps-v">${d.review_count ?? 0}</span></div>
    <div class="ps-row"><span class="ps-k">Actions activas</span><span class="ps-v">${d.active_actions ?? 0}</span></div>
    <div class="ps-row"><span class="ps-k">Última revisión</span><span class="ps-v">${d.last_review ? "#" + d.last_review.number + " · " + d.last_review.score + "/10" : "—"}</span></div>`;
}

function val(id: string) { return ($("#" + id) as HTMLInputElement | HTMLSelectElement).value.trim(); }
function $(s: string, r: ParentNode = document) { return r.querySelector<HTMLElement>(s); }
function $$(s: string, r: ParentNode = document) { return Array.from(r.querySelectorAll<HTMLElement>(s)); }
