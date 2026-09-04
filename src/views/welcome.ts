import { state, el, esc, toast } from "../main";
import { api } from "../api";
import { ico } from "../icons";
import { runMcpInit } from "./mcpinit";

// Shared animated background markup (orbs + grid + floating research shapes).
// Reused by the splash, setup wizard, lock screen and tutorial so every
// pre-shell surface shares the same ambient identity.
export const BG = `<div class="welcome-bg" aria-hidden="true">
  <div class="welcome-grid"></div>
  <div class="welcome-orb o1"></div><div class="welcome-orb o2"></div><div class="welcome-orb o3"></div>
  <svg class="welcome-poly p1" width="46" height="40" viewBox="0 0 46 40" fill="none"><path d="M23 8 Q14 4 6 6 L6 32 Q14 30 23 34 Q32 30 40 32 L40 6 Q32 4 23 8 Z" stroke="var(--accent)" stroke-width="1.5" opacity="0.22" fill="none"/><path d="M23 8 L23 34" stroke="var(--accent)" stroke-width="1.5" opacity="0.18"/></svg>
  <svg class="welcome-poly p2" width="40" height="44" viewBox="0 0 40 44" fill="none"><path d="M8 4 L26 4 L34 12 L34 40 L8 40 Z" stroke="var(--dot-4)" stroke-width="1.5" opacity="0.20" fill="none"/><path d="M26 4 L26 12 L34 12" stroke="var(--dot-4)" stroke-width="1.5" opacity="0.20" fill="none"/><path d="M13 20 L29 20 M13 25 L29 25 M13 30 L23 30" stroke="var(--dot-4)" stroke-width="1.2" opacity="0.15" stroke-linecap="round"/></svg>
  <svg class="welcome-poly p3" width="48" height="44" viewBox="0 0 48 44" fill="none"><circle cx="24" cy="10" r="4" stroke="var(--accent)" stroke-width="1.5" opacity="0.22" fill="none"/><circle cx="10" cy="28" r="4" stroke="var(--accent)" stroke-width="1.5" opacity="0.22" fill="none"/><circle cx="38" cy="28" r="4" stroke="var(--accent)" stroke-width="1.5" opacity="0.22" fill="none"/><circle cx="24" cy="38" r="4" stroke="var(--accent)" stroke-width="1.5" opacity="0.22" fill="none"/><path d="M24 14 L11 25 M24 14 L37 25 M13 31 L22 36 M35 31 L26 36" stroke="var(--accent)" stroke-width="1.2" opacity="0.18"/></svg>
  <svg class="welcome-poly p4" width="36" height="34" viewBox="0 0 36 34" fill="none"><path d="M6 24 Q6 14 14 8 L14 13 Q10 16 10 22 L14 22 L14 28 L6 28 Z" stroke="var(--dot-2)" stroke-width="1.5" opacity="0.20" fill="none"/><path d="M20 24 Q20 14 28 8 L28 13 Q24 16 24 22 L28 22 L28 28 L20 28 Z" stroke="var(--dot-2)" stroke-width="1.5" opacity="0.20" fill="none"/></svg>
  <svg class="welcome-poly p5" width="30" height="40" viewBox="0 0 30 40" fill="none"><path d="M6 4 L24 4 L24 34 L15 28 L6 34 Z" stroke="var(--accent)" stroke-width="1.5" opacity="0.20" fill="none"/></svg>
  <svg class="welcome-poly p6" width="38" height="38" viewBox="0 0 38 38" fill="none"><circle cx="15" cy="15" r="9" stroke="var(--dot-4)" stroke-width="1.5" opacity="0.20" fill="none"/><path d="M22 22 L32 32" stroke="var(--dot-4)" stroke-width="2" opacity="0.20" stroke-linecap="round"/></svg>
  <svg class="welcome-poly p7" width="38" height="38" viewBox="0 0 38 38" fill="none"><path d="M4 34 L10 28 L28 10 L32 14 L14 32 L8 34 Z" stroke="var(--accent)" stroke-width="1.5" opacity="0.18" fill="none" stroke-linejoin="round"/><path d="M26 12 L30 16" stroke="var(--accent)" stroke-width="1.5" opacity="0.18"/></svg>
  <svg class="welcome-poly p8" width="38" height="34" viewBox="0 0 38 34" fill="none"><rect x="4" y="11" width="14" height="12" rx="6" stroke="var(--dot-2)" stroke-width="1.5" opacity="0.18" fill="none"/><rect x="20" y="11" width="14" height="12" rx="6" stroke="var(--dot-2)" stroke-width="1.5" opacity="0.18" fill="none"/><path d="M18 17 L20 17" stroke="var(--dot-2)" stroke-width="1.5" opacity="0.18"/></svg>
</div>`;

// Native window decorations are disabled (decorations:false in tauri.conf),
// so the faux macOS traffic lights are no longer rendered. Kept as an empty
// export so legacy `${WIN_CONTROLS}` template sites stay valid.
export const WIN_CONTROLS = "";

const LANGS = [
  { flag: "🇪🇸", name: "Español", native: "Español", code: "es" },
  { flag: "🇬🇧", name: "Inglés", native: "English", code: "en" },
  { flag: "🇨🇳", name: "Chino", native: "中文", code: "zh" },
  { flag: "🇧🇷", name: "Portugués", native: "Português", code: "pt" },
];

/**
 * Render the splash hero. Instead of a start button, the splash shows the MCP
 * init visualization ("how Research Core starts"); when it finishes (or is
 * skipped) it auto-advances via `onNext`.
 */
export function renderSplash(app: HTMLElement, onNext: () => void) {
  app.innerHTML = `<div class="app-window">${WIN_CONTROLS}
    <div class="splash-body">
      ${BG}
      <div class="splash-hero">
        <div class="splash-logo">${ico.bookLogo}</div>
        <h1 class="splash-title">Research Core</h1>
        <p class="splash-subtitle">Tu gestor de investigación local-first. Papers, referencias, revisiones y síntesis — todo en un solo lugar, siempre en tu equipo.</p>
        <div class="splash-init" id="splash-init"></div>
        <div class="splash-foot">${ico.shield}<span>Tus datos nunca salen de tu equipo</span></div>
      </div>
      ${langBarHtml()}
    </div>
  </div>`;
  wireLangBar(app);
  runMcpInit($("#splash-init", app)!, onNext);
}

/** Render the local-first login card. */
export function renderLogin(app: HTMLElement, onEnter: (name: string, key: string) => void) {
  const last = state.settings.user_name || "";
  app.innerHTML = `<div class="app-window">${WIN_CONTROLS}
    <div class="welcome-body">
      ${BG}
      <div class="welcome-composer">
        <div class="welcome-card">
          <div class="welcome-logo">${ico.bookLogo}</div>
          <h1 class="welcome-title">Research Core</h1>
          <p class="welcome-subtitle">Tu gestor de investigación local-first. Papers, referencias y revisiones en un solo lugar.</p>
          <div class="welcome-field"><label for="login-name">Nombre</label><input id="login-name" type="text" value="${esc(last)}" placeholder="Tu nombre" autocomplete="off"/></div>
          <div class="welcome-field"><label for="login-key">Clave de acceso local</label><input id="login-key" type="password" placeholder="••••••••" autocomplete="off"/></div>
          <div id="login-error"></div>
          <button class="btn btn-primary welcome-btn" id="login-enter">Entrar</button>
          <div class="welcome-foot">${ico.shield}<span>100% local · tus datos nunca salen de tu equipo</span></div>
        </div>
      </div>
      ${langBarHtml()}
    </div>
  </div>`;
  wireLangBar(app);
  const nameEl = $("#login-name", app) as HTMLInputElement;
  const keyEl = $("#login-key", app) as HTMLInputElement;
  nameEl.focus();
  const submit = () => {
    const name = nameEl.value.trim();
    if (!name) { $("#login-error", app)!.innerHTML = `<div class="welcome-error">Escribe tu nombre para continuar.</div>`; nameEl.focus(); return; }
    onEnter(name, keyEl.value);
  };
  $("#login-enter", app)!.addEventListener("click", submit);
  keyEl.addEventListener("keydown", (e) => { if (e.key === "Enter") submit(); });
  nameEl.addEventListener("keydown", (e) => { if (e.key === "Enter") keyEl.focus(); });
}

function langBarHtml() {
  const cur = LANGS.find((l) => l.code === (state.settings.lang || "es")) ?? LANGS[0];
  return `<div class="lang-bar"><button class="lang-trigger" type="button" aria-haspopup="listbox" aria-expanded="false" id="lang-trigger">
    <span class="lang-trigger-flag">${cur.flag}</span><span class="lang-trigger-name">${cur.name}</span>${ico.chevron}
  </button></div>`;
}

function wireLangBar(app: HTMLElement) {
  const trigger = $("#lang-trigger", app);
  if (!trigger) return;
  trigger.addEventListener("click", () => openLangModal(app));
}

function openLangModal(app: HTMLElement) {
  if ($("#lang-modal-overlay", app)) return;
  const cur = state.settings.lang || "es";
  const overlay = el(`<div class="lang-modal-overlay" id="lang-modal-overlay">
    <div class="lang-modal" role="dialog" aria-label="Selecciona tu idioma">
      <div class="lang-modal-head"><div class="lang-modal-title">Idioma de la interfaz</div><div class="lang-modal-sub">Elige cómo se mostrará Research Core</div></div>
      <div class="lang-modal-list" role="listbox">
        ${LANGS.map((l) => `<button class="lang-modal-item ${l.code === cur ? "is-active" : ""}" type="button" role="option" data-code="${l.code}">
          <span class="lang-modal-item-flag">${l.flag}</span>
          <div class="lang-modal-item-name">${l.name}</div>
          <span class="lang-modal-item-native">${l.native}</span>
          <svg class="lang-modal-item-check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
        </button>`).join("")}
      </div>
    </div>
  </div>`);
  $(".splash-body, .welcome-body", app)!.appendChild(overlay);
  overlay.addEventListener("click", (e) => { if (e.target === overlay) overlay.remove(); });
  $$(".lang-modal-item", overlay).forEach((b) =>
    b.addEventListener("click", async () => {
      const code = b.dataset.code!;
      state.settings.lang = code;
      try { await api.updateSetting("lang", code); } catch {}
      overlay.remove();
      // refresh the trigger label
      const lang = LANGS.find((l) => l.code === code)!;
      const t = $("#lang-trigger", app)!;
      t.querySelector(".lang-trigger-flag")!.textContent = lang.flag;
      t.querySelector(".lang-trigger-name")!.textContent = lang.name;
      toast(`Idioma: ${lang.name}`);
    }));
}

function $(s: string, r: ParentNode = document) { return r.querySelector<HTMLElement>(s); }
function $$(s: string, r: ParentNode = document) { return Array.from(r.querySelectorAll<HTMLElement>(s)); }
