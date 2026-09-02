import { api } from "../api";
import { state, el, esc, toast } from "../main";
import { ico } from "../icons";

let settings: Record<string, string> = {};

export async function renderAjustes(view: HTMLElement) {
  settings = { ...state.settings, ...(await api.getSettings()) };
  view.innerHTML = `<div class="ajustes-body">
    <div class="ajustes-scroll">

      <section class="pane aj-section">
        <div class="aj-head"><h3>${ico.brain} Proveedor de IA</h3><span class="aj-sub">configura el LLM para revisiones y el asistente</span></div>
        <div class="aj-grid">
          <div class="field"><label>Proveedor</label><select id="set-provider">
            ${["openai-compatible|OpenAI-compatible","openai|OpenAI","anthropic|Anthropic","local|Local (Ollama)"]
              .map(o=>{const[v,l]=o.split("|");return `<option value="${v}" ${settings.provider===v?"selected":""}>${l}</option>`}).join("")}
          </select></div>
          <div class="field"><label>Modelo</label><input id="set-model" type="text" class="mono" value="${esc(settings.model)}" placeholder="gpt-4o-mini"></div>
          <div class="field aj-wide"><label>Base URL</label><input id="set-base_url" type="text" class="mono" value="${esc(settings.base_url)}" placeholder="https://api.openai.com/v1"></div>
          <div class="field aj-wide"><label>API Key</label><input id="set-api_key" type="password" class="mono" value="${esc(settings.api_key)}" placeholder="sk-…"></div>
        </div>
        <div class="aj-note">${ico.lock} La clave se guarda localmente en SQLite, nunca se envía fuera de tu equipo salvo al proveedor configurado.</div>
      </section>

      <section class="pane aj-section">
        <div class="aj-head"><h3>${ico.server} Agente CLI</h3><span class="aj-sub">ruta al binario del agente para integración MCP</span></div>
        <div class="field"><label>Ruta del agente</label><input id="set-agent_path" type="text" class="mono" value="${esc(settings.agent_path)}" placeholder="~/.local/bin/claude"></div>
      </section>

      <section class="pane aj-section">
        <div class="aj-head"><h3>${ico.layers} Preferencias</h3><span class="aj-sub">comportamiento general de la app</span></div>
        <div class="aj-toggles">
          ${toggle("local_first", "Local-first", "Prioriza almacenamiento local antes que sincronización")}
          ${toggle("sync_zotero", "Sincronizar Zotero", "Importa referencias desde tu biblioteca Zotero")}
          ${toggle("cache_pdfs", "Cachear PDFs", "Descarga y guarda adjuntos PDF localmente")}
        </div>
        <div class="field" style="margin-top:14px"><label>Idioma</label><select id="set-lang">
          ${["es|Español","en|English"].map(o=>{const[v,l]=o.split("|");return `<option value="${v}" ${settings.lang===v?"selected":""}>${l}</option>`}).join("")}
        </select></div>
      </section>

      <section class="pane aj-section aj-danger">
        <div class="aj-head"><h3>${ico.shield} Zona de peligro</h3><span class="aj-sub">acciones irreversibles</span></div>
        <p class="aj-danger-desc">Borra <b>toda</b> la base de datos —proyectos, referencias, revisiones, acciones, chats y ajustes— y la recrea desde cero. Tras reiniciar, la app volverá al asistente de configuración inicial. No se puede deshacer.</p>
        <button class="btn btn-danger" id="aj-reset">Borrar base de datos y reiniciar</button>
      </section>

      <div class="aj-actions">
        <button class="btn btn-primary" id="aj-save">Guardar ajustes</button>
        <span class="aj-status" id="aj-status"></span>
      </div>
    </div>
  </div>`;
  wireToggles();
  $("#aj-save")!.addEventListener("click", save);
  $("#aj-reset")!.addEventListener("click", confirmReset);
}

async function confirmReset() {
  const btn = $("#aj-reset")!;
  // Two-step confirm: first click arms (text changes), second click within 4s fires.
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

function toggle(key: string, label: string, desc: string) {
  const on = settings[key] === "true" || settings[key] === "1";
  return `<div class="aj-toggle">
    <div class="aj-toggle-info"><div class="aj-toggle-label">${label}</div><div class="aj-toggle-desc">${desc}</div></div>
    <button class="agent-toggle ${on ? "is-on" : ""}" data-key="${key}" role="switch" aria-checked="${on}"><span class="knob"></span></button>
  </div>`;
}

function wireToggles() {
  $$("#aj-status, .aj-toggle .agent-toggle").forEach(() => {});
  $$(".aj-toggle .agent-toggle").forEach((b) =>
    b.addEventListener("click", () => {
      const on = !b.classList.contains("is-on");
      b.classList.toggle("is-on", on);
      b.setAttribute("aria-checked", String(on));
      settings[b.dataset.key!] = on ? "true" : "false";
    }));
}

async function save() {
  const fields = ["provider", "model", "base_url", "api_key", "agent_path", "lang"];
  try {
    for (const k of fields) settings[k] = val("set-" + k);
    for (const [k, v] of Object.entries(settings)) await api.updateSetting(k, v);
    state.settings = { ...settings };
    $("#aj-status")!.textContent = "Guardado ✓";
    toast("Ajustes guardados");
    setTimeout(() => { const s = $("#aj-status"); if (s) s.textContent = ""; }, 2000);
  } catch (e) {
    toast("Error al guardar: " + e);
  }
}

function val(id: string) { return ($("#" + id) as HTMLInputElement | HTMLSelectElement).value.trim(); }
function $(s: string, r: ParentNode = document) { return r.querySelector<HTMLElement>(s); }
function $$(s: string, r: ParentNode = document) { return Array.from(r.querySelectorAll<HTMLElement>(s)); }
