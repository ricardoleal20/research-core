import { state, el, esc, toast } from "../main";
import { api } from "../api";
import { ico } from "../icons";
import { BG, WIN_CONTROLS } from "./welcome";

/**
 * Post-login setup wizard. Four steps:
 *   1. Data folder confirmation (hidden system folder + logs)
 *   2. First project (name + native folder picker)
 *   3. Lock policy (never / on_launch / idle / sensitive) — hashes the key
 *   4. LLM integration (CLI first / OpenRouter / simulated)
 * Then `onDone` → tutorial → shell.
 *
 * `key` is the local access key captured at login; Step 3 hashes + persists it.
 *
 * Markup mirrors OpenDesign frames 10–13: `.setup-body` ambient stage →
 * `.setup-composer` → `.setup-card` with `.step-dots` progress, left-aligned
 * `.setup-logo`/title/subtitle, step content and `.setup-actions`.
 */
export function renderWizard(app: HTMLElement, key: string, onDone: () => void) {
  const total = 4;
  let step = 1;
  const wiz = el(`<div class="app-window">${WIN_CONTROLS}
      <div class="setup-body">${BG}<div class="setup-composer"><div class="setup-card" id="wiz-card"></div></div></div></div>`);
  app.innerHTML = "";
  app.appendChild(wiz);
  const card = $("#wiz-card", wiz)!;

  const render = () => {
    card.innerHTML = `<div class="step-dots" aria-label="Paso ${step} de ${total}">${Array.from({ length: total }, (_, i) =>
      `<span class="step-dot ${i + 1 < step ? "is-done" : ""} ${i + 1 === step ? "is-active" : ""}"></span>`
    ).join("")}</div><div id="wiz-step"></div>`;
    const host = $("#wiz-step", card)!;
    if (step === 1) renderStep1(host);
    else if (step === 2) renderStep2(host);
    else if (step === 3) renderStep3(host);
    else renderStep4(host);
  };

  // ---------- Step 1: data folder ----------
  function renderStep1(host: HTMLElement) {
    host.innerHTML = `<div class="setup-logo">${ico.folder}</div>
      <h2 class="setup-title">Carpeta de la app</h2>
      <p class="setup-subtitle">Research Core guarda su base de datos y registros en una carpeta oculta del sistema. Tu información nunca sale de tu equipo.</p>
      <div id="wiz-paths"><div class="path-row"><span class="pr-value">Resolviendo rutas…</span></div></div>
      <div class="setup-actions">
        <button class="btn btn-ghost" id="wiz-reveal" type="button">${ico.folder}<span>Mostrar en Finder</span></button>
        <button class="btn btn-primary" id="wiz-next" type="button">Continuar</button>
      </div>
      <div class="setup-foot">${ico.shield}<span>Tus datos nunca salen de tu equipo</span></div>`;
    // Resolve real paths from the backend.
    api.getAppPaths().then((p) => {
      $("#wiz-paths", host)!.innerHTML = `
        <div class="path-row"><span class="pr-label">${ico.folder}Datos</span><span class="pr-value">${esc(p.data_dir)}</span></div>
        <div class="path-row"><span class="pr-label">${ico.doc}Registros</span><span class="pr-value">${esc(p.log_dir)}</span></div>`;
      ($("#wiz-reveal", host) as HTMLButtonElement).addEventListener("click", () => {
        api.revealPath(p.log_dir).catch(() => toast("No se pudo abrir la carpeta"));
      });
    }).catch(() => { $("#wiz-paths", host)!.innerHTML = `<div class="path-row"><span class="pr-value">No se pudo resolver la ruta.</span></div>`; });
    $("#wiz-next", host)!.addEventListener("click", () => { step++; render(); });
  }

  // ---------- Step 2: first project ----------
  function renderStep2(host: HTMLElement) {
    host.innerHTML = `<div class="setup-logo">${ico.book}</div>
      <h2 class="setup-title">Crea tu primer proyecto</h2>
      <p class="setup-subtitle">Donde vivirán tus referencias, revisiones y notas. Elige un nombre y una carpeta para guardarlo.</p>
      <div class="setup-field"><label for="wiz-proj-name">Nombre del proyecto</label>
        <input id="wiz-proj-name" type="text" placeholder="Ej. Tesis — Capítulo 2" autocomplete="off"/></div>
      <div class="setup-field"><label>Carpeta del proyecto</label>
        <button class="picker-btn" id="wiz-proj-folder" type="button">
          <span class="pb-icon">${ico.folder}</span>
          <span class="pb-value" id="wiz-folder-val">Selecciona una carpeta…</span>
          ${ico.chevron}</button></div>
      <div id="wiz-proj-error"></div>
      <div class="setup-actions">
        <button class="btn btn-ghost" id="wiz-back" type="button">Atrás</button>
        <button class="btn btn-primary" id="wiz-create" type="button">Crear y continuar</button>
      </div>`;
    let folder = "";
    ($("#wiz-proj-folder", host) as HTMLButtonElement).addEventListener("click", async () => {
      const picked = await api.pickFolder();
      if (picked) { folder = picked; $("#wiz-folder-val", host)!.textContent = picked; }
    });
    $("#wiz-back", host)!.addEventListener("click", () => { step--; render(); });
    $("#wiz-create", host)!.addEventListener("click", async () => {
      const name = ($("#wiz-proj-name", host) as HTMLInputElement).value.trim();
      if (!name) { $("#wiz-proj-error", host)!.innerHTML = `<div class="setup-error">Ponle un nombre al proyecto.</div>`; return; }
      if (!folder) { $("#wiz-proj-error", host)!.innerHTML = `<div class="setup-error">Selecciona una carpeta.</div>`; return; }
      try {
        const p = await api.createProject({ name, folder, kind: "paper", tags: "", color: "#3B5BDB" });
        await api.updateSetting("active_project", p.id);
        state.active = p;
        toast("Proyecto creado");
        step++; render();
      } catch (e) {
        $("#wiz-proj-error", host)!.innerHTML = `<div class="setup-error">No se pudo crear: ${esc(String(e))}</div>`;
      }
    });
  }

  // ---------- Step 3: lock policy ----------
  function renderStep3(host: HTMLElement) {
    const policies = [
      { id: "never", title: "Nunca", desc: "La app permanece desbloqueada tras iniciar sesión." },
      { id: "on_launch", title: "Al abrir la app", desc: "Pide la clave cada vez que abres Research Core." },
      { id: "idle", title: "Tras X minutos", desc: "Bloquea tras un tiempo de inactividad." },
      { id: "sensitive", title: "Antes de acciones sensibles", desc: "Pide la clave al borrar proyectos, refs o vaciar acciones." },
    ];
    host.innerHTML = `<div class="setup-logo">${ico.shield}</div>
      <h2 class="setup-title">Seguridad de acceso</h2>
      <p class="setup-subtitle">¿Cuándo debe Research Core pedir tu clave de acceso local?</p>
      <div class="opt-list" id="wiz-lock-opts">
        ${policies.map((p) => `<button class="opt-card" data-id="${p.id}" type="button">
          <div class="opt-card-body"><div class="opt-card-title">${p.title}</div><div class="opt-card-desc">${p.desc}</div></div>
          <span class="opt-radio"></span></button>`).join("")}
      </div>
      <div class="opt-sub" id="wiz-idle-field" hidden><label class="opt-sub-label" for="wiz-idle-min">Minutos de inactividad</label>
        <input id="wiz-idle-min" type="number" min="1" max="240" value="15"/></div>
      <div class="setup-actions">
        <button class="btn btn-ghost" id="wiz-back" type="button">Atrás</button>
        <button class="btn btn-primary" id="wiz-next" type="button">Continuar</button>
      </div>`;
    let chosen = "never";
    const opts = $$(".opt-card", host);
    opts.forEach((o) => o.addEventListener("click", () => {
      chosen = o.dataset.id!;
      opts.forEach((x) => x.classList.toggle("is-active", x === o));
      $("#wiz-idle-field", host)!.hidden = chosen !== "idle";
    }));
    // default-select "never"
    opts[0]?.classList.add("is-active");
    $("#wiz-back", host)!.addEventListener("click", () => { step--; render(); });
    $("#wiz-next", host)!.addEventListener("click", async () => {
      try {
        const idleMin = ($("#wiz-idle-min", host) as HTMLInputElement)?.value || "15";
        await api.updateSetting("lock_policy", chosen);
        await api.updateSetting("lock_idle_min", chosen === "idle" ? idleMin : "");
        // Store the hashed key (backend hashes; we pass the raw key once).
        await api.setLockKey(key);
        toast("Seguridad configurada");
        step++; render();
      } catch (e) {
        toast("No se pudo guardar: " + e);
      }
    });
  }

  // ---------- Step 4: LLM ----------
  function renderStep4(host: HTMLElement) {
    host.innerHTML = `<div class="setup-logo">${ico.brain}</div>
      <h2 class="setup-title">Conecta tu LLM</h2>
      <p class="setup-subtitle">¿Cómo generará Research Core las revisiones y respuestas?</p>
      <div class="opt-list" id="wiz-llm-opts">
        <button class="opt-card" data-id="cli" type="button"><div class="opt-card-body"><div class="opt-card-title">CLI local</div>
          <div class="opt-card-desc">Claude Code, Codex u OpenCode. Local-first.</div></div><span class="opt-radio"></span></button>
        <button class="opt-card" data-id="provider" type="button"><div class="opt-card-body"><div class="opt-card-title">Proveedor externo</div>
          <div class="opt-card-desc">OpenRouter u otro compatible con OpenAI.</div></div><span class="opt-radio"></span></button>
        <button class="opt-card" data-id="simulate" type="button"><div class="opt-card-body"><div class="opt-card-title">Simulado</div>
          <div class="opt-card-desc">Sin LLM por ahora — respuestas de prueba.</div></div><span class="opt-radio"></span></button>
      </div>
      <div id="wiz-llm-config"></div>
      <div class="setup-actions">
        <button class="btn btn-ghost" id="wiz-back" type="button">Atrás</button>
        <button class="btn btn-primary" id="wiz-finish" type="button">Finalizar</button>
      </div>`;
    let mode = "cli";
    const opts = $$(".opt-card", host);
    opts.forEach((o) => o.addEventListener("click", () => {
      mode = o.dataset.id!;
      opts.forEach((x) => x.classList.toggle("is-active", x === o));
      renderLlmConfig($("#wiz-llm-config", host)!, mode);
    }));
    opts[0]?.classList.add("is-active");
    renderLlmConfig($("#wiz-llm-config", host)!, "cli");
    $("#wiz-back", host)!.addEventListener("click", () => { step--; render(); });
    $("#wiz-finish", host)!.addEventListener("click", async () => {
      try {
        await saveLlmConfig(host, mode);
        await api.updateSetting("onboarding_complete", "true");
        await api.updateSetting("tutorial_seen", ""); // ensure tutorial shows
        state.settings.onboarding_complete = "true";
        await api.appLog("wizard: complete");
        state.pendingKey = "";
        onDone();
      } catch (e) {
        toast("No se pudo finalizar: " + e);
      }
    });
  }

  function renderLlmConfig(host: HTMLElement, mode: string) {
    if (mode === "cli") {
      host.innerHTML = `<div class="opt-sub">
        <div class="cli-grid" id="wiz-cli-grid">
          ${["claude", "codex", "opencode"].map((c) => `<button class="cli-pick" data-cmd="${c}" type="button"><span class="cli-pick-name">${c}</span><span class="cli-pick-path" id="wiz-cli-${c}">detectando…</span></button>`).join("")}
        </div>
        <div class="setup-field" style="margin-bottom:0"><label for="wiz-cli-model">Modelo (opcional)</label>
          <input id="wiz-cli-model" type="text" placeholder="Ej. sonnet (deja vacío para el default del CLI)"/></div></div>`;
      // Probe each CLI.
      ["claude", "codex", "opencode"].forEach(async (c) => {
        const cell = $(`#wiz-cli-${c}`, host);
        try {
          const res = await api.testCli(c);
          if (cell) { cell.textContent = res.path ? res.path : "no encontrado"; cell.classList.toggle("is-found", !!res.path); }
        } catch { if (cell) cell.textContent = "no encontrado"; }
      });
      $$(".cli-pick", host).forEach((b) => b.addEventListener("click", () => {
        $$(".cli-pick", host).forEach((x) => x.classList.toggle("is-active", x === b));
      }));
      $$(".cli-pick", host)[0]?.classList.add("is-active");
    } else if (mode === "provider") {
      host.innerHTML = `<div class="opt-sub">
        <div class="setup-field"><label for="wiz-prov-key">API Key de OpenRouter</label>
          <input id="wiz-prov-key" type="password" placeholder="sk-or-…"/></div>
        <div class="setup-field" style="margin-bottom:0"><label for="wiz-prov-model">Modelo</label>
          <input id="wiz-prov-model" type="text" placeholder="Ej. anthropic/claude-3.5-sonnet" value="anthropic/claude-3.5-sonnet"/></div></div>`;
    } else {
      host.innerHTML = `<div class="setup-foot">${ico.shield}<span>Usarás el modo simulado. Las revisiones y respuestas serán de prueba. Puedes conectar un LLM real en Ajustes cuando quieras.</span></div>`;
    }
    host.dataset.mode = mode;
  }

  async function saveLlmConfig(host: HTMLElement, mode: string): Promise<void> {
    if (mode === "cli") {
      const cmd = $$(".cli-pick", host).find((b) => b.classList.contains("is-active"))?.dataset.cmd || "claude";
      const model = ($("#wiz-cli-model", host) as HTMLInputElement)?.value.trim() || "";
      await api.updateSetting("llm_mode", "cli");
      await api.updateSetting("llm_cli", cmd);
      await api.updateSetting("llm_cli_model", model);
      await api.updateSetting("provider", "");
      await api.updateSetting("api_key", "");
    } else if (mode === "provider") {
      const key = ($("#wiz-prov-key", host) as HTMLInputElement)?.value.trim() || "";
      const model = ($("#wiz-prov-model", host) as HTMLInputElement)?.value.trim() || "";
      if (!key) throw new Error("Falta la API key");
      await api.updateSetting("llm_mode", "provider");
      await api.updateSetting("base_url", "https://openrouter.ai/api/v1");
      await api.updateSetting("api_key", key);
      await api.updateSetting("model", model);
    } else {
      await api.updateSetting("llm_mode", "simulate");
      await api.updateSetting("api_key", "");
      await api.updateSetting("base_url", "");
    }
  }

  render();
}

function $(s: string, r: ParentNode = document) { return r.querySelector<HTMLElement>(s); }
function $$(s: string, r: ParentNode = document) { return Array.from(r.querySelectorAll<HTMLElement>(s)); }
