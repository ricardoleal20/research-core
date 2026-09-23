// The bible's continuity screens rendered as the live app: references
// (the migrated library via listRefs), AI review and actions (the bible's
// demo feel), the assistant (bible chat visuals over the mock provider),
// status (real MCP servers + trust connections), the morning digest (the
// real 90-second fold), and settings — including the v1 trust center tab
// (autonomy dials, hard ceilings, kill switch, compute targets) over the
// real getTrustStatus read model.
import { t, getLang } from "../i18n";
import { api } from "../api";
import { mockActive } from "../mock-backend";
import { icon, esc, badge, btn, card, rcSelect, pageHeader, fmtCents, fmtTs, demoChip } from "./helpers";
import { RC, ctx } from "./rc";

// ========== REFERENCES ==========
const refSource = (r) =>
  r.source === "arxiv" || r.source === "zotero" || r.source === "manual"
    ? { arxiv: "arXiv", zotero: "Zotero", manual: "Manual" }[r.source]
    : (r.doi || "").startsWith("10.48550/arXiv.")
      ? "arXiv"
      : r.attachment
        ? "Zotero"
        : "Manual";
const refSourceColor = { arXiv: "primary", Zotero: "warning", "Semantic Scholar": "success", Manual: "muted", MCP: "medium" };
const refReviewed = (r) => r.status === "read" || r.status === "reviewed";

export function renderRefs(app) {
  const filter = (app.data.refFilter || "").toLowerCase();
  const statusFilter = app.data.refStatusFilter || "all";
  const all = app.data.refs;
  const rows = all.filter(
    (r) =>
      (statusFilter === "all" || (statusFilter === "removed" ? r.removed : !r.removed)) &&
      (r.title.toLowerCase().includes(filter) || (r.authors || "").toLowerCase().includes(filter)),
  );
  const archivedCount = all.filter((r) => r.removed).length;
  const chip = (key, label, count) => `
    <button onclick="RC.setRefStatusFilter('${key}')" class="rounded-full px-3 py-1 text-xs font-medium ring-1 ring-inset transition ${statusFilter === key ? "bg-primary/10 text-primary ring-primary/20" : "bg-white text-muted ring-border hover:bg-gray-50 hover:text-foreground"}">
      ${label}${key === "removed" && count ? ` (${count})` : ""}
    </button>`;
  return `
    <div class="space-y-6">
      ${pageHeader(t("rc.refs.title"), t("rc.refs.title"), btn({ label: t("rc.refs.add"), variant: "default", iconName: "plus", onClick: "RC.openRefAdd()" }))}
      <div class="flex flex-col sm:flex-row gap-3">
        <div class="relative flex-1">
          ${icon("search", "w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-muted")}
          <input id="ref-search" value="${esc(app.data.refFilter || "")}" placeholder="${t("rc.refs.search")}" class="w-full rounded-lg border border-border bg-white pl-9 pr-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30">
        </div>
        <div class="flex items-center gap-2">
          ${chip("all", t("rc.refs.filter.all"))}
          ${chip("active", t("rc.refs.filter.active"))}
          ${chip("removed", t("rc.refs.filter.archived"), archivedCount)}
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
                <tr onclick="RC.openRefDetail('${esc(r.id)}')" class="hover-row cursor-pointer ${r.removed ? "opacity-60" : ""}">
                  <td class="px-6 py-4 font-medium ${r.removed ? "text-muted line-through" : "text-foreground"}">${esc(r.title)}</td>
                  <td class="px-6 py-4 text-muted">${esc(r.authors)}</td>
                  <td class="px-6 py-4">${esc(r.year ?? "")}</td>
                  <td class="px-6 py-4">${badge(refSource(r), refSourceColor[refSource(r)] || "muted")}</td>
                  <td class="px-6 py-4"><div class="flex flex-wrap gap-1">${(r.tags || "").split(",").filter(Boolean).map((tag) => badge(tag.trim(), "muted")).join("")}</div></td>
                  <td class="px-6 py-4">${r.removed ? badge(t("rc.refs.removedBadge"), "destructive") : badge(refReviewed(r) ? t("rc.refs.reviewed") : t("rc.refs.toReview"), refReviewed(r) ? "success" : "warning")}</td>
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
          <p class="font-medium text-lg leading-snug ${r.removed ? "text-muted line-through" : ""}">${esc(r.title)}</p>
          ${r.removed ? `<div class="mt-1.5">${badge(t("rc.refs.removedBadge"), "destructive")}</div>` : ""}
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
        <div>
          <p class="text-xs text-muted uppercase tracking-wider mb-2">${t("rc.refs.timeline")}</p>
          ${(r.timeline || []).length
            ? `<div class="space-y-1.5 font-mono text-[11px] text-muted tabular">${r.timeline.map((e) => `
              <p>#${e.seq} ${fmtTs(e.ts)} · ${esc(e.actor)} · <span class="text-foreground">${esc(e.kind)}</span></p>`).join("")}</div>`
            : `<p class="text-xs text-muted">${t("rc.refs.timelineEmpty")}</p>`}
        </div>
        <div class="pt-2 border-t border-border space-y-2">
          ${r.removed
            ? `<div class="flex gap-2">${btn({ label: t("rc.refs.restore"), variant: "default", iconName: "history", onClick: `RC.restoreRef('${esc(r.id)}')` })}</div>`
            : app.state.refConfirmingRemove === r.id
              ? `<p class="text-xs text-rose-700 leading-relaxed">${t("rc.refs.confirmRemove")}</p>
                 <div class="flex gap-2">
                   ${btn({ label: t("rc.common.cancel"), variant: "ghost", onClick: "RC.cancelRemoveRef()" })}
                   ${btn({ label: t("rc.refs.confirmBtn"), variant: "destructive", iconName: "danger", onClick: `RC.removeRef('${esc(r.id)}')` })}
                 </div>`
              : `<div class="flex gap-2">${btn({ label: t("rc.refs.remove"), variant: "destructive", iconName: "trash", onClick: `RC.confirmRemoveRef('${esc(r.id)}')` })}</div>`}
        </div>
      </div>
    </div>`;
  document.body.appendChild(overlay);
}

// ---- The add composer (FR-15.1/15.2/15.3): the bible's slide-over drawer
// (the openRefDetail idiom) with three modes — arXiv paste (the shared
// fetch adapter behind the onboarding flow), manual entry, and the Zotero
// connect/import.
function openRefAdd() {
  const app = ctx.app;
  app.state.refAdd = { mode: "arxiv", saving: false, error: null, zoteroResult: null };
  RCRefAddDrawer(app);
}

function RCRefAddDrawer(app) {
  const f = app.state.refAdd;
  if (!f) return;
  document.querySelectorAll(".rc-modal").forEach((el) => el.remove());
  const overlay = document.createElement("div");
  overlay.className = "rc-modal fixed inset-0 z-[60] flex justify-end";
  const tab = (key, label) => `
    <button onclick="RC.setRefAddMode('${key}')" class="rounded-md py-1.5 px-3 text-xs font-medium transition ${f.mode === key ? "bg-white shadow-sm text-foreground" : "text-muted hover:text-foreground"}">${label}</button>`;
  const v = (id) => f.values?.[id] ?? "";
  const field = (id, label, ph, value = "") => `
    <div><label class="block text-sm font-medium mb-1.5">${label}</label>
    <input id="${id}" value="${esc(value)}" placeholder="${esc(ph)}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"></div>`;
  const body =
    f.mode === "arxiv"
      ? `<div class="space-y-4">
          ${field("ref-arxiv-url", t("rc.refs.arxiv.label"), t("rc.refs.arxiv.ph"), v("ref-arxiv-url"))}
          <p class="text-xs text-muted">${t("rc.refs.arxiv.help")}</p>
        </div>`
      : f.mode === "manual"
        ? `<div class="space-y-4">
            ${field("ref-manual-title", t("rc.refs.f.title"), t("rc.refs.f.titlePh"), v("ref-manual-title"))}
            ${field("ref-manual-authors", t("rc.refs.f.authors"), "Vaswani et al.", v("ref-manual-authors"))}
            <div class="grid grid-cols-2 gap-4">
              ${field("ref-manual-year", t("rc.refs.f.year"), "2017", v("ref-manual-year"))}
              ${field("ref-manual-venue", t("rc.refs.f.venue"), "NeurIPS", v("ref-manual-venue"))}
            </div>
            <div class="grid grid-cols-2 gap-4">
              ${field("ref-manual-doi", t("rc.refs.f.doi"), "10.48550/arXiv.1706.03762", v("ref-manual-doi"))}
              ${field("ref-manual-url", t("rc.refs.f.url"), "https://arxiv.org/abs/1706.03762", v("ref-manual-url"))}
            </div>
            ${field("ref-manual-tags", t("rc.refs.f.tags"), t("rc.refs.f.tagsPh"), v("ref-manual-tags"))}
            <p class="text-xs text-muted">${t("rc.refs.f.identifierHint")}</p>
          </div>`
        : `<div class="space-y-4">
            <p class="text-sm text-muted leading-relaxed">${t("rc.refs.zotero.desc")}</p>
            ${f.zoteroResult ? `
              <div class="rounded-lg border border-border p-4 space-y-2">
                <div class="flex flex-wrap gap-2">
                  ${badge(`${f.zoteroResult.imported} ${t("rc.refs.zotero.imported")}`, f.zoteroResult.imported ? "success" : "muted")}
                  ${badge(`${f.zoteroResult.skipped} ${t("rc.refs.zotero.skipped")}`, f.zoteroResult.skipped ? "warning" : "muted")}
                  ${f.zoteroResult.failed ? badge(`${f.zoteroResult.failed} ${t("rc.refs.zotero.failed")}`, "destructive") : ""}
                </div>
                ${(f.zoteroResult.skippedItems || []).map((s) => `<p class="text-xs text-muted">· ${esc(s)}</p>`).join("")}
                ${(f.zoteroResult.failedItems || []).map((s) => `<p class="text-xs text-rose-700">· ${esc(s)}</p>`).join("")}
                ${!f.zoteroResult.imported && !f.zoteroResult.skipped && !f.zoteroResult.failed ? `<p class="text-xs text-muted">${t("rc.refs.zotero.noneNew")}</p>` : ""}
              </div>` : ""}
          </div>`;
  const submit =
    f.mode === "arxiv"
      ? btn({ label: f.saving ? t("rc.refs.adding") : t("rc.refs.addBtn"), variant: "default", iconName: "plus", onClick: "RC.submitRefAddArxiv()", disabled: f.saving })
      : f.mode === "manual"
        ? btn({ label: f.saving ? t("rc.refs.adding") : t("rc.refs.addBtn"), variant: "default", iconName: "plus", onClick: "RC.submitRefAddManual()", disabled: f.saving })
        : btn({ label: f.saving ? t("rc.refs.zotero.importing") : t("rc.refs.zotero.connect"), variant: "default", iconName: "bolt", onClick: "RC.submitZoteroImport()", disabled: f.saving });
  overlay.innerHTML = `
    <div class="absolute inset-0 bg-black/30 backdrop-blur-sm" onclick="RC.closeRefDetail()"></div>
    <div class="relative w-full max-w-md h-full bg-white border-l border-border shadow-xl p-6 overflow-y-auto animate-[fadeUp_220ms_ease-out]">
      <div class="flex items-center justify-between mb-5">
        <h2 class="font-serif text-2xl italic">${t("rc.refs.addTitle")}</h2>
        <button onclick="RC.closeRefDetail()" class="p-1 rounded hover:bg-gray-100">${icon("close", "w-5 h-5")}</button>
      </div>
      <div class="grid grid-cols-3 gap-1 rounded-lg border border-border bg-gray-50 p-1 mb-5">
        ${tab("arxiv", t("rc.refs.mode.arxiv"))}
        ${tab("manual", t("rc.refs.mode.manual"))}
        ${tab("zotero", t("rc.refs.mode.zotero"))}
      </div>
      ${f.error ? `<div class="mb-4 rounded-lg bg-rose-50 border border-rose-200 px-4 py-3 text-xs text-rose-700 leading-relaxed break-words">${esc(f.error)}</div>` : ""}
      ${body}
      <div class="mt-6 flex justify-end gap-2">
        ${btn({ label: t("rc.common.cancel"), variant: "ghost", onClick: "RC.closeRefDetail()" })}
        ${submit}
      </div>
    </div>`;
  document.body.appendChild(overlay);
  const first = document.getElementById("ref-arxiv-url") || document.getElementById("ref-manual-title");
  if (first) first.focus();
}

// ========== AI REVIEW (the bible's demo feel) ==========
export function renderReview(app) {
  const en = getLang() === "en";
  const findings = [
    { type: "theme", title: en ? "Attention dominance" : "Dominio de mecanismos de atención", content: en ? "Most recent work prioritizes attention as the central primitive, but linear-complexity alternatives (Mamba) are emerging." : "La mayoría de los trabajos recientes priorizan la atención como primitiva central, pero emergen alternativas de complejidad lineal (Mamba).", citations: ["Vaswani et al. 2017", "Gu & Dao 2023"] },
    { type: "gap", title: en ? "Scaling analysis gap" : "Vacío de escalado", content: en ? "No empirical studies of scaling laws on moderate-size multilingual corpora were found." : "No se encontraron estudios empíricos sobre leyes de escalado en corpus multilingües de tamaño moderado.", citations: ["Kaplan et al. 2020"] },
    { type: "conflict", title: en ? "Pretraining vs. instructions" : "Preentrenamiento vs. instrucciones", content: en ? "Tension between bidirectional pretraining (BERT) and post-RLHF instruction learning." : "Tensión entre la eficacia del preentrenamiento bidireccional (BERT) y el aprendizaje por instrucciones post-RLHF.", citations: ["Devlin et al. 2019", "Ouyang et al. 2022"] },
  ];
  return `
    <div class="space-y-6">
      ${pageHeader(t("rc.review.title"), t("rc.review.title"))}
      <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
        ${card(`
          <div class="p-6 space-y-5">
            <h3 class="font-semibold">${t("rc.review.select")}</h3>
            <div class="space-y-2 max-h-64 overflow-y-auto pr-1">
              ${app.data.refs.filter((r) => !r.removed).map((r) => `
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
      <p class="mt-4 font-mono text-xs text-muted h-5">${t("rc.review.initializing")}</p>
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
// The scoped surface (Stories 5.4–5.6): a mission selector + skill selector
// in the header, the "context: M-XX · board sincronizado" chip on scoped
// conversations, and the composer's attachment chips.

/// The conversation's effective scope: the active chat's binding, or the
/// draft selection for a conversation not yet created.
function chatScope(app) {
  const chat = app.data.chats.find((c) => c.id === app.data.activeChatId);
  if (chat) return { missionId: chat.mission_id || null, skill: chat.skill || null, model: chat.model || null };
  const draft = app.data.assistantDraft || (app.data.assistantDraft = { missionId: null, skill: null, model: null });
  return { missionId: draft.missionId, skill: draft.skill, model: draft.model };
}

/// The active provider+model pair the header shows in mono (Story 5.9,
/// the status-card provider idiom): the conversation's chosen model, else
/// the provider's configured default; CLI bridges show "default · vía CLI".
function activeModelPair(app) {
  const ai = app.data.aiConfig;
  if (!ai) return null;
  const scope = chatScope(app);
  const isCli = ai.mode === "cli";
  const model = scope.model || ai.model || (isCli ? "default" : "");
  const provider = isCli ? `cli/${ai.cli}` : ai.provider || "";
  return { provider, model, isCli };
}

/// The attachment chips the composer renders: the active conversation's
/// folded attachments, or the pending picks of a conversation not yet sent.
function composerAttachments(app) {
  if (app.data.activeChatId) return app.data.chatAttachments[app.data.activeChatId] || [];
  return app.data.pendingAttachments || [];
}

function renderAttachmentChip(a, pending = false) {
  const flags = [];
  if (!pending && !a.included) flags.push(a.note || t("rc.assistant.attachUnsupported"));
  if (a.truncated) flags.push(t("rc.assistant.attachTruncated"));
  const key = pending ? a.name : a.id;
  return `
  <span class="inline-flex items-center gap-1.5 rounded-full border border-border bg-gray-50 pl-2.5 pr-1 py-1 text-xs text-foreground max-w-full" title="${esc((a.note || "") + (a.digest ? " · " + String(a.digest).slice(0, 16) : ""))}">
    ${icon("fileText", "w-3.5 h-3.5 text-muted shrink-0")}
    <span class="max-w-[160px] truncate font-medium">${esc(a.name)}</span>
    <span class="font-mono text-[10px] text-muted uppercase shrink-0">${esc(pending ? "…" : a.kind)}</span>
    ${flags.length ? `<span class="text-[10px] font-medium text-amber-600 shrink-0">${esc(flags.join(" · "))}</span>` : ""}
    <button type="button" onclick="RC.removeAttachment('${esc(key)}', ${pending})" class="rounded-full p-0.5 text-muted hover:text-foreground hover:bg-gray-200 transition shrink-0" aria-label="${t("rc.assistant.attachRemove")}">${icon("close", "w-3.5 h-3.5")}</button>
  </span>`;
}

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
  const scope = chatScope(app);
  const mission = scope.missionId ? app.data.missions.find((m) => m.id === scope.missionId) : null;
  const skill = scope.skill ? app.data.skills.find((s) => s.name === scope.skill) : null;
  // Story 5.7 (FR-17.1/NFR-11): with NO real provider configured the view
  // shows the explicit configure-provider state and sends are refused —
  // no fabricated replies on the assistant path, ever.
  const ai = app.data.aiConfig;
  // While the config read is still in flight the view stays neutral — the
  // configure state renders only from a LOADED, unconfigured state (and
  // the send path's typed refusal is the backstop either way).
  const unconfigured = ai ? !ai.configured : false;
  const pair = activeModelPair(app);
  // Story 5.9 (FR-17.4): the model picker lists the active provider's
  // models (curated; free entry for custom URLs; CLI = ["default"]). With
  // exactly one model the resolved pair renders read-only — no empty
  // dropdown.
  const modelOptions = ai ? ai.models.map((m) => ({
    value: m,
    label: ai.mode === "cli" ? `${m} · ${t("rc.assistant.viaCli")}` : m,
  })) : [];
  const activeModel = scope.model || (ai ? ai.model : "") || modelOptions[0]?.value || "";
  const modelPicker = unconfigured ? "" : (modelOptions.length > 1
    ? `<div class="w-40 hidden md:block">${rcSelect({ id: "chat-model-select", size: "sm", cls: "w-full", options: modelOptions, value: activeModel, onChange: "RC.setChatModel(this.value)" })}</div>`
    : (modelOptions.length === 0
      ? `<input id="chat-model-input" value="${esc(scope.model || "")}" onchange="RC.setChatModel(this.value)" placeholder="${t("rc.assistant.modelDefault")}" title="${t("prov.modelFreeEntry")}" class="hidden md:block w-40 rounded-lg border border-border bg-white px-2.5 py-1.5 text-xs font-mono focus:outline-none focus:ring-2 focus:ring-primary/30 transition">`
      : ""));
  const missionOptions = [
    { value: "", label: t("rc.assistant.scopeGeneral") },
    ...app.data.missions.map((m) => ({ value: m.id, label: `M-${m.seq} · ${m.question.slice(0, 46)}` })),
  ];
  const skillOptions = [
    { value: "", label: t("rc.assistant.skillNone") },
    ...app.data.skills.map((s) => ({ value: s.name, label: t(`rc.skill.${s.name}`) })),
  ];
  // The configure-provider state (FR-17.1): the aurora empty state
  // adapted, with a CTA straight to Ajustes → IA. No composer — sends are
  // refused until a real provider exists.
  if (unconfigured) {
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
      </div>
      <div class="relative flex-1 flex flex-col items-center justify-center max-w-3xl mx-auto w-full px-4 -mt-6 overflow-hidden">
        <div class="relative -mb-10 flex flex-col items-center w-full">
          <div class="pointer-events-none absolute -inset-x-16 -top-12 -bottom-16 z-0">
            <div class="rc-ai-aurora"></div>
            <div class="rc-ai-particle" style="left:12%; top:34%; animation-delay:0s"></div>
            <div class="rc-ai-particle" style="left:84%; top:52%; animation-delay:1.8s"></div>
            <div class="rc-ai-particle" style="left:20%; top:72%; animation-delay:3.2s"></div>
            <div class="rc-ai-particle" style="left:78%; top:22%; animation-delay:4.6s"></div>
          </div>
          <div class="relative z-10 flex h-11 w-11 items-center justify-center rounded-2xl bg-primary/10 text-primary mb-6 rc-intro">${icon("key", "w-5 h-5")}</div>
          <h2 class="relative z-10 font-serif text-5xl italic mb-4 text-center rc-intro rc-intro-1">${t("rc.assistant.needsProviderTitle")}</h2>
          <p class="relative z-10 body-lg text-muted text-center max-w-md mb-8 rc-intro rc-intro-2">${t("rc.assistant.needsProviderSub")}</p>
          <div class="relative z-10 rc-intro rc-intro-3">
            ${btn({ label: t("rc.assistant.needsProviderCta"), onClick: "RC.openSettingsAi()" })}
          </div>
        </div>
      </div>
    </div>`;
  }
  return `
    <div class="h-[calc(100vh-112px)] flex flex-col">
      <div class="flex items-center justify-between gap-3 mb-4">
        <div class="flex items-center gap-3 min-w-0">
          <div class="flex h-9 w-9 items-center justify-center rounded-xl bg-primary/10 text-primary shrink-0">${icon("sparkle", "w-4 h-4")}</div>
          <div class="min-w-0">
            <p class="caption text-muted">${t("rc.assistant.title")}</p>
            <span class="text-sm font-medium leading-snug truncate">${app.data.activeChatId && app.data.chats.find((c) => c.id === app.data.activeChatId) ? esc(app.data.chats.find((c) => c.id === app.data.activeChatId).title) : t("rc.assistant.newChatTitle")}</span>
            ${(mission || skill) ? `
            <span class="flex flex-wrap items-center gap-1.5 mt-1">
              ${mission ? `<span class="inline-flex items-center gap-1 rounded-full bg-primary/5 px-2 py-0.5 text-[11px] font-medium text-primary ring-1 ring-inset ring-primary/20">${t("rc.assistant.contextChip", { label: "M-" + mission.seq })}</span>` : ""}
              ${skill ? `<span class="inline-flex items-center gap-1 rounded-full bg-gray-100 px-2 py-0.5 text-[11px] font-medium text-muted ring-1 ring-inset ring-gray-500/10">${t("rc.assistant.skillChip", { name: t(`rc.skill.${skill.name}`) })}</span>` : ""}
            </span>` : ""}
            ${pair && pair.provider ? `<span class="font-mono text-[10px] text-muted mt-0.5">${esc(pair.provider)}${pair.model ? " · " + esc(pair.model) : ""}${pair.isCli ? " · " + t("rc.assistant.viaCli") : ""}</span>` : ""}
          </div>
        </div>
        <div class="flex items-center gap-2">
          ${modelPicker}
          <div class="w-48 hidden sm:block">${rcSelect({ id: "chat-mission-select", size: "sm", cls: "w-full", options: missionOptions, value: scope.missionId || "", onChange: "RC.setChatMission(this.value)" })}</div>
          <div class="w-36 hidden sm:block">${rcSelect({ id: "chat-skill-select", size: "sm", cls: "w-full", options: skillOptions, value: scope.skill || "", onChange: "RC.setChatSkill(this.value)" })}</div>
          <button onclick="RC.newChat()" title="${t("rc.assistant.newChat")}" class="p-2 rounded-lg border border-border bg-card text-muted hover:text-foreground hover:border-primary/40 transition ring-focus">${icon("plus", "w-5 h-5")}</button>
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
        <div class="relative z-10 w-full rc-intro rc-intro-4">${renderChatComposer(app)}</div>
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
        <div class="pt-3 pb-1">${renderChatComposer(app)}</div>
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
  // Story 5.7/5.9: the reply attributes its provider + model (the
  // pin-confidence attribution idiom, "GLM-5.3 · …") from its meta.
  let attribution = "";
  if (m.meta) {
    try {
      const meta = JSON.parse(m.meta);
      if (meta && meta.provider && meta.model) {
        attribution = `<span class="font-mono text-[10px] tabular text-muted mt-1">${esc(meta.provider)} · ${esc(meta.model)}</span>`;
      }
    } catch { /* meta is not attribution JSON — nothing to show */ }
  }
  return `
  <div class="flex justify-start gap-2${anim}">
    <div class="flex h-6 w-6 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary mt-0.5">${icon("book", "w-3 h-3")}</div>
    <div class="flex flex-col max-w-[82%]">
      <div class="rc-bubble-ai rounded-2xl rounded-bl-md bg-gray-100 text-foreground text-sm whitespace-pre-wrap break-words">${esc(m.content)}</div>
      ${attribution}
    </div>
  </div>`;
}

function renderChatComposer(app) {
  const attachments = composerAttachments(app);
  return `
  <div id="chat-composer" class="chat-composer relative rounded-2xl border border-border bg-card shadow-sm p-3 focus-within:border-primary/40 focus-within:ring-2 focus-within:ring-primary/30 transition">
    ${attachments.length ? `
    <div class="flex flex-wrap gap-1.5 px-1.5 pt-0.5 pb-2">
      ${attachments.map((a) => renderAttachmentChip(a, !app.data.activeChatId)).join("")}
    </div>` : ""}
    <textarea id="chat-input" rows="1" class="w-full resize-none bg-transparent px-1.5 py-1.5 text-[15px] leading-relaxed focus:outline-none max-h-40 placeholder:text-muted" placeholder="${t("rc.assistant.placeholder")}"></textarea>
    <div class="flex items-center justify-between mt-1.5">
      <div class="flex items-center gap-1">
        <input type="file" id="chat-file-input" multiple class="hidden" onchange="RC.handleAttachmentFiles(this)">
        <button id="chat-attach" type="button" onclick="RC.pickAttachments()" title="${t("rc.assistant.attach")}" class="p-2 rounded-lg text-muted hover:text-foreground hover:bg-gray-100 transition ring-focus" aria-label="${t("rc.assistant.attach")}">${icon("paperclip", "w-5 h-5")}</button>
      </div>
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
          <div class="mt-2 flex flex-col items-start gap-1">
            ${demoChip()}
            <p class="text-xs text-muted">${t("demo.actionsNote")}</p>
          </div>
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
  // Story 6.10 (FR-23.3): a sweep night's one-line verdict names the
  // support rollup — the unsupported count never buried.
  else if (r.supportChecks > 0)
    verdict = t("digest.verdict.support", { checked: r.supportChecks, unsupported: r.supportUnsupported });
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
                <td class="px-6 py-4 font-medium">${esc(s.name)}</td>
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
    { id: "bridge", label: t("bridge.title") },
    { id: "local", label: t("rc.settings.local") },
    { id: "manuscript", label: t("rc.settings.manuscript") },
    { id: "vault", label: t("rc.settings.vault") },
    { id: "danger", label: t("rc.settings.danger") },
  ];
  let content = "";
  if (activeTab === "interface") content = renderSettingsInterface(app);
  else if (activeTab === "ai") content = renderSettingsAi(app);
  else if (activeTab === "manuscript") content = renderSettingsManuscript(app);
  else if (activeTab === "trust") content = renderTrustCenter(app);
  else if (activeTab === "bridge") content = renderBridgeCenter(app);
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

// The bridge tab's data (Story 6.14): the status read + the paired list.
async function loadBridgeData() {
  const app = ctx.app;
  try {
    app.data.bridge = await api.bridgeStatus();
  } catch (e) {
    app.data.bridge = null;
  }
  try {
    app.data.bridgeDevices = await api.listBridgeDevices();
  } catch (e) {
    app.data.bridgeDevices = [];
  }
  ctx.renderMainOnly();
}

// The bridge settings surface (Story 6.14, FR-21.1, NFR-13): the ONE
// channel — off by default, one channel at a time, pairing as a user
// action with a visible paired-devices list. Data lands in app.data.bridge
// (status) + app.data.bridgeDevices (the paired list).
function renderBridgeCenter(app) {
  const st = app.data.bridge;
  const devices = app.data.bridgeDevices || [];
  const s = app.state.bridgeForm || (app.state.bridgeForm = { mode: "tunnel", addr: "", url: "", token: "", pairName: "", receipt: null });
  return `
    <div class="space-y-6">
      <div class="flex items-center justify-between rounded-xl border ${st?.active ? "border-emerald-200 bg-emerald-50" : "border-border bg-white"} p-4">
        <div class="min-w-0">
          <p class="text-sm font-semibold">${t("bridge.title")} <span class="text-muted font-normal text-xs">— ${t("bridge.sub")}</span></p>
          <p class="text-xs ${st?.active ? "text-emerald-700" : "text-muted"} mt-0.5 font-mono truncate">${st?.active ? `${t("bridge.active")} · ${esc(st.describe || st.mode)}` : t("bridge.inactive")}</p>
        </div>
        ${st?.active
          ? btn({ label: t("bridge.disable"), variant: "destructive", size: "sm", onClick: "RC.disableBridge()" })
          : ""}
      </div>
      ${app.data.bridgeError ? `<div class="rounded-lg border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-700 font-medium">${esc(app.data.bridgeError)}</div>` : ""}
      ${!st?.active ? `
      <div class="space-y-3">
        <p class="text-sm font-semibold">${t("bridge.mode")}</p>
        <div class="rounded-xl border border-border p-4 space-y-3">
          <div class="grid grid-cols-2 gap-1 rounded-lg border border-border bg-gray-50 p-1">
            ${[["tunnel", t("bridge.mode.tunnel")], ["chopflow", t("bridge.mode.chopflow")]].map(([v, l]) => `
              <button type="button" onclick="RC.bridgeMode('${v}')" class="rounded-md py-1.5 text-xs font-medium transition ${s.mode === v ? "bg-white shadow-sm text-foreground" : "text-muted hover:text-foreground"}">${l}</button>`).join("")}
          </div>
          ${s.mode === "tunnel" ? `
            <div><label class="block text-sm font-medium mb-1.5">${t("bridge.listenAddr")}</label><input id="bridge-addr" value="${esc(s.addr)}" placeholder="0.0.0.0:4762" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
          ` : `
            <div><label class="block text-sm font-medium mb-1.5">${t("bridge.chopflowUrl")}</label><input id="bridge-url" value="${esc(s.url)}" placeholder="https://chopflow.example" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
            <div><label class="block text-sm font-medium mb-1.5">${t("bridge.chopflowToken")}</label><input id="bridge-token" value="${esc(s.token)}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
          `}
          <div class="flex justify-end pt-1">
            ${btn({ label: t("bridge.enable"), variant: "default", size: "sm", onClick: "RC.enableBridge()" })}
          </div>
        </div>
      </div>` : ""}
      <div class="space-y-3">
        <p class="text-sm font-semibold">${t("bridge.paired")} <span class="font-mono text-xs text-muted font-normal">${devices.length}</span></p>
        <div class="rounded-xl border border-border divide-y divide-border overflow-hidden">
          ${devices.map((d) => `
            <div class="px-4 py-3 flex items-center justify-between gap-4 bg-white">
              <div class="min-w-0">
                <p class="text-sm font-medium truncate">${esc(d.device)}</p>
                <p class="text-[11px] text-muted font-mono mt-0.5">${esc(d.fingerprint)} · ${fmtTs(d.pairedTs)}</p>
              </div>
              ${btn({ label: t("bridge.unpair"), variant: "ghost", size: "sm", onClick: `RC.unpairBridgeDevice('${esc(d.device)}')` })}
            </div>`).join("") || `<p class="px-4 py-6 text-sm text-muted text-center">${t("bridge.none")}</p>`}
        </div>
        <div class="flex gap-2">
          <input id="bridge-pair-name" value="${esc(s.pairName)}" placeholder="${t("bridge.pairNamePh")}" class="flex-1 rounded-lg border border-border bg-white px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30">
          ${btn({ label: t("bridge.pair"), variant: "secondary", size: "sm", onClick: "RC.pairBridgeDevice()" })}
        </div>
        ${s.receipt ? `
          <div class="rounded-xl border border-emerald-200 bg-emerald-50 p-4 space-y-2">
            <p class="text-xs font-semibold text-emerald-700">${t("bridge.tokenOnce")}</p>
            <p class="font-mono text-xs bg-white rounded-lg border border-emerald-200 px-3 py-2 break-all select-all">${esc(s.receipt.token)}</p>
            <p class="text-[11px] text-emerald-700">${esc(s.receipt.device)} · ${esc(s.receipt.fingerprint)}</p>
          </div>` : ""}
      </div>
      ${st?.active && st.mode === "tunnel" ? `<p class="text-[11px] text-muted">${t("bridge.companionAt")} <span class="font-mono">${esc((st.describe || "").replace("tunnel ", ""))}/m</span></p>` : ""}
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

// Ajustes → Manuscrito (Story 6.6, FR-20.1): the registration surface —
// the repo IS the manuscript, so registration only REFERENCES a .tex
// project dir (picked on disk) for a mission. Once registered, the
// manuscript surface opens on the mission's board (progressive
// disclosure: the board itself never shows manuscript vocabulary until
// then, FR-1.4/FR-8.2).
function renderSettingsManuscript(app) {
  const missions = app.data.missions || [];
  const registered = app.data.manuscriptsList || [];
  const draft = app.state.msReg || (app.state.msReg = {
    missionId: missions[0]?.id || "",
    dir: "",
    mainFile: "main.tex",
    saving: false,
  });
  const regByMission = new Map(registered.map((m) => [m.missionId, m]));
  return `
    <div class="space-y-6">
      <div>
        <p class="text-sm font-medium">${t("ms.reg.title")}</p>
        <p class="text-xs text-muted mt-1">${t("ms.reg.sub")}</p>
      </div>
      ${missions.length ? `
      <div class="space-y-4">
        <div><label class="block text-sm font-medium mb-1.5">${t("ms.reg.mission")}</label>${rcSelect({ id: "ms-reg-mission", options: missions.map((m) => ({ value: m.id, label: `M-${m.seq} — ${m.question.slice(0, 60)}` })), value: draft.missionId || missions[0].id, onChange: "RC.msRegField('missionId', this.value)" })}</div>
        <div>
          <label class="block text-sm font-medium mb-1.5">${t("ms.reg.dir")}</label>
          <div class="flex gap-2">
            <input id="ms-reg-dir" value="${esc(draft.dir)}" oninput="RC.msRegField('dir', this.value)" placeholder="/Users/…/papers/stiff-systems" class="flex-1 rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30">
            ${btn({ label: t("ms.reg.pick"), variant: "secondary", size: "sm", onClick: "RC.msRegPickFolder()" })}
          </div>
        </div>
        <div><label class="block text-sm font-medium mb-1.5">${t("ms.reg.mainFile")}</label><input id="ms-reg-main" value="${esc(draft.mainFile)}" oninput="RC.msRegField('mainFile', this.value)" placeholder="main.tex" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
        <div class="flex justify-end">
          ${btn({ label: draft.saving ? t("ms.reg.registering") : t("ms.reg.register"), onClick: "RC.msRegSubmit()", disabled: draft.saving })}
        </div>
      </div>` : `<p class="text-sm text-muted">${t("ms.reg.noneMissions")}</p>`}
      <div class="pt-4 border-t border-border">
        <p class="caption text-muted mb-2">${t("ms.reg.registered")}</p>
        ${registered.length ? `
        <div class="space-y-1.5">
          ${registered.map((m) => {
            const mission = missions.find((x) => x.id === m.missionId);
            return `
            <div class="flex items-center justify-between gap-3 rounded-lg border border-border px-3 py-2">
              <div class="min-w-0">
                <p class="text-sm font-medium truncate">${mission ? esc(mission.question) : m.missionId}</p>
                <p class="font-mono text-[11px] text-muted truncate" title="${esc(m.dir)}">${esc(m.dir)} · ${esc(m.mainFile)}</p>
              </div>
              ${btn({ label: t("ms.reg.open"), variant: "secondary", size: "sm", onClick: `RC.openBoard('${esc(m.missionId)}')` })}
            </div>`;
          }).join("")}
        </div>` : `<p class="text-sm text-muted">${t("ms.reg.none")}</p>`}
      </div>
    </div>`;
}

// Ajustes → IA (Stories 5.7/5.8, FR-17.2/17.3): the provider-config
// section — API providers (OpenAI / Anthropic / Google / OpenRouter /
// custom base URL + key stored via the keychain) with a test-connection
// run that doubles as the live model list, and CLI bridge providers
// (codex, claude) with honest detection chips. Never the key itself —
// only its presence (NFR-10).
function renderSettingsAi(app) {
  const ai = app.data.aiConfig;
  if (!ai) return `<p class="text-sm text-muted py-8 text-center">${t("rc.common.loading")}</p>`;
  const draft = app.state.aiDraft || (app.state.aiDraft = {
    provider: ai.provider && ai.provider !== "openai-compatible" ? ai.provider : "openai",
    baseUrl: ai.baseUrl || "",
    model: ai.model || "",
    key: "",
  });
  const test = app.state.aiTest;
  const isCli = ai.mode === "cli";
  // The local provider row's state (Story 6.1, FR-24.1): the base URL draft
  // (settings-stored — a local URL is not a secret, no keychain), the
  // honest detection chip, and the model chips from the live list.
  const local = ai.local || { baseUrl: "http://localhost:11434", reachable: false, models: [], error: null };
  const localDraft = app.state.localDraft || (app.state.localDraft = {
    baseUrl: local.baseUrl || "http://localhost:11434",
    model: ai.mode === "local" ? ai.model || "" : "",
  });
  const localTest = app.state.localTest;
  const isLocalActive = ai.mode === "local";
  const localModelChips = (localTest && localTest.ok ? localTest.models : local.models) || [];
  const providerOptions = ["openai", "anthropic", "google", "openrouter", "custom"].map((p) => ({
    value: p,
    label: t(`prov.${p}`),
  }));
  const custom = draft.provider === "custom";
  const savedModels = test && test.ok ? test.models : [];
  const cliRow = (name) => {
    const detected = ai.cliAvailable && ai.cliAvailable[name];
    const active = isCli && ai.cli === name;
    return `
    <div class="flex items-center justify-between gap-3 rounded-lg border border-border p-4 ${active ? "ring-1 ring-primary/30" : ""}">
      <div class="min-w-0">
        <div class="flex items-center gap-2 flex-wrap">
          <span class="text-sm font-medium font-mono">${name}</span>
          ${detected ? badge(t("prov.cliPresent"), "success") : badge(t("prov.cliAbsent"), "destructive")}
          ${active ? badge(t("prov.active"), "primary") : ""}
        </div>
        <p class="text-xs text-muted mt-1 truncate" title="${detected ? esc(detected.path) : ""}">${detected ? esc(detected.path) : ""}</p>
      </div>
      ${detected ? btn({ label: t("prov.useCli"), variant: "secondary", size: "sm", onClick: `RC.useCliBridge('${name}')` }) : ""}
    </div>`;
  };
  return `
    <div class="space-y-6">
      <div class="flex items-center gap-2 flex-wrap">
        <span class="text-sm font-medium">${t("prov.title")}</span>
        <span class="font-mono text-xs text-muted">${isCli ? esc("cli/" + ai.cli) : esc(ai.provider || "—")}${ai.model ? " · " + esc(ai.model) : isCli ? " · default" : ""}</span>
        ${ai.configured ? badge(t("prov.active"), "success") : badge(t("prov.notConfigured"), "muted")}
      </div>

      <div>
        <p class="text-sm font-medium mb-2">${t("prov.apiSection")}</p>
        <div class="space-y-4">
          <div><label class="block text-sm font-medium mb-1.5">${t("prov.provider")}</label>${rcSelect({ id: "ai-provider-select", options: providerOptions, value: draft.provider, onChange: "RC.aiDraftProvider(this.value)" })}</div>
          ${custom ? `<div><label class="block text-sm font-medium mb-1.5">${t("prov.baseUrl")}</label><input value="${esc(draft.baseUrl)}" oninput="RC.aiDraftField('baseUrl', this.value)" placeholder="https://api.example.com/v1" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>` : ""}
          <div><label class="block text-sm font-medium mb-1.5">${t("prov.apiKey")}</label><input id="ai-key-input" type="password" value="${esc(draft.key)}" oninput="RC.aiDraftField('key', this.value)" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"><p class="text-xs text-muted mt-1">${t("prov.note")} ${t("prov.keyKeep")}</p></div>
          <div><label class="block text-sm font-medium mb-1.5">${t("prov.model")} ${custom ? `<span class="text-xs text-muted font-normal">(${t("prov.modelFreeEntry")})</span>` : ""}</label><input value="${esc(draft.model)}" oninput="RC.aiDraftField('model', this.value)" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
          ${savedModels.length ? `<div class="flex flex-wrap gap-1.5">${savedModels.map((m) => `<button type="button" onclick="RC.aiPickModel('${esc(m)}')" class="rounded-full border border-border bg-card px-3 py-1 text-xs font-mono text-muted hover:border-primary/40 hover:text-primary transition">${esc(m)}</button>`).join("")}</div>` : ""}
          <div class="flex flex-wrap items-center gap-2">
            ${btn({ label: t("prov.save"), onClick: "RC.saveAiProvider()" })}
            ${btn({ label: test && test.testing ? t("prov.testing") : t("prov.test"), variant: "secondary", onClick: "RC.testAiConnection()" })}
          </div>
          ${test ? (test.ok
            ? `<p class="text-xs font-medium text-emerald-600">${t("prov.testOk")} — ${test.models.length} models</p>`
            : `<p class="text-xs font-medium text-rose-600">${t("prov.testFail")}: ${esc(test.error || "")}</p>`) : ""}
        </div>
      </div>

      <div>
        <p class="text-sm font-medium mb-1">${t("prov.localSection")}</p>
        <p class="text-xs text-muted mb-2">${t("prov.localDesc")}</p>
        <div class="space-y-3 rounded-xl border border-border p-4 ${isLocalActive ? "ring-1 ring-primary/30" : ""}">
          <div class="flex items-center gap-2 flex-wrap">
            <span class="text-sm font-medium font-mono">local</span>
            ${local.reachable ? badge(t("prov.localReachable"), "success") : badge(t("prov.localUnreachable"), "destructive")}
            ${isLocalActive ? badge(t("prov.active"), "primary") : ""}
            ${local.reachable ? `<span class="text-xs text-muted">${t("prov.localModels", { n: local.models.length })}</span>` : ""}
          </div>
          ${local.error ? `<p class="text-xs text-rose-600 break-words">${esc(local.error)}</p>` : ""}
          <div><label class="block text-sm font-medium mb-1.5">${t("prov.baseUrl")}</label><input value="${esc(localDraft.baseUrl)}" oninput="RC.localDraftField('baseUrl', this.value)" placeholder="http://localhost:11434" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
          ${localModelChips.length ? `<div><p class="text-xs text-muted mb-1.5">${t("prov.localPickModel")}</p><div class="flex flex-wrap gap-1.5">${localModelChips.map((m) => `<button type="button" onclick="RC.pickLocalModel('${esc(m)}')" class="rounded-full border px-3 py-1 text-xs font-mono transition ${localDraft.model === m ? "border-primary/40 bg-primary/5 text-primary" : "border-border bg-card text-muted hover:border-primary/40 hover:text-primary"}">${esc(m)}</button>`).join("")}</div></div>` : ""}
          <div class="flex flex-wrap items-center gap-2">
            ${btn({ label: t("prov.localUse"), onClick: "RC.useLocalProvider()" })}
            ${btn({ label: localTest && localTest.testing ? t("prov.testing") : t("prov.test"), variant: "secondary", onClick: "RC.testLocalProvider()" })}
          </div>
          ${localTest && !localTest.testing ? (localTest.ok
            ? `<p class="text-xs font-medium text-emerald-600">${t("prov.testOk")} — ${t("prov.localModels", { n: localTest.models.length })}</p>`
            : `<p class="text-xs font-medium text-rose-600">${t("prov.testFail")}: ${esc(localTest.error || "")}</p>`) : ""}
        </div>
      </div>

      <div>
        <p class="text-sm font-medium mb-1">${t("prov.cliSection")}</p>
        <p class="text-xs text-muted mb-2">${t("prov.cliDesc")}</p>
        <div class="space-y-2">
          ${cliRow("codex")}
          ${cliRow("claude")}
        </div>
      </div>
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

// ---------------------------------------------------------------------------
// Compute targets (Stories 3.3 + 6.2–6.5): the declare form, the
// allowlist editor, the per-row probe state, and the autonomy dials —
// the settings row for every registered kind.
// ---------------------------------------------------------------------------

/// The config fields each first-party kind declares (key → i18n label).
const TARGET_CONFIG_FIELDS = {
  scheduler: ["flavor", "submitPrefix", "pollPrefix", "acctPrefix"],
  kubernetes: ["context", "namespace", "image", "kubectlPrefix"],
  chopflow: ["endpoint", "queue"],
};

function targetKindChip(app, adapter) {
  const form = app.state.targetForm;
  const selected = form && form.kind === adapter.kind;
  return `<button type="button" onclick="RC.setTargetKind('${esc(adapter.kind)}')" class="rounded-full border px-2.5 py-1 text-[11px] font-medium font-mono transition ${selected ? "border-primary/30 bg-primary/5 text-primary" : "border-border bg-white text-muted hover:text-foreground"}">${esc(adapter.kind)}</button>`;
}

function configFieldsFor(app, kind) {
  const form = app.state.targetForm;
  const fields = TARGET_CONFIG_FIELDS[kind];
  if (!fields || !fields.length) return "";
  return `
    <div class="grid grid-cols-1 sm:grid-cols-2 gap-2 mt-2">
      ${fields.map((key) => {
        const value = (form.config && form.config[key]) || "";
        const required = (kind === "kubernetes" && (key === "context" || key === "image")) || (kind === "chopflow" && key === "endpoint");
        return `<div><label class="block text-[11px] text-muted mb-1 font-mono">${esc(key)}${required ? " *" : ""}</label>
        <input id="tgt-cfg-${esc(key)}" value="${esc(value)}" placeholder="${esc(key)}" class="w-full rounded-lg border border-border bg-white px-2.5 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"></div>`;
      }).join("")}
    </div>`;
}

function probeChip(probe) {
  if (!probe || probe.status === "probing") {
    return `<span class="text-[11px] text-muted font-mono">${probe ? "…" : ""}</span>`;
  }
  const tone = probe.status === "ok" ? "text-emerald-700" : probe.status === "unreachable" ? "text-amber-700" : "text-muted";
  return `<span class="text-[11px] font-mono ${tone}" title="${esc(probe.detail)}">${esc(probe.status)}<span class="text-muted">${probe.detail ? " · " + esc(probe.detail.slice(0, 48)) : ""}</span></span>`;
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
        ${(() => {
          const form = (app.state.targetForm ??= { kind: "ssh", name: "", host: "", config: {}, error: null });
          const hasDeclareFields = form.kind !== "local";
          return `
          <div class="rounded-xl border border-border p-4 space-y-3 bg-white">
            <p class="text-xs font-medium">${t("trust.declare")}</p>
            <div class="flex flex-wrap gap-1.5">
              ${(app.data.adapters || []).map((a) => targetKindChip(app, a)).join("")}
            </div>
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
              <div>
                <label class="block text-[11px] text-muted mb-1">${t("trust.targetName")} *</label>
                <input id="tgt-name" value="${esc(form.name)}" placeholder="cluster-1" class="w-full rounded-lg border border-border bg-white px-2.5 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30">
              </div>
              ${form.kind === "ssh" || form.kind === "scheduler" ? `
              <div>
                <label class="block text-[11px] text-muted mb-1">${t("trust.host")}${form.kind === "ssh" ? " *" : ""}</label>
                <input id="tgt-host" value="${esc(form.host)}" placeholder="login.hpc.edu" class="w-full rounded-lg border border-border bg-white px-2.5 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30">
              </div>` : ""}
            </div>
            ${hasDeclareFields ? configFieldsFor(app, form.kind) : ""}
            ${form.kind && form.kind !== "local" && !TARGET_CONFIG_FIELDS[form.kind] ? `<p class="text-[11px] text-muted">${t("trust.noConfigFields")} <span class="font-mono">${esc(form.kind)}</span></p>` : ""}
            ${form.error ? `<p class="text-xs text-rose-600">${esc(form.error)}</p>` : ""}
            <div class="flex items-center gap-2">
              ${btn({ label: t("trust.add"), variant: "default", size: "sm", iconName: "plus", onClick: "RC.declareTarget()" })}
              ${btn({ label: t("trust.cancel"), variant: "ghost", size: "sm", onClick: "RC.resetTargetForm()" })}
            </div>
          </div>
          <div class="rounded-xl border border-border p-4 space-y-2 bg-white">
            <div class="flex items-center justify-between">
              <p class="text-xs font-medium">${t("trust.hostAllowlist")}</p>
              ${btn({ label: t("trust.hostAllowlistSave"), variant: "secondary", size: "sm", onClick: "RC.saveAllowlist()" })}
            </div>
            <p class="text-[11px] text-muted">${t("trust.hostAllowlistHint")}</p>
            <textarea id="tgt-allowlist" rows="3" placeholder="gpu-01.lab, lab-gpu, login.hpc.edu" class="w-full rounded-lg border border-border bg-white px-2.5 py-1.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30">${esc((app.data.allowlist || []).join(", "))}</textarea>
          </div>`;
        })()}
        <div class="rounded-xl border border-border divide-y divide-border overflow-hidden">
          ${(app.data.targets || []).map((tg) => `
          <div class="px-4 py-3 flex items-center justify-between gap-4 bg-white">
            <div class="min-w-0">
              <p class="text-sm font-medium font-mono">${esc(tg.name)} <span class="text-[10px] text-muted font-sans">${esc(tg.kind)}${tg.host ? " · " + esc(tg.host) : ""}</span></p>
              <p class="text-[11px] font-mono text-muted mt-0.5">${
                tg.config ? Object.entries(tg.config).map(([k, v]) => `${esc(k)}=${esc(v)}`).join(" ") : "—"
              }</p>
              ${tg.allowlisted === false ? `<p class="text-[11px] text-amber-700 mt-0.5">${t("trust.notAllowlisted")}</p>` : ""}
              ${probeChip(app.state.targetProbes && app.state.targetProbes[tg.name])}
            </div>
            <div class="flex items-center gap-2 shrink-0">
              ${btn({ label: t("trust.probe"), variant: "ghost", size: "sm", iconName: "activity", onClick: `RC.probeTarget('${esc(tg.name)}')` })}
              ${(() => {
              const tm = (trust.targets || []).find((x) => x.target === tg.name);
              if (!tm) return `<span class="text-xs text-muted">${t("trust.unset")}</span>`;
              return `<div class="w-56 shrink-0">${autonomyDial({ scope: "target", scopeId: tg.name }, tm.dial || "watch")}</div>`;
            })()}
            </div>
          </div>`).join("") || `<p class="px-4 py-6 text-sm text-muted text-center">${t("trust.noSshTargets")}</p>`}
        </div>
        <p class="text-[11px] text-muted">${t("trust.targetsFootnote")}</p>
      </div>
    </div>`;
}

export function bindSettings(app) {}

// ========== OPEN EXPORT COMPOSER (Story 3.1, FR-7.1/7.2) ==========
// The header's Exportar action: scope picker + target folder, the staleness
// warning for a previous export at that folder staled by a rollback, and the
// result manifest — over the export_workspace / inspect_export commands.
export function renderExportModal(app) {
  const s = app.state.exportComposer;
  const overlay = document.createElement("div");
  overlay.className = "rc-modal fixed inset-0 z-[60] flex items-center justify-center p-6";
  const scopes = ["all", "missions", "hypotheses", "evidence", "timeline", "search_log"];
  overlay.innerHTML = `
    <div class="absolute inset-0 bg-black/30 backdrop-blur-sm" onclick="RC.closeExport()"></div>
    <div class="relative w-full max-w-lg bg-white border border-border rounded-xl shadow-xl overflow-hidden animate-scale-in">
      <div class="flex items-center justify-between px-5 py-4 border-b border-border">
        <h2 class="font-serif text-2xl italic">${t("ex.title")}</h2>
        <button onclick="RC.closeExport()" class="p-1 rounded hover:bg-gray-100">${icon("close", "w-5 h-5")}</button>
      </div>
      ${s.result ? renderExportResult(s.result) : `
      <div class="p-5 space-y-4">
        <p class="text-xs text-muted leading-relaxed">${t("ex.sub")}</p>
        <div>
          <label class="block text-sm font-medium mb-1.5">${t("ex.scope")}</label>
          ${rcSelect({ id: "ex-scope", options: scopes.map((sc) => ({ value: sc, label: t("ex.scope." + sc) })), value: s.scope, onChange: "RC.setExportScope(this.value)" })}
        </div>
        <div>
          <label class="block text-sm font-medium mb-1.5">${t("ex.dir")}</label>
          <input id="ex-dir" value="${esc(s.dir)}" placeholder="${t("ex.dirPh")}" onchange="RC.inspectExportDir()" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30">
        </div>
        ${s.inspect === null ? "" : s.inspect.stale ? `
          <div class="rounded-lg border-amber-300 border-2 bg-amber-50 px-3 py-2 flex items-start gap-2">
            ${icon("danger", "w-4 h-4 text-amber-700 shrink-0 mt-0.5")}
            <p class="text-xs font-medium text-amber-700 leading-relaxed">${t("ex.stale", { seq: s.inspect.cutSeq })}</p>
          </div>` : `
          <div class="rounded-lg border border-border bg-gray-50 px-3 py-2 flex items-center gap-2">
            ${icon("check", "w-4 h-4 text-emerald-600 shrink-0")}
            <p class="text-xs text-muted">${t("ex.fresh", { seq: s.inspect.cutSeq })}</p>
          </div>`}
        ${btn({ label: s.running ? t("ex.running") : t("ex.run"), variant: "default", cls: "w-full", iconName: "fileText", onClick: "RC.runExport()", disabled: s.running || !s.dir.trim() })}
      </div>`}
    </div>`;
  document.body.appendChild(overlay);
}

function renderExportResult(r) {
  return `
    <div class="p-5 space-y-4">
      <div class="rounded-xl border border-emerald-200 bg-emerald-50 px-4 py-3 flex items-center gap-3">
        ${icon("check", "w-5 h-5 text-emerald-600 shrink-0")}
        <p class="text-sm font-medium text-emerald-700">${t("ex.result")}</p>
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div><p class="caption text-muted mb-0.5">${t("ex.scope")}</p><p class="text-sm font-medium">${t("ex.scope." + r.manifest.scope)}</p></div>
        <div><p class="caption text-muted mb-0.5">${t("ex.dir")}</p><p class="text-sm font-mono text-xs break-all">${esc(r.dir)}</p></div>
        <div><p class="caption text-muted mb-0.5">${t("ex.cut")}</p><p class="font-mono text-sm tabular">e-${r.manifest.cutSeq}</p></div>
        <div><p class="caption text-muted mb-0.5">${t("ex.rendered")}</p><p class="font-mono text-xs tabular">${fmtTs(r.manifest.renderedTs)}</p></div>
      </div>
      ${r.manifest.staleNotice ? `
        <div class="rounded-lg border-amber-300 border-2 bg-amber-50 px-3 py-2">
          <p class="text-xs font-medium text-amber-700">${t("ex.stale", { seq: r.manifest.staleNotice.previousCut })}</p>
        </div>` : ""}
      <div>
        <p class="caption text-muted mb-1.5">${t("ex.files")} · ${r.manifest.fileCount}</p>
        <div class="max-h-44 overflow-y-auto rounded-xl border border-border divide-y divide-border bg-white">
          ${r.files.map((f) => `
            <div class="px-3 py-1.5 font-mono text-[11px] text-muted break-all">${esc(f)}</div>`).join("")}
        </div>
      </div>
      ${btn({ label: t("ex.close"), variant: "secondary", cls: "w-full", onClick: "RC.closeExport()" })}
    </div>`;
}

// ========== HANDLERS ==========
Object.assign(RC, {
  // ---- Open export composer (Story 3.1, FR-7.1/7.2) ----
  openExport() {
    const app = ctx.app;
    app.state.exportOpen = true;
    app.state.exportComposer = { scope: "all", dir: "", inspect: null, result: null, running: false };
    ctx.render();
  },
  closeExport() {
    const app = ctx.app;
    app.state.exportOpen = false;
    app.state.exportComposer = null;
    ctx.render();
  },
  setExportScope(scope) {
    const s = ctx.app.state.exportComposer;
    if (!s) return;
    s.scope = scope;
  },
  async inspectExportDir() {
    const app = ctx.app;
    const s = app.state.exportComposer;
    if (!s) return;
    const dir = document.getElementById("ex-dir")?.value.trim() ?? s.dir;
    s.dir = dir;
    if (!dir) { s.inspect = null; ctx.render(); return; }
    try {
      s.inspect = await api.inspectExport(dir);
    } catch (e) {
      console.error(e);
      s.inspect = null;
    }
    ctx.render();
  },
  async runExport() {
    const app = ctx.app;
    const s = app.state.exportComposer;
    if (!s || s.running) return;
    const dir = document.getElementById("ex-dir")?.value.trim() || s.dir;
    if (!dir) return;
    s.dir = dir;
    s.running = true;
    s.error = null;
    ctx.render();
    try {
      s.result = await api.exportWorkspace(dir, s.scope);
    } catch (e) {
      alert(t("onb.error") + " " + (e?.message || e));
    }
    s.running = false;
    ctx.render();
  },
  openRefDetail,
  closeRefDetail() {
    document.querySelectorAll(".rc-modal").forEach((el) => el.remove());
    const app = ctx.app;
    if (app.state) {
      app.state.refAdd = null;
      app.state.refConfirmingRemove = null;
    }
  },
  // ---- Evented references CRUD (FR-15, Epic 5) ----
  setRefStatusFilter(filter) {
    const app = ctx.app;
    app.data.refStatusFilter = filter;
    ctx.renderMainOnly();
  },
  openRefAdd,
  setRefAddMode(mode) {
    const app = ctx.app;
    const f = app.state.refAdd;
    if (!f) return;
    f.mode = mode;
    f.error = null;
    f.zoteroResult = null;
    RCRefAddDrawer(app);
  },
  async submitRefAddArxiv() {
    const app = ctx.app;
    const f = app.state.refAdd;
    if (!f || f.saving) return;
    const url = document.getElementById("ref-arxiv-url")?.value.trim() || "";
    if (!url) return;
    f.values = { "ref-arxiv-url": url };
    f.saving = true;
    f.error = null;
    try {
      await api.addRefFromArxiv(url);
      RC.closeRefDetail();
      await ctx.loadRefs();
    } catch (e) {
      f.saving = false;
      f.error = (e?.message || String(e));
      RCRefAddDrawer(app);
    }
  },
  async submitRefAddManual() {
    const app = ctx.app;
    const f = app.state.refAdd;
    if (!f || f.saving) return;
    const read = (id) => document.getElementById(id)?.value.trim() ?? "";
    const values = {
      "ref-manual-title": read("ref-manual-title"),
      "ref-manual-authors": read("ref-manual-authors"),
      "ref-manual-year": read("ref-manual-year"),
      "ref-manual-venue": read("ref-manual-venue"),
      "ref-manual-doi": read("ref-manual-doi"),
      "ref-manual-url": read("ref-manual-url"),
      "ref-manual-tags": read("ref-manual-tags"),
    };
    if (!values["ref-manual-title"]) return;
    f.values = values;
    f.saving = true;
    f.error = null;
    try {
      await api.addRefManual(
        values["ref-manual-title"],
        values["ref-manual-authors"],
        values["ref-manual-year"] ? parseInt(values["ref-manual-year"], 10) : null,
        values["ref-manual-venue"],
        values["ref-manual-doi"],
        values["ref-manual-url"],
        values["ref-manual-tags"],
      );
      RC.closeRefDetail();
      await ctx.loadRefs();
    } catch (e) {
      f.saving = false;
      f.error = (e?.message || String(e));
      RCRefAddDrawer(app);
    }
  },
  async submitZoteroImport() {
    const app = ctx.app;
    const f = app.state.refAdd;
    if (!f || f.saving) return;
    f.saving = true;
    f.error = null;
    try {
      f.zoteroResult = await api.importRefsFromZotero();
      f.saving = false;
      await ctx.loadRefs();
      RCRefAddDrawer(app);
    } catch (e) {
      f.saving = false;
      f.error = (e?.message || String(e));
      RCRefAddDrawer(app);
    }
  },
  confirmRemoveRef(id) {
    const app = ctx.app;
    app.state.refConfirmingRemove = id;
    openRefDetail(id);
  },
  cancelRemoveRef() {
    const app = ctx.app;
    const id = app.state.refConfirmingRemove;
    app.state.refConfirmingRemove = null;
    if (id) openRefDetail(id);
  },
  async removeRef(id) {
    const app = ctx.app;
    try {
      await api.removeRef(id);
      app.state.refConfirmingRemove = null;
      await ctx.loadRefs();
      const fresh = app.data.refs.find((r) => r.id === id);
      if (fresh) openRefDetail(id);
      else RC.closeRefDetail();
    } catch (e) {
      alert(t("rc.refs.removeError") + (e?.message || e));
    }
  },
  async restoreRef(id) {
    const app = ctx.app;
    try {
      await api.restoreRef(id);
      await ctx.loadRefs();
      if (app.data.refs.some((r) => r.id === id)) openRefDetail(id);
      else RC.closeRefDetail();
    } catch (e) {
      alert(t("rc.refs.removeError") + (e?.message || e));
    }
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
        // A conversation is born with its draft scope: the mission binding
        // and skill selected in the header (Stories 5.4/5.6).
        const draft = app.data.assistantDraft || (app.data.assistantDraft = { missionId: null, skill: null, model: null });
        const chat = await api.createChat(app.data.project?.id || "p1", "asistente", text.slice(0, 46), draft.missionId, draft.skill, draft.model);
        app.data.activeChatId = chat.id;
        app.data.chatMessages[chat.id] = [];
        app.data.chats.unshift(chat);
        // pending attachments land on the conversation the moment it exists
        // (they belong to the conversation, not to the message — FR-16.1)
        const pending = app.data.pendingAttachments || [];
        if (pending.length) {
          app.data.pendingAttachments = [];
          await RC.addPickedAttachments(pending);
        }
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
      app.data.chatAttachments[chatId] = fresh.attachments || [];
    } catch (e) {
      console.error(e);
      const msg = e?.message || String(e);
      if (msg.startsWith("no_provider_configured")) {
        // The typed refusal (Story 5.7): refresh the honest config state —
        // the view flips to the configure-provider screen, never a
        // fabricated reply bubble.
        try { app.data.aiConfig = await api.getAiConfig(); } catch { /* keep the stale state */ }
      } else {
        list.push({ id: "m" + Date.now() + "x", role: "agent", content: "(" + msg + ")" });
      }
    }
    app.data.chatThinking = false;
    ctx.renderMainOnly();
  },
  // A new conversation: the active chat clears and the draft scope (the
  // header selectors) applies to the next send.
  newChat() {
    const app = ctx.app;
    app.data.activeChatId = null;
    app.data.pendingAttachments = [];
    (app.data.assistantDraft || (app.data.assistantDraft = { missionId: null, skill: null, model: null })).model = null;
    ctx.renderMainOnly();
  },
  // The mission selector (Story 5.4): re-scopes the active conversation
  // (an explicit, history-preserving event in the core) or sets the draft
  // scope of a conversation not yet created.
  async setChatMission(value) {
    const app = ctx.app;
    const missionId = value || null;
    if (app.data.activeChatId) {
      try {
        const chat = await api.setChatScope(app.data.activeChatId, missionId);
        const i = app.data.chats.findIndex((c) => c.id === chat.id);
        if (i >= 0) app.data.chats[i] = { ...app.data.chats[i], ...chat };
        else app.data.chats.unshift(chat);
      } catch (e) {
        alert(e?.message || e);
      }
    } else {
      (app.data.assistantDraft || (app.data.assistantDraft = { missionId: null, skill: null })).missionId = missionId;
    }
    ctx.renderMainOnly();
  },
  // The skill selector (Story 5.6): per-conversation, evented in the core.
  async setChatSkill(value) {
    const app = ctx.app;
    const skill = value || null;
    if (app.data.activeChatId) {
      try {
        const chat = await api.setChatSkill(app.data.activeChatId, skill);
        const i = app.data.chats.findIndex((c) => c.id === chat.id);
        if (i >= 0) app.data.chats[i] = { ...app.data.chats[i], ...chat };
        else app.data.chats.unshift(chat);
      } catch (e) {
        alert(e?.message || e);
      }
    } else {
      (app.data.assistantDraft || (app.data.assistantDraft = { missionId: null, skill: null })).skill = skill;
    }
    ctx.renderMainOnly();
  },
  // The model picker (Story 5.9, FR-17.4): per-conversation, evented in
  // the core — one chat.model_set event; null = the provider default.
  async setChatModel(value) {
    const app = ctx.app;
    const model = value || null;
    if (app.data.activeChatId) {
      try {
        const chat = await api.setChatModel(app.data.activeChatId, model);
        const i = app.data.chats.findIndex((c) => c.id === chat.id);
        if (i >= 0) app.data.chats[i] = { ...app.data.chats[i], ...chat };
        else app.data.chats.unshift(chat);
      } catch (e) {
        alert(e?.message || e);
      }
    } else {
      (app.data.assistantDraft || (app.data.assistantDraft = { missionId: null, skill: null, model: null })).model = model;
    }
    ctx.renderMainOnly();
  },
  // The configure-provider CTA (Story 5.7): straight to Ajustes → IA.
  openSettingsAi() {
    ctx.app.state.settingsTab = "ai";
    RC.navigate("settings");
  },
  // Ajustes → IA form state (Stories 5.7/5.8): transient draft inputs kept
  // in app.state so re-renders do not wipe what was typed.
  aiDraftField(field, value) {
    const app = ctx.app;
    const draft = app.state.aiDraft || (app.state.aiDraft = { provider: "openai", baseUrl: "", model: "", key: "" });
    draft[field] = value;
  },
  // Ajustes → Manuscrito registration form (Story 6.6, FR-20.1): transient
  // draft — the dir is picked on disk (pickFolder) or typed; registration
  // appends one manuscript.registered event (the dir is referenced, never
  // copied — the repo IS the manuscript).
  msRegField(field, value) {
    const app = ctx.app;
    const draft = app.state.msReg || (app.state.msReg = { missionId: "", dir: "", mainFile: "main.tex", saving: false });
    draft[field] = value;
  },
  async msRegPickFolder() {
    const app = ctx.app;
    try {
      const dir = await api.pickFolder();
      if (dir) {
        const draft = app.state.msReg || (app.state.msReg = { missionId: "", dir: "", mainFile: "main.tex", saving: false });
        draft.dir = dir;
        ctx.renderMainOnly();
      }
    } catch (e) {
      alert((e?.message || e));
    }
  },
  async msRegSubmit() {
    const app = ctx.app;
    const draft = app.state.msReg;
    if (!draft || draft.saving) return;
    const read = (id) => document.getElementById(id)?.value.trim() || "";
    const missionId = draft.missionId || read("ms-reg-mission") || app.data.missions?.[0]?.id;
    const dir = read("ms-reg-dir") || draft.dir;
    const mainFile = read("ms-reg-main") || draft.mainFile || "main.tex";
    if (!missionId || !dir) return;
    draft.saving = true;
    ctx.renderMainOnly();
    try {
      await api.registerManuscript(missionId, dir, mainFile);
      await ctx.loadManuscriptsList();
    } catch (e) {
      alert(t("ms.reg.error") + (e?.message || e));
    }
    draft.saving = false;
    ctx.renderMainOnly();
  },
  aiDraftProvider(value) {
    const app = ctx.app;
    const draft = app.state.aiDraft || (app.state.aiDraft = { provider: "openai", baseUrl: "", model: "", key: "" });
    draft.provider = value || "openai";
    if (value !== "custom") draft.baseUrl = "";
    ctx.renderMainOnly();
  },
  aiPickModel(model) {
    ctx.app.state.aiDraft.model = model;
    ctx.renderMainOnly();
  },
  // Configure the API provider (FR-17.2): the key goes to the OS keychain
  // via the core — never the database, never the log (NFR-10).
  async saveAiProvider() {
    const app = ctx.app;
    const draft = app.state.aiDraft || {};
    try {
      app.data.aiConfig = await api.configureAiProvider(
        draft.provider || "openai",
        draft.baseUrl || "",
        draft.model || "",
        draft.key || "",
      );
      draft.key = "";
      app.state.aiTest = null;
      ctx.renderMainOnly();
    } catch (e) {
      alert(t("trust.saveError") + (e?.message || e));
    }
  },
  // The test-connection run (Story 5.7): through the provider layer's
  // models endpoint — a success doubles as the live model list.
  async testAiConnection() {
    const app = ctx.app;
    app.state.aiTest = { testing: true };
    ctx.renderMainOnly();
    try {
      app.state.aiTest = await api.testProviderConnection();
    } catch (e) {
      app.state.aiTest = { ok: false, models: [], error: e?.message || String(e) };
    }
    ctx.renderMainOnly();
  },
  // Switch onto a CLI bridge (FR-17.3): the CLI's own auth stays with the
  // CLI — no key is stored, none is ever embedded in the spawn (NFR-10).
  async useCliBridge(name) {
    const app = ctx.app;
    try {
      app.data.aiConfig = await api.useCliBridge(name);
      app.state.aiTest = null;
      ctx.renderMainOnly();
    } catch (e) {
      alert(e?.message || e);
    }
  },
  // The local provider row's draft state (Story 6.1): the base URL +
  // chosen model kept in app.state so re-renders do not wipe them.
  localDraftField(field, value) {
    const app = ctx.app;
    const draft = app.state.localDraft || (app.state.localDraft = { baseUrl: "http://localhost:11434", model: "" });
    draft[field] = value;
  },
  pickLocalModel(model) {
    const app = ctx.app;
    const draft = app.state.localDraft || (app.state.localDraft = { baseUrl: "http://localhost:11434", model: "" });
    draft.model = draft.model === model ? "" : model;
    ctx.renderMainOnly();
  },
  // Switch onto the local provider (Story 6.1, FR-24.1): settings only —
  // the base URL is not a secret, no key at all. The core guards
  // localhost-only (zero egress, FR-24.3); an unreachable endpoint renders
  // the honest unconfigured state, never a simulated fallback (NFR-14).
  async useLocalProvider() {
    const app = ctx.app;
    const draft = app.state.localDraft || {};
    try {
      app.data.aiConfig = await api.useLocalProvider(draft.baseUrl || "", draft.model || "");
      app.state.aiTest = null;
      app.state.localTest = null;
      ctx.renderMainOnly();
    } catch (e) {
      alert(e?.message || e);
    }
  },
  // The local endpoint's test/refresh run (Story 6.1): GET /api/tags
  // through the provider layer — a success doubles as the live model list
  // for the row's chips and every picker.
  async testLocalProvider() {
    const app = ctx.app;
    const draft = app.state.localDraft || {};
    app.state.localTest = { testing: true };
    ctx.renderMainOnly();
    try {
      app.state.localTest = await api.testLocalProvider(draft.baseUrl || "");
    } catch (e) {
      app.state.localTest = { ok: false, models: [], error: e?.message || String(e) };
    }
    ctx.renderMainOnly();
  },
  // The composer's attach button (Story 5.5): the desktop opens the native
  // picker; the browser opens the hidden file input (the mock classifies
  // client-read content).
  async pickAttachments() {
    if (mockActive) {
      document.getElementById("chat-file-input")?.click();
      return;
    }
    try {
      const picks = await api.pickAttachmentFiles();
      if (picks && picks.length) await RC.addPickedAttachments(picks);
    } catch (e) {
      alert(e?.message || e);
    }
  },
  async handleAttachmentFiles(input) {
    const files = Array.from(input.files || []);
    input.value = "";
    if (!files.length) return;
    // the browser mock reads text kinds client-side; pdf/binary pass by
    // name and the transport classifies honestly
    const picks = [];
    for (const f of files) {
      const lower = f.name.toLowerCase();
      if (lower.endsWith(".md") || lower.endsWith(".txt") || lower.endsWith(".tex")) {
        picks.push({ name: f.name, content: await f.text() });
      } else {
        picks.push({ name: f.name });
      }
    }
    await RC.addPickedAttachments(picks);
  },
  // Attach picks to the active conversation — or hold them as pending chips
  // until the first send creates it (attachments belong to the conversation).
  async addPickedAttachments(picks) {
    const app = ctx.app;
    if (!picks.length) return;
    if (app.data.activeChatId) {
      try {
        const out = await api.addChatAttachments(app.data.activeChatId, picks);
        if (out.refused && out.refused.length) {
          alert(
            t("rc.assistant.attachRefused", { names: out.refused.map((r) => r.name).join(", ") }) +
              "\n\n" + out.refused.map((r) => `${r.name}: ${r.reason}`).join("\n"),
          );
        }
        const fresh = await api.getChat(app.data.activeChatId);
        app.data.chatAttachments[app.data.activeChatId] = fresh.attachments || [];
      } catch (e) {
        alert(e?.message || e);
      }
    } else {
      app.data.pendingAttachments = [...(app.data.pendingAttachments || []), ...picks];
    }
    ctx.renderMainOnly();
  },
  async removeAttachment(key, pending) {
    const app = ctx.app;
    if (pending) {
      app.data.pendingAttachments = (app.data.pendingAttachments || []).filter((a) => a.name !== key);
      ctx.renderMainOnly();
      return;
    }
    try {
      const list = await api.removeChatAttachment(app.data.activeChatId, key);
      app.data.chatAttachments[app.data.activeChatId] = list || [];
    } catch (e) {
      alert(e?.message || e);
    }
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
          ${chats.map((c) => {
            const m = c.mission_id ? app.data.missions.find((x) => x.id === c.mission_id) : null;
            const s = c.skill ? app.data.skills.find((x) => x.name === c.skill) : null;
            return `
          <button onclick="RC.loadChat('${esc(c.id)}')" class="w-full flex items-center gap-3 rounded-lg px-3 py-2.5 text-left transition ${c.id === app.data.activeChatId ? "bg-primary/10" : "hover:bg-gray-50"}">
            ${icon("message", `w-4 h-4 shrink-0 ${c.id === app.data.activeChatId ? "text-primary" : "text-muted"}`)}
            <span class="min-w-0 flex-1">
              <span class="block text-sm font-medium truncate ${c.id === app.data.activeChatId ? "text-primary" : "text-foreground"}">${esc(c.title)}</span>
              <span class="flex items-center gap-1.5 mt-0.5">
                <span class="inline-flex items-center rounded-full px-1.5 py-0.5 text-[10px] font-medium ring-1 ring-inset ${m ? "bg-primary/5 text-primary ring-primary/20" : "bg-gray-100 text-muted ring-gray-500/10"}">${m ? `<span class="font-mono">M-${m.seq}</span>` : t("rc.assistant.scopeGeneral")}</span>
                ${s ? `<span class="inline-flex items-center rounded-full bg-gray-100 px-1.5 py-0.5 text-[10px] font-medium text-muted ring-1 ring-inset ring-gray-500/10">${esc(t(`rc.skill.${s.name}`))}</span>` : ""}
                <span class="text-xs text-muted truncate">${esc(c.preview || "")}</span>
              </span>
            </span>
          </button>`;
          }).join("")}
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
      app.data.chatAttachments[id] = fresh.attachments || [];
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
    if (tab === "bridge") loadBridgeData();
    ctx.renderMainOnly();
  },
  // ---- The bridge (Story 6.14, FR-21.1): the ONE channel — enable,
  // disable, and the pairing ledger. One channel at a time; pairing is a
  // user action minting a one-time token.
  bridgeMode(v) {
    const app = ctx.app;
    const s = app.state.bridgeForm || (app.state.bridgeForm = { mode: "tunnel", addr: "", url: "", token: "", pairName: "", receipt: null });
    s.mode = v;
    ctx.renderMainOnly();
  },
  async enableBridge() {
    const app = ctx.app;
    const s = app.state.bridgeForm || (app.state.bridgeForm = {});
    if (s.mode === "tunnel") s.addr = document.getElementById("bridge-addr")?.value.trim() || s.addr || "";
    else {
      s.url = document.getElementById("bridge-url")?.value.trim() || s.url || "";
      s.token = document.getElementById("bridge-token")?.value.trim() || s.token || "";
    }
    app.data.bridgeError = null;
    try {
      app.data.bridge = await api.enableBridge(s.mode, s.addr || null, s.url || null, s.token || null);
    } catch (e) {
      app.data.bridgeError = String(e?.message || e);
    }
    await loadBridgeData();
  },
  async disableBridge() {
    const app = ctx.app;
    app.data.bridgeError = null;
    try {
      app.data.bridge = await api.disableBridge();
    } catch (e) {
      app.data.bridgeError = String(e?.message || e);
    }
    await loadBridgeData();
  },
  async pairBridgeDevice() {
    const app = ctx.app;
    const s = app.state.bridgeForm || (app.state.bridgeForm = {});
    s.pairName = document.getElementById("bridge-pair-name")?.value.trim() || "";
    if (!s.pairName) return;
    s.receipt = null;
    try {
      s.receipt = await api.pairBridgeDevice(s.pairName);
      s.pairName = "";
      await loadBridgeData();
    } catch (e) {
      alert(t("bridge.error") + (e?.message || e));
    }
    ctx.renderMainOnly();
  },
  async unpairBridgeDevice(deviceName) {
    try {
      await api.unpairBridgeDevice(deviceName);
    } catch (e) {
      alert(t("bridge.error") + (e?.message || e));
    }
    await loadBridgeData();
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
  // ---- compute targets (Stories 6.2–6.5) ----
  setTargetKind(kind) {
    const app = ctx.app;
    const form = (app.state.targetForm ??= { kind: "ssh", name: "", host: "", config: {}, error: null });
    form.kind = kind;
    form.error = null;
    ctx.renderMainOnly();
  },
  resetTargetForm() {
    const app = ctx.app;
    app.state.targetForm = { kind: "ssh", name: "", host: "", config: {}, error: null };
    ctx.renderMainOnly();
  },
  async declareTarget() {
    const app = ctx.app;
    const form = (app.state.targetForm ??= { kind: "ssh", name: "", host: "", config: {}, error: null });
    const name = (document.getElementById("tgt-name")?.value || "").trim();
    const host = (document.getElementById("tgt-host")?.value || "").trim();
    const config = {};
    for (const key of (TARGET_CONFIG_FIELDS[form.kind] || [])) {
      const v = (document.getElementById(`tgt-cfg-${key}`)?.value || "").trim();
      if (v) config[key] = v;
    }
    try {
      await api.declareComputeTarget(name, form.kind, host || null, config);
      form.name = "";
      form.host = "";
      form.config = {};
      form.error = null;
      app.data.targets = await api.listComputeTargets();
      ctx.renderMainOnly();
    } catch (e) {
      form.error = (e?.message || String(e));
      ctx.renderMainOnly();
    }
  },
  async saveAllowlist() {
    const app = ctx.app;
    const raw = (document.getElementById("tgt-allowlist")?.value || "");
    const hosts = raw.split(/[\s,]+/).map((h) => h.trim()).filter(Boolean);
    try {
      app.data.allowlist = await api.setHostAllowlist(hosts);
      app.data.targets = await api.listComputeTargets();
      ctx.renderMainOnly();
    } catch (e) {
      alert(t("trust.saveError") + (e?.message || e));
    }
  },
  async probeTarget(name) {
    const app = ctx.app;
    (app.state.targetProbes ??= {})[name] = { status: "probing", detail: "" };
    ctx.renderMainOnly();
    try {
      const probe = await api.probeComputeTarget(name);
      app.state.targetProbes[name] = probe;
    } catch (e) {
      app.state.targetProbes[name] = { status: "unreachable", detail: (e?.message || String(e)) };
    }
    ctx.renderMainOnly();
  },
});
