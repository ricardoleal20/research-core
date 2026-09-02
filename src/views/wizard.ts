import { state, el, esc, toast } from "../main";
import { api } from "../api";
import { ico } from "../icons";

/**
 * Post-login setup wizard. Four steps:
 *   1. Data folder confirmation (hidden system folder + logs)
 *   2. First project (name + native folder picker)
 *   3. Lock policy (never / on_launch / idle / sensitive) — hashes the key
 *   4. LLM integration (CLI first / OpenRouter / simulated)
 * Then `onDone` → tutorial → shell.
 *
 * `key` is the local access key captured at login; Step 3 hashes + persists it.
 */
export function renderWizard(app: HTMLElement, key: string, onDone: () => void) {
  const total = 4;
  let step = 1;
  const wiz = el(`<div class="app-window"><div class="welcome-win-controls" aria-hidden="true">
      <span class="wc" style="background:#FF5F57"></span><span class="wc" style="background:#FEBC2E"></span><span class="wc" style="background:#28C840"></span>
    </div><div class="wiz-body" id="wiz-body"></div></div>`);
  app.innerHTML = "";
  app.appendChild(wiz);
  const body = $("#wiz-body", wiz)!;

  const render = () => {
    body.innerHTML = `<div class="wiz-card">
      <div class="wiz-progress">${Array.from({ length: total }, (_, i) =>
        `<span class="wiz-dot ${i + 1 < step ? "is-done" : ""} ${i + 1 === step ? "is-active" : ""}"></span>`
      ).join("")}</div>
      <div id="wiz-step"></div>
    </div>`;
    const host = $("#wiz-step", body)!;
    if (step === 1) renderStep1(host);
    else if (step === 2) renderStep2(host);
    else if (step === 3) renderStep3(host);
    else renderStep4(host);
  };

  // ---------- Step 1: data folder ----------
  function renderStep1(host: HTMLElement) {
    host.innerHTML = `<div class="wiz-head">
        <div class="wiz-logo">${ico.layers}</div>
        <h1 class="wiz-title">Carpeta de la app</h1>
        <p class="wiz-sub">Research Core guarda su base de datos y registros en una carpeta oculta del sistema. Tu información nunca sale de tu equipo.</p>
      </div>
      <div class="wiz-field" id="wiz-paths"><div class="wiz-path-skel">Resolviendo rutas…</div></div>
      <div class="wiz-actions">
        <button class="btn btn-ghost wiz-reveal" id="wiz-reveal">${ico.folder}<span>Mostrar en Finder</span></button>
        <button class="btn btn-primary wiz-next" id="wiz-next">Continuar</button>
      </div>`;
    // Resolve real paths from the backend.
    api.getAppPaths().then((p) => {
      $("#wiz-paths", host)!.innerHTML = `
        <div class="wiz-path-row"><span class="wiz-path-label">Datos</span><code class="wiz-path-val">${esc(p.data_dir)}</code></div>
        <div class="wiz-path-row"><span class="wiz-path-label">Registros</span><code class="wiz-path-val">${esc(p.log_dir)}</code></div>`;
      ($("#wiz-reveal", host) as HTMLButtonElement).addEventListener("click", () => {
        api.revealPath(p.log_dir).catch(() => toast("No se pudo abrir la carpeta"));
      });
    }).catch(() => { $("#wiz-paths", host)!.innerHTML = `<div class="wiz-path-row">No se pudo resolver la ruta.</div>`; });
    $("#wiz-next", host)!.addEventListener("click", () => { step++; render(); });
  }

  // ---------- Step 2: first project ----------
  function renderStep2(host: HTMLElement) {
    host.innerHTML = `<div class="wiz-head">
        <div class="wiz-logo">${ico.book}</div>
        <h1 class="wiz-title">Crea tu primer proyecto</h1>
        <p class="wiz-sub">Donde vivirán tus referencias, revisiones y notas. Elige un nombre y una carpeta para guardarlo.</p>
      </div>
      <div class="wiz-field"><label for="wiz-proj-name">Nombre del proyecto</label>
        <input id="wiz-proj-name" type="text" placeholder="Ej. Tesis — Capítulo 2" autocomplete="off"/></div>
      <div class="wiz-field"><label>Carpeta del proyecto</label>
        <button class="wiz-folder-pick" id="wiz-proj-folder"><span id="wiz-folder-val">Selecciona una carpeta…</span>${ico.chevron}</button></div>
      <div id="wiz-proj-error"></div>
      <div class="wiz-actions">
        <button class="btn btn-ghost" id="wiz-back">Atrás</button>
        <button class="btn btn-primary" id="wiz-create">Crear y continuar</button>
      </div>`;
    let folder = "";
    ($("#wiz-proj-folder", host) as HTMLButtonElement).addEventListener("click", async () => {
      const picked = await api.pickFolder();
      if (picked) { folder = picked; $("#wiz-folder-val", host)!.textContent = picked; }
    });
    $("#wiz-back", host)!.addEventListener("click", () => { step--; render(); });
    $("#wiz-create", host)!.addEventListener("click", async () => {
      const name = ($("#wiz-proj-name", host) as HTMLInputElement).value.trim();
      if (!name) { $("#wiz-proj-error", host)!.innerHTML = `<div class="wiz-error">Ponle un nombre al proyecto.</div>`; return; }
      if (!folder) { $("#wiz-proj-error", host)!.innerHTML = `<div class="wiz-error">Selecciona una carpeta.</div>`; return; }
      try {
        const p = await api.createProject({ name, folder, kind: "paper", tags: "", color: "#3B5BDB" });
        await api.updateSetting("active_project", p.id);
        state.active = p;
        toast("Proyecto creado");
        step++; render();
      } catch (e) {
        $("#wiz-proj-error", host)!.innerHTML = `<div class="wiz-error">No se pudo crear: ${esc(String(e))}</div>`;
      }
    });
  }

  // ---------- Step 3: lock policy ----------
  function renderStep3(host: HTMLElement) {
    const policies = [
      { id: "never", title: "Nunca", desc: "La app permanece desbloqueada tras iniciar sesión. Máxima comodidad." },
      { id: "on_launch", title: "Al abrir la app", desc: "Pide la clave cada vez que abres Research Core." },
      { id: "idle", title: "Tras X minutos", desc: "Bloquea tras un tiempo de inactividad." },
      { id: "sensitive", title: "Antes de acciones sensibles", desc: "Pide la clave al borrar proyectos, refs o vaciar acciones." },
    ];
    host.innerHTML = `<div class="wiz-head">
        <div class="wiz-logo">${ico.shield}</div>
        <h1 class="wiz-title">Seguridad de acceso</h1>
        <p class="wiz-sub">¿Cuándo debe Research Core pedir tu clave de acceso local?</p>
      </div>
      <div class="wiz-options" id="wiz-lock-opts">
        ${policies.map((p) => `<button class="wiz-opt" data-id="${p.id}">
          <div class="wiz-opt-text"><div class="wiz-opt-title">${p.title}</div><div class="wiz-opt-desc">${p.desc}</div></div>
          <span class="wiz-opt-radio"></span></button>`).join("")}
      </div>
      <div class="wiz-field wiz-idle" id="wiz-idle-field" hidden><label for="wiz-idle-min">Minutos de inactividad</label>
        <input id="wiz-idle-min" type="number" min="1" max="240" value="15"/></div>
      <div class="wiz-actions">
        <button class="btn btn-ghost" id="wiz-back">Atrás</button>
        <button class="btn btn-primary" id="wiz-next">Continuar</button>
      </div>`;
    let chosen = "never";
    const opts = $$(".wiz-opt", host);
    opts.forEach((o) => o.addEventListener("click", () => {
      chosen = o.dataset.id!;
      opts.forEach((x) => x.classList.toggle("is-active", x === o));
      $("#wiz-idle-field", host)!.hidden = chosen !== "idle";
    }));
    // default-select "never"
    opts[0]?.classList.add("is-active");
    $("#wiz-back", host)!.addEventListener("click", () => { step--; render(); });
    $("#wiz-next", host)!.addEventListener("click", async () => {
      // Persist the lock policy. The key (from login) is hashed + salted in
      // the backend via verify_key's storage — but we must store the hash here.
      // We rely on a backend command to hash; reuse verify_key's storage by
      // calling a dedicated path. For now store policy + idle; hashing is done
      // by a backend helper exposed through a setting command.
      try {
        const idleMin = ($("#wiz-idle-min", host) as HTMLInputElement)?.value || "15";
        // Hash the key: send to backend via a one-shot command. We reuse
        // update_setting for policy/idle and a verify_key warm-up; the hash is
        // set by the backend when it first sees a key — but to keep this
        // explicit, store policy and idle here. The hash itself is written by
        // the wizard-finish step below via a dedicated call.
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
    host.innerHTML = `<div class="wiz-head">
        <div class="wiz-logo">${ico.brain}</div>
        <h1 class="wiz-title">Conecta tu LLM</h1>
        <p class="wiz-sub">¿Cómo generará Research Core las revisiones y respuestas? Puedes usar un CLI local o un proveedor externo.</p>
      </div>
      <div class="wiz-options" id="wiz-llm-opts">
        <button class="wiz-opt" data-id="cli"><div class="wiz-opt-text"><div class="wiz-opt-title">CLI local</div>
          <div class="wiz-opt-desc">Claude Code, Codex u OpenCode. Local-first.</div></div><span class="wiz-opt-radio"></span></button>
        <button class="wiz-opt" data-id="provider"><div class="wiz-opt-text"><div class="wiz-opt-title">Proveedor externo</div>
          <div class="wiz-opt-desc">OpenRouter u otro compatible con OpenAI.</div></div><span class="wiz-opt-radio"></span></button>
        <button class="wiz-opt" data-id="simulate"><div class="wiz-opt-text"><div class="wiz-opt-title">Simulado</div>
          <div class="wiz-opt-desc">Sin LLM por ahora — respuestas de prueba. Puedes cambiarlo luego en Ajustes.</div></div><span class="wiz-opt-radio"></span></button>
      </div>
      <div id="wiz-llm-config"></div>
      <div class="wiz-actions">
        <button class="btn btn-ghost" id="wiz-back">Atrás</button>
        <button class="btn btn-primary" id="wiz-finish">Finalizar</button>
      </div>`;
    let mode = "cli";
    const opts = $$(".wiz-opt", host);
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
      host.innerHTML = `<div class="wiz-field"><label>CLI a usar</label>
        <div class="wiz-cli-grid" id="wiz-cli-grid">
          ${["claude", "codex", "opencode"].map((c) => `<button class="wiz-cli-pick" data-cmd="${c}"><span class="wiz-cli-name">${c}</span><span class="wiz-cli-det" id="wiz-cli-${c}">detectando…</span></button>`).join("")}
        </div></div>
        <div class="wiz-field"><label for="wiz-cli-model">Modelo (opcional)</label>
          <input id="wiz-cli-model" type="text" placeholder="Ej. sonnet (deja vacío para el default del CLI)"/></div>`;
      // Probe each CLI.
      ["claude", "codex", "opencode"].forEach(async (c) => {
        const cell = $(`#wiz-cli-${c}`, host);
        try {
          const res = await api.testCli(c);
          if (cell) cell.textContent = res.path ? res.path : "no encontrado";
          if (cell) cell.classList.toggle("is-found", !!res.path);
        } catch { if (cell) cell.textContent = "no encontrado"; }
      });
      let chosenCmd = "claude";
      $$(".wiz-cli-pick", host).forEach((b) => b.addEventListener("click", () => {
        chosenCmd = b.dataset.cmd!;
        $$(".wiz-cli-pick", host).forEach((x) => x.classList.toggle("is-active", x === b));
      }));
      host.dataset.cmd = "claude";
    } else if (mode === "provider") {
      host.innerHTML = `<div class="wiz-field"><label for="wiz-prov-key">API Key de OpenRouter</label>
        <input id="wiz-prov-key" type="password" placeholder="sk-or-…"/></div>
        <div class="wiz-field"><label for="wiz-prov-model">Modelo</label>
          <input id="wiz-prov-model" type="text" placeholder="Ej. anthropic/claude-3.5-sonnet" value="anthropic/claude-3.5-sonnet"/></div>`;
    } else {
      host.innerHTML = `<div class="wiz-note">${ico.shield}<span>Usarás el modo simulado. Las revisiones y respuestas serán de prueba. Puedes conectar un LLM real en Ajustes cuando quieras.</span></div>`;
    }
    host.dataset.mode = mode;
  }

  async function saveLlmConfig(host: HTMLElement, mode: string): Promise<void> {
    if (mode === "cli") {
      const cmd = $$(".wiz-cli-pick", host).find((b) => b.classList.contains("is-active"))?.dataset.cmd || "claude";
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
