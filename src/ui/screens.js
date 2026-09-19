// The bible's continuity screens rendered as the live app: references
// (the migrated library via listRefs), AI review and actions (the bible's
// demo feel), the assistant (bible chat visuals over the mock provider),
// status (real MCP servers + trust connections), the morning digest (the
// real 90-second fold), and settings — including the v1 trust center tab
// (autonomy dials, hard ceilings, kill switch, compute targets) over the
// real getTrustStatus read model.
import { t, getLang } from "../i18n";
import { api } from "../api";
import { icon, esc, badge, btn, card, rcSelect, pageHeader, fmtCents, fmtTs } from "./helpers";
import { RC, ctx } from "./rc";

// ========== REFERENCES ==========
const refSource = (r) => (r.doi || "").startsWith("10.48550/arXiv.") ? "arXiv" : r.attachment ? "Zotero" : "Manual";
const refSourceColor = { arXiv: "primary", Zotero: "warning", "Semantic Scholar": "success", Manual: "muted", MCP: "medium" };
const refReviewed = (r) => r.status === "read" || r.status === "reviewed";

export function renderRefs(app) {
  const filter = (app.data.refFilter || "").toLowerCase();
  const rows = app.data.refs.filter(
    (r) => r.title.toLowerCase().includes(filter) || (r.authors || "").toLowerCase().includes(filter),
  );
  return `
    <div class="space-y-6">
      ${pageHeader(t("rc.refs.title"), t("rc.refs.title"))}
      <div class="flex flex-col sm:flex-row gap-3">
        <div class="relative flex-1">
          ${icon("search", "w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-muted")}
          <input id="ref-search" value="${esc(app.data.refFilter || "")}" placeholder="${t("rc.refs.search")}" class="w-full rounded-lg border border-border bg-white pl-9 pr-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30">
        </div>
      </div>
      ${card(`
        <div class="overflow-x-auto">
          <table class="w-full text-sm">
            <thead class="bg-gray-50 border-b border-border">
              <tr>
                <th class="px-6 py-3 text-left font-medium text-muted">${t("rc.refs.h.title")}</th>
                <th class="px-6 py-3 text-left font-medium text-muted">${t("rc.refs.h.authors")}</th>
                <th class="px-6 py-3 text-left font-medium text-muted">${t("rc.refs.h.year")}</th>
                <th class="px-6 py-3 text-left font-medium text-muted">${t("rc.refs.h.source")}</th>
                <th class="px-6 py-3 text-left font-medium text-muted">${t("rc.refs.h.tags")}</th>
                <th class="px-6 py-3 text-left font-medium text-muted">${t("rc.refs.h.status")}</th>
              </tr>
            </thead>
            <tbody class="divide-y divide-border">
              ${rows.length
                ? rows.map((r) => `
                <tr onclick="RC.openRefDetail('${esc(r.id)}')" class="hover-row cursor-pointer">
                  <td class="px-6 py-4 font-medium text-foreground">${esc(r.title)}</td>
                  <td class="px-6 py-4 text-muted">${esc(r.authors)}</td>
                  <td class="px-6 py-4">${esc(r.year ?? "")}</td>
                  <td class="px-6 py-4">${badge(refSource(r), refSourceColor[refSource(r)] || "muted")}</td>
                  <td class="px-6 py-4"><div class="flex flex-wrap gap-1">${(r.tags || "").split(",").filter(Boolean).map((tag) => badge(tag.trim(), "muted")).join("")}</div></td>
                  <td class="px-6 py-4">${badge(refReviewed(r) ? t("rc.refs.reviewed") : t("rc.refs.toReview"), refReviewed(r) ? "success" : "warning")}</td>
                </tr>`)
                .join("") : `<tr><td colspan="6" class="px-6 py-12 text-center text-muted">${t("rc.refs.empty")}</td></tr>`}
            </tbody>
          </table>
        </div>`)}
    </div>`;
}

export function bindRefs(app) {
  const search = document.getElementById("ref-search");
  if (search)
    search.oninput = (e) => {
      app.data.refFilter = e.target.value;
      const main = document.getElementById("main-content");
      if (main) {
        main.innerHTML = `<div class="max-w-7xl mx-auto animate-fade-up">${renderRefs(app)}</div>`;
        bindRefs(app);
        const next = document.getElementById("ref-search");
        if (next) {
          next.focus();
          next.setSelectionRange(next.value.length, next.value.length);
        }
      }
    };
}

function openRefDetail(id) {
  const app = ctx.app;
  app.data.selectedRef = app.data.refs.find((r) => r.id === id) || null;
  const r = app.data.selectedRef;
  if (!r) return;
  const overlay = document.createElement("div");
  overlay.className = "rc-modal fixed inset-0 z-[60] flex justify-end";
  overlay.innerHTML = `
    <div class="absolute inset-0 bg-black/30 backdrop-blur-sm" onclick="RC.closeRefDetail()"></div>
    <div class="relative w-full max-w-md h-full bg-white border-l border-border shadow-xl p-6 overflow-y-auto animate-[fadeUp_220ms_ease-out]">
      <div class="flex items-center justify-between mb-6">
        <h2 class="font-serif text-2xl italic">${t("rc.refs.detailTitle")}</h2>
        <button onclick="RC.closeRefDetail()" class="p-1 rounded hover:bg-gray-100">${icon("close", "w-5 h-5")}</button>
      </div>
      <div class="space-y-5">
        <div>
          <p class="text-xs text-muted uppercase tracking-wider mb-1">${t("rc.refs.h.title")}</p>
          <p class="font-medium text-lg leading-snug">${esc(r.title)}</p>
        </div>
        <div class="grid grid-cols-2 gap-4">
          <div><p class="text-xs text-muted uppercase tracking-wider mb-1">${t("rc.refs.h.authors")}</p><p class="text-sm">${esc(r.authors)}</p></div>
          <div><p class="text-xs text-muted uppercase tracking-wider mb-1">${t("rc.refs.h.year")}</p><p class="text-sm">${esc(r.year ?? "")}</p></div>
        </div>
        <div><p class="text-xs text-muted uppercase tracking-wider mb-1">${t("rc.refs.d.venue")}</p><p class="text-sm">${esc(r.venue || "")}</p></div>
        ${r.doi ? `<div><p class="text-xs text-muted uppercase tracking-wider mb-1">${t("rc.refs.d.doi")}</p><p class="text-sm font-mono">${esc(r.doi)}</p></div>` : ""}
        ${r.url ? `<div><p class="text-xs text-muted uppercase tracking-wider mb-1">${t("rc.refs.d.url")}</p><a href="${esc(r.url)}" target="_blank" rel="noopener noreferrer" class="text-sm text-primary hover:underline break-all">${esc(r.url)}</a></div>` : ""}
        <div><p class="text-xs text-muted uppercase tracking-wider mb-1">${t("rc.refs.h.tags")}</p><div class="flex flex-wrap gap-2">${(r.tags || "").split(",").filter(Boolean).map((tag) => badge(tag.trim(), "muted")).join("")}</div></div>
        <div><p class="text-xs text-muted uppercase tracking-wider mb-1">${t("rc.refs.d.abstract")}</p><p class="text-sm text-muted leading-relaxed">${t("rc.refs.d.noAbstract")}</p></div>
      </div>
    </div>`;
  document.body.appendChild(overlay);
}

// ========== AI REVIEW (the bible's demo feel) ==========
export function renderReview(app) {
  const findings = [
    { type: "theme", title: "Dominio de mecanismos de atención / Attention dominance", content: getLang() === "en" ? "Most recent work prioritizes attention as the central primitive, but linear-complexity alternatives (Mamba) are emerging." : "La mayoría de los trabajos recientes priorizan la atención como primitiva central, pero emergen alternativas de complejidad lineal (Mamba).", citations: ["Vaswani et al. 2017", "Gu & Dao 2023"] },
    { type: "gap", title: "Scaling analysis gap / Vacío de escalado", content: getLang() === "en" ? "No empirical studies of scaling laws on moderate-size multilingual corpora were found." : "No se encontraron estudios empíricos sobre leyes de escalado en corpus multilingües de tamaño moderado.", citations: ["Kaplan et al. 2020"] },
    { type: "conflict", title: "Pretraining vs. instructions / Preentrenamiento vs. instrucciones", content: getLang() === "en" ? "Tension between bidirectional pretraining (BERT) and post-RLHF instruction learning." : "Tensión entre la eficacia del preentrenamiento bidireccional (BERT) y el aprendizaje por instrucciones post-RLHF.", citations: ["Devlin et al. 2019", "Ouyang et al. 2022"] },
  ];
  return `
    <div class="space-y-6">
      ${pageHeader(t("rc.review.title"), t("rc.review.title"))}
      <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
        ${card(`
          <div class="p-6 space-y-5">
            <h3 class="font-semibold">${t("rc.review.select")}</h3>
            <div class="space-y-2 max-h-64 overflow-y-auto pr-1">
              ${app.data.refs.map((r) => `
                <label class="flex items-center gap-3 rounded-lg border border-border p-3 hover:bg-gray-50 cursor-pointer transition">
                  <input type="checkbox" class="review-ref accent-primary w-4 h-4" checked>
                  <div class="text-sm"><p class="font-medium">${esc(r.title)}</p><p class="text-xs text-muted">${esc(r.authors)} ${r.year ?? ""}</p></div>
                </label>`).join("")}
            </div>
            ${app.data.reviewRunning
              ? btn({ label: t("rc.review.running"), variant: "secondary", cls: "w-full opacity-80", iconName: "bolt" })
              : btn({ label: t("rc.review.run"), variant: "default", cls: "w-full", iconName: "sparkle", onClick: "RC.runReview()" })}
          </div>`)}
        ${app.data.reviewRunning ? renderReviewRunning() : renderReviewResults(findings)}
      </div>
    </div>`;
}
function renderReviewRunning() {
  return card(`
    <div class="p-6 lg:col-span-2 flex flex-col items-center justify-center min-h-[320px] text-center">
      <div class="relative mb-6">
        <div class="absolute inset-0 rounded-full bg-primary/20 animate-[pulse-ring_2s_ease-in-out_infinite]"></div>
        <div class="relative flex h-16 w-16 items-center justify-center rounded-full bg-primary text-white">${icon("sparkle", "w-7 h-7")}</div>
      </div>
      <h3 class="text-lg font-semibold">${t("rc.review.running")}</h3>
      <div class="mt-4 h-1.5 w-64 rounded-full bg-gray-100 overflow-hidden">
        <div id="review-bar" class="h-full bg-gradient-to-r from-sky-600 to-teal-500 transition-all duration-300" style="width: 40%"></div>
      </div>
      <p class="mt-4 font-mono text-xs text-muted h-5">Initializing review agent...</p>
    </div>`, "lg:col-span-2");
}
function renderReviewResults(findings) {
  return card(`
    <div class="p-6 lg:col-span-2">
      <h3 class="font-semibold mb-5">${t("rc.review.findings")}</h3>
      <div class="space-y-4">
        ${findings.map((f, i) => `
          <div class="rounded-xl border border-border p-5 hover:shadow-md transition">
            <div class="flex items-center gap-2 mb-2">
              ${badge(f.type === "theme" ? t("rc.review.themes") : f.type === "gap" ? t("rc.review.gaps") : t("rc.review.conflicts"), f.type === "theme" ? "primary" : f.type === "gap" ? "warning" : "destructive")}
            </div>
            <h4 class="font-semibold text-base mb-1">${esc(f.title)}</h4>
            <p class="text-sm text-muted leading-relaxed">${esc(f.content)}</p>
            <div class="mt-3 flex flex-wrap gap-2">
              ${f.citations.map((c) => `<span class="inline-flex items-center gap-1 rounded-md bg-primary/10 px-2 py-1 text-xs font-medium text-primary">${icon("book", "w-3 h-3")} ${esc(c)}</span>`).join("")}
            </div>
          </div>`).join("")}
      </div>
    </div>`, "lg:col-span-2");
}
export function bindReview(app) {}

// ========== ASSISTANT (bible chat visuals over the mock provider) ==========
export function renderAssistant(app) {
  const msgs = app.data.chatMessages[app.data.activeChatId || ""] || [];
  const empty = msgs.length === 0 && !app.data.chatThinking;
  const project = app.data.project;
  const name = project ? project.name : "";
  const field = project ? project.tags || "" : "";
  const suggestions = [
    t("rc.assistant.suggest1", { project: name, field }),
    t("rc.assistant.suggest2", { project: name, field }),
    t("rc.assistant.suggest3", { project: name, field }),
    t("rc.assistant.suggest4", { project: name, field }),
  ];
  return `
    <div class="h-[calc(100vh-112px)] flex flex-col">
      <div class="flex items-center justify-between gap-3 mb-4">
        <div class="flex items-center gap-3 min-w-0">
          <div class="flex h-9 w-9 items-center justify-center rounded-xl bg-primary/10 text-primary shrink-0">${icon("sparkle", "w-4 h-4")}</div>
          <div class="min-w-0">
            <p class="caption text-muted">${t("rc.assistant.title")}</p>
            <span class="text-sm font-medium leading-snug truncate">${t("rc.assistant.newChatTitle")}</span>
          </div>
        </div>
        <div class="flex items-center gap-2">
          <button onclick="RC.openChatHistory()" title="${t("rc.assistant.history")}" class="p-2 rounded-lg border border-border bg-card text-muted hover:text-foreground hover:border-primary/40 transition ring-focus">${icon("history", "w-5 h-5")}</button>
        </div>
      </div>
      ${empty ? `
      <div class="relative flex-1 flex flex-col items-center justify-center max-w-3xl mx-auto w-full px-4 -mt-6 overflow-hidden">
        <div class="relative -mb-10 flex flex-col items-center w-full">
          <div class="pointer-events-none absolute -inset-x-16 -top-12 -bottom-16 z-0">
            <div class="rc-ai-aurora"></div>
            <div class="rc-ai-particle" style="left:12%; top:34%; animation-delay:0s"></div>
            <div class="rc-ai-particle" style="left:84%; top:52%; animation-delay:1.8s"></div>
            <div class="rc-ai-particle" style="left:20%; top:72%; animation-delay:3.2s"></div>
            <div class="rc-ai-particle" style="left:78%; top:22%; animation-delay:4.6s"></div>
            <div class="rc-ai-particle" style="left:50%; top:84%; animation-delay:2.6s"></div>
          </div>
          <div class="relative z-10 flex h-11 w-11 items-center justify-center rounded-2xl bg-primary/10 text-primary mb-6 rc-intro">${icon("sparkle", "w-5 h-5")}</div>
          <h2 class="relative z-10 font-serif text-6xl italic mb-4 text-center rc-intro rc-intro-1">${t("rc.assistant.welcomeTitle")}</h2>
          <p class="relative z-10 body-lg text-muted text-center max-w-md mb-8 rc-intro rc-intro-2">${t("rc.assistant.welcomeSub")}</p>
          <div class="relative z-10 flex flex-wrap justify-center gap-2.5 w-full rc-intro rc-intro-3">
            ${suggestions.map((q) => `
              <button type="button" onclick="RC.fillChatSuggestion(this.dataset.q)" data-q="${esc(q)}" class="rounded-full border border-border bg-card px-4 py-2 text-sm text-muted hover:-translate-y-0.5 hover:border-primary/40 hover:bg-primary/5 hover:text-primary hover:shadow-sm active:scale-[0.97] transition-all duration-200 ring-focus">${esc(q)}</button>`).join("")}
          </div>
        </div>
        <div class="relative z-10 w-full rc-intro rc-intro-4">${renderChatComposer()}</div>
      </div>` : `
      <div class="flex-1 flex flex-col min-h-0 max-w-3xl mx-auto w-full px-4">
        <div id="chat-list" class="flex-1 overflow-y-auto py-2 space-y-3">
          ${msgs.map((m, i) => renderChatMessage(m, i === msgs.length - 1)).join("")}
          ${app.data.chatThinking ? `
          <div class="flex justify-start">
            <div class="inline-flex items-center gap-2.5 rounded-full bg-gray-100 px-3 py-2">
              <span class="rc-orb"></span><span class="rc-orb"></span><span class="rc-orb"></span>
              <span class="text-xs font-medium text-muted">${t("rc.review.running")}</span>
            </div>
          </div>` : ""}
        </div>
        <div class="pt-3 pb-1">${renderChatComposer()}</div>
      </div>`}
    </div>`;
}

function renderChatMessage(m, animate = false) {
  const anim = animate ? " animate-fade-up" : "";
  if (m.role === "user") {
    return `
    <div class="flex justify-end${anim}">
      <div class="rc-bubble-user max-w-[85%] rounded-2xl rounded-br-md bg-primary text-white text-sm whitespace-pre-wrap break-words shadow-sm">${esc(m.content)}</div>
    </div>`;
  }
  return `
  <div class="flex justify-start gap-2${anim}">
    <div class="flex h-6 w-6 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary mt-0.5">${icon("book", "w-3 h-3")}</div>
    <div class="rc-bubble-ai max-w-[82%] rounded-2xl rounded-bl-md bg-gray-100 text-foreground text-sm whitespace-pre-wrap break-words">${esc(m.content)}</div>
  </div>`;
}

function renderChatComposer() {
  return `
  <div id="chat-composer" class="chat-composer relative rounded-2xl border border-border bg-card shadow-sm p-3 focus-within:border-primary/40 focus-within:ring-2 focus-within:ring-primary/30 transition">
    <textarea id="chat-input" rows="1" class="w-full resize-none bg-transparent px-1.5 py-1.5 text-[15px] leading-relaxed focus:outline-none max-h-40 placeholder:text-muted" placeholder="${t("rc.assistant.placeholder")}"></textarea>
    <div class="flex items-center justify-end mt-1.5">
      <button id="chat-send" onclick="RC.sendChatMessage()" class="rounded-lg bg-primary text-white h-9 w-9 grid place-items-center hover:opacity-90 active:scale-95 transition ring-focus" aria-label="${t("rc.assistant.send")}">${icon("arrowRight", "w-5 h-5")}</button>
    </div>
  </div>`;
}

export function bindAssistant(app) {
  const input = document.getElementById("chat-input");
  if (!input) return;
  input.onkeydown = (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      RC.sendChatMessage();
    }
  };
  input.oninput = () => {
    input.style.height = "auto";
    input.style.height = Math.min(input.scrollHeight, 160) + "px";
  };
  setTimeout(() => {
    const list = document.getElementById("chat-list");
    if (list) list.scrollTop = list.scrollHeight;
  }, 50);
}

// ========== ACTIONS (the bible's demo feel) ==========
const ACTION_STATUSES = ["backlog", "todo", "in_progress", "done"];
const demoActions = [
  { id: 1, key: "AI-1", title: "Revisar citas de Kaplan et al. 2020", description: "Verificar que las citas de las leyes de escalado estén correctamente vinculadas y no haya sobre-citas.", status: "in_progress", priority: "high", due: "2026-09-08" },
  { id: 2, key: "AI-2", title: "Sincronizar librería Zotero", description: "Traer las últimas referencias añadidas desde Zotero y resolver duplicados.", status: "todo", priority: "medium", due: "2026-09-10" },
  { id: 3, key: "AI-3", title: "Verificar conector MCP de arXiv", description: "Comprobar que el servidor MCP de arXiv responde y devuelve resultados relevantes.", status: "done", priority: "high", due: "2026-09-07" },
  { id: 4, key: "AI-4", title: "Comparar hallazgos de Hu et al. 2021", description: "Contrastar los resultados de LoRA con los métodos de fine-tuning completos citados.", status: "backlog", priority: "low", due: "2026-09-15" },
  { id: 5, key: "AI-5", title: "Analizar trade-off atención vs. Mamba", description: "Resumir las diferencias de complejidad y casos de uso entre transformers y SSM.", status: "in_progress", priority: "urgent", due: "2026-09-09" },
  { id: 6, key: "AI-6", title: "Compilar tabla de arquitecturas", description: "Crear una tabla comparativa de arquitecturas incluyendo año, complejidad y entrenamiento.", status: "todo", priority: "high", due: "2026-09-11" },
];
const actionStatusDot = (status) =>
  status === "done" ? "bg-emerald-500" : status === "in_progress" ? "bg-amber-500" : status === "todo" ? "bg-primary" : "bg-slate-400";

export function renderActions(app) {
  const f = app.data.actionFilters || (app.data.actionFilters = { status: [...ACTION_STATUSES], search: "" });
  const q = (f.search || "").toLowerCase();
  const items = demoActions.filter((a) => f.status.includes(a.status) && (!q || a.title.toLowerCase().includes(q) || a.description.toLowerCase().includes(q)));
  return `
    <div class="space-y-5">
      <div class="flex flex-col md:flex-row md:items-end justify-between gap-4">
        <div>
          <p class="caption text-muted mb-1">${t("rc.actions.title")}</p>
          <h1 class="display-md">${t("rc.actions.title")}</h1>
          <p class="text-sm text-muted mt-1">${esc(app.data.project?.name || "")} · <span class="tabular">${items.length}</span> ${t("rc.actions.counts")}</p>
        </div>
      </div>
      <div class="flex flex-col xl:flex-row gap-3">
        <div class="relative flex-1">
          ${icon("search", "w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-muted")}
          <input id="action-search" value="${esc(f.search)}" placeholder="${t("rc.actions.search")}" class="w-full rounded-lg border border-border bg-white pl-9 pr-4 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30">
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <span class="text-xs text-muted">${t("rc.actions.f.status")}:</span>
          ${ACTION_STATUSES.map((st) => `
            <button onclick="RC.toggleActionStatusFilter('${st}')" class="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-medium transition ${f.status.includes(st) ? "bg-primary/10 border-primary/30 text-primary" : "border-border text-muted hover:text-foreground"}">
              ${f.status.includes(st) ? icon("check", "w-3 h-3") : '<span class="w-3 h-3"></span>'}
              ${t("rc.actions.s." + st)}
            </button>`).join("")}
        </div>
      </div>
      <div class="space-y-4">
        ${ACTION_STATUSES.map((st) => {
          const group = items.filter((a) => a.status === st);
          return `
          <div class="rounded-xl border border-border bg-card overflow-hidden">
            <div class="w-full flex items-center justify-between px-4 py-3 bg-gray-50/50">
              <div class="flex items-center gap-3">
                <span class="h-2.5 w-2.5 rounded-full ${actionStatusDot(st)}"></span>
                <span class="heading-3">${t("rc.actions.s." + st)}</span>
                <span class="tabular text-xs text-muted bg-gray-100 rounded-full px-2 py-0.5">${group.length}</span>
              </div>
            </div>
            <div class="p-3 grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-3">
              ${group.length
                ? group.map((a, i) => `
              <div class="animate-fade-up" style="animation-delay:${i * 40}ms">
                <div class="group rounded-lg border border-border bg-white p-4 cursor-pointer hover:border-primary/40 hover:shadow-sm transition relative">
                  <div class="flex items-start justify-between gap-2 mb-2">
                    <span class="font-mono text-[10px] text-muted tabular">${esc(a.key)}</span>
                    ${badge(t("rc.actions.p." + a.priority), a.priority)}
                  </div>
                  <h4 class="font-medium text-sm leading-snug mb-2 ${a.status === "done" ? "line-through text-muted" : ""}">${esc(a.title)}</h4>
                  <p class="text-xs text-muted line-clamp-2 mb-3">${esc(a.description)}</p>
                  <div class="flex items-center justify-between">
                    <span class="text-[10px] text-muted tabular">${esc(a.due)}</span>
                  </div>
                </div>
              </div>`)
                : `<p class="text-sm text-muted py-4 text-center col-span-full">${t("rc.actions.noTasks")}</p>`}
            </div>
          </div>`;
        }).join("")}
      </div>
    </div>`;
}

export function bindActions(app) {
  const search = document.getElementById("action-search");
  if (search)
    search.oninput = (e) => {
      app.data.actionFilters.search = e.target.value;
      const main = document.getElementById("main-content");
      if (main) {
        main.innerHTML = `<div class="max-w-7xl mx-auto animate-fade-up">${renderActions(app)}</div>`;
        bindActions(app);
        const next = document.getElementById("action-search");
        if (next) {
          next.focus();
          next.setSelectionRange(next.value.length, next.value.length);
        }
      }
    };
}

// ========== MORNING DIGEST (the real 90-second fold) ==========
const digestOutcome = {
  no_runs: ["muted", "—"],
  all_finished: ["success", "✓"],
  partial_success: ["warning", "!"],
  all_failed: ["destructive", "×"],
};
const missionStatusColor = { active: "primary", awaiting_review: "warning", completed: "success", stopped: "muted", failed: "destructive" };

export function renderDigest(app) {
  const d = app.data.digest;
  const header = pageHeader(
    t("digest.title"),
    t("digest.title"),
    btn({ label: app.data.digestRunning ? t("digest.running") : t("digest.runNow"), variant: "default", size: "sm", iconName: "bolt", onClick: "RC.runNightShift()", disabled: !!app.data.digestRunning }),
  );
  if (!d) return `${header}${card(`<div class="p-12 text-center text-muted text-sm">${t("rc.common.loading")}</div>`)}`;
  return `
    <div class="space-y-6">
      ${header}
      ${d.alerts.length || d.connectionAlerts.length ? `
      <div class="space-y-2">
        ${d.alerts.map((a) => `
          <div class="rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 flex items-center gap-3">
            ${icon("danger", "w-5 h-5 text-rose-600 shrink-0")}
            <div class="min-w-0">
              <p class="text-xs font-semibold text-rose-700 uppercase tracking-wider">${t("digest.alertLabel")}</p>
              <p class="text-sm text-rose-600 mt-0.5">${t("digest.alertBody", { runId: a.runId, time: fmtTs(a.heartbeatTs) })} · <button onclick="RC.openReceipt('${esc(a.runId)}')" class="font-medium underline hover:no-underline">${t("digest.receipts")}</button></p>
            </div>
          </div>`).join("")}
        ${d.connectionAlerts.map((c) => `
          <div class="rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 flex items-center gap-3">
            ${icon("globe", "w-5 h-5 text-rose-600 shrink-0")}
            <div class="min-w-0">
              <p class="text-xs font-semibold text-rose-700 uppercase tracking-wider">${t("digest.connAlertLabel")}</p>
              <p class="text-sm text-rose-600 mt-0.5">${t("digest.connAlertBody", { connection: c.connection, code: c.errorCode, time: fmtTs(c.failedTs) })}</p>
            </div>
          </div>`).join("")}
      </div>` : ""}
      ${card(`
        <div class="divide-y divide-border">
          <div class="px-6 py-4 flex items-center justify-between gap-3 bg-gray-50/50">
            <div class="flex items-center gap-2">
              ${badge(t("digest.outcome." + d.outcome), digestOutcome[d.outcome]?.[0] || "muted")}
              <span class="font-mono text-xs tabular text-muted">${fmtCents(d.spendCents)} ${t("digest.ofCeiling")} ${fmtCents(d.ceilingCents)}</span>
            </div>
            <span class="caption text-muted">${fmtTs(d.generatedAt)}</span>
          </div>
          ${d.rows.length ? d.rows.map((r) => digestRow(r)).join("") : `<p class="px-6 py-10 text-center text-muted text-sm">${t("digest.empty")}</p>`}
        </div>`)}
      <p class="text-xs text-muted">${t("digest.rowsNote")}</p>
    </div>`;
}

function digestRow(r) {
  let verdict;
  if (r.failed > 0) verdict = t("digest.verdict.failed", { reason: r.failureReason || "?" });
  else if (r.ceilingReached) verdict = t("digest.verdict.ceiling");
  else if (r.jobsFinished || r.jobsFailed)
    verdict = r.jobsFailed
      ? t("digest.job.failed", { target: r.jobVerdict?.target || "", job: (r.jobVerdict?.jobId || "").slice(0, 8), reason: r.jobVerdict?.reason || "?" })
      : t("digest.verdict.jobs", { count: r.jobsFinished });
  else verdict = t("digest.verdict.ok", { runs: r.runs, proposals: r.proposalsPending });
  return `
    <div class="px-6 py-4 hover-row">
      <div class="flex items-center justify-between gap-3">
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <span class="font-mono text-[10px] text-muted tabular shrink-0">M-${r.missionSeq}</span>
            <p class="text-sm font-medium truncate">${esc(r.question)}</p>
          </div>
          <p class="text-xs text-muted mt-0.5 truncate">${verdict}</p>
        </div>
        <div class="flex items-center gap-2 shrink-0">
          ${badge(t("missions.status." + r.status), missionStatusColor[r.status] || "muted")}
          <button onclick="RC.openReceipt('${esc(r.runId)}')" class="text-xs font-medium text-primary hover:underline">${t("digest.receipts")}</button>
        </div>
      </div>
    </div>`;
}

// ========== STATUS ==========
export function renderStatus(app) {
  const servers = app.data.mcp;
  const trust = app.data.trust;
  return `
    <div class="space-y-6">
      ${pageHeader(t("rc.status.title"), t("rc.status.title"))}
      ${card(`
        <div class="overflow-x-auto">
          <table class="w-full text-sm">
            <thead class="bg-gray-50 border-b border-border"><tr>
              <th class="px-6 py-3 text-left font-medium text-muted">${t("rc.common.name")}</th>
              <th class="px-6 py-3 text-left font-medium text-muted">${t("rc.common.status")}</th>
              <th class="px-6 py-3 text-left font-medium text-muted">${t("rc.status.health")}</th>
              <th class="px-6 py-3 text-left font-medium text-muted">${t("rc.status.transport")}</th>
              <th class="px-6 py-3 text-right font-medium text-muted">${t("rc.common.configure")}</th>
            </tr></thead>
            <tbody class="divide-y divide-border">
              ${servers.map((s) => `
              <tr>
                <td class="px-6 py-4 font-medium font-mono text-xs">${esc(s.name)}</td>
                <td class="px-6 py-4">${badge(s.connected ? t("rc.status.connected") : t("rc.status.deactivate"), s.connected ? "success" : "muted")}</td>
                <td class="px-6 py-4">${badge(s.connected ? t("rc.status.healthy") : t("rc.status.degraded"), s.connected ? "success" : "warning")}</td>
                <td class="px-6 py-4 text-muted font-mono text-xs">${esc(s.transport)}</td>
                <td class="px-6 py-4 text-right">
                  ${btn({ label: s.connected ? t("rc.status.deactivate") : t("rc.status.connect"), variant: s.connected ? "ghost" : "default", size: "sm", onClick: `RC.toggleMcp('${esc(s.id)}')` })}
                </td>
              </tr>`).join("")}
            </tbody>
          </table>
        </div>`)}
      ${trust?.connections?.length ? card(`
        <div class="p-5 space-y-3">
          <p class="text-sm font-semibold">${t("trust.connections")}</p>
          <div class="flex flex-wrap gap-2">
            ${trust.connections.map((c) => badge(c.connection + " · " + (c.up ? t("trust.connUp") : t("trust.connDown") + " " + (c.lastErrorCode || "")), c.up ? "success" : "destructive")).join("")}
          </div>
        </div>`) : ""}
      <div class="grid grid-cols-1 md:grid-cols-3 gap-6">
        ${card(`<div class="p-5"><p class="text-sm font-semibold mb-1">${t("local.store")}</p><p class="text-xs text-muted font-mono">~/ResearchCore/Data/researchcore.db</p></div>`)}
        ${card(`<div class="p-5"><p class="text-sm font-semibold mb-1">${t("rc.settings.vault")}</p><p class="text-xs text-muted">${t("sec.policyNever")}</p></div>`)}
        ${card(`<div class="p-5"><p class="text-sm font-semibold mb-1">${t("ia.provider")}</p><p class="text-xs text-muted font-mono">simulated / GLM-5.3</p></div>`)}
      </div>
    </div>`;
}

// ========== SETTINGS (bible tabs + the v1 trust center) ==========
export function renderSettings(app) {
  const activeTab = app.state.settingsTab || "interface";
  const tabs = [
    { id: "interface", label: t("rc.settings.interface") },
    { id: "ai", label: t("rc.settings.ai") },
    { id: "trust", label: t("sec.trust") },
    { id: "local", label: t("rc.settings.local") },
    { id: "vault", label: t("rc.settings.vault") },
    { id: "danger", label: t("rc.settings.danger") },
  ];
  let content = "";
  if (activeTab === "interface") content = renderSettingsInterface(app);
  else if (activeTab === "ai") content = renderSettingsAi(app);
  else if (activeTab === "trust") content = renderTrustCenter(app);
  else if (activeTab === "local") content = `
    <div class="space-y-5">
      <label class="flex items-center justify-between rounded-lg border border-border p-4"><span class="text-sm font-medium">${t("local.store")}</span><input type="checkbox" class="switch" checked></label>
      <label class="flex items-center justify-between rounded-lg border border-border p-4"><span class="text-sm font-medium">${t("local.zotero")}</span><input type="checkbox" class="switch"></label>
      <label class="flex items-center justify-between rounded-lg border border-border p-4"><span class="text-sm font-medium">${t("local.cache")}</span><input type="checkbox" class="switch" checked></label>
      <p class="text-xs text-muted">${t("rc.settings.dataFolderLabel")}: <span class="font-mono">~/ResearchCore/Data</span></p>
    </div>`;
  else if (activeTab === "vault") content = `
    <div class="space-y-5">
      <label class="flex items-center justify-between rounded-lg border border-border p-4"><span class="text-sm font-medium">${t("rc.wizard.vaultPass")}</span><input type="checkbox" class="switch"></label>
      <div><label class="block text-sm font-medium mb-1.5">${t("rc.lock.placeholder")}</label><input type="password" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
      <div><label class="block text-sm font-medium mb-1.5">${t("rc.wizard.timeout")}</label><input type="number" value="10" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
    </div>`;
  else content = `
    <div class="space-y-4">
      <div class="rounded-xl border border-rose-200 bg-rose-50 p-5">
        <h3 class="font-semibold text-rose-700 flex items-center gap-2">${icon("danger", "w-5 h-5")} ${t("rc.settings.reset")}</h3>
        <p class="text-sm text-rose-600 mt-2">${t("rc.settings.resetDesc")}</p>
        ${btn({ label: t("rc.settings.reset"), variant: "destructive", cls: "mt-4", onClick: "RC.resetWizard()" })}
      </div>
      <button onclick="RC.navigate('missions')" class="text-sm text-primary hover:underline">${t("rc.settings.backToWizard")}</button>
    </div>`;
  return `
    <div class="max-w-3xl mx-auto py-6 space-y-6">
      <div class="text-center sm:text-left">
        <p class="caption text-muted mb-1">${t("rc.settings.title")}</p>
        <h1 class="display-md">${t("rc.settings.title")}</h1>
      </div>
      <div class="flex flex-wrap justify-center sm:justify-start gap-2 border-b border-border pb-1">
        ${tabs.map((tab) => `
          <button onclick="RC.setSettingsTab('${tab.id}')" class="px-4 py-2 text-sm font-medium rounded-t-lg transition ${activeTab === tab.id ? "text-primary border-b-2 border-primary" : "text-muted hover:text-foreground"}">${tab.label}</button>`).join("")}
      </div>
      ${card(`<div class="p-6">${content}</div>`)}
    </div>`;
}

function renderSettingsInterface(app) {
  return `
    <div class="space-y-5">
      <div><label class="block text-sm font-medium mb-1.5">${t("rc.settings.language")}</label>${rcSelect({ id: "set-lang", options: [{ value: "es", label: t("rc.lang.es") }, { value: "en", label: t("rc.lang.en") }, { value: "pt", label: t("rc.lang.pt") }, { value: "fr", label: t("rc.lang.fr") }], value: getLang(), onChange: "RC.setLang(this.value)" })}</div>
      <div><label class="block text-sm font-medium mb-1.5">${t("rc.settings.theme")}</label><div class="grid grid-cols-3 gap-2 rounded-lg border border-border bg-gray-50 p-1">${[["light", t("rc.settings.themeLight")], ["dark", t("rc.settings.themeDark")], ["system", t("rc.settings.themeSystem")]].map(([val, label]) => `<button type="button" onclick="RC.setTheme('${val}')" class="rounded-md py-2 text-sm font-medium transition ${app.state.settings.theme === val ? "bg-white shadow-sm text-foreground" : "text-muted hover:text-foreground"}">${label}</button>`).join("")}</div></div>
      <div><label class="block text-sm font-medium mb-1.5">${t("rc.wizard.accent")}</label><div class="flex flex-wrap gap-3">${Object.entries(app.ACCENTS).map(([k, v]) => `<button type="button" onclick="RC.setAccent('${k}')" class="flex flex-col items-center gap-1"><span class="h-8 w-8 rounded-full ring-2 ring-offset-2 transition-transform ${app.state.settings.accent === k ? "scale-110" : "ring-transparent"}" style="background:${v.hsl}; ${app.state.settings.accent === k ? `--tw-ring-color:${v.hsl}` : ""}"></span><span class="text-[10px] ${app.state.settings.accent === k ? "font-semibold" : "text-muted"}">${v.label[getLang()] || v.label.en}</span></button>`).join("")}</div></div>
      <label class="flex items-center justify-between rounded-lg border border-border p-4 cursor-pointer hover:border-primary/30 transition">
        <span class="text-sm font-medium">${t("rc.wizard.animations")}<span class="block text-xs font-normal text-muted">${t("rc.wizard.animationsDesc")}</span></span>
        <input type="checkbox" class="switch" ${app.state.settings.animations ? "checked" : ""} onchange="RC.setAnimations(this.checked)">
      </label>
    </div>`;
}

function renderSettingsAi(app) {
  const s = app.data.settings || (app.data.settings = { baseUrl: "https://api.tokenfactory.corvex.cloud/v1", apiKey: "", model: "zai-org/GLM-5.3" });
  return `
    <div class="space-y-4">
      <div><label class="block text-sm font-medium mb-1.5">${t("prov.baseUrl")}</label><input id="set-base" value="${esc(s.baseUrl)}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
      <div><label class="block text-sm font-medium mb-1.5">${t("prov.apiKey")}</label><input id="set-key" type="password" value="${esc(s.apiKey)}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
      <div><label class="block text-sm font-medium mb-1.5">${t("prov.model")}</label><input id="set-model" value="${esc(s.model)}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
      <p class="text-xs text-muted">${t("prov.note")}</p>
    </div>`;
}

// The trust center (FR-5): runtime kill switch, autonomy dials over every
// scope, hard ceilings with live meters, and the compute targets — all over
// the real getTrustStatus read model.
function autonomyDial(id, mode) {
  const stops = [["watch", t("trust.watch")], ["suggest", t("trust.suggest")], ["act_with_receipts", t("trust.act")]];
  return `
    <div class="grid grid-cols-3 gap-1 rounded-lg border border-border bg-gray-50 p-1">
      ${stops.map(([v, l]) => `<button type="button" onclick="RC.dial('${id.scope}','${id.scopeId || ""}','${v}')" class="rounded-md py-1.5 text-xs font-medium transition ${mode === v ? "bg-white shadow-sm text-foreground" : "text-muted hover:text-foreground"}">${l}</button>`).join("")}
    </div>`;
}

function renderTrustCenter(app) {
  const trust = app.data.trust;
  if (!trust) return `<p class="text-sm text-muted py-8 text-center">${t("rc.common.loading")}</p>`;
  const killed = trust.runtimeState === "killed";
  const ceilingMeter = (spend, ceiling) => {
    if (ceiling === null || ceiling === undefined) return `<span class="text-xs text-muted font-mono">${t("trust.unset")}</span>`;
    const pct = Math.min(100, Math.round((spend / ceiling) * 100));
    const cls = pct >= 100 ? "bg-rose-500" : pct >= 80 ? "bg-amber-500" : "bg-emerald-500";
    return `
      <div class="flex-1 min-w-[100px]">
        <div class="flex items-baseline justify-between gap-2 mb-1">
          <span class="font-mono text-[10px] tabular text-muted">${fmtCents(spend)} ${t("trust.of")} ${fmtCents(ceiling)}</span>
        </div>
        <div class="h-1.5 rounded-full bg-border overflow-hidden"><div class="h-full rounded-full ${cls}" style="width:${pct}%"></div></div>
      </div>`;
  };
  return `
    <div class="space-y-6">
      <div class="flex items-center justify-between rounded-xl border ${killed ? "border-rose-200 bg-rose-50" : "border-border bg-white"} p-4">
        <div>
          <p class="text-sm font-semibold">${t("trust.autonomousWork")}</p>
          <p class="text-xs ${killed ? "text-rose-600" : "text-muted"} mt-0.5">${killed ? t("trust.killOff") : t("trust.killOn")}</p>
        </div>
        ${killed
          ? btn({ label: t("trust.resume"), variant: "default", size: "sm", onClick: "RC.resumeRuntime()" })
          : btn({ label: t("trust.kill"), variant: "destructive", size: "sm", iconName: "power", onClick: "RC.confirmKill()" })}
      </div>
      <div class="space-y-3">
        <p class="text-sm font-semibold">${t("trust.autonomy")} <span class="text-muted font-normal text-xs">— ${t("trust.autonomySub")}</span></p>
        <div class="rounded-xl border border-border p-4 space-y-3">
          <div class="flex items-center justify-between gap-4">
            <span class="text-sm font-medium">${t("trust.global")}</span>
            <div class="w-64">${autonomyDial({ scope: "global", scopeId: null }, trust.globalAutonomy || "suggest")}</div>
          </div>
          <p class="text-[11px] text-muted">${t("trust.mostRestrictive")}</p>
        </div>
        ${trust.missions.length ? `
        <div class="rounded-xl border border-border divide-y divide-border overflow-hidden">
          ${trust.missions.map((m) => `
          <div class="px-4 py-3 flex items-center justify-between gap-4 bg-white">
            <div class="min-w-0 flex-1">
              <p class="text-sm font-medium truncate">${esc(m.question)}</p>
              <div class="mt-1">${ceilingMeter(m.spendCents, m.ceilingCents)}</div>
            </div>
            <div class="w-56 shrink-0">${autonomyDial({ scope: "mission", scopeId: m.missionId }, m.dial)}</div>
          </div>`).join("")}
        </div>` : ""}
      </div>
      <div class="space-y-3">
        <p class="text-sm font-semibold">${t("trust.spend")} <span class="text-muted font-normal text-xs">— ${t("trust.spendSub")}</span></p>
        <div class="rounded-xl border border-border p-4 space-y-4">
          <div class="flex items-center justify-between gap-4">
            <span class="text-sm font-medium">${t("trust.monthlyCeiling")}</span>
            <div class="flex items-center gap-3 flex-1 justify-end">
              ${ceilingMeter(trust.globalSpendCents, trust.globalCeilingCents)}
              <input id="trust-global-ceiling" value="${((trust.globalCeilingCents || 0) / 100).toFixed(2)}" class="w-20 rounded-lg border border-border bg-white px-2 py-1 text-sm font-mono text-right focus:outline-none focus:ring-2 focus:ring-primary/30">
              ${btn({ label: t("ajustes.save"), variant: "secondary", size: "sm", onClick: "RC.saveGlobalCeiling()" })}
            </div>
          </div>
          ${trust.lastRun ? `<p class="text-xs text-muted font-mono">${t("trust.lastRun")}: ${fmtCents(trust.lastRun.spendCents)} ${t("trust.of")} ${trust.lastRun.ceilingCents !== null ? fmtCents(trust.lastRun.ceilingCents) : t("trust.unset")} · <span class="font-mono">${esc(trust.lastRun.runId)}</span></p>` : ""}
        </div>
      </div>
      <div class="space-y-3">
        <p class="text-sm font-semibold">${t("trust.targets")}</p>
        <div class="rounded-xl border border-border divide-y divide-border overflow-hidden">
          ${(app.data.targets || []).map((tg) => `
          <div class="px-4 py-3 flex items-center justify-between gap-4 bg-white">
            <div class="min-w-0">
              <p class="text-sm font-medium font-mono">${esc(tg.name)} <span class="text-[10px] text-muted font-sans">${esc(tg.kind)}${tg.host ? " · " + esc(tg.host) : ""}</span></p>
              ${tg.allowlisted === false ? `<p class="text-[11px] text-amber-700 mt-0.5">${t("trust.notAllowlisted")}</p>` : ""}
            </div>
            ${(() => {
              const tm = (trust.targets || []).find((x) => x.target === tg.name);
              if (!tm) return `<span class="text-xs text-muted">${t("trust.unset")}</span>`;
              return `<div class="w-56 shrink-0">${autonomyDial({ scope: "target", scopeId: tg.name }, tm.dial || "watch")}</div>`;
            })()}
          </div>`).join("") || `<p class="px-4 py-6 text-sm text-muted text-center">${t("trust.noSshTargets")}</p>`}
        </div>
        <p class="text-[11px] text-muted">${t("trust.targetsFootnote")}</p>
      </div>
    </div>`;
}

export function bindSettings(app) {}

// ========== HANDLERS ==========
Object.assign(RC, {
  openRefDetail,
  closeRefDetail() {
    document.querySelectorAll(".rc-modal").forEach((el) => el.remove());
  },
  runReview() {
    const app = ctx.app;
    app.data.reviewRunning = true;
    ctx.renderMainOnly();
    setTimeout(() => {
      app.data.reviewRunning = false;
      ctx.renderMainOnly();
    }, 4200);
  },
  async sendChatMessage() {
    const app = ctx.app;
    const input = document.getElementById("chat-input");
    const text = input ? input.value.trim() : "";
    if (!text) return;
    if (!app.data.activeChatId) {
      try {
        const chat = await api.createChat(app.data.project?.id || "p1", "asistente", text.slice(0, 46));
        app.data.activeChatId = chat.id;
        app.data.chatMessages[chat.id] = [];
        app.data.chats.unshift(chat);
      } catch (e) {
        console.error(e);
      }
    }
    const chatId = app.data.activeChatId;
    const list = app.data.chatMessages[chatId] || (app.data.chatMessages[chatId] = []);
    list.push({ id: "m" + Date.now(), role: "user", content: text });
    input.value = "";
    app.data.chatThinking = true;
    ctx.renderMainOnly();
    try {
      await api.sendMessage(chatId, text);
      const fresh = await api.getChat(chatId);
      app.data.chatMessages[chatId] = fresh.messages || [];
    } catch (e) {
      console.error(e);
      list.push({ id: "m" + Date.now() + "x", role: "agent", content: "(" + (e?.message || e) + ")" });
    }
    app.data.chatThinking = false;
    ctx.renderMainOnly();
  },
  fillChatSuggestion(q) {
    const input = document.getElementById("chat-input");
    if (!input) return;
    input.value = q;
    RC.sendChatMessage();
  },
  async openChatHistory() {
    const app = ctx.app;
    try {
      app.data.chats = await api.listChats(app.data.project?.id || "p1", "asistente");
    } catch (e) {
      console.error(e);
    }
    app.data.chatHistoryOpen = true;
    const overlay = document.createElement("div");
    overlay.className = "rc-modal fixed inset-0 z-[60] flex justify-end";
    const chats = app.data.chats;
    overlay.innerHTML = `
      <div class="absolute inset-0 bg-black/30 backdrop-blur-sm" onclick="RC.closeChatHistory()"></div>
      <div class="relative w-full max-w-md h-full bg-white border-l border-border shadow-xl p-6 overflow-y-auto animate-slide-right">
        <div class="flex items-center justify-between mb-5">
          <h2 class="font-serif text-2xl italic">${t("rc.assistant.history")}</h2>
          <button onclick="RC.closeChatHistory()" class="p-1 rounded hover:bg-gray-100">${icon("close", "w-5 h-5")}</button>
        </div>
        ${chats.length ? `
        <div class="space-y-1">
          ${chats.map((c) => `
          <button onclick="RC.loadChat('${esc(c.id)}')" class="w-full flex items-center gap-3 rounded-lg px-3 py-2.5 text-left transition ${c.id === app.data.activeChatId ? "bg-primary/10" : "hover:bg-gray-50"}">
            ${icon("message", `w-4 h-4 shrink-0 ${c.id === app.data.activeChatId ? "text-primary" : "text-muted"}`)}
            <span class="min-w-0 flex-1">
              <span class="block text-sm font-medium truncate ${c.id === app.data.activeChatId ? "text-primary" : "text-foreground"}">${esc(c.title)}</span>
              <span class="block text-xs text-muted mt-0.5 truncate">${esc(c.preview || "")}</span>
            </span>
          </button>`).join("")}
        </div>` : `<p class="text-sm text-muted text-center py-8">${t("rc.assistant.emptyChats")}</p>`}
      </div>`;
    document.body.appendChild(overlay);
  },
  closeChatHistory() {
    document.querySelectorAll(".rc-modal").forEach((el) => el.remove());
  },
  async loadChat(id) {
    const app = ctx.app;
    try {
      const fresh = await api.getChat(id);
      app.data.chatMessages[id] = fresh.messages || [];
      app.data.activeChatId = id;
    } catch (e) {
      console.error(e);
    }
    RC.closeChatHistory();
    ctx.renderMainOnly();
  },
  toggleActionStatusFilter(st) {
    const app = ctx.app;
    const f = app.data.actionFilters || (app.data.actionFilters = { status: [...ACTION_STATUSES], search: "" });
    const arr = f.status;
    if (arr.includes(st)) arr.splice(arr.indexOf(st), 1);
    else arr.push(st);
    ctx.renderMainOnly();
  },
  async runNightShift() {
    const app = ctx.app;
    app.data.digestRunning = true;
    ctx.renderMainOnly();
    try {
      app.data.digest = await api.runNightShiftNow();
    } catch (e) {
      alert(t("digest.loadError") + (e?.message || e));
    }
    app.data.digestRunning = false;
    ctx.renderMainOnly();
  },
  async toggleMcp(id) {
    const app = ctx.app;
    const s = app.data.mcp.find((x) => x.id === id);
    if (!s) return;
    try {
      if (s.connected) await api.updateMcpServer({ ...s, connected: 0 });
      else await api.updateMcpServer({ ...s, connected: 1 });
      s.connected = s.connected ? 0 : 1;
    } catch (e) {
      s.connected = s.connected ? 0 : 1;
    }
    ctx.renderMainOnly();
  },
  setSettingsTab(tab) {
    ctx.app.state.settingsTab = tab;
    ctx.renderMainOnly();
  },
  resetWizard() {
    localStorage.removeItem("rc-onboarding");
    ctx.app.state.onboardingCompleted = false;
    ctx.app.state.wizard.step = 0;
    ctx.app.state.wizard.maxStepReached = 0;
    ctx.app.state.view = "wizard";
    ctx.render();
  },
  async confirmKill() {
    const app = ctx.app;
    if (!window.confirm(t("trust.confirmStopDesc"))) return;
    try {
      app.data.trust = await api.killRuntime();
      ctx.renderMainOnly();
    } catch (e) {
      alert(t("trust.saveError") + (e?.message || e));
    }
  },
  async resumeRuntime() {
    const app = ctx.app;
    try {
      app.data.trust = await api.resumeRuntime();
      ctx.renderMainOnly();
    } catch (e) {
      alert(t("trust.saveError") + (e?.message || e));
    }
  },
  async dial(scope, scopeId, mode) {
    const app = ctx.app;
    try {
      app.data.trust = await api.configureAutonomy(scope, scopeId || null, mode);
      ctx.renderMainOnly();
    } catch (e) {
      alert(t("trust.saveError") + (e?.message || e));
    }
  },
  async saveGlobalCeiling() {
    const app = ctx.app;
    const v = parseFloat(document.getElementById("trust-global-ceiling")?.value || "0");
    try {
      app.data.trust = await api.configureCeiling("global", null, Math.round((isNaN(v) ? 0 : v) * 100));
      ctx.renderMainOnly();
    } catch (e) {
      alert(t("trust.saveError") + (e?.message || e));
    }
  },
});
