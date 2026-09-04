import { api } from "../api";
import { state, setTab, esc, toast } from "../main";
import { ico } from "../icons";
import type { McpServer } from "../types";

let settings: Record<string, string> = {};
let servers: McpServer[] = [];

// Sidebar sections — each maps to a card id and scrolls into view.
const SECTIONS: { id: string; label: string; card: string }[] = [
  { id: "general", label: "General", card: "card-ia" },
  { id: "mcp", label: "MCP", card: "card-mcp" },
  { id: "agente", label: "Agente", card: "card-agent" },
  { id: "idioma", label: "Idioma", card: "card-lang" },
  { id: "local", label: "Local-first", card: "card-local" },
  { id: "datos", label: "Datos", card: "card-danger" },
];

export async function renderAjustes(view: HTMLElement) {
  settings = { ...state.settings, ...(await api.getSettings()) };
  try { servers = await api.listMcpServers(); } catch { servers = []; }

  view.innerHTML = `<div class="settings-body">
    <aside class="settings-side">
      <div class="sb-label">Ajustes</div>
      <nav class="settings-nav">
        ${SECTIONS.map((s, i) =>
          `<button class="settings-nav-item ${i === 0 ? "is-active" : ""}" data-section="${s.card}">${s.label}</button>`
        ).join("")}
      </nav>
    </aside>

    <section class="settings-main" id="aj-main">
      <div class="settings-header">
        <div>
          <h2 class="settings-h2">Ajustes</h2>
          <p class="settings-sub">Configuración global de Research Core — proveedor de IA, agente, idioma y preferencias local-first.</p>
        </div>
        <button class="btn btn-primary btn-sm" id="aj-save">Guardar cambios</button>
      </div>

      ${cardIa()}
      ${cardMcp()}
      ${cardAgent()}
      ${cardLang()}
      ${cardLocal()}
      ${cardDanger()}
    </section>
  </div>`;

  wireSidebar();
  wireToggles();
  wireLang();
  $("#aj-save")!.addEventListener("click", save);
  $("#aj-reset")!.addEventListener("click", confirmReset);
}

/* ---------- cards ---------- */

function cardIa() {
  return `<div class="card" id="card-ia">
    <div class="card-head"><div class="card-title">${ico.brain} Proveedor de IA</div></div>
    <div class="field-row">
      <div class="field"><label>Proveedor</label><select id="set-provider">
        ${["openai-compatible|OpenAI-compatible","openai|OpenAI","anthropic|Anthropic","local|Local (Ollama)"]
          .map(o=>{const[v,l]=o.split("|");return `<option value="${v}" ${settings.provider===v?"selected":""}>${l}</option>`}).join("")}
      </select></div>
      <div class="field"><label>Modelo</label><input id="set-model" type="text" class="mono" value="${esc(settings.model)}" placeholder="gpt-4o-mini"></div>
    </div>
    <div class="field"><label>Base URL</label><input id="set-base_url" type="text" class="mono" value="${esc(settings.base_url)}" placeholder="https://api.openai.com/v1"></div>
    <div class="field" style="margin-bottom:0"><label>API Key</label><input id="set-api_key" type="password" class="mono" value="${esc(settings.api_key)}" placeholder="sk-…"></div>
    <p class="aj-note">${ico.lock} La clave se guarda localmente en SQLite; solo se envía al proveedor configurado.</p>
  </div>`;
}

function cardMcp() {
  const rows = servers.length
    ? servers.map(mcpRow).join("")
    : `<div class="empty-row">No hay servidores MCP configurados.</div>`;
  return `<div class="card" id="card-mcp">
    <div class="card-head">
      <div class="card-title">${ico.server} Servidores MCP</div>
      <button class="btn btn-ghost btn-sm" id="aj-mcp-add">${ico.plus} Gestionar</button>
    </div>
    ${rows}
  </div>`;
}

function mcpRow(s: McpServer) {
  const on = !!s.connected;
  const tags = (s.tags || "").split(",").map(t => t.trim()).filter(Boolean);
  const meta = s.transport === "http" ? (s.url || "—") : [s.command, s.args].filter(Boolean).join(" ");
  return `<div class="mcp-row">
    <div class="mcp-icon">${ico.server}</div>
    <div class="mcp-info"><div class="mcp-name">${esc(s.name)}</div><div class="mcp-meta mono">${esc(meta || "—")}</div></div>
    <div class="mcp-tags">${tags.map(t => `<span class="chip">${esc(t)}</span>`).join("")}</div>
    <span class="status-badge ${on ? "is-active" : "is-off"}"><span class="dot"></span>${on ? "Conectado" : "Desconectado"}</span>
  </div>`;
}

function cardAgent() {
  return `<div class="card" id="card-agent">
    <div class="card-head"><div class="card-title">${ico.layers} Agente de revisión por defecto</div></div>
    <div class="agent-box" style="margin-bottom:16px">
      <span class="ai">${ico.spark}</span>
      <div>
        <div class="agent-name">CLI de agente</div>
        <div class="agent-path mono">${esc(settings.agent_path || "—")}</div>
      </div>
    </div>
    <div class="field" style="margin-bottom:0">
      <label>Ruta del ejecutable</label>
      <input id="set-agent_path" type="text" class="mono" value="${esc(settings.agent_path)}" placeholder="~/.local/bin/claude">
    </div>
  </div>`;
}

function cardLang() {
  const cur = settings.lang || "es";
  const opts = [
    ["es", "Español (es-ES)"], ["en", "English (en-US)"],
    ["pt", "Português (pt-BR)"], ["fr", "Français (fr-FR)"],
  ];
  return `<div class="card" id="card-lang">
    <div class="card-head"><div class="card-title">${ico.globe} Idioma de la interfaz</div></div>
    <div class="lang-grid">
      ${opts.map(([v, l]) =>
        `<button class="lang-opt ${cur === v ? "is-active" : ""}" data-lang="${v}"><span class="radio"></span>${l}</button>`
      ).join("")}
    </div>
  </div>`;
}

function cardLocal() {
  return `<div class="card" id="card-local">
    <div class="card-head"><div class="card-title">${ico.shield} Local-first</div></div>
    ${toggle("local_first", "Almacenar datos localmente", "Mantiene proyectos, refs y revisiones en tu disco. No se envía nada a la nube.")}
    ${toggle("sync_zotero", "Sincronizar refs con Zotero", "Copia bidireccional de referencias entre Research Core y Zotero local.")}
    ${toggle("cache_pdfs", "Caché offline de PDFs", "Descarga PDFs de refs para acceso sin conexión.")}
  </div>`;
}

function cardDanger() {
  return `<div class="card" id="card-danger" style="border-color:var(--st-unread)">
    <div class="card-head"><div class="card-title">${ico.shield} Zona de peligro</div></div>
    <p class="aj-danger-desc">Borra <b>toda</b> la base de datos —proyectos, referencias, revisiones, acciones, chats y ajustes— y la recrea desde cero. La app volverá al asistente de configuración inicial. No se puede deshacer.</p>
    <button class="btn btn-danger" id="aj-reset">Borrar base de datos y reiniciar</button>
  </div>`;
}

/* ---------- wiring ---------- */

function wireSidebar() {
  $$(".settings-nav-item").forEach((b) =>
    b.addEventListener("click", () => {
      $$(".settings-nav-item").forEach((x) => x.classList.toggle("is-active", x === b));
      const card = $("#" + b.dataset.section!)!;
      card?.scrollIntoView({ behavior: "smooth", block: "start" });
    }));
  $("#aj-mcp-add")?.addEventListener("click", () => setTab("status"));
}

function toggle(key: string, label: string, desc: string) {
  const on = settings[key] === "true" || settings[key] === "1";
  return `<div class="toggle-row">
    <div class="toggle-info"><div class="toggle-name">${label}</div><div class="toggle-desc">${desc}</div></div>
    <button class="toggle ${on ? "is-on" : ""}" data-key="${key}" role="switch" aria-checked="${on}"><span class="toggle-knob"></span></button>
  </div>`;
}

function wireToggles() {
  $$(".toggle[data-key]").forEach((b) =>
    b.addEventListener("click", () => {
      const on = !b.classList.contains("is-on");
      b.classList.toggle("is-on", on);
      b.setAttribute("aria-checked", String(on));
      settings[b.dataset.key!] = on ? "true" : "false";
    }));
}

function wireLang() {
  $$(".lang-opt").forEach((b) =>
    b.addEventListener("click", () => {
      $$(".lang-opt").forEach((x) => x.classList.toggle("is-active", x === b));
      settings.lang = b.dataset.lang!;
    }));
}

async function confirmReset() {
  const btn = $("#aj-reset")!;
  if (btn.dataset.armed !== "1") {
    btn.dataset.armed = "1";
    btn.textContent = "¿Seguro? Clic otra vez para confirmar";
    window.setTimeout(() => { if (btn.dataset.armed === "1") { btn.dataset.armed = "0"; btn.textContent = "Borrar base de datos y reiniciar"; } }, 4000);
    return;
  }
  btn.dataset.armed = "0";
  btn.textContent = "Reiniciando…";
  btn.setAttribute("disabled", "true");
  try {
    await api.resetDatabase();
    toast("Base de datos recreada. Reiniciando…");
    await api.appLog("reset: database recreated by user");
    setTimeout(() => location.reload(), 900);
  } catch (e) {
    btn.removeAttribute("disabled");
    btn.textContent = "Borrar base de datos y reiniciar";
    toast("No se pudo reiniciar: " + e);
  }
}

async function save() {
  const fields = ["provider", "model", "base_url", "api_key", "agent_path"];
  try {
    for (const k of fields) settings[k] = val("set-" + k);
    for (const [k, v] of Object.entries(settings)) await api.updateSetting(k, v);
    state.settings = { ...settings };
    toast("Ajustes guardados");
  } catch (e) {
    toast("Error al guardar: " + e);
  }
}

function val(id: string) { return ($("#" + id) as HTMLInputElement | HTMLSelectElement).value.trim(); }
function $(s: string, r: ParentNode = document) { return r.querySelector<HTMLElement>(s); }
function $$(s: string, r: ParentNode = document) { return Array.from(r.querySelectorAll<HTMLElement>(s)); }
