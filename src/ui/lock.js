// The lock screen (the one dark frame — WebGL fbm shader, glass card) and
// the onboarding wizard, both carried from prototype/index.html. The lock
// keeps the bible's demo behavior; the wizard's language step binds the
// real i18n layer and its AI-init step binds run_first_value (the core's
// sixty-second first value) when a URL is pasted.
import { t, setLang, getLang } from "../i18n";
import { mockActive } from "../mock-backend";
import { icon, esc, btn, card, demoChip } from "./helpers";

let _lockShaderRunning = false;
let _lockShaderRaf = 0;
let _lockShaderResize = null;

function startLockShader() {
  const VS = `
    attribute vec2 a_position;
    void main() { gl_Position = vec4(a_position, 0.0, 1.0); }
  `;
  const FS = `
    precision mediump float;
    uniform float u_time;
    uniform vec2 u_resolution;

    float hash(vec2 p) { return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453); }
    float noise(vec2 p) {
      vec2 i = floor(p);
      vec2 f = fract(p);
      float a = hash(i);
      float b = hash(i + vec2(1.0, 0.0));
      float c = hash(i + vec2(0.0, 1.0));
      float d = hash(i + vec2(1.0, 1.0));
      vec2 u = f * f * (3.0 - 2.0 * f);
      return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
    }
    float fbm(vec2 p) {
      float v = 0.0;
      float a = 0.5;
      for (int i = 0; i < 5; i++) {
        v += a * noise(p);
        p *= 2.0;
        a *= 0.5;
      }
      return v;
    }
    void main() {
      vec2 uv = gl_FragCoord.xy / u_resolution.xy;
      float t = u_time * 0.04;
      float n = fbm(uv * 2.5 + vec2(t * 0.3, t * 0.1));
      float m = fbm(uv * 3.0 - vec2(t * 0.2, t * 0.25) + n * 0.5);
      vec3 base = vec3(0.082, 0.082, 0.098);
      vec3 blue = vec3(0.19, 0.41, 0.66);
      vec3 teal = vec3(0.20, 0.49, 0.51);
      vec3 indigo = vec3(0.24, 0.32, 0.61);
      vec3 col = base;
      col = mix(col, indigo, smoothstep(0.3, 0.7, m) * 0.35);
      col = mix(col, blue, smoothstep(0.4, 0.8, n) * 0.30);
      col = mix(col, teal, smoothstep(0.35, 0.75, m * n) * 0.20);
      gl_FragColor = vec4(col, 1.0);
    }
  `;
  try {
    const canvas = document.getElementById("lock-shader");
    if (!canvas) return;
    const gl = canvas.getContext("webgl2") || canvas.getContext("webgl") || canvas.getContext("experimental-webgl");
    if (!gl) return;
    if (_lockShaderRaf) { cancelAnimationFrame(_lockShaderRaf); _lockShaderRaf = 0; }
    _lockShaderRunning = false;
    if (_lockShaderResize) { window.removeEventListener("resize", _lockShaderResize); _lockShaderResize = null; }

    function compile(type, src) {
      const s = gl.createShader(type);
      if (!s) return null;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) { gl.deleteShader(s); return null; }
      return s;
    }
    const vs = compile(gl.VERTEX_SHADER, VS);
    const fs = compile(gl.FRAGMENT_SHADER, FS);
    if (!vs || !fs) return;
    const prog = gl.createProgram();
    if (!prog) return;
    gl.attachShader(prog, vs);
    gl.attachShader(prog, fs);
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return;
    gl.useProgram(prog);

    const posLoc = gl.getAttribLocation(prog, "a_position");
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1.0, -1.0, 3.0, -1.0, -1.0, 3.0]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(posLoc);
    gl.vertexAttribPointer(posLoc, 2, gl.FLOAT, false, 0, 0);

    const uTime = gl.getUniformLocation(prog, "u_time");
    const uRes = gl.getUniformLocation(prog, "u_resolution");

    function resize() {
      const w = canvas.clientWidth;
      const h = canvas.clientHeight;
      if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; }
      gl.viewport(0, 0, w, h);
      gl.uniform2f(uRes, w, h);
    }
    _lockShaderResize = resize;
    window.addEventListener("resize", resize);
    resize();

    const motionOff = document.documentElement.classList.contains("rc-no-motion") || (window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches);
    if (motionOff) {
      gl.uniform1f(uTime, 0.0);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      return;
    }

    _lockShaderRunning = true;
    const start = performance.now();
    function frame(now) {
      if (!_lockShaderRunning || !document.getElementById("lock-shader")) return;
      gl.uniform1f(uTime, (now - start) * 0.001);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      _lockShaderRaf = requestAnimationFrame(frame);
    }
    _lockShaderRaf = requestAnimationFrame(frame);
  } catch (e) {
    // shader unavailable: leave CSS fallback visible
  }
}

export function renderLock(app) {
  const welcome = !app.state.onboardingCompleted;
  return `
    <div class="fixed inset-0 overflow-y-auto bg-[hsl(240_10%_9%)] text-white z-50">
      <canvas id="lock-shader" class="pointer-events-none fixed inset-0 w-full h-full" style="z-index:0"></canvas>
      <div class="absolute inset-0 opacity-10" style="background: radial-gradient(circle at 30% 30%, hsl(211 58% 45%), transparent 50%), radial-gradient(circle at 70% 70%, hsl(188 48% 38%), transparent 50%);"></div>
      <div class="relative z-10 min-h-full flex items-center justify-center p-6">
      <div class="relative w-full max-w-md px-6 animate-fade-up">
        <div class="mx-auto mb-8 flex h-14 w-14 items-center justify-center rounded-2xl bg-white/10 backdrop-blur ring-1 ring-white/20">${icon("lock", "w-7 h-7")}</div>
        <h1 class="text-center font-serif text-4xl italic mb-2">${t("rc.appName")}</h1>
        <p class="text-center text-white/60 mb-8">${t("rc.tagline")}</p>
        <div class="rounded-2xl bg-white/10 backdrop-blur border border-white/10 p-6">
          ${welcome ? `
            <h2 class="text-center text-2xl font-semibold mb-2">${t("rc.lock.welcome")}</h2>
            <p class="text-center text-white/70 text-sm mb-6">${t("rc.tagline")}</p>
            <button id="lock-btn" class="w-full rounded-lg bg-white text-[hsl(240_10%_9%)] font-semibold py-2.5 hover:bg-white/90 active:scale-[0.98] transition">${t("rc.lock.getStarted")}</button>
            <p class="mt-4 text-center text-xs text-white/40">${t("rc.lock.firstRunHint")}</p>
          ` : `
            <label class="block text-sm font-medium text-white/80 mb-2">${t("rc.lock.placeholder")}</label>
            <div class="relative">
              <input id="lock-input" type="password" class="w-full rounded-lg bg-white/10 border border-white/20 px-4 py-3 pr-10 text-white placeholder-white/40 focus:outline-none focus:border-white/40 transition" placeholder="••••••••">
              <button type="button" onclick="RC.togglePw('lock-input', this)" aria-label="${t("rc.lock.placeholder")}" class="absolute right-3 top-1/2 -translate-y-1/2 text-white/50 hover:text-white/80 transition">${icon("eye", "w-4 h-4")}</button>
            </div>
            <button id="lock-btn" class="mt-4 w-full rounded-lg bg-white text-[hsl(240_10%_9%)] font-semibold py-2.5 hover:bg-white/90 active:scale-[0.98] transition">${t("rc.lock.unlock")}</button>
            <p class="mt-4 text-center text-xs text-white/40">${t("rc.lock.idle")}</p>
          `}
        </div>
        ${mockActive && !welcome ? `
        <div class="mt-4 flex flex-col items-center gap-1.5">
          ${demoChip()}
          <p class="text-[11px] text-white/40 text-center">${t("demo.lockNote")}</p>
        </div>` : ""}
        ${welcome ? `
        <div class="mt-6 flex items-center justify-center gap-2">
          ${[{ code: "en", label: t("rc.lang.en") }, { code: "es", label: t("rc.lang.es") }, { code: "pt", label: t("rc.lang.pt") }, { code: "fr", label: t("rc.lang.fr") }].map(({ code, label }) => `
            <button type="button" onclick="RC.setLang('${code}')" class="inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-xs font-medium transition ${getLang() === code ? "border-white/40 bg-white/20 text-white" : "border-white/10 bg-white/5 text-white/60 hover:bg-white/10"}"><span>${label}</span></button>
          `).join("")}
        </div>
        ` : ""}
      </div>
      </div>
    </div>`;
}

export function bindLock(app) {
  const welcome = !app.state.onboardingCompleted;
  const el = document.getElementById("lock-btn");
  if (welcome) {
    if (el) el.onclick = () => app.unlock();
  } else {
    const unlock = () => app.unlock();
    if (el) el.onclick = unlock;
    const input = document.getElementById("lock-input");
    if (input) {
      input.onkeydown = (e) => { if (e.key === "Enter") unlock(); };
      input.focus();
    }
  }
  startLockShader();
}

// ========== WIZARD ==========
const LLM_PROVIDERS = ["OpenAI", "Anthropic", "Ollama", "Personal"];

export function renderWizard(app) {
  const s = app.state.wizard;
  const steps = ["lang", "folder", "ai", "init", "vault", "appearance", "mcp"];
  const step = s.step;
  const canAdvance =
    step === 0 ? true :
    step === 1 ? true :
    step === 2 ? true :
    step === 3 ? true :
    true;
  let content = "";
  if (step === 0) content = `
    <div class="space-y-6">
      <div>
        <h3 class="text-lg font-semibold">${t("rc.wizard.langTitle")}</h3>
        <p class="text-muted text-sm mt-1">${t("rc.wizard.langDesc")}</p>
      </div>
      <div class="space-y-3" role="radiogroup" aria-label="${t("rc.wizard.langTitle")}">
        ${[{ code: "es", label: t("rc.lang.es") }, { code: "en", label: t("rc.lang.en") }, { code: "pt", label: t("rc.lang.pt") }, { code: "fr", label: t("rc.lang.fr") }].map(({ code, label }) => `
          <button type="button" onclick="RC.setLang('${code}')" role="radio" aria-checked="${getLang() === code}" class="w-full flex items-center gap-4 rounded-xl border px-4 py-3.5 text-left transition ${getLang() === code ? "border-primary bg-primary/10 ring-1 ring-primary/30" : "border-border bg-white hover:bg-gray-50"}">
            <span class="flex-1 text-sm font-medium">${label}</span>
            <span class="h-4 w-4 rounded-full border flex items-center justify-center ${getLang() === code ? "bg-primary border-primary" : "border-muted"}">
              ${getLang() === code ? `<svg class="w-2.5 h-2.5 text-white" fill="none" stroke="currentColor" stroke-width="3" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7"/></svg>` : ""}
            </span>
          </button>
        `).join("")}
      </div>
    </div>`;
  else if (step === 1) content = `
    <div class="space-y-5">
      <div>
        <h3 class="text-lg font-semibold">${t("rc.wizard.dataTitle")}</h3>
        <p class="text-muted text-sm mt-1">${t("rc.wizard.dataDesc")}</p>
      </div>
      <div class="flex gap-3">
        <input id="wiz-folder" value="${esc(s.dataFolder)}" placeholder="~/ResearchCore/Data" class="flex-1 rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30">
      </div>
      <p class="text-xs text-muted">${t("rc.wizard.folderHint")}</p>
    </div>`;
  else if (step === 2) content = `
    <div class="space-y-4">
      <div>
        <h3 class="text-lg font-semibold">IA</h3>
        <p class="text-muted text-sm mt-1">${t("prov.note")}</p>
      </div>
      <div class="flex flex-wrap gap-2 mb-2">
        ${LLM_PROVIDERS.map((p) => `
          <button type="button" onclick="RC.wizProvider('${p}')" class="inline-flex items-center gap-2 rounded-lg border px-4 py-2 text-sm font-medium transition ${s.llmProvider === p ? "border-primary bg-primary/10 text-primary" : "border-border bg-white text-foreground hover:bg-gray-50"}">
            ${p}
          </button>
        `).join("")}
      </div>
      <div><label class="block text-sm font-medium mb-1.5">${t("prov.baseUrl")}</label><input id="wiz-base" value="${esc(s.baseUrl)}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
      <div><label class="block text-sm font-medium mb-1.5">${t("prov.apiKey")}</label><input id="wiz-key" type="password" value="${esc(s.apiKey)}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
      <div><label class="block text-sm font-medium mb-1.5">${t("prov.model")}</label><input id="wiz-model" value="${esc(s.llmModel)}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
    </div>`;
  else if (step === 3) content = renderInitStep(app);
  else if (step === 4) content = `
    <div class="space-y-6">
      <div>
        <h3 class="text-lg font-semibold">${t("rc.wizard.vaultTitle")}</h3>
        <p class="text-muted text-sm mt-1">${t("rc.lock.idle")}</p>
      </div>
      <div class="space-y-4">
        <label class="flex items-center justify-between rounded-xl border border-border bg-white p-4 cursor-pointer hover:border-primary/30 transition">
          <span class="text-sm font-medium">${t("rc.wizard.vaultOff")}</span>
          <input type="radio" name="vault" value="off" ${!s.vaultEnabled ? "checked" : ""} class="accent-primary w-4 h-4" onchange="RC.wizVault(false)">
        </label>
        <label class="flex items-center justify-between rounded-xl border border-border bg-white p-4 cursor-pointer hover:border-primary/30 transition">
          <span class="text-sm font-medium">${t("rc.wizard.vaultPass")}</span>
          <input type="radio" name="vault" value="on" ${s.vaultEnabled ? "checked" : ""} class="accent-primary w-4 h-4" onchange="RC.wizVault(true)">
        </label>
        <div class="space-y-4 ${s.vaultEnabled ? "" : "hidden"}">
          <div>
            <label class="block text-sm font-medium mb-1.5">${t("rc.lock.placeholder")}</label>
            <input id="wiz-pass" type="password" value="${esc(s.vaultPassphrase)}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30">
          </div>
          <div>
            <label class="block text-sm font-medium mb-1.5">${t("rc.wizard.vaultHint")}</label>
            <input id="wiz-hint" type="text" value="${esc(s.vaultHint)}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30">
          </div>
          <div>
            <label class="block text-sm font-medium mb-1.5">${t("rc.wizard.timeout")}</label>
            <input id="wiz-timeout" type="number" value="${esc(s.idleTimeout)}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30">
          </div>
        </div>
      </div>
    </div>`;
  else if (step === 5) content = `
    <div class="space-y-6">
      <div>
        <h3 class="text-lg font-semibold">${t("rc.wizard.appearanceTitle")}</h3>
        <p class="text-muted text-sm mt-1">${t("rc.wizard.appearanceDesc")}</p>
      </div>
      <div class="space-y-5">
        <div>
          <label class="block text-sm font-medium mb-2.5">${t("rc.settings.theme")}</label>
          <div class="grid grid-cols-3 gap-2 rounded-lg border border-border bg-gray-50 p-1">
            ${[["light", t("rc.settings.themeLight")], ["dark", t("rc.settings.themeDark")], ["system", t("rc.settings.themeSystem")]].map(([val, label]) => `
              <button type="button" onclick="RC.setTheme('${val}')" class="rounded-md py-2 text-sm font-medium transition ${app.state.settings.theme === val ? "bg-white shadow-sm text-foreground" : "text-muted hover:text-foreground"}">${label}</button>
            `).join("")}
          </div>
        </div>
        <div>
          <label class="block text-sm font-medium mb-2.5">${t("rc.wizard.accent")}</label>
          <div class="flex flex-wrap gap-3">
            ${Object.entries(app.ACCENTS).map(([k, v]) => `
              <button type="button" onclick="RC.setAccent('${k}')" class="flex flex-col items-center gap-1.5 group">
                <span class="relative h-10 w-10 rounded-full flex items-center justify-center ring-2 ring-offset-2 transition-transform ${app.state.settings.accent === k ? "scale-110" : "ring-transparent"}" style="background:${v.hsl}; ${app.state.settings.accent === k ? `--tw-ring-color:${v.hsl}` : ""}">
                  ${app.state.settings.accent === k ? `<svg class="w-5 h-5 text-white" fill="none" stroke="currentColor" stroke-width="3" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7"/></svg>` : ""}
                </span>
                <span class="text-xs ${app.state.settings.accent === k ? "font-semibold text-foreground" : "text-muted"}">${v.label[getLang()] || v.label.en}</span>
              </button>
            `).join("")}
          </div>
        </div>
        <label class="flex items-center justify-between rounded-xl border border-border bg-white p-4 cursor-pointer hover:border-primary/30 transition">
          <span class="text-sm font-medium">${t("rc.wizard.animations")}<span class="block text-xs font-normal text-muted">${t("rc.wizard.animationsDesc")}</span></span>
          <input type="checkbox" class="switch" ${app.state.settings.animations ? "checked" : ""} onchange="RC.setAnimations(this.checked)">
        </label>
      </div>
    </div>`;
  else if (step === 6) content = `
    <div class="space-y-6">
      <div>
        <h3 class="text-lg font-semibold">${t("rc.wizard.mcpTitle")}</h3>
        <p class="text-muted text-sm mt-1">${t("rc.wizard.mcpDesc")}</p>
      </div>
      <div class="space-y-2.5">
        ${app.data.mcp.map((s2) => `
          <div class="flex items-center justify-between rounded-xl border border-border bg-white p-4 ${s2.connected ? "ring-1 ring-primary/30" : ""} transition">
            <div class="min-w-0">
              <p class="text-sm font-medium truncate">${esc(s2.name)}</p>
              <p class="text-xs ${s2.connected ? "text-primary font-medium" : "text-muted"}">${s2.connected ? t("rc.wizard.mcpEnabled") : t("rc.wizard.mcpAvailable")}</p>
            </div>
            ${s2.connected
              ? `<button onclick="RC.wizToggleMcp('${esc(s2.id)}')" class="inline-flex items-center gap-2 rounded-md px-3 py-1.5 text-xs font-medium bg-primary/10 text-primary hover:bg-primary/20 transition">${t("rc.wizard.mcpEnabled")}</button>`
              : `${btn({ label: t("rc.wizard.mcpActivate"), variant: "default", size: "sm", onClick: `RC.wizToggleMcp('${esc(s2.id)}')` })}`}
          </div>
        `).join("") || `<p class="text-sm text-muted text-center py-6">${t("mcp.empty")}</p>`}
      </div>
      <p class="text-xs text-muted">${t("rc.wizard.mcpLater")}</p>
    </div>`;
  return `
    <div class="fixed inset-0 z-50 overflow-y-auto bg-background">
      <div class="rc-wizard-wave"></div>
      <div class="relative z-10 min-h-full flex items-center justify-center p-6">
      <div class="w-full max-w-xl animate-fade-up">
        <div class="mb-8 text-center">
          <h1 class="font-serif text-3xl italic">${t("rc.appName")}</h1>
          <p class="text-muted text-sm mt-2">${t("rc.tagline")}</p>
        </div>
        <div class="mb-8 flex items-center justify-between">
          ${steps.map((st, i) => `
            <div class="flex flex-1 items-center">
              <button type="button" onclick="RC.wizGoTo(${i})" class="flex h-8 w-8 items-center justify-center rounded-full text-sm font-semibold transition ${i <= step ? "bg-primary text-white" : "bg-gray-200 text-muted"} ${i <= s.maxStepReached ? "hover:ring-2 hover:ring-primary/40 cursor-pointer" : "cursor-not-allowed opacity-80"}">${i + 1}</button>
              ${i < steps.length - 1 ? `<div class="mx-2 h-1 flex-1 rounded ${i < step ? "bg-primary" : "bg-gray-200"}"></div>` : ""}
            </div>
          `).join("")}
        </div>
        ${card(`<div class="p-6 ${app.state.settings.animations ? (s.direction > 0 ? "animate-slide-right" : "animate-slide-left") : ""}">${content}</div>`, "mb-6")}
        <div class="flex justify-between">
          ${btn({ label: t("rc.wizard.back"), variant: "ghost", onClick: "RC.wizBack()", cls: step === 0 ? "invisible" : "" })}
          <div class="flex items-center gap-2">
            ${step < steps.length - 1
              ? (canAdvance
                ? btn({ label: t("rc.wizard.next"), onClick: "RC.wizNext()" })
                : `<button disabled class="inline-flex items-center justify-center rounded-lg bg-primary px-5 py-2.5 text-sm font-medium text-white opacity-50 pointer-events-none">${t("rc.wizard.next")}</button>`)
              : btn({ label: t("rc.wizard.finish"), onClick: "RC.wizFinish()" })}
          </div>
        </div>
      </div>
      </div>
    </div>`;
}

function renderInitStep(app) {
  const s = app.state.wizard;
  const mode = s.initMode;
  return `
    <div class="space-y-5">
      <div>
        <h3 class="text-lg font-semibold">${t("onb.pasteTitle")}</h3>
        <p class="text-muted text-sm mt-1">${t("missions.emptyHint")}</p>
      </div>
      <div class="grid grid-cols-2 gap-2 rounded-lg border border-border bg-gray-50 p-1">
        ${[["ai", t("onb.modeArxiv")], ["manual", t("onb.modeManual")]].map(([val, label]) => `
          <button type="button" onclick="RC.wizInitMode('${val}')" class="rounded-md py-2 text-sm font-medium transition ${mode === val ? "bg-white shadow-sm text-foreground" : "text-muted hover:text-foreground"}">${label}</button>
        `).join("")}
      </div>
      ${mode === "ai" ? `
        <div>
          <label class="block text-sm font-medium mb-1.5">${t("onb.pasteTitle")}</label>
          <input id="wiz-arxiv" value="${esc(s.initUrl)}" placeholder="https://arxiv.org/abs/1706.03762" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30">
          ${btn({ label: s.initLoading ? t("onb.generating") : t("onb.generate"), variant: "default", cls: "mt-3 w-full", iconName: "sparkle", onClick: "RC.wizFirstValue()", disabled: s.initLoading })}
        </div>
        ${s.initResult ? renderFirstValueResult(app, s.initResult) : ""}
      ` : `
        <div class="space-y-4">
          <div class="flex flex-col gap-1">
            ${demoChip()}
            <p class="text-xs text-muted">${t("demo.wizardNote")}</p>
          </div>
          <div><label class="block text-sm font-medium mb-1.5">${t("missions.composer.question")}</label><input id="wiz-q" value="${esc(s.initQuestion)}" class="w-full rounded-lg border border-border bg-white px-4 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
          <div><label class="block text-sm font-medium mb-1.5">${t("missions.stopCondition")}</label><input id="wiz-stop" value="${esc(s.initStop)}" class="w-full rounded-lg border border-border bg-white px-4 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
        </div>
      `}
    </div>`;
}

function renderFirstValueResult(app, r) {
  return `
    <div class="mt-4 rounded-xl border border-primary/30 bg-primary/5 p-4 animate-fade-up">
      <p class="text-xs font-semibold text-primary uppercase tracking-wider mb-1">${t("onb.resultKicker")}</p>
      <p class="text-sm font-medium">${esc(r.paper.title)}</p>
      <p class="text-xs text-muted mt-0.5">${esc(r.paper.authors)}${r.paper.year ? " · " + r.paper.year : ""}</p>
      <p class="text-xs text-muted mt-2">${t("onb.missionLabel")}: <span class="text-foreground font-medium">${esc(r.mission.question)}</span></p>
      <p class="text-xs text-muted mt-1">${t("onb.candidatesTitle")}: ${r.candidates.length}</p>
      <div class="mt-2 space-y-1">
        ${r.candidates.map((c) => `<p class="text-xs text-muted">· H-${c.seq} ${esc(c.statement)} <span class="font-mono">${(c.confidence * 100).toFixed(0)}%</span></p>`).join("")}
      </div>
      ${r.receipt.simulated ? `<p class="text-[11px] text-muted mt-2 font-mono">${t("onb.simulatedNote")}</p>` : ""}
    </div>`;
}

export function bindWizard(app) {
  // no-op — the wizard binds through RC handlers
}
