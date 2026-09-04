import "./styles.css";
import { api } from "./api";
import type { Project } from "./types";
import { ico } from "./icons";
import { renderDashboard } from "./views/dashboard";
import { renderRefs } from "./views/refs";
import { renderReview } from "./views/review";
import { renderAsistente } from "./views/asistente";
import { renderAcciones } from "./views/acciones";
import { renderStatus } from "./views/status";
import { renderAjustes } from "./views/ajustes";
import { renderSplash, renderLogin } from "./views/welcome";
import { renderWizard } from "./views/wizard";
import { renderTutorial } from "./views/tutorial";
import { showLockScreen } from "./views/lockscreen";

export type Tab = "investigacion" | "refs" | "ai-review" | "asistente" | "acciones" | "status" | "ajustes";

export const state: {
  projects: Project[];
  active: Project | null;
  tab: Tab;
  settings: Record<string, string>;
  onboarded: boolean;
  mcpInitShown: boolean;
  /** Pending local access key captured at login, consumed by the wizard Step 3
   *  to hash + persist the lock policy. Cleared after the wizard completes. */
  pendingKey: string;
} = {
  projects: [],
  active: null,
  tab: "investigacion",
  settings: {},
  onboarded: false,
  mcpInitShown: false,
  pendingKey: "",
};

export const $ = (sel: string, root: ParentNode = document) => root.querySelector<HTMLElement>(sel);
export const $$ = (sel: string, root: ParentNode = document) => Array.from(root.querySelectorAll<HTMLElement>(sel));

/** Build an element from an HTML string. */
export function el<T extends HTMLElement = HTMLElement>(html: string): T {
  const t = document.createElement("template");
  t.innerHTML = html.trim();
  return t.content.firstElementChild as T;
}

/** Escape text for safe interpolation. */
export function esc(s: unknown): string {
  return String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]!));
}

let toastTimer: number | undefined;
export function toast(msg: string) {
  let t = $("#toast") as HTMLElement | null;
  if (!t) {
    t = el(`<div id="toast" class="toast"></div>`);
    document.body.appendChild(t);
  }
  t.textContent = msg;
  t.classList.add("show");
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => t!.classList.remove("show"), 2600);
}

export function setActive(p: Project | null) {
  state.active = p;
}

const TABS: { id: Tab; label: string }[] = [
  { id: "investigacion", label: "Inicio" },
  { id: "refs", label: "Refs" },
  { id: "ai-review", label: "AI Review" },
  { id: "asistente", label: "Asistente" },
  { id: "acciones", label: "Acciones" },
  { id: "status", label: "Status y config" },
];

function navTabsHtml() {
  return TABS.map((t) =>
    `<button class="nav-tab ${state.tab === t.id ? "is-active" : ""}" data-tab="${t.id}">${t.label}</button>`
  ).join("");
}

function projSelectorHtml() {
  const p = state.active;
  if (!p) {
    return `<button class="proj-selector" data-action="open-projects" style="color:var(--muted);border-style:dashed">
      <span>Sin proyecto</span>${ico.chevron}</button>`;
  }
  return `<button class="proj-selector" data-action="open-projects">
    <span class="pdot" style="background:${p.color}"></span>
    <span>${esc(p.name)}</span>${ico.chevron}</button>`;
}

export function renderShell() {
  const app = $("#app")!;
  app.innerHTML = `<div class="app-window">
    <header class="app-nav" data-tauri-drag-region>
      ${projSelectorHtml()}
      <nav class="nav-tabs" aria-label="Pestañas del proyecto">${navTabsHtml()}</nav>
      <div class="nav-spacer" data-tauri-drag-region></div>
      <button class="nav-tab is-global ${state.tab === "ajustes" ? "is-active" : ""}" data-tab="ajustes">Ajustes</button>
      <div class="identity-chip" title="Usuario">RC</div>
    </header>
    <div id="subheader"></div>
    <main id="view" style="flex:1;min-height:0;display:flex;flex-direction:column"></main>
  </div>`;
  wireNav();
  renderSubheader();
  renderView();
}

function wireNav() {
  $$("#app [data-tab]").forEach((b) =>
    b.addEventListener("click", () => setTab(b.dataset.tab as Tab))
  );
  const ps = $("#app [data-action='open-projects']");
  ps?.addEventListener("click", openProjectsModal);
}

export function setTab(tab: Tab) {
  state.tab = tab;
  // refresh active tab styling
  $$("#app .nav-tabs [data-tab]").forEach((b) =>
    b.classList.toggle("is-active", b.dataset.tab === tab)
  );
  renderSubheader();
  renderView();
}

function renderSubheader() {
  const sh = $("#subheader")!;
  if (!state.active || state.tab === "ajustes") {
    sh.innerHTML = "";
    sh.style.display = "none";
    return;
  }
  sh.style.display = "";
  const p = state.active;
  const kindLabel: Record<string, string> = {
    "thesis-chapter": "Capítulo de Tesis",
    "research-done": "Investigación hecha",
    paper: "Paper / artículo",
    other: "Otro",
  };
  sh.innerHTML = `<div class="sub-header">
    <span class="sub-title">${esc(p.name)}</span>
    <span class="type-badge">${kindLabel[p.kind] ?? "Proyecto"}</span>
    <span class="folder-path">${ico.folder} ${esc(p.folder)}</span>
    <span class="sub-spacer"></span>
    <span class="sub-meta" id="sub-meta">cargando…</span>
  </div>`;
  api.getDashboard(p.id).then((d) => {
    $("#sub-meta")!.textContent = `${d.refs_total} refs · ${d.refs_used} usadas · ${d.active_actions} actions activas`;
  }).catch(() => {});
}

export async function renderView() {
  const view = $("#view")!;
  if (state.tab === "ajustes") {
    await renderAjustes(view);
    return;
  }
  if (!state.active) {
    renderZeroProjects(view);
    return;
  }
  switch (state.tab) {
    case "investigacion": await renderDashboard(view); break;
    case "refs": await renderRefs(view); break;
    case "ai-review": await renderReview(view); break;
    case "asistente": await renderAsistente(view); break;
    case "acciones": await renderAcciones(view); break;
    case "status": await renderStatus(view); break;
  }
}

/** Zero-projects hero (Frame 02) — shown when no project is active. */
function renderZeroProjects(view: HTMLElement) {
  const name = state.settings.user_name || "";
  const hello = name ? `Hola, ${name}` : "Bienvenido";
  view.innerHTML = `<div class="no-proj-body">
    <div class="no-proj-hero">
      <div class="no-proj-icon">${ico.book}</div>
      <div class="no-proj-hello">${esc(hello)}</div>
      <h2 class="no-proj-title">Empieza tu primera investigación</h2>
      <p class="no-proj-desc">Crea un proyecto para organizar papers, referencias, revisiones y tu manuscrito. Todo se guarda localmente en tu equipo.</p>
      <div class="no-proj-cta">
        <button class="btn btn-primary" data-action="open-projects">Crear proyecto</button>
        <button class="btn btn-secondary" data-action="open-projects">Abrir existente</button>
      </div>
      <div class="no-proj-tips">
        <div class="no-proj-tip"><div class="no-proj-tip-icon">${ico.doc}</div><div class="no-proj-tip-title">Importa PDFs</div><div class="no-proj-tip-desc">Arrastra PDFs y se extrae el DOI automáticamente para vincular el paper.</div></div>
        <div class="no-proj-tip"><div class="no-proj-tip-icon">${ico.check}</div><div class="no-proj-tip-title">Revisa con IA</div><div class="no-proj-tip-desc">Usa jueces personalizados o predefinidos para revisar tu manuscrito y código.</div></div>
        <div class="no-proj-tip"><div class="no-proj-tip-icon">${ico.clock}</div><div class="no-proj-tip-title">Conecta tu agente</div><div class="no-proj-tip-desc">Vincula un CLI agent mediante MCP para automatizar revisiones y tareas.</div></div>
      </div>
    </div>
  </div>`;
  $$("[data-action='open-projects']", view).forEach((b) =>
    b.addEventListener("click", openProjectsModal));
}

// ---------- Project modal ----------
function openProjectsModal() {
  const overlay = el(`<div class="modal-overlay" id="proj-modal-overlay">
    <div class="modal" role="dialog" aria-modal="true">
      <div class="modal-head">
        <div class="modal-title">Proyecto</div>
        <div class="modal-tabs" role="tablist">
          <button class="modal-tab" data-mtab="open">Abrir existente</button>
          <button class="modal-tab is-active" data-mtab="new">Crear nuevo</button>
        </div>
      </div>
      <div class="modal-body" id="modal-body"></div>
      <div class="modal-foot">
        <button class="btn btn-secondary" data-action="close-modal">Cancelar</button>
        <button class="btn btn-primary" id="modal-confirm">Crear proyecto</button>
      </div>
    </div>
  </div>`);
  $(".app-window")!.appendChild(overlay);
  let mtab: "open" | "new" = "new";
  const colors = ["#3B5BDB", "#2F9E6B", "#D98324", "#9B5DE5", "#E64980"];

  function renderBody() {
    $$("#proj-modal-overlay .modal-tab").forEach((b) =>
      b.classList.toggle("is-active", b.dataset.mtab === mtab));
    const confirm = $("#modal-confirm")!;
    const body = $("#modal-body")!;
    if (mtab === "open") {
      confirm.textContent = "Abrir proyecto";
      body.innerHTML = `<div class="proj-list" id="proj-list"><p class="ds-label">Cargando…</p></div>`;
      api.listProjects().then((projects) => {
        const list = $("#proj-list")!;
        if (!projects.length) { list.innerHTML = `<p style="color:var(--muted)">No hay proyectos. Crea uno nuevo.</p>`; return; }
        list.innerHTML = projects.map((p) =>
          `<button class="proj-card" data-pid="${p.id}">
            <span class="pdot" style="background:${p.color}"></span>
            <span class="pinfo"><span class="pname">${esc(p.name)}</span>
              <span class="pmeta">${esc(p.folder)}</span></span>
            <span class="pcount">${p.is_active ? "activo" : "abrir"}</span>
          </button>`).join("");
        $$("#proj-modal-overlay .proj-card").forEach((c) =>
          c.addEventListener("click", async () => {
            await api.setActiveProject(c.dataset.pid!);
            await reloadProjects();
            closeModal();
          }));
      });
    } else {
      confirm.textContent = "Crear proyecto";
      body.innerHTML = `
        <div class="field"><label>Nombre del proyecto</label><input id="pf-name" type="text" placeholder="Ej: Optimización de Modelos de Atención"></div>
        <div class="field"><label>Carpeta local</label><input id="pf-folder" type="text" class="mono" placeholder="~/research/mi-proyecto"></div>
        <div class="field"><label>Etiquetas</label><input id="pf-tags" type="text" placeholder="transformer, attention, NLP"></div>
        <div class="field" style="margin-bottom:0"><label>Tipo de proyecto</label>
          <div class="type-grid">
            ${["thesis-chapter|Capítulo de Tesis","research-done|Investigación hecha","paper|Paper / artículo","other|Otro"]
              .map((o, i) => {const [v,l]=o.split("|");return `<button class="type-opt ${i===0?"is-active":""}" data-kind="${v}"><span class="radio"></span>${l}</button>`}).join("")}
          </div>
        </div>`;
      let kind = "thesis-chapter";
      $$("#proj-modal-overlay .type-opt").forEach((o) =>
        o.addEventListener("click", () => {
          kind = o.dataset.kind!;
          $$("#proj-modal-overlay .type-opt").forEach((x) => x.classList.toggle("is-active", x === o));
        }));
      ($("#modal-confirm") as any)._create = () => kind;
    }
  }
  $$("#proj-modal-overlay .modal-tab").forEach((b) =>
    b.addEventListener("click", () => { mtab = b.dataset.mtab as any; renderBody(); }));
  $("#modal-confirm")!.addEventListener("click", async () => {
    if (mtab === "open") {
      // open handled per-card
      return;
    }
    const name = ($("#pf-name") as HTMLInputElement)?.value?.trim();
    if (!name) { toast("Escribe un nombre"); return; }
    const folder = ($("#pf-folder") as HTMLInputElement)?.value?.trim() ?? "";
    const tags = ($("#pf-tags") as HTMLInputElement)?.value?.trim() ?? "";
    const kind = ($("#modal-confirm") as any)._create?.() ?? "thesis-chapter";
    try {
      const p = await api.createProject({ name, folder, kind, tags, color: colors[0] });
      await reloadProjects();
      setActive(p);
      closeModal();
      renderShell();
    } catch (e) { toast("Error: " + e); }
  });
  $("#proj-modal-overlay [data-action='close-modal']")!.addEventListener("click", closeModal);
  $("#proj-modal-overlay")!.addEventListener("click", (e) => { if (e.target === $("#proj-modal-overlay")) closeModal(); });
  renderBody();
}

function closeModal() { $("#proj-modal-overlay")?.remove(); }

export async function reloadProjects() {
  state.projects = await api.listProjects();
  state.active = state.projects.find((p) => p.is_active) ?? null;
}

// Global error surface — never let the window go blank without an explanation.
function fatalError(stage: string, e: unknown) {
  const msg = e instanceof Error ? `${e.message}\n${e.stack ?? ""}` : String(e);
  console.error(`[fatal:${stage}]`, e);
  try { api.appLog("FATAL " + stage + ": " + msg); } catch {}
  const app = document.getElementById("app");
  if (app) {
    app.innerHTML = `<div style="font-family:Inter,system-ui,sans-serif;padding:40px;color:#1a1a1a;max-width:680px;margin:0 auto">
      <h2 style="color:#d00">Research Core no pudo iniciar</h2>
      <p style="color:#666">Etapa: <b>${stage}</b></p>
      <pre style="background:#f4f4f5;padding:14px;border-radius:8px;overflow:auto;font-size:12px;white-space:pre-wrap">${msg}</pre>
      <button onclick="location.reload()" style="margin-top:16px;padding:8px 16px;background:#3B5BDB;color:#fff;border:none;border-radius:8px;cursor:pointer">Reintentar</button>
    </div>`;
  }
}
(window as any).fatalError = fatalError;
window.addEventListener("error", (e) => fatalError("window.error", e.error ?? e.message));
window.addEventListener("unhandledrejection", (e) => fatalError("promise", e.reason));

// Module-load marker — confirms the JS bundle executed at all.
api.appLog("module: loaded");

async function boot() {
  await api.appLog("boot: start");
  try {
    state.settings = await api.getSettings();
    await api.appLog("boot: settings loaded keys=" + Object.keys(state.settings).length);
  } catch (e) {
    await api.appLog("boot: settings FAILED " + String(e));
    state.settings = {};
  }
  state.onboarded = state.settings.onboarded === "true";
  try {
    await reloadProjects();
    await api.appLog("boot: projects loaded count=" + state.projects.length + " active=" + (state.active?.id ?? "none"));
  } catch (e) {
    await api.appLog("boot: projects FAILED " + String(e));
  }
  try {
    await api.appLog("boot: showing splash + mcp init");
    // Splash runs the MCP init visualization; on completion (or skip) it
    // routes into the onboarding/app flow.
    renderSplash($("#app")!, onSplashDone);
    await api.appLog("boot: routed");
  } catch (e) {
    await api.appLog("boot: THREW " + String(e));
    fatalError("boot-route", e);
  }
}

/** Called when the splash + MCP init finishes. Routes to the next screen. */
async function onSplashDone() {
  // Returning user with on_launch lock → challenge before anything else.
  if (state.onboarded && state.settings.onboarding_complete === "true") {
    const lock = await api.lockState().catch(() => ({ policy: "never", idle_min: "", configured: false }));
    if (lock.policy === "on_launch" && lock.configured) {
      showLockScreen($("#app")!, () => enterApp());
      return;
    }
    enterApp();
    return;
  }
  // First run (or wizard not finished): login → wizard → tutorial → shell.
  if (!state.onboarded) {
    renderLogin($("#app")!, handleLogin);
  } else {
    // Onboarded but wizard incomplete — resume the wizard.
    startWizard(state.pendingKey);
  }
}

/** Login submit — persist name + onboarded flag, stash the key for the wizard,
 *  then start the setup wizard. */
async function handleLogin(name: string, key: string) {
  try {
    await api.updateSetting("user_name", name);
    await api.updateSetting("onboarded", "true");
    state.settings.user_name = name;
    state.pendingKey = key;
  } catch (e) {
    toast("No se pudo guardar: " + e);
  }
  startWizard(key);
}

/** Launch the setup wizard. The key is the local access key captured at login,
 *  consumed by Step 3 to configure the lock policy. */
function startWizard(key: string) {
  renderWizard($("#app")!, key, () => {
    // Wizard complete → tutorial (if unseen) → shell.
    if (state.settings.tutorial_seen !== "true") {
      renderTutorial($("#app")!, enterApp);
    } else {
      enterApp();
    }
  });
}

/** Transition into the main app shell. Sets up the idle-lock timer if the
 *  policy demands it. */
async function enterApp() {
  try {
    state.settings = await api.getSettings();
    state.onboarded = true;
    await reloadProjects();
    renderShell();
    await api.appLog("enterApp: shell rendered, active=" + (state.active?.id ?? "none"));
    armIdleLock();
  } catch (e) {
    fatalError("enterApp", e);
  }
}

/** If the lock policy is "idle", re-lock after X minutes of no interaction. */
function armIdleLock() {
  const policy = state.settings.lock_policy;
  const min = parseInt(state.settings.lock_idle_min || "0", 10);
  if (policy !== "idle" || !min || !state.settings.lock_hash) return;
  let timer: number | undefined;
  const reset = () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => {
      api.appLog("lock: idle timeout");
      showLockScreen($("#app")!, () => enterApp());
    }, min * 60 * 1000);
  };
  ["mousemove", "keydown", "click"].forEach((ev) => window.addEventListener(ev, reset, { passive: true }));
  reset();
}

boot().catch((e) => { fatalError("boot", e); });
