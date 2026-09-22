// ResearchCore v1 — the live app over the design bible
// (prototype/index.html). The shell, render loop, theming, and component
// idioms are carried from the bible; the data flows through src/api.ts
// (Tauri commands, same-origin served reads, or the in-memory mock).
import { api } from "./api";
import { t, setLang, getLang } from "./i18n";
import { icon, esc, btn, toggleRcSelect, pickRcSelect, closeRcSelects } from "./ui/helpers";
import { renderLock, bindLock, renderWizard } from "./ui/lock";
import { RC as RC_BUS, ctx } from "./ui/rc";
import { renderMissionsHome, renderBoard, bindBoard, renderReadinessDrawer, renderReceiptDrawer } from "./ui/missions";
import { renderRefs, bindRefs, renderReview, bindReview, renderAssistant, bindAssistant, renderActions, bindActions, renderDigest, renderStatus, renderSettings, bindSettings } from "./ui/screens";

// ========== ACCENT SLOT (the bible's six swappable options) ==========
const ACCENTS = {
  blue: { label: { es: "Azul", en: "Blue", pt: "Azul", fr: "Bleu" }, hsl: "hsl(211 58% 45%)" },
  teal: { label: { es: "Verde azulado", en: "Teal", pt: "Teal", fr: "Sarcelle" }, hsl: "hsl(188 48% 38%)" },
  slate: { label: { es: "Pizarra", en: "Slate", pt: "Ardósia", fr: "Ardoise" }, hsl: "hsl(210 16% 38%)" },
  indigo: { label: { es: "Índigo", en: "Indigo", pt: "Índigo", fr: "Indigo" }, hsl: "hsl(225 44% 44%)" },
  emerald: { label: { es: "Esmeralda", en: "Emerald", pt: "Esmeralda", fr: "Émeraude" }, hsl: "hsl(158 44% 38%)" },
  amber: { label: { es: "Ámbar", en: "Amber", pt: "Âmbar", fr: "Ambre" }, hsl: "hsl(38 70% 44%)" },
};

// ========== STATE ==========
const app = {
  ACCENTS,
  state: {
    view: "lock",
    previousView: null,
    locked: true,
    sidebarCollapsed: false,
    sidebarFadeLabels: false,
    onboardingCompleted: localStorage.getItem("rc-onboarding") === "1",
    settings: {
      theme: localStorage.getItem("rc-theme") || "light",
      accent: localStorage.getItem("rc-accent") || "blue",
      animations:
        localStorage.getItem("rc-animations") !== null
          ? localStorage.getItem("rc-animations") === "1"
          : !(window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches),
    },
    wizard: {
      step: 0,
      maxStepReached: 0,
      direction: 1,
      dataFolder: "~/ResearchCore/Data",
      llmProvider: "Personal",
      baseUrl: "https://api.tokenfactory.corvex.cloud/v1",
      apiKey: "",
      llmModel: "zai-org/GLM-5.3",
      initMode: "ai",
      initUrl: "",
      initLoading: false,
      initResult: null,
      initQuestion: "",
      initStop: "",
      vaultEnabled: false,
      vaultPassphrase: "",
      vaultHint: "",
      idleTimeout: 10,
    },
    boardMissionId: null,
    readinessOpen: false,
    readinessMissionId: null,
    receiptRunId: null,
    commandPaletteOpen: false,
    projectMenuOpen: false,
  },
  data: {
    project: null,
    projects: [],
    missions: [],
    missionsLoaded: false,
    runs: {}, // missionId -> MissionRun[]
    runsOpen: {}, // missionId -> bool (drill-down toggle)
    board: {}, // missionId -> { hyps: [], evidence: Record<hypId, Claim[]>, proposals: [] }
    digest: null,
    trust: null,
    targets: [],
    refs: [],
    refFilter: "",
    mcp: [],
    chats: [],
    chatMessages: {},
    activeChatId: null,
    chatThinking: false,
  },
};

// theming
function _sysDark() { return !!(window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches); }
function applyAccent() {
  const a = ACCENTS[app.state.settings.accent] || ACCENTS.blue;
  document.documentElement.style.setProperty("--accent", a.hsl);
}
let _themeMqBound = false;
function applyTheme() {
  const s = app.state.settings;
  const dark = s.theme === "dark" || (s.theme === "system" && _sysDark());
  document.documentElement.classList.toggle("dark", dark);
  if (!_themeMqBound && window.matchMedia) {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    if (mq.addEventListener) mq.addEventListener("change", () => { if (app.state.settings.theme === "system") applyTheme(); });
    _themeMqBound = true;
  }
}
function applyMotion() {
  document.documentElement.classList.toggle("rc-no-motion", !app.state.settings.animations);
}

// ========== ROUTER ==========
const routes = ["missions", "refs", "review", "assistant", "actions", "digest", "status", "settings", "board"];
function navigate(view) {
  if (!routes.includes(view)) view = "missions";
  app.state.previousView = app.state.view;
  app.state.view = view;
  app.state.commandPaletteOpen = false;
  render();
  loadViewData(view);
}
function lockApp() { app.state.locked = true; app.state.view = "lock"; render(); }
app.unlock = unlock;
function unlock() {
  app.state.locked = false;
  if (!app.state.onboardingCompleted) { app.state.view = "wizard"; render(); }
  else { navigate("missions"); }
}

// ========== SHELL ==========
function renderShell() {
  const st = app.state;
  const navItems = [
    { id: "missions", label: t("rc.nav.home"), icon: "target" },
    { id: "refs", label: t("rc.nav.refs"), icon: "book" },
    { id: "review", label: t("rc.nav.review"), icon: "sparkle" },
    { id: "assistant", label: t("rc.nav.assistant"), icon: "message" },
    { id: "actions", label: t("rc.nav.actions"), icon: "check" },
    { id: "digest", label: t("rc.nav.digest"), icon: "sun" },
    { id: "status", label: t("rc.nav.status"), icon: "activity" },
    { id: "settings", label: t("rc.nav.settings"), icon: "settings" },
  ];
  return `
    <div class="flex h-screen overflow-hidden bg-background">
      <aside id="sidebar" class="${st.sidebarCollapsed ? "w-[68px]" : "w-64"} flex-shrink-0 border-r border-border bg-card flex flex-col overflow-hidden">
        <div class="h-16 flex items-center px-5 border-b border-border flex-shrink-0">
          <div class="flex h-8 w-8 items-center justify-center rounded-lg bg-primary text-white mr-3">${icon("sparkle", "w-4 h-4")}</div>
          ${!st.sidebarCollapsed ? `<span class="font-serif text-xl italic whitespace-nowrap${st.sidebarFadeLabels ? " animate-rc-sidebar-fade" : ""}">${t("rc.appName")}</span>` : ""}
        </div>
        <nav class="flex-1 overflow-y-auto py-4 px-3 space-y-1">
          ${navItems.map((item) => `
            <button onclick="RC.navigate('${item.id}')" class="w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition whitespace-nowrap ${st.view === item.id ? "bg-primary/10 text-primary" : "text-muted hover:bg-gray-100 hover:text-foreground"} ${st.sidebarCollapsed ? "justify-center" : ""}">
              ${icon(item.icon, "w-5 h-5 shrink-0")}
              ${!st.sidebarCollapsed ? `<span class="${st.sidebarFadeLabels ? "animate-rc-sidebar-fade" : ""}">${item.label}</span>` : ""}
            </button>
          `).join("")}
        </nav>
        <div class="p-3 border-t border-border flex-shrink-0">
          <button onclick="RC.toggleSidebar()" class="w-full flex items-center justify-center gap-2 rounded-lg px-3 py-2 text-xs font-medium text-muted hover:bg-gray-100 transition whitespace-nowrap">
            ${st.sidebarCollapsed ? icon("chevron", "w-4 h-4 rotate-180") : icon("chevron", "w-4 h-4")}
            ${!st.sidebarCollapsed ? `<span class="${st.sidebarFadeLabels ? "animate-rc-sidebar-fade" : ""}">${t("rc.common.collapse")}</span>` : ""}
          </button>
        </div>
      </aside>
      <div class="flex-1 flex flex-col min-w-0">
        <header class="h-16 flex items-center justify-between px-6 border-b border-border bg-card/80 backdrop-blur sticky top-0 z-30">
          <div class="flex items-center gap-4">
            <div class="relative">
              <button onclick="RC.toggleProjectMenu(event)" class="flex items-center gap-2 rounded-lg px-3 py-1.5 text-sm font-medium text-foreground hover:bg-gray-100 transition ring-focus">
                ${icon("folder", "w-4 h-4 text-muted")} <span class="max-w-[200px] truncate">${esc(app.data.project?.name || t("rc.project.empty"))}</span> ${icon("chevronDown", "w-4 h-4 text-muted")}
              </button>
              ${st.projectMenuOpen ? renderProjectMenu() : ""}
            </div>
          </div>
          <div class="flex items-center gap-2">
            <button onclick="RC.navigate('settings')" class="rounded-lg border border-border bg-white p-2 text-muted hover:text-foreground hover:border-primary/30 transition ring-focus" aria-label="${t("rc.nav.settings")}">${icon("settings", "w-4 h-4")}</button>
            <button onclick="RC.lockApp()" class="rounded-lg border border-border bg-white p-2 text-muted hover:text-foreground hover:border-primary/30 transition ring-focus" aria-label="${t("rc.lock.title")}">${icon("lock", "w-4 h-4")}</button>
          </div>
        </header>
        <main id="main-content" class="flex-1 overflow-y-auto p-6"></main>
      </div>
    </div>`;
}

function renderProjectMenu() {
  return `
    <div class="absolute left-0 top-full mt-1 w-72 rounded-xl border border-border bg-white shadow-lg z-40 overflow-hidden animate-fade-up">
      <div class="px-3 py-2 border-b border-border">
        <p class="text-xs font-semibold text-muted uppercase tracking-wider">${esc(app.data.project?.name || t("rc.project.active"))}</p>
      </div>
      <div class="max-h-72 overflow-y-auto py-1">
        ${app.data.projects.map((p) => `
          <div class="flex items-center gap-1 px-2 py-1">
            <button class="min-w-0 flex-1 flex items-center justify-between gap-2 px-2 py-1.5 rounded-lg text-left hover:bg-gray-50 transition">
              <div class="min-w-0">
                <p class="text-sm font-medium truncate ${app.data.project?.id === p.id ? "text-primary" : "text-foreground"}">${esc(p.name)}</p>
                <p class="text-xs text-muted truncate">${esc(p.tags || "")}</p>
              </div>
              ${app.data.project?.id === p.id ? `<span class="text-xs font-medium text-primary shrink-0">${t("rc.project.active")}</span>` : ""}
            </button>
          </div>
        `).join("") || `<p class="px-3 py-4 text-sm text-muted text-center">${t("rc.project.empty")}</p>`}
      </div>
    </div>`;
}

function toggleSidebar() {
  const wasCollapsed = app.state.sidebarCollapsed;
  app.state.sidebarCollapsed = !wasCollapsed;
  app.state.sidebarFadeLabels = !app.state.sidebarCollapsed;
  render();
  app.state.sidebarFadeLabels = false;
  const sb = document.getElementById("sidebar");
  if (!sb || !app.state.settings.animations) return;
  const fromW = wasCollapsed ? "68px" : "256px";
  const toW = app.state.sidebarCollapsed ? "68px" : "256px";
  sb.style.transition = "none";
  sb.style.width = fromW;
  void sb.offsetWidth;
  sb.style.transition = "width 280ms cubic-bezier(0.16, 1, 0.3, 1)";
  sb.style.width = toW;
  sb.addEventListener("transitionend", function onSidebarWidthEnd(e) {
    if (e.target !== sb || e.propertyName !== "width") return;
    sb.style.transition = "";
    sb.style.width = "";
    sb.removeEventListener("transitionend", onSidebarWidthEnd);
  });
}

// ========== RENDER ==========
function render() {
  applyMotion();
  const root = document.getElementById("app");
  document.getElementById("cmd-overlay")?.remove();
  document.querySelectorAll(".rc-modal").forEach((el) => el.remove());
  if (app.state.view === "lock") { root.innerHTML = renderLock(app); bindLock(app); return; }
  if (app.state.view === "wizard") { root.innerHTML = renderWizard(app); return; }
  root.innerHTML = renderShell();
  renderMain();
  if (app.state.commandPaletteOpen) renderCommandPalette();
  if (app.state.readinessOpen) renderReadinessDrawer(app);
  if (app.state.receiptRunId) renderReceiptDrawer(app);
}

function renderMain() {
  const main = document.getElementById("main-content");
  if (!main) return;
  let html = "";
  switch (app.state.view) {
    case "missions": html = renderMissionsHome(app); break;
    case "refs": html = renderRefs(app); break;
    case "review": html = renderReview(app); break;
    case "assistant": html = renderAssistant(app); break;
    case "actions": html = renderActions(app); break;
    case "digest": html = renderDigest(app); break;
    case "status": html = renderStatus(app); break;
    case "settings": html = renderSettings(app); break;
    case "board": html = renderBoard(app); break;
    default: html = renderMissionsHome(app);
  }
  main.innerHTML = `<div class="max-w-7xl mx-auto animate-fade-up">${html}</div>`;
  if (app.state.view === "refs") bindRefs(app);
  if (app.state.view === "review") bindReview(app);
  if (app.state.view === "assistant") bindAssistant(app);
  if (app.state.view === "actions") bindActions(app);
  if (app.state.view === "settings") bindSettings(app);
  if (app.state.view === "board") bindBoard(app);
}

function renderMainOnly() {
  renderMain();
}

// ========== DATA LOADERS ==========
async function guard(p, errKey) {
  try { return await p; } catch (e) { console.error(e); return null; }
}

async function loadBase() {
  const [projects, project] = await Promise.all([
    guard(api.listProjects(), null),
    guard(api.getActiveProject(), null),
  ]);
  if (projects) app.data.projects = projects;
  if (project) app.data.project = project;
  else app.data.project = projects?.[0] || null;
  const mcp = await guard(api.listMcpServers(), null);
  if (mcp) app.data.mcp = mcp;
}

async function loadMissions() {
  try {
    app.data.missions = await api.listMissions();
    app.data.missionsLoaded = true;
    renderMainOnly();
  } catch (e) {
    console.error(e);
    app.data.missionsLoaded = true;
    renderMainOnly();
  }
}

async function loadRuns(missionId) {
  try {
    app.data.runs[missionId] = await api.getMissionRuns(missionId);
    renderMainOnly();
  } catch (e) { console.error(e); }
}

async function loadBoard(missionId) {
  try {
    const hyps = await api.listHypotheses(missionId);
    const proposals = await api.listProposals(missionId);
    const evidence = {};
    await Promise.all(hyps.map(async (h) => {
      try { evidence[h.id] = await api.listEvidence(h.id); } catch (e) { evidence[h.id] = []; }
    }));
    app.data.board[missionId] = { hyps, evidence, proposals };
    renderMainOnly();
  } catch (e) {
    console.error(e);
    app.data.board[missionId] = { hyps: [], evidence: {}, proposals: [] };
    renderMainOnly();
  }
}

async function loadDigest() {
  try {
    app.data.digest = await api.getMorningDigest();
    renderMainOnly();
  } catch (e) { console.error(e); }
}

async function loadTrust() {
  try {
    app.data.trust = await api.getTrustStatus();
    renderMainOnly();
  } catch (e) { console.error(e); }
}

async function loadTargets() {
  try {
    app.data.targets = await api.listComputeTargets();
    renderMainOnly();
  } catch (e) { console.error(e); }
}

async function loadRefs() {
  try {
    app.data.refs = await api.listRefs(app.data.project?.id || "p1", null);
    renderMainOnly();
  } catch (e) { console.error(e); }
}

function loadViewData(view) {
  if (view === "missions") loadMissions();
  if (view === "refs") loadRefs();
  if (view === "digest") loadDigest();
  if (view === "settings") { loadTrust(); loadTargets(); }
}

// ========== COMMAND PALETTE ==========
function renderCommandPalette() {
  const commands = [
    { name: t("rc.nav.home"), action: "RC.navigate('missions')" },
    { name: t("rc.nav.refs"), action: "RC.navigate('refs')" },
    { name: t("rc.nav.review"), action: "RC.navigate('review')" },
    { name: t("rc.nav.assistant"), action: "RC.navigate('assistant')" },
    { name: t("rc.nav.actions"), action: "RC.navigate('actions')" },
    { name: t("rc.nav.digest"), action: "RC.navigate('digest')" },
    { name: t("rc.nav.status"), action: "RC.navigate('status')" },
    { name: t("rc.nav.settings"), action: "RC.navigate('settings')" },
    { name: t("rc.lock.title"), action: "RC.lockApp()" },
  ];
  const overlay = document.createElement("div");
  overlay.id = "cmd-overlay";
  overlay.className = "fixed inset-0 z-[60] flex items-start justify-center bg-black/30 backdrop-blur-sm pt-[15vh]";
  overlay.innerHTML = `
    <div class="w-full max-w-lg animate-scale-in">
      <div class="rounded-xl border border-border bg-white shadow-xl overflow-hidden">
        <div class="flex items-center gap-3 border-b border-border px-4 py-3">
          ${icon("search", "w-5 h-5 text-muted")}
          <input id="cmd-input" class="flex-1 bg-transparent text-sm focus:outline-none" placeholder="${t("rc.cmd.placeholder")}" autofocus>
          <kbd class="rounded border border-border px-1.5 text-[10px] text-muted">ESC</kbd>
        </div>
        <div id="cmd-list" class="max-h-[50vh] overflow-y-auto py-2">
          <p class="px-4 py-1.5 text-xs font-semibold text-muted uppercase tracking-wider">${t("rc.cmd.views")}</p>
          ${commands.map((c, i) => `
            <button data-cmd="${i}" onclick="RC.closeCommandPalette(); ${c.action}" class="cmd-item w-full px-4 py-2.5 text-left text-sm hover:bg-gray-50 flex items-center justify-between ${i === 0 ? "bg-gray-50" : ""}">
              <span>${c.name}</span>
              ${icon("chevron", "w-4 h-4 text-muted")}
            </button>
          `).join("")}
        </div>
      </div>
    </div>`;
  document.body.appendChild(overlay);
  const input = document.getElementById("cmd-input");
  input.focus();
  input.oninput = () => {
    const q = input.value.toLowerCase();
    document.querySelectorAll(".cmd-item").forEach((item) => {
      const match = item.textContent.toLowerCase().includes(q);
      item.style.display = match ? "flex" : "none";
      item.classList.toggle("bg-gray-50", false);
    });
    const visible = document.querySelectorAll('.cmd-item[style*="display: flex"], .cmd-item:not([style*="display: none"])');
    if (visible.length) visible[0].classList.add("bg-gray-50");
  };
  input.onkeydown = (e) => {
    if (e.key === "Escape") closeCommandPalette();
    const items = Array.from(document.querySelectorAll(".cmd-item")).filter((x) => x.style.display !== "none");
    let idx = items.findIndex((x) => x.classList.contains("bg-gray-50"));
    if (e.key === "ArrowDown") { e.preventDefault(); idx = (idx + 1) % items.length; }
    if (e.key === "ArrowUp") { e.preventDefault(); idx = (idx - 1 + items.length) % items.length; }
    items.forEach((x) => x.classList.remove("bg-gray-50"));
    if (items[idx]) items[idx].classList.add("bg-gray-50");
    if (e.key === "Enter" && items[idx]) items[idx].click();
  };
  overlay.onclick = (e) => { if (e.target === overlay) closeCommandPalette(); };
}
function openCommandPalette() { app.state.commandPaletteOpen = true; render(); }
function closeCommandPalette() { app.state.commandPaletteOpen = false; render(); }

// ========== GLOBAL EVENTS ==========
document.addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
    e.preventDefault();
    openCommandPalette();
    return;
  }
  if (e.key === "Escape") {
    if (app.state.commandPaletteOpen) { closeCommandPalette(); return; }
    if (app.state.readinessOpen) { app.state.readinessOpen = false; render(); return; }
    if (app.state.receiptRunId) { app.state.receiptRunId = null; render(); return; }
    if (app.state.projectMenuOpen) { app.state.projectMenuOpen = false; render(); return; }
    closeRcSelects();
  }
});
document.addEventListener("click", (e) => {
  if (!e.target.closest(".rc-select")) closeRcSelects();
  if (app.state.projectMenuOpen && !e.target.closest('[onclick*="toggleProjectMenu"]') && !e.target.closest(".relative > .absolute")) {
    app.state.projectMenuOpen = false;
    render();
  }
});

// ========== WIZARD HANDLERS ==========
function wizSaveInputs() {
  const s = app.state.wizard;
  const g = (id) => document.getElementById(id)?.value;
  if (s.step === 1) s.dataFolder = g("wiz-folder") ?? s.dataFolder;
  if (s.step === 2) { s.baseUrl = g("wiz-base") ?? s.baseUrl; s.apiKey = g("wiz-key") ?? s.apiKey; s.llmModel = g("wiz-model") ?? s.llmModel; }
  if (s.step === 3) {
    if (s.initMode === "ai") s.initUrl = g("wiz-arxiv") ?? s.initUrl;
    else { s.initQuestion = g("wiz-q") ?? s.initQuestion; s.initStop = g("wiz-stop") ?? s.initStop; }
  }
  if (s.step === 4) { s.vaultPassphrase = g("wiz-pass") ?? s.vaultPassphrase; s.vaultHint = g("wiz-hint") ?? s.vaultHint; s.idleTimeout = parseInt(g("wiz-timeout") || "10", 10); }
}

async function wizFirstValue() {
  const s = app.state.wizard;
  const url = document.getElementById("wiz-arxiv")?.value.trim() || s.initUrl;
  if (!url) return;
  s.initUrl = url;
  s.initLoading = true;
  render();
  try {
    s.initResult = await api.runFirstValue(url);
    app.data.missionsLoaded = false;
  } catch (e) {
    console.error(e);
    alert(t("onb.error") + " " + (e?.message || e));
  }
  s.initLoading = false;
  render();
}

function wizNext() {
  const s = app.state.wizard;
  wizSaveInputs();
  // The AI-init step needs a result (or the manual fields) before advancing.
  if (s.step === 3 && s.initMode === "ai" && !s.initResult) { wizFirstValue(); return; }
  s.direction = 1;
  s.step = Math.min(s.step + 1, 6);
  s.maxStepReached = Math.max(s.maxStepReached, s.step);
  render();
}
function wizBack() {
  const s = app.state.wizard;
  wizSaveInputs();
  s.direction = -1;
  s.step = Math.max(s.step - 1, 0);
  render();
}
function wizGoTo(i) {
  const s = app.state.wizard;
  if (i > s.maxStepReached) return;
  wizSaveInputs();
  s.direction = i > s.step ? 1 : -1;
  s.step = i;
  render();
}
function wizFinish() {
  wizSaveInputs();
  localStorage.setItem("rc-onboarding", "1");
  app.state.onboardingCompleted = true;
  app.state.locked = false;
  navigate("missions");
}
function wizProvider(p) {
  app.state.wizard.llmProvider = p;
  const defaults = {
    OpenAI: ["https://api.openai.com/v1", "gpt-4o"],
    Anthropic: ["https://api.anthropic.com/v1", "claude-sonnet-4-5"],
    Ollama: ["http://localhost:11434", "llama3.1"],
    Personal: ["https://api.tokenfactory.corvex.cloud/v1", "zai-org/GLM-5.3"],
  };
  const [base, model] = defaults[p] || defaults.Personal;
  app.state.wizard.baseUrl = base;
  app.state.wizard.llmModel = model;
  render();
}
function wizVault(on) { app.state.wizard.vaultEnabled = on; render(); }
function wizInitMode(m) { app.state.wizard.initMode = m; render(); }
function wizToggleMcp(id) {
  const s = app.data.mcp.find((x) => x.id === id);
  if (s) s.connected = s.connected ? 0 : 1;
  render();
}

// ========== SETTINGS HANDLERS ==========
function setTheme(val) { app.state.settings.theme = val; localStorage.setItem("rc-theme", val); applyTheme(); render(); }
function setAccent(name) {
  if (!ACCENTS[name]) return;
  app.state.settings.accent = name;
  localStorage.setItem("rc-accent", name);
  applyAccent();
  render();
}
function setAnimations(b) {
  app.state.settings.animations = !!b;
  localStorage.setItem("rc-animations", b ? "1" : "0");
  applyMotion();
  render();
}

// Core handlers land on the shared RC namespace (screen modules attach
// their own); the live context is published for them.
Object.assign(RC_BUS, {
  navigate, lockApp,
  toggleSidebar,
  toggleProjectMenu: (e) => { if (e) e.stopPropagation(); app.state.projectMenuOpen = !app.state.projectMenuOpen; render(); },
  setLang: (l) => { setLang(l); localStorage.setItem("rc-lang", l); document.documentElement.lang = getLang(); render(); },
  toggleRcSelect, pickRcSelect,
  togglePw: (id, el) => {
    const input = document.getElementById(id);
    if (!input) return;
    input.type = input.type === "password" ? "text" : "password";
    if (el) el.innerHTML = icon(input.type === "password" ? "eye" : "eyeOff", "w-4 h-4");
  },
  // wizard
  wizNext, wizBack, wizGoTo, wizProvider, wizVault, wizInitMode, wizFirstValue, wizFinish, wizToggleMcp,
  // settings
  setTheme, setAccent, setAnimations,
  // command palette
  openCommandPalette, closeCommandPalette,
});
window.RC = RC_BUS;
Object.assign(ctx, {
  app, render, renderMainOnly, navigate,
  loadMissions, loadRuns, loadBoard, loadDigest, loadTrust, loadTargets, loadRefs, loadViewData,
});

export { app, render, renderMainOnly, navigate };
export {
  loadMissions, loadRuns, loadBoard, loadDigest, loadTrust, loadTargets, loadRefs, loadViewData,
};

// ========== BOOT ==========
const savedLang = localStorage.getItem("rc-lang");
if (savedLang) setLang(savedLang);
document.documentElement.lang = getLang();
applyAccent();
applyTheme();
applyMotion();
render();
loadBase().then(() => {
  render();
  if (!app.state.locked && app.state.view !== "lock" && app.state.view !== "wizard") loadViewData(app.state.view);
});
