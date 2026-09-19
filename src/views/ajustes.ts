import { api } from "../api";
import { state, setTab, esc, el, toast } from "../main";
import { ico } from "../icons";
import { t, setLang, getLang } from "../i18n";
import { showSettingsSkeleton } from "../skeleton";
import type { McpServer } from "../types";

const APP_VERSION = "0.1.0";
const GITHUB_URL = "https://github.com/ricardoleal20/research-core";

let settings: Record<string, string> = {};
let servers: McpServer[] = [];
let ajView: HTMLElement | null = null;
// Remember the open section so it survives a language-change re-render.
let activeSection = "general";

// Sidebar layout: top items, an IA group with sub-items, and an App item.
// Every clickable entry carries data-section; group labels are decorative.
const IA_CHILDREN: { id: string; label: string }[] = [
  { id: "mcp", label: "ia.mcp" },
  { id: "agent", label: "ia.agent" },
  { id: "provider", label: "ia.provider" },
];

export async function renderAjustes(view: HTMLElement) {
  ajView = view;

  // Show a skeleton placeholder while data loads, guaranteed visible for at
  // least SKELETON_MIN (500ms) even on instant loads.
  const waitSkeleton = showSettingsSkeleton(view, 3);

  // Load data in parallel with the minimum skeleton dwell time.
  const [loaded] = await Promise.all([
    (async () => {
      const s = { ...state.settings, ...(await api.getSettings()) };
      let sv: McpServer[] = [];
      try { sv = await api.listMcpServers(); } catch { sv = []; }
      return { s, sv };
    })(),
    waitSkeleton(),
  ]);
  settings = loaded.s;
  servers = loaded.sv;
  setLang(settings.lang || "es");

  view.innerHTML = `<div class="settings-body">
    <aside class="settings-side">
      <div class="sb-label">${t("ajustes.title")}</div>
      <nav class="settings-nav">
        <button class="settings-nav-item ${activeSection === "general" ? "is-active" : ""}" data-section="general">${t("sec.general")}</button>

        <hr class="settings-nav-sep">

        <div class="settings-nav-group">
          <div class="settings-nav-subtitle">${t("sec.ia")}</div>
          ${IA_CHILDREN.map((c) =>
            `<button class="settings-nav-item settings-nav-sub ${activeSection === c.id ? "is-active" : ""}" data-section="${c.id}">${t(c.label)}</button>`
          ).join("")}
        </div>

        <hr class="settings-nav-sep">

        <button class="settings-nav-item ${activeSection === "app" ? "is-active" : ""}" data-section="app">${t("sec.app")}</button>
      </nav>
    </aside>

    <section class="settings-main" id="aj-main">
      <div class="settings-header">
        <div>
          <h2 class="settings-h2">${t("ajustes.title")}</h2>
          <p class="settings-sub">${t("ajustes.sub")}</p>
        </div>
        <button class="btn btn-primary btn-sm" id="aj-save" type="button">${t("ajustes.save")}</button>
      </div>

      <div class="aj-section ${activeSection === "general" ? "is-active" : ""}" data-section="general" ${activeSection === "general" ? "" : "hidden"}>
        ${cardProfile()}
        ${cardLang()}
        ${cardSecurity()}
        ${cardLocal()}
        ${cardDanger()}
      </div>
      <div class="aj-section ${activeSection === "mcp" ? "is-active" : ""}" data-section="mcp" ${activeSection === "mcp" ? "" : "hidden"}>${cardMcp()}</div>
      <div class="aj-section ${activeSection === "agent" ? "is-active" : ""}" data-section="agent" ${activeSection === "agent" ? "" : "hidden"}>${cardAgent()}</div>
      <div class="aj-section ${activeSection === "provider" ? "is-active" : ""}" data-section="provider" ${activeSection === "provider" ? "" : "hidden"}>${cardIa()}</div>
      <div class="aj-section ${activeSection === "app" ? "is-active" : ""}" data-section="app" ${activeSection === "app" ? "" : "hidden"}>${cardApp()}</div>
    </section>
  </div>`;

  wireSidebar();
  wireToggles();
  wireLang();
  wireSecurity();
  $("#aj-save")!.addEventListener("click", save);
  $("#aj-reset")!.addEventListener("click", confirmReset);
  $("#aj-change-key")?.addEventListener("click", changeKey);
  $("#aj-mcp-add")?.addEventListener("click", () => setTab("status"));
}

/* ---------- General cards ---------- */

function cardProfile() {
  return `<div class="card" id="card-profile">
    <div class="card-head"><div class="card-title">${ico.pen} ${t("profile.title")}</div></div>
    <div class="field" style="margin-bottom:0"><label>${t("profile.name")}</label><input id="set-user_name" type="text" value="${esc(settings.user_name)}" placeholder="${t("profile.namePh")}"></div>
  </div>`;
}

function cardLang() {
  const cur = getLang();
  const opts: [string, string][] = [
    ["es", "Español (es-ES)"], ["en", "English (en-US)"],
    ["pt", "Português (pt-BR)"], ["fr", "Français (fr-FR)"],
  ];
  return `<div class="card" id="card-lang">
    <div class="card-head"><div class="card-title">${ico.globe} ${t("lang.title")}</div></div>
    <div class="field" style="margin-bottom:0"><label>${t("lang.title")}</label>
      <select id="set-lang">
        ${opts.map(([v, l]) => `<option value="${v}" ${cur === v ? "selected" : ""}>${l}</option>`).join("")}
      </select>
    </div>
  </div>`;
}

function cardSecurity() {
  const policy = settings.lock_policy || "never";
  const policies: [string, string][] = [
    ["never", "sec.policyNever"],
    ["on_launch", "sec.policyOnLaunch"],
    ["idle", "sec.policyIdle"],
    ["sensitive", "sec.policySensitive"],
  ];
  return `<div class="card" id="card-security">
    <div class="card-head"><div class="card-title">${ico.lock} ${t("sec.security")}</div></div>
    <div class="field-row">
      <div class="field"><label>${t("sec.lockPolicy")}</label><select id="set-lock_policy">
        ${policies.map(([v, k]) => `<option value="${v}" ${policy === v ? "selected" : ""}>${t(k)}</option>`).join("")}
      </select></div>
      <div class="field ${policy === "idle" ? "" : "is-hidden"}" id="sec-idle-wrap"><label>${t("sec.idleMin")}</label><input id="set-lock_idle_min" type="number" min="1" max="240" value="${esc(settings.lock_idle_min || "15")}"></div>
    </div>
    <div class="sec-key">
      <div class="sec-key-title">${t("sec.changeKey")}</div>
      <div class="field-row">
        <div class="field"><label>${t("sec.newKey")}</label><input id="set-new_key" type="password" class="mono" placeholder="••••••••"></div>
        <div class="field" style="margin-bottom:0"><label>${t("sec.confirmKey")}</label><input id="set-confirm_key" type="password" class="mono" placeholder="••••••••"></div>
      </div>
      <button class="btn btn-ghost btn-sm" id="aj-change-key" type="button">${t("sec.updateKey")}</button>
    </div>
  </div>`;
}

function cardLocal() {
  return `<div class="card" id="card-local">
    <div class="card-head"><div class="card-title">${ico.shield} ${t("local.title")}</div></div>
    ${toggle("local_first", t("local.store"), t("local.storeDesc"))}
    ${toggle("sync_zotero", t("local.zotero"), t("local.zoteroDesc"))}
    ${toggle("cache_pdfs", t("local.cache"), t("local.cacheDesc"))}
  </div>`;
}

function cardDanger() {
  return `<div class="card" id="card-danger" style="border-color:var(--st-unread)">
    <div class="card-head"><div class="card-title">${ico.shield} ${t("danger.title")}</div></div>
    <p class="aj-danger-desc">${t("danger.desc")}</p>
    <button class="btn btn-danger" id="aj-reset">${t("danger.reset")}</button>
  </div>`;
}

/* ---------- IA cards ---------- */

function cardIa() {
  return `<div class="card" id="card-ia">
    <div class="card-head"><div class="card-title">${ico.brain} ${t("prov.title")}</div></div>
    <div class="field-row">
      <div class="field"><label>${t("prov.provider")}</label><select id="set-provider">
        ${["openai-compatible|OpenAI-compatible","openai|OpenAI","anthropic|Anthropic","google|Google (Gemini)","openrouter|OpenRouter","local|Local (Ollama)"]
          .map(o=>{const[v,l]=o.split("|");return `<option value="${v}" ${settings.provider===v?"selected":""}>${l}</option>`}).join("")}
      </select></div>
      <div class="field"><label>${t("prov.model")}</label><input id="set-model" type="text" class="mono" value="${esc(settings.model)}" placeholder="gpt-4o-mini"></div>
    </div>
    <div class="field"><label>${t("prov.baseUrl")}</label><input id="set-base_url" type="text" class="mono" value="${esc(settings.base_url)}" placeholder="https://api.openai.com/v1"></div>
    <div class="field" style="margin-bottom:0"><label>${t("prov.apiKey")}</label><input id="set-api_key" type="password" class="mono" value="${esc(settings.api_key)}" placeholder="sk-…"></div>
    <p class="aj-note">${ico.lock} ${t("prov.note")}</p>
  </div>`;
}

function cardMcp() {
  const rows = servers.length
    ? servers.map(mcpRow).join("")
    : `<div class="empty-row">${t("mcp.empty")}</div>`;
  return `<div class="card" id="card-mcp">
    <div class="card-head">
      <div class="card-title">${ico.server} ${t("mcp.title")}</div>
      <button class="btn btn-ghost btn-sm" id="aj-mcp-add">${ico.plus} ${t("mcp.manage")}</button>
    </div>
    ${rows}
  </div>`;
}

function mcpRow(s: McpServer) {
  const on = !!s.connected;
  const tags = (s.tags || "").split(",").map(tg => tg.trim()).filter(Boolean);
  const meta = s.transport === "http" ? (s.url || "—") : [s.command, s.args].filter(Boolean).join(" ");
  return `<div class="mcp-row">
    <div class="mcp-icon">${ico.server}</div>
    <div class="mcp-info"><div class="mcp-name">${esc(s.name)}</div><div class="mcp-meta mono">${esc(meta || "—")}</div></div>
    <div class="mcp-tags">${tags.map(tg => `<span class="chip">${esc(tg)}</span>`).join("")}</div>
    <span class="status-badge ${on ? "is-active" : "is-off"}"><span class="dot"></span>${on ? t("mcp.connected") : t("mcp.disconnected")}</span>
  </div>`;
}

function cardAgent() {
  return `<div class="card" id="card-agent">
    <div class="card-head"><div class="card-title">${ico.layers} ${t("agent.title")}</div></div>
    <div class="agent-box" style="margin-bottom:16px">
      <span class="ai">${ico.spark}</span>
      <div>
        <div class="agent-name">${t("agent.cli")}</div>
        <div class="agent-path mono">${esc(settings.agent_path || "—")}</div>
      </div>
    </div>
    <div class="field" style="margin-bottom:0">
      <label>${t("agent.path")}</label>
      <input id="set-agent_path" type="text" class="mono" value="${esc(settings.agent_path)}" placeholder="~/.local/bin/claude">
    </div>
  </div>`;
}

/* ---------- App card ---------- */

function cardApp() {
  return `<div class="card" id="card-app">
    <div class="card-head"><div class="card-title">${ico.bookLogo} ${t("app.title")}</div></div>
    <div class="app-info-grid">
      <div class="app-info-row">
        <span class="app-info-label">${t("app.version")}</span>
        <span class="app-info-value mono">v${APP_VERSION}</span>
      </div>
      <div class="app-info-row">
        <span class="app-info-label">${t("app.github")}</span>
        <a class="app-info-link" href="${GITHUB_URL}" target="_blank" rel="noopener noreferrer">${ico.link} ${GITHUB_URL.replace("https://", "")}</a>
      </div>
    </div>
    <p class="aj-note">${t("app.githubDesc")}</p>
  </div>`;
}

/* ---------- wiring ---------- */

function wireSidebar() {
  $$(".settings-nav-item[data-section]").forEach((b) =>
    b.addEventListener("click", () => {
      activeSection = b.dataset.section!;
      $$(".settings-nav-item[data-section]").forEach((x) => x.classList.toggle("is-active", x === b));
      $$(".aj-section").forEach((s) => {
        const match = s.dataset.section === activeSection;
        s.classList.toggle("is-active", match);
        s.hidden = !match;
      });
      $("#aj-main")!.scrollTop = 0;
    }));
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
  const sel = $("#set-lang") as HTMLSelectElement | null;
  if (!sel) return;
  sel.addEventListener("change", async () => {
    const code = sel.value;
    settings.lang = code;
    state.settings.lang = code;
    setLang(code);
    try { await api.updateSetting("lang", code); } catch {}
    // Re-render so every string adopts the new language; the open section is
    // preserved (activeSection) and listeners rebind fresh.
    if (ajView) await renderAjustes(ajView);
  });
}

function wireSecurity() {
  const sel = $("#set-lock_policy") as HTMLSelectElement | null;
  if (!sel) return;
  sel.addEventListener("change", () => {
    const idle = $("#sec-idle-wrap")!;
    idle.classList.toggle("is-hidden", sel.value !== "idle");
  });
}

async function changeKey() {
  const btn = $("#aj-change-key") as HTMLButtonElement;
  const k1 = val("set-new_key");
  const k2 = val("set-confirm_key");
  if (!k1) { toast(t("sec.keyEnter")); return; }
  if (k1.length < 4) { toast(t("sec.keyShort")); return; }
  if (k1 !== k2) { toast(t("sec.keyMismatch")); return; }
  const prev = btn.textContent;
  btn.disabled = true;
  btn.textContent = t("sec.updating");
  try {
    await api.setLockKey(k1);
    ($("#set-new_key") as HTMLInputElement).value = "";
    ($("#set-confirm_key") as HTMLInputElement).value = "";
    toast(t("sec.keyUpdated"));
  } catch (e) {
    toast(t("sec.keyError") + e);
  } finally {
    btn.disabled = false;
    btn.textContent = prev;
  }
}

const RESET_WAIT = 10; // seconds the confirm button stays disabled

/** Open a centered confirmation modal. The "erase" button is disabled for
 *  RESET_WAIT seconds, showing a live countdown, before the user can confirm. */
function confirmReset() {
  // Guard against a second modal if one is already open.
  if ($("#aj-reset-overlay")) return;

  const overlay = el(`<div class="modal-overlay aj-reset-overlay" id="aj-reset-overlay">
    <div class="modal aj-reset-modal" role="dialog" aria-modal="true" aria-labelledby="aj-reset-title">
      <div class="modal-head">
        <div class="aj-reset-ic">${ico.shield}</div>
        <div>
          <div class="modal-title" id="aj-reset-title">${t("danger.modalTitle")}</div>
        </div>
      </div>
      <div class="modal-body">
        <p class="aj-reset-warn">${t("danger.modalWarn")}</p>
        <p class="aj-reset-irrev">${t("danger.modalIrreversible")}</p>
      </div>
      <div class="modal-foot">
        <button class="btn btn-ghost" id="aj-reset-cancel" type="button">${t("danger.modalCancel")}</button>
        <button class="btn btn-danger aj-reset-confirm" id="aj-reset-confirm" type="button" disabled>
          <span class="aj-reset-label">${t("danger.modalConfirm")}</span>
          <span class="aj-reset-wait">${t("danger.modalWait", { s: RESET_WAIT })}</span>
        </button>
      </div>
    </div>
  </div>`);

  document.body.appendChild(overlay);

  const cancel = () => closeResetModal();
  $("#aj-reset-cancel")!.addEventListener("click", cancel);
  overlay.addEventListener("click", (e) => { if (e.target === overlay) closeResetModal(); });

  // Countdown: enable the confirm button after RESET_WAIT seconds.
  const confirmBtn = $("#aj-reset-confirm") as HTMLButtonElement;
  const waitLabel = $(".aj-reset-wait", confirmBtn)!;
  let remaining = RESET_WAIT;
  const tick = window.setInterval(() => {
    remaining -= 1;
    if (remaining <= 0) {
      window.clearInterval(tick);
      confirmBtn.disabled = false;
      confirmBtn.classList.add("is-ready");
      waitLabel.textContent = "";
    } else {
      waitLabel.textContent = t("danger.modalWait", { s: remaining });
    }
  }, 1000);
  // Stash the timer so closing the modal cleans it up.
  (overlay as any)._tick = tick;

  confirmBtn.addEventListener("click", () => {
    if (confirmBtn.disabled) return;
    doReset(confirmBtn);
  });
}

function closeResetModal() {
  const overlay = $("#aj-reset-overlay");
  if (!overlay) return;
  const tick = (overlay as any)._tick as number | undefined;
  if (tick) window.clearInterval(tick);
  overlay.remove();
}

async function doReset(btn: HTMLButtonElement) {
  const label = $(".aj-reset-label", btn)!;
  btn.disabled = true;
  btn.classList.remove("is-ready");
  label.textContent = t("danger.restarting");
  try {
    await api.resetDatabase();
    closeResetModal();
    toast(t("danger.recreatedToast"));
    await api.appLog("reset: database recreated by user");
    setTimeout(() => location.reload(), 900);
  } catch (e) {
    btn.disabled = false;
    label.textContent = t("danger.modalConfirm");
    toast(t("danger.resetError") + e);
  }
}

const SAVE_DELAY = 1100; // ms — artificial "Guardando…" feedback

async function save() {
  const btn = $("#aj-save") as HTMLButtonElement;
  const fields = ["provider", "model", "base_url", "agent_path", "user_name", "lock_policy"];
  // The API key lives in the OS keychain: a non-empty input stores it there
  // (account = provider); an empty input leaves the stored credential as-is.
  const newKey = val("set-api_key").trim();
  const original = btn.innerHTML;
  btn.disabled = true;
  btn.classList.add("is-saving");
  btn.innerHTML = `<span class="btn-spin"></span> ${t("ajustes.saving")}`;
  try {
    for (const k of fields) settings[k] = val("set-" + k);
    settings.lock_idle_min = settings.lock_policy === "idle" ? val("set-lock_idle_min") : "";
    const write = async () => {
      for (const [k, v] of Object.entries(settings)) await api.updateSetting(k, v);
      if (newKey) await api.setProviderKey(newKey);
      state.settings = { ...settings };
    };
    await Promise.all([write(), delay(SAVE_DELAY)]);
    btn.classList.remove("is-saving");
    btn.classList.add("is-saved");
    btn.innerHTML = `${ico.check} ${t("ajustes.saved")}`;
    toast(t("ajustes.savedToast"));
    window.setTimeout(() => {
      btn.disabled = false;
      btn.classList.remove("is-saved");
      btn.innerHTML = original;
    }, 1400);
  } catch (e) {
    btn.disabled = false;
    btn.classList.remove("is-saving");
    btn.innerHTML = original;
    toast(t("ajustes.saveError") + e);
  }
}

function delay(ms: number) { return new Promise<void>((r) => window.setTimeout(r, ms)); }
function val(id: string) { return ($("#" + id) as HTMLInputElement | HTMLSelectElement).value.trim(); }
function $(s: string, r: ParentNode = document) { return r.querySelector<HTMLElement>(s); }
function $$(s: string, r: ParentNode = document) { return Array.from(r.querySelectorAll<HTMLElement>(s)); }
