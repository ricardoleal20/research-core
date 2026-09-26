// The v1 agent surfaces, built in the bible's exact design language:
// missions home (question box + mission cards), the hypothesis board with
// evidence pins and quarantine review, the readiness drawer, and the run
// receipt drawer. All data flows through api.ts; every mutation is a core
// command. Visual anatomy per DESIGN.md: mission-card, hypothesis-card,
// evidence-pin, quarantine-diff, spend-meter, digest-item,
// readiness-blocking-item.
import { t, getLang } from "../i18n";
import { api, servedByCoreFlag } from "../api";
import { icon, esc, badge, btn, card, rcSelect, pageHeader, fmtCents, fmtTs } from "./helpers";
import { RC, ctx } from "./rc";

// Shared chip idioms (Story 5.10, FR-18.2/18.3): the dashboard composes
// the same lifecycle/status chips the missions home and board render.
export const lifecycleChip = {
  proposed: ["primary", "proposed"],
  testing: ["warning", "testing"],
  supported: ["success", "supported"],
  refuted: ["destructive", "refuted"],
  revised: ["primary", "revised"],
};
export const statusColor = {
  draft: "warning",
  active: "primary",
  awaiting_review: "warning",
  completed: "success",
  stopped: "muted",
  failed: "destructive",
};

const spendMeterFill = (state) =>
  state === "blocked" ? "bg-rose-500" : state === "near" ? "bg-amber-500" : "bg-primary";

export const spendMeter = (spendCents, ceilingCents, state) => {
  const pct = ceilingCents > 0 ? Math.min(100, Math.round((spendCents / ceilingCents) * 100)) : 0;
  return `
    <div class="flex items-center gap-3">
      <div class="flex-1 min-w-[110px]">
        <div class="flex items-baseline justify-between gap-2 mb-1">
          <span class="caption text-muted">${t("missions.spend")}</span>
          <span class="text-[10px] font-mono tabular text-muted">${fmtCents(spendCents)} ${t("missions.spendOf")} ${fmtCents(ceilingCents)}</span>
        </div>
        <div class="h-1.5 rounded-full bg-border overflow-hidden">
          <div class="h-full rounded-full transition-all duration-500 ${spendMeterFill(state)}" style="width:${pct}%"></div>
        </div>
      </div>
    </div>`;
};

// ========== MISSIONS HOME ==========
export function renderMissionsHome(app) {
  const { missions, missionsLoaded } = app.data;
  const header = pageHeader(
    t("missions.title"),
    t("missions.greeting"),
    missions.length ? btn({ label: t("missions.composer.title"), variant: "default", size: "sm", iconName: "plus", onClick: "RC.toggleQuestionBox()" }) : "",
  );
  if (!missionsLoaded) {
    return `${header}${card(`<div class="p-12 text-center text-muted text-sm">${t("rc.common.loading")}</div>`)}`;
  }
  if (!missions.length) return `${header}${renderEmptyHome(app)}`;
  const boxOpen = app.state.questionBoxOpen;
  return `
    <div class="space-y-6">
      ${header}
      ${boxOpen ? renderQuestionComposer(app) : ""}
      <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
        ${missions.map((m, i) => renderMissionCard(app, m, i)).join("")}
      </div>
    </div>`;
}

function renderEmptyHome(app) {
  const w = app.state.wizard;
  return `
    <div class="relative flex flex-col items-center justify-center py-10 overflow-hidden">
      <div class="pointer-events-none absolute -inset-x-16 -top-12 -bottom-16 z-0 opacity-60">
        <div class="rc-ai-aurora"></div>
        <div class="rc-ai-particle" style="left:12%; top:34%; animation-delay:0s"></div>
        <div class="rc-ai-particle" style="left:84%; top:52%; animation-delay:1.8s"></div>
        <div class="rc-ai-particle" style="left:20%; top:72%; animation-delay:3.2s"></div>
      </div>
      <div class="relative z-10 w-full max-w-xl">
        ${card(`
          <div class="p-6 space-y-5 rc-intro">
            <div class="flex items-center gap-3">
              <div class="flex h-9 w-9 items-center justify-center rounded-xl bg-primary/10 text-primary">${icon("sparkle", "w-4 h-4")}</div>
              <h2 class="heading-2">${t("onb.pasteTitle")}</h2>
            </div>
            <p class="text-sm text-muted leading-relaxed">${t("missions.emptyHint")}</p>
            <div class="flex gap-2">
              <input id="first-value-url" value="${esc(w.initUrl)}" placeholder="${esc(t("onb.urlPh"))}" class="flex-1 rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30">
              ${btn({ label: w.initLoading ? t("onb.generating") : t("onb.generate"), variant: "default", iconName: "sparkle", onClick: "RC.firstValue()", disabled: w.initLoading })}
            </div>
            <p class="text-xs text-muted">${t("onb.urlHint")}</p>
            ${w.initResult ? renderFirstValueCard(w.initResult) : ""}
          </div>`)}
      </div>
    </div>`;
}

function renderFirstValueCard(r) {
  return `
    <div class="rounded-xl border border-primary/30 bg-primary/5 p-4 animate-fade-up">
      <p class="text-xs font-semibold text-primary uppercase tracking-wider mb-1">${t("onb.resultKicker")}</p>
      <p class="text-sm font-medium">${esc(r.paper.title)}</p>
      <p class="text-xs text-muted mt-0.5">${esc(r.paper.authors)}${r.paper.year ? " · " + r.paper.year : ""}</p>
      <p class="text-xs text-muted mt-2">${t("onb.missionLabel")}: <span class="text-foreground font-medium">${esc(r.mission.question)}</span></p>
      <div class="mt-2 space-y-1">
        ${r.candidates.map((c) => `
          <p class="text-xs text-muted">· <span class="font-mono">H-${c.seq}</span> ${esc(c.statement)} <span class="font-mono tabular">${(c.confidence * 100).toFixed(0)}%</span></p>
        `).join("")}
      </div>
      ${r.receipt.simulated ? `<p class="text-[11px] text-muted mt-2 font-mono">${t("onb.simulatedNote")}</p>` : ""}
      <div class="mt-3">${btn({ label: t("onb.continue"), variant: "default", size: "sm", onClick: "RC.openBoard('${esc(r.mission.id)}')" })}</div>
    </div>`;
}

function renderQuestionComposer(app) {
  const s = app.state.qbox || (app.state.qbox = { question: "", stop: "", criterion: "", autonomy: "suggest", ceiling: "10.00" });
  return `
    ${card(`
      <div class="p-6 space-y-4">
        <h3 class="heading-3">${t("missions.composer.title")}</h3>
        <div><label class="block text-sm font-medium mb-1.5">${t("missions.composer.question")}</label><input id="qb-question" value="${esc(s.question)}" placeholder="${t("missions.questionPh")}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div><label class="block text-sm font-medium mb-1.5">${t("missions.stopCondition")}</label><input id="qb-stop" value="${esc(s.stop)}" placeholder="${t("missions.stopConditionPh")}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
          <div><label class="block text-sm font-medium mb-1.5">${t("missions.successCriterion")}</label><input id="qb-criterion" value="${esc(s.criterion)}" placeholder="${t("missions.successCriterionPh")}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
        </div>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div>
            <label class="block text-sm font-medium mb-1.5">${t("missions.autonomy")}</label>
            <div class="grid grid-cols-3 gap-1 rounded-lg border border-border bg-gray-50 p-1">
              ${[["watch", t("missions.autonomy.watch")], ["suggest", t("missions.autonomy.suggest")], ["act_with_receipts", t("missions.autonomy.act_with_receipts")]].map(([v, l]) => `
                <button type="button" onclick="RC.qboxAutonomy('${v}')" class="rounded-md py-1.5 text-xs font-medium transition ${s.autonomy === v ? "bg-white shadow-sm text-foreground" : "text-muted hover:text-foreground"}">${l}</button>
              `).join("")}
            </div>
            <p class="text-xs text-muted mt-1">${t("missions.autonomyNote")}</p>
          </div>
          <div><label class="block text-sm font-medium mb-1.5">${t("missions.spendCeiling")} <span class="text-muted font-normal">(${t("trust.hard")})</span></label><input id="qb-ceiling" value="${esc(s.ceiling)}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"><p class="text-xs text-muted mt-1">${t("missions.spendCeilingHint")}</p></div>
        </div>
        <div class="flex justify-end gap-2 pt-1">
          ${btn({ label: t("missions.cancel"), variant: "ghost", onClick: "RC.toggleQuestionBox()" })}
          ${btn({ label: s.launching ? t("missions.landing") : t("missions.launch"), variant: "default", iconName: "sparkle", onClick: "RC.launchMission()", disabled: s.launching })}
        </div>
      </div>`, "mb-6")}`;
}

function renderMissionCard(app, m, i) {
  // A quick-captured draft (Story 6.15, FR-21.3): the pending mission card
  // — the question landed, the terminators did not. The owner completes it
  // here; capture never launched anything (FR-1.2).
  if (m.status === "draft") return renderDraftCard(app, m, i);
  const runs = app.data.runs[m.id] || [];
  const open = !!app.data.runsOpen[m.id];
  return `
    <div class="animate-fade-up" style="animation-delay:${i * 40}ms">
    ${card(`
      <div class="p-6 space-y-4">
        <div class="flex items-start justify-between gap-3">
          <span class="font-mono text-[10px] text-muted tabular">M-${m.seq}</span>
          ${badge(t("missions.status." + m.status), statusColor[m.status] || "muted")}
        </div>
        <h3 class="heading-3 leading-snug cursor-pointer hover:text-primary transition" onclick="RC.openBoard('${esc(m.id)}')">${esc(m.question)}</h3>
        <div class="space-y-1.5">
          <p class="text-xs text-muted"><span class="font-medium text-foreground">${t("missions.stopCondition")}:</span> ${esc(m.stopCondition)}</p>
          <p class="text-xs text-muted"><span class="font-medium text-foreground">${t("missions.successCriterion")}:</span> ${esc(m.successCriterion)}</p>
        </div>
        ${spendMeter(m.spendCents, m.spendCeilingCents, m.spendState)}
        <div class="flex items-center justify-between pt-2 border-t border-border">
          <div class="flex items-center gap-3 text-xs text-muted">
            <span class="font-mono tabular">${m.schedule === "off" ? "— " + t("missions.autonomy." + m.autonomy) : t("missions.autonomy." + m.autonomy)}</span>
          </div>
          <div class="flex items-center gap-2">
            ${btn({ label: t("board.title"), variant: "secondary", size: "sm", onClick: `RC.openBoard('${esc(m.id)}')` })}
            ${btn({ label: open ? t("missions.hideRuns") : t("missions.showRuns"), variant: "ghost", size: "sm", onClick: `RC.toggleRuns('${esc(m.id)}')` })}
          </div>
        </div>
        ${open ? renderRunsDrill(app, m.id, runs) : ""}
      </div>`)}
    </div>`;
}

// The pending mission card's completion form (Story 6.15): the stop
// condition, the falsifiable success criterion, the autonomy dial, and the
// ceiling — the same composer idioms; only the owner launches.
function renderDraftCard(app, m, i) {
  const s = app.state.drafts?.[m.id] || { autonomy: "suggest", ceiling: "10.00", completing: false };
  return `
    <div class="animate-fade-up" style="animation-delay:${i * 40}ms">
    ${card(`
      <div class="p-6 space-y-4 border-l-4 border-l-amber-400">
        <div class="flex items-start justify-between gap-3">
          <span class="font-mono text-[10px] text-muted tabular">M-${m.seq}</span>
          <div class="flex items-center gap-2">
            <span class="text-[10px] font-medium text-amber-700">${t("missions.draft.capturedOn", { surface: "mobile" })}</span>
            ${badge(t("missions.status.draft"), "warning")}
          </div>
        </div>
        <h3 class="heading-3 leading-snug">${esc(m.question)}</h3>
        <div class="rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 flex items-start gap-2">
          ${icon("history", "w-4 h-4 text-amber-700 shrink-0 mt-0.5")}
          <p class="text-xs font-medium text-amber-700 leading-snug">${t("missions.draft.awaiting")}</p>
        </div>
        <div class="grid grid-cols-1 gap-4">
          <div><label class="block text-sm font-medium mb-1.5">${t("missions.stopCondition")}</label><input id="dc-stop-${m.seq}" placeholder="${t("missions.draft.stopPh")}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
          <div><label class="block text-sm font-medium mb-1.5">${t("missions.successCriterion")}</label><input id="dc-criterion-${m.seq}" placeholder="${t("missions.draft.criterionPh")}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
        </div>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div>
            <label class="block text-sm font-medium mb-1.5">${t("missions.autonomy")}</label>
            <div class="grid grid-cols-3 gap-1 rounded-lg border border-border bg-gray-50 p-1">
              ${[["watch", t("missions.autonomy.watch")], ["suggest", t("missions.autonomy.suggest")], ["act_with_receipts", t("missions.autonomy.act_with_receipts")]].map(([v, l]) => `
                <button type="button" onclick="RC.draftAutonomy('${esc(m.id)}','${v}')" class="rounded-md py-1.5 text-xs font-medium transition ${s.autonomy === v ? "bg-white shadow-sm text-foreground" : "text-muted hover:text-foreground"}">${l}</button>
              `).join("")}
            </div>
          </div>
          <div><label class="block text-sm font-medium mb-1.5">${t("missions.spendCeiling")} <span class="text-muted font-normal">(${t("trust.hard")})</span></label><input id="dc-ceiling-${m.seq}" value="${esc(s.ceiling)}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
        </div>
        <div class="flex justify-end gap-2 pt-1">
          ${btn({ label: s.completing ? t("missions.draft.completing") : t("missions.draft.complete"), variant: "default", iconName: "sparkle", onClick: `RC.completeCapture('${esc(m.id)}')`, disabled: s.completing })}
        </div>
      </div>`)}
    </div>`;
}

function renderRunsDrill(app, missionId, runs) {
  return `
    <div class="mt-3 rounded-xl border border-border bg-gray-50/50 p-4 animate-fade-up">
      <p class="caption text-muted mb-2">${t("missions.runs")}</p>
      ${runs.length ? `
        <div class="space-y-1">
          ${runs.map((r) => `
            <div class="flex items-center justify-between gap-3 rounded-lg px-2 py-1.5 hover:bg-gray-100 transition">
              <span class="font-mono text-xs tabular text-muted truncate">e-${r.seq} · ${esc(r.kind)} · ${esc(r.actor)}${r.role ? " · " + esc(r.role) : ""}${r.detail ? ` · <span class="text-amber-700">${esc(r.detail)}</span>` : ""}</span>
              <span class="flex items-center gap-2 shrink-0">
                <span class="font-mono text-[10px] text-muted tabular">${fmtTs(r.ts)}</span>
                ${r.runId ? `<button onclick="RC.openReceipt('${esc(r.runId)}')" class="text-xs font-medium text-primary hover:underline">${t("receipt.open")}</button>` : ""}
              </span>
            </div>
          `).join("")}
        </div>` : `<p class="text-sm text-muted py-2 text-center">${t("missions.runsEmpty")}</p>`}
    </div>`;
}

// ========== BOARD ==========
export function renderBoard(app) {
  const missionId = app.state.boardMissionId;
  const mission = app.data.missions.find((m) => m.id === missionId);
  const b = app.data.board[missionId];
  const header = `
    <div class="flex flex-col md:flex-row md:items-end justify-between gap-4">
      <div>
        <div class="flex items-center gap-2 mb-1">
          <button onclick="RC.navigate('missions')" class="inline-flex items-center gap-1 text-xs font-medium text-primary hover:underline">${icon("chevron", "w-3.5 h-3.5 rotate-180")} ${t("missions.title")}</button>
          ${mission ? `<span class="font-mono text-[10px] text-muted tabular">M-${mission.seq}</span>` : ""}
        </div>
        <h1 class="font-serif text-4xl italic max-w-2xl">${esc(mission ? mission.question : t("board.title"))}</h1>
      </div>
      <div class="flex gap-2">
        ${btn({ label: t("cp.title"), variant: "secondary", size: "sm", iconName: "history", onClick: "RC.openCheckpoints()" })}
        ${btn({ label: t("rd.report"), variant: "secondary", size: "sm", iconName: "check", onClick: `RC.openReadiness('${esc(missionId || "")}')` })}
        ${mission ? btn({ label: t("missions.roles.drafter"), variant: "default", size: "sm", iconName: "bolt", onClick: `RC.runStep('${esc(mission.id)}','drafter')` }) : ""}
      </div>
    </div>`;
  if (!mission) return `${header}${card(`<div class="p-12 text-center text-muted text-sm">${t("board.loadError")}</div>`)}`;
  if (!b) return `${header}${card(`<div class="p-12 text-center text-muted text-sm">${t("rc.common.loading")}</div>`)}`;
  const pending = b.proposals.filter((p) => p.status === "pending");
  const decided = b.proposals.filter((p) => p.status !== "pending");
  return `
    <div class="space-y-6">
      ${header}
      <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
        <div class="lg:col-span-2 space-y-5">
          <div class="flex items-center justify-between">
            <h2 class="heading-2">${t("hyp.board")}</h2>
            <div class="flex items-center gap-2">
              ${btn({ label: t("ev.verify"), variant: "outline", size: "sm", iconName: "check", onClick: "RC.verifyPins()" })}
              ${btn({ label: t("ev.support.check"), variant: "outline", size: "sm", iconName: "shield", onClick: "RC.checkSupport()" })}
            </div>
          </div>
          ${renderAddHypothesis()}
          ${b.hyps.length ? b.hyps.map((h, i) => renderHypothesisCard(app, h, b.evidence[h.id] || [], i)).join("") : card(`<div class="p-8 text-center text-muted text-sm">${t("hyp.empty")}</div>`)}
        </div>
        <div class="space-y-5">
          ${renderQuarantine(app, pending, decided)}
          ${renderDisclosure(app, missionId)}
          ${renderMissionMeta(mission)}
        </div>
      </div>
      ${renderManuscriptSection(app, mission)}
    </div>`;
}

// ========== MANUSCRIPT (Stories 6.6–6.8, FR-20) ==========
// The .tex repo IS the manuscript: this section renders ONLY when the
// mission has one registered (progressive disclosure, FR-1.4/FR-8.2 —
// registration lives in Ajustes). Source editing beside the compiled PDF,
// compiled on demand; toolchain detection is honest (tex_not_found is a
// state with an install hint, never a fake render); agent edits land as
// quarantined diffs (Story 6.7) merged only by the human.

function renderManuscriptSection(app, mission) {
  const view = app.data.manuscripts ? app.data.manuscripts[mission.id] : null;
  if (!view) return ""; // not registered (or still loading) — no surface
  const st = app.state.ms && app.state.ms.missionId === mission.id ? app.state.ms : null;
  const last = view.lastCompile;
  const compiling = st && st.compiling;
  const tool = view.toolchain;
  return `
    <div class="pt-2 space-y-4">
      <div class="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2 class="heading-2">${t("ms.title")}</h2>
          <p class="text-xs text-muted mt-0.5">${t("ms.sub")}</p>
          <p class="font-mono text-[11px] text-muted mt-1 truncate max-w-xl" title="${esc(view.manuscript.dir)}">${esc(view.manuscript.dir)} · ${esc(view.manuscript.mainFile)}</p>
        </div>
        <div class="flex items-center gap-2">
          ${tool
            ? `<span class="inline-flex items-center gap-1 rounded-full bg-emerald-50 px-2.5 py-1 text-[11px] font-medium text-emerald-700 ring-1 ring-inset ring-emerald-600/20 font-mono">${esc(tool.name)}${tool.configured ? " · " + esc(t("ms.toolchain")) : ""}</span>`
            : `<span class="inline-flex items-center gap-1 rounded-full bg-rose-50 px-2.5 py-1 text-[11px] font-medium text-rose-700 ring-1 ring-inset ring-rose-600/20">${t("ms.texNotFound")}</span>`}
          ${btn({ label: compiling ? t("ms.compiling") : t("ms.compile"), variant: "default", size: "sm", iconName: "bolt", onClick: `RC.msCompile('${esc(mission.id)}')`, disabled: compiling })}
        </div>
      </div>
      ${!tool ? `
      <div class="rounded-xl border border-rose-200 bg-rose-50 p-4">
        <p class="text-sm font-medium text-rose-700">${t("ms.texNotFound")}</p>
        <p class="text-xs text-rose-600 mt-1">${t("ms.installHint")}</p>
      </div>` : ""}
      <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <div class="space-y-4">
          ${renderMsFileList(app, mission.id, view, st)}
          ${renderMsEditor(app, mission.id, st)}
        </div>
        <div class="space-y-4">
          ${renderMsPdfPane(last)}
          ${renderMsLog(last)}
        </div>
      </div>
      ${renderMsDiffs(app, mission.id)}
    </div>`;
}

function renderMsFileList(app, missionId, view, st) {
  const active = st ? st.file : view.manuscript.mainFile;
  return card(`
    <div class="p-4 space-y-2">
      <p class="caption text-muted">${t("ms.files")}</p>
      <div class="divide-y divide-border rounded-lg border border-border overflow-hidden">
        ${view.files.length ? view.files.map((f) => `
          <button onclick="RC.msOpenFile('${esc(missionId)}','${esc(f.path)}')" class="w-full flex items-center justify-between gap-3 px-3 py-2 text-left transition ${active === f.path ? "bg-primary/5" : "bg-white hover:bg-gray-50"}">
            <span class="font-mono text-xs truncate ${active === f.path ? "text-primary font-medium" : "text-foreground"}">${esc(f.path)}${f.path === view.manuscript.mainFile ? ` <span class="text-muted">· ${t("ms.main")}</span>` : ""}</span>
            <span class="shrink-0 font-mono text-[10px] text-muted tabular">${f.words} ${t("ms.words")} · ${f.hypMarkers + f.claimMarkers} ${t("ms.markers")}</span>
          </button>
        `).join("") : `<p class="px-3 py-4 text-sm text-muted text-center">—</p>`}
      </div>
    </div>`);
}

function renderMsEditor(app, missionId, st) {
  if (!st) return "";
  const ro = servedByCoreFlag();
  const dirty = st.draft !== st.content;
  return card(`
    <div class="p-4 space-y-3">
      <div class="flex items-center justify-between gap-2">
        <p class="caption text-muted">${t("ms.editor")} — <span class="font-mono">${esc(st.file)}</span></p>
        ${dirty ? `<span class="text-[10px] font-medium text-amber-600">${t("ms.dirty")}</span>` : st.saved ? `<span class="text-[10px] font-medium text-emerald-600">${t("ms.saved")}</span>` : ""}
      </div>
      ${ro ? `<p class="text-[11px] text-muted">${t("ms.editorRo")}</p>` : ""}
      <textarea id="ms-editor" rows="16" ${ro ? "readonly" : ""} oninput="RC.msDraft(this.value)" class="w-full rounded-lg border border-border bg-white px-3 py-2.5 text-xs font-mono leading-relaxed focus:outline-none focus:ring-2 focus:ring-primary/30">${esc(st.draft)}</textarea>
      ${!ro ? `<div class="flex justify-end">${btn({ label: st.saving ? t("ms.saving") : t("ms.save"), variant: "secondary", size: "sm", onClick: `RC.msSave('${esc(missionId)}')`, disabled: st.saving || !dirty })}</div>` : ""}
    </div>`);
}

function renderMsPdfPane(last) {
  const inner = last && last.outcome === "ok" && last.pdfUrl
    ? `<object data="${esc(last.pdfUrl)}" type="application/pdf" class="w-full h-[520px] rounded-lg border border-border bg-gray-50">
        <p class="p-4 text-sm text-muted">${t("ms.pdf")}: <a class="text-primary hover:underline" href="${esc(last.pdfUrl)}" target="_blank" rel="noreferrer">${esc(last.pdfUrl)}</a></p>
      </object>`
    : `<div class="h-[520px] flex flex-col items-center justify-center gap-2 rounded-lg border border-dashed border-border bg-gray-50/50 p-6 text-center">
        <p class="text-sm text-muted">${t("ms.pdfEmpty")}</p>
      </div>`;
  return card(`
    <div class="p-4 space-y-2">
      <div class="flex items-center justify-between">
        <p class="caption text-muted">${t("ms.pdf")}</p>
        ${last ? (last.outcome === "ok" ? badge(t("ms.compileOk"), "success") : last.outcome === "error" ? badge(t("ms.compileError"), "destructive") : badge(t("ms.texNotFound"), "destructive")) : ""}
      </div>
      ${inner}
    </div>`);
}

function renderMsLog(last) {
  if (!last) return "";
  const errorish = last.outcome !== "ok";
  return card(`
    <div class="p-4 space-y-2">
      <div class="flex items-center justify-between">
        <p class="caption text-muted">${t("ms.log")}</p>
        <span class="font-mono text-[10px] text-muted tabular">e-${last.seq}</span>
      </div>
      <pre class="rounded-lg ${errorish ? "bg-rose-50 border border-rose-200" : "bg-gray-50 border border-border"} p-3 text-[11px] font-mono whitespace-pre-wrap break-words leading-relaxed">${esc(last.logTail)}</pre>
      ${last.outcome === "tex_not_found" ? `<p class="text-xs text-rose-600">${t("ms.installHint")}</p>` : ""}
    </div>`);
}

function renderMsDiffs(app, missionId) {
  const diffs = (app.data.msDiffs && app.data.msDiffs[missionId]) || [];
  const pending = diffs.filter((d) => d.status === "pending");
  const decided = diffs.filter((d) => d.status !== "pending");
  return card(`
    <div class="p-5 space-y-4">
      <div>
        <h3 class="heading-3">${t("ms.diffs.title")}</h3>
        <p class="text-xs text-muted mt-0.5">${t("ms.diffs.sub")}</p>
      </div>
      ${pending.length ? pending.map((d) => renderMsDiffCard(d)).join("") : `<p class="text-sm text-muted py-2 text-center">${t("ms.diffs.empty")}</p>`}
      ${decided.length ? `
      <div class="pt-3 border-t border-border">
        <div class="flex items-center justify-between mb-1.5">
          <p class="caption text-muted">${t("ms.diffs.history")}</p>
          ${decided.some((d) => d.status === "merged") ? btn({ label: t("ms.diffs.compileCheck"), variant: "outline", size: "sm", iconName: "bolt", onClick: `RC.msCompile('${esc(missionId)}')` }) : ""}
        </div>
        <div class="space-y-1">
          ${decided.slice(-6).reverse().map((d) => `
            <div class="flex items-center justify-between gap-2 text-xs">
              <span class="font-mono text-[10px] text-muted tabular">pr-${d.seq}</span>
              <span class="font-mono text-[10px] text-muted truncate flex-1">${esc(d.file)}</span>
              ${badge(t(d.status === "merged" ? "ms.diffs.merged" : "ms.diffs.rejected"), d.status === "merged" ? "success" : "muted")}
            </div>
          `).join("")}
        </div>
      </div>` : ""}
    </div>`);
}

function renderMsDiffCard(d) {
  return `
    <div class="rounded-xl border ${d.basisStale ? "border-amber-300 ring-1 ring-amber-300/40" : "border-border"} bg-white p-4 space-y-3">
      <div class="flex items-center justify-between gap-2">
        <span class="inline-flex items-center gap-1.5 font-mono text-[10px] text-amber-700 tabular">${icon("history", "w-3.5 h-3.5")} pr-${d.seq} · ${t("ms.diffs.pending")}</span>
        <span class="font-mono text-[10px] text-muted truncate">${esc(d.file)} · ${d.hunks.length} ${t("ms.diffs.hunks")}</span>
      </div>
      ${d.hunks.map((h, i) => `
        <div class="rounded-lg border border-border overflow-hidden">
          <pre class="bg-rose-50 px-3 py-2 text-[11px] font-mono whitespace-pre-wrap break-words leading-relaxed text-rose-800">- ${esc(h.before)}</pre>
          <pre class="bg-emerald-50 px-3 py-2 text-[11px] font-mono whitespace-pre-wrap break-words leading-relaxed text-emerald-800">+ ${esc(h.after)}</pre>
        </div>
      `).join("")}
      ${d.note ? `<p class="text-xs text-muted leading-relaxed">${esc(d.note)}</p>` : ""}
      ${d.basisStale ? `
        <div class="rounded-lg border-2 border-amber-300 bg-amber-50 px-3 py-2 flex items-center gap-2">
          ${icon("danger", "w-4 h-4 text-amber-700 shrink-0")}
          <p class="text-xs font-medium text-amber-700">${t("ms.diffs.basisStale")}</p>
        </div>` : ""}
      <div class="flex items-center gap-2 pt-1">
        ${btn({ label: t("ms.diffs.reject"), variant: "secondary", size: "sm", onClick: `RC.msDiffReject('${esc(d.id)}')` })}
        ${d.basisStale
          ? btn({ label: t("ms.diffs.force"), variant: "destructive", size: "sm", onClick: `RC.msDiffApprove('${esc(d.id)}', true)` })
          : btn({ label: t("ms.diffs.approve"), variant: "default", size: "sm", onClick: `RC.msDiffApprove('${esc(d.id)}', false)` })}
      </div>
    </div>`;
}

function renderAddHypothesis() {
  return card(`
    <div class="p-4 flex gap-2">
      <input id="new-hyp" placeholder="${t("hyp.statementPh")}" class="flex-1 rounded-lg border border-border bg-white px-4 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30">
      ${btn({ label: t("hyp.add"), variant: "default", size: "sm", iconName: "plus", onClick: "RC.addHypothesis()" })}
    </div>`);
}

function renderHypothesisCard(app, h, claims, i) {
  const [chipColor, chipLabel] = lifecycleChip[h.status] || ["muted", h.status];
  const allowed = { proposed: ["testing"], testing: ["supported", "refuted"], supported: ["revised"], refuted: ["revised"], revised: ["testing"] }[h.status] || [];
  return `
    <div class="animate-fade-up" style="animation-delay:${i * 40}ms">
    ${card(`
      <div class="p-5 space-y-3">
        <div class="flex items-start justify-between gap-3">
          <span class="font-mono text-[10px] text-muted tabular">H-${h.seq}</span>
          <div class="flex items-center gap-2">
            ${btn({ label: t("hyp.relate"), variant: "ghost", size: "sm", onClick: `RC.openRelateForm('${esc(h.id)}')` })}
            ${allowed.length ? rcSelect({ id: `hyp-status-${h.seq}`, size: "sm", cls: "w-auto min-w-[9rem]", options: [{ value: h.status, label: t("hyp.status." + h.status) }, ...allowed.map((a) => ({ value: a, label: "→ " + t("hyp.status." + a) }))], value: h.status, onChange: `RC.transitionHyp('${esc(h.id)}', this.value)` }) : badge(t("hyp.status." + h.status), chipColor)}
          </div>
        </div>
        <p class="text-sm font-medium leading-snug">${esc(h.statement)}</p>
        ${h.relations.length ? `
        <div class="flex flex-wrap gap-1.5">
          ${h.relations.map((r) => `<span class="inline-flex items-center gap-1 rounded-full bg-gray-100 px-2.5 py-0.5 text-[11px] font-medium text-muted ring-1 ring-inset ring-gray-500/10">${esc(t("hyp.rel." + r.kind + (r.direction === "outgoing" ? ".out" : ".in")))} <span class="font-mono">H-${r.otherSeq}</span></span>`).join("")}
        </div>` : ""}
        ${claims.length ? renderEvidenceRows(h, claims) : ""}
        <div class="flex items-center gap-2 pt-1">
          <input id="claim-${esc(h.id)}" placeholder="${t("ev.addPh")}" class="flex-1 rounded-lg border border-border bg-white px-3 py-1.5 text-xs focus:outline-none focus:ring-2 focus:ring-primary/30">
          ${btn({ label: t("ev.add"), variant: "secondary", size: "sm", onClick: `RC.addClaim('${esc(h.id)}')` })}
        </div>
        <p class="pt-2 border-t border-border caption text-muted">e-${h.audit.seq} · ${esc(h.audit.actor)} · ${esc(h.audit.basis)} · <span class="font-mono">${fmtTs(h.audit.ts)}</span></p>
      </div>`)}
    </div>`;
}

function verificationChip(v) {
  if (!v) return `<span class="inline-flex items-center gap-1 text-[10px] text-muted">${icon("circle", "w-3 h-3")} ${t("ev.unverified")}</span>`;
  if (v.status === "verified") return `<span class="inline-flex items-center gap-1 text-[10px] text-emerald-600">${icon("check", "w-3 h-3")} ${t("ev.verified")}</span>`;
  if (v.status === "stale") return `<span class="inline-flex items-center gap-1 text-[10px] text-muted">${icon("history", "w-3 h-3")} ${t("ev.stale")}</span>`;
  return `<span class="inline-flex items-center gap-1 text-[10px] text-rose-600">${icon("danger", "w-3 h-3")} ${t("ev.verificationFailed")}</span>`;
}

// The support chip (Story 6.9, FR-23.2): the THIRD signal, deliberately a
// different visual idiom from the existence dot (plain icon + text) and the
// confidence line (plain text) — a ringed chip with the shield icon, one
// color per verdict, and the judging model in mono (attribution: an LLM
// judgment with a name, never "verified by code"). null = unchecked
// ("por verificar") — a state of its own, never a default green.
function supportChip(s) {
  if (!s) return `<span class="inline-flex items-center gap-1 rounded-md bg-gray-50 px-1.5 py-0.5 text-[10px] font-medium text-muted ring-1 ring-inset ring-gray-500/15">${icon("shield", "w-3 h-3")} ${t("ev.support.unchecked")}</span>`;
  const styles = {
    supported: "bg-emerald-50 text-emerald-700 ring-emerald-600/20",
    partially: "bg-amber-50 text-amber-700 ring-amber-600/20",
    unsupported: "bg-rose-50 text-rose-700 ring-rose-600/20",
    unverifiable: "bg-slate-100 text-slate-600 ring-slate-500/20",
    stale: "bg-gray-50 text-muted ring-gray-500/15",
  };
  const labels = {
    supported: "ev.support.supported",
    partially: "ev.support.partial",
    unsupported: "ev.support.unsupported",
    unverifiable: "ev.support.unverifiable",
    stale: "ev.support.stale",
  };
  const cls = styles[s.status] || styles.unverifiable;
  const label = labels[s.status] || labels.unverifiable;
  return `<span class="inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[10px] font-medium ring-1 ring-inset ${cls}">${s.status === "stale" ? icon("history", "w-3 h-3") : icon("shield", "w-3 h-3")} ${t(label)} <span class="font-mono opacity-70">${esc(s.judgingModel)}</span></span>`;
}

function renderEvidenceRows(h, claims) {
  return `
    <div class="space-y-1.5 rounded-xl border border-border bg-gray-50/50 p-3">
      <p class="caption text-muted">${t("ev.claims")}</p>
      ${claims.map((c) => `
        <div class="rounded-lg bg-white border border-border px-3 py-2">
          <div class="flex items-start justify-between gap-2">
            <p class="text-xs leading-snug"><span class="font-mono text-[10px] text-muted tabular">CLAIMS-${c.seq}</span> ${esc(c.text)}</p>
            ${c.pinned ? "" : `<span class="flex items-center gap-1.5 shrink-0">${badge(t("ev.unpinned"), "warning")}${btn({ label: t("ev.pin"), variant: "secondary", size: "sm", onClick: `RC.openPinForm('${esc(c.id)}','${esc(h.id)}')` })}</span>`}
          </div>
          ${c.pinned && c.pin ? `
            <div class="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1">
              ${c.pin.kind === "citation"
                ? `<span class="inline-flex items-center gap-1 rounded-md bg-primary/10 px-2 py-1 text-xs font-medium text-primary">${icon("book", "w-3 h-3")} ${esc(c.pin.refLabel || c.pin.refId || "")}</span>`
                : `<span class="inline-flex items-center gap-1 rounded-md bg-slate-100 px-2 py-1 text-xs font-medium text-slate-700">${icon("fileText", "w-3 h-3")} <span class="font-mono">${esc((c.pin.digest || "").slice(0, 10))}…</span></span>`}
              ${c.pin.refRemoved
                ? `<span class="inline-flex items-center gap-1 rounded-md bg-amber-50 px-2 py-1 text-xs font-medium text-amber-700 ring-1 ring-inset ring-amber-600/20">${icon("danger", "w-3 h-3")} ${t("ev.sourceRemoved")}</span>`
                : ""}
              <span class="text-[10px] text-muted">${t("ev.confidence")} <span class="font-mono tabular">${(c.pin.confidence * 100).toFixed(0)}%</span> · ${esc(c.pin.assessingModel)}</span>
              ${verificationChip(c.pin.verification)}
              ${supportChip(c.pin.support)}
            </div>` : ""}
        </div>
      `).join("")}
    </div>`;
}

function renderQuarantine(app, pending, decided) {
  return card(`
    <div class="p-5 space-y-4">
      <div>
        <h3 class="heading-3">${t("quarantine.title")}</h3>
        <p class="text-xs text-muted mt-0.5">${t("quarantine.sub")}</p>
      </div>
      ${pending.length ? pending.map((p) => renderProposal(app, p)).join("") : `<p class="text-sm text-muted py-2 text-center">${t("quarantine.empty")}</p>`}
      ${decided.length ? `
        <div class="pt-3 border-t border-border">
          <p class="caption text-muted mb-1.5">${t("quarantine.history")}</p>
          <div class="space-y-1">
            ${decided.slice(0, 6).map((p) => `
              <div class="flex items-center justify-between gap-2 text-xs">
                <span class="font-mono text-[10px] text-muted tabular">pr-${p.seq}</span>
                <span class="text-muted truncate flex-1">${esc(p.targetLabel || p.targetEntity)}</span>
                ${badge(t("quarantine.decided") + " · " + p.status, p.status === "merged" ? "success" : p.status === "rejected" ? "muted" : "low")}
              </div>
            `).join("")}
          </div>
        </div>` : ""}
    </div>`);
}

function renderProposal(app, p) {
  const isPin = p.proposedKind === "evidence.pinned";
  const submission = p.proposedKind === "submission.created";
  const itemCheck = p.proposedKind === "submission.item_checked";
  const payload = p.proposedPayload;
  const target = p.targetSeq ? `H-${p.targetSeq}` : (p.targetLabel || "");
  return `
    <div class="rounded-xl border ${p.basisStale ? "border-amber-300 ring-1 ring-amber-300/40" : "border-border"} bg-white p-4 space-y-2.5">
      <div class="flex items-center justify-between gap-2">
        <span class="inline-flex items-center gap-1.5 font-mono text-[10px] text-amber-700 tabular">${icon("history", "w-3.5 h-3.5")} pr-${p.seq} · ${t("quarantine.pending")}</span>
        ${badge(isPin ? t("quarantine.pinProposes") : t("quarantine.change"), "warning")}
      </div>
      ${isPin ? `
        <p class="text-xs text-muted">${t("quarantine.artifact")} <span class="font-mono">${esc(payload.artifact_ref || "")}</span></p>
        <pre class="rounded-lg bg-gray-50 border border-border p-3 text-[11px] font-mono whitespace-pre-wrap break-all leading-relaxed">${esc(payload.excerpt || "")}</pre>
      ` : submission ? `
        <p class="text-sm">${icon("send", "w-3.5 h-3.5 inline text-muted")} <span class="font-medium">${t("quarantine.submissionProposes")}</span></p>
        <p class="text-xs text-muted leading-relaxed"><span class="font-medium text-foreground">${t("quarantine.venue")}:</span> <span class="font-mono">${esc(payload.venue_id || "")}</span></p>
        <p class="text-xs text-muted leading-relaxed"><span class="font-medium text-foreground">${t("quarantine.stopCondition")}:</span> ${esc(payload.stop_condition || "")}</p>
      ` : itemCheck ? `
        <p class="text-sm"><span class="font-mono text-xs text-muted">${esc(p.targetLabel || "")}</span> <span class="text-muted">→</span> <span class="font-mono text-xs font-medium">${esc(payload.item_id || "")}</span></p>
        ${payload.note ? `<p class="text-xs text-muted leading-relaxed"><span class="font-medium text-foreground">${t("quarantine.evidence")}:</span> <span class="font-mono text-[11px]">${esc(payload.note)}</span></p>` : ""}
      ` : `
        <p class="text-sm"><span class="font-mono text-xs text-muted">${esc(target)}</span> <span class="font-mono text-xs">${esc(payload.from)}</span> <span class="text-muted">→</span> <span class="font-mono text-xs font-medium">${esc(payload.to)}</span></p>
      `}
      <p class="text-xs text-muted leading-relaxed"><span class="font-medium text-foreground">${t("quarantine.basis")}:</span> ${esc(isPin ? (payload.assessing_model || "") : (payload.basis || ""))}</p>
      ${submission || itemCheck ? `<p class="text-[11px] text-muted">${t("quarantine.submissionNote")}</p>` : ""}
      ${p.basisStale ? `
        <div class="rounded-lg border-2 border-amber-300 bg-amber-50 px-3 py-2 flex items-center gap-2">
          ${icon("danger", "w-4 h-4 text-amber-700 shrink-0")}
          <p class="text-xs font-medium text-amber-700">${t("quarantine.basisStale")}</p>
        </div>` : ""}
      <div class="flex items-center gap-2 pt-1">
        ${btn({ label: t("quarantine.reject"), variant: "secondary", size: "sm", onClick: `RC.rejectProposal('${esc(p.id)}')` })}
        ${p.basisStale
          ? btn({ label: t("quarantine.forceApprove"), variant: "destructive", size: "sm", onClick: `RC.approveProposal('${esc(p.id)}', true)` })
          : btn({ label: t("quarantine.approve"), variant: "default", size: "sm", onClick: `RC.approveProposal('${esc(p.id)}', false)` })}
      </div>
    </div>`;
}

function renderMissionMeta(mission) {
  return card(`
    <div class="p-5 space-y-3">
      <h3 class="heading-3">${t("missions.roles.title")}</h3>
      ${mission.roles.map((r) => `
        <div class="flex items-center justify-between text-sm">
          <span class="font-medium">${t(r.name === "drafter" ? "missions.roles.drafter" : "missions.roles.critic")}</span>
          <span class="font-mono text-xs text-muted">${esc(r.provider)} / ${esc(r.model)}</span>
        </div>
      `).join("")}
      <div class="pt-3 border-t border-border">
        ${spendMeter(mission.spendCents, mission.spendCeilingCents, mission.spendState)}
      </div>
      <div class="flex items-center justify-between text-xs text-muted">
        <span>${t("missions.autonomy")}</span>
        <span class="font-medium text-foreground">${t("missions.autonomy." + mission.autonomy)}</span>
      </div>
      ${btn({ label: t("missions.roles.critic"), variant: "outline", size: "sm", iconName: "bolt", cls: "w-full", onClick: `RC.runStep('${esc(mission.id)}','critic')` })}
    </div>`);
}

// ========== SEARCH DISCLOSURE (Story 4.1, FR-12.1) ==========
// The mission's Divulgación card: every search.run row in seq order —
// PRISMA-style, null results named, nothing summarized away.
function renderDisclosure(app, missionId) {
  const d = app.data.disclosure[missionId];
  const rows = d ? d.rows : null;
  return card(`
    <div class="p-5 space-y-3">
      <div>
        <h3 class="heading-3">${t("sd.title")}</h3>
        <p class="text-xs text-muted mt-0.5">${t("sd.sub")}</p>
      </div>
      ${!rows ? `<p class="text-sm text-muted py-3 text-center">${t("rc.common.loading")}</p>` : rows.length === 0 ? `
        <p class="text-sm text-muted py-3 text-center">${t("sd.empty")}</p>` : `
        <div class="flex items-center gap-2">
          ${badge(t("sd.total", { count: d.total }), "muted")}
          ${d.nullResultCount ? badge(t("sd.nulls", { count: d.nullResultCount }), "warning") : ""}
        </div>
        <div class="space-y-2">
          ${rows.map((r) => `
            <div class="rounded-lg border border-border bg-white px-3 py-2.5">
              <div class="flex items-start justify-between gap-2">
                <p class="text-xs leading-snug min-w-0"><span class="font-mono text-[10px] text-muted tabular">#${r.seq}</span> ${esc(r.query)}</p>
                ${r.nullResult ? badge(t("sd.nullResult"), "warning") : ""}
              </div>
              <div class="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1">
                <span class="inline-flex items-center rounded bg-gray-100 px-1.5 py-0.5 font-mono text-[10px] text-muted">${esc(r.database)}</span>
                ${r.order ? `<span class="text-[10px] text-muted">${esc(r.order)}</span>` : ""}
                <span class="text-[10px] text-muted tabular">${fmtTs(r.startedAt)}</span>
                <span class="text-[10px] font-medium ${r.nullResult ? "text-amber-700" : "text-foreground"} tabular">${t("sd.results", { count: r.resultCount })}</span>
                ${r.firstPage ? `<span class="text-[10px] text-muted">${t("sd.firstPage")}</span>` : ""}
              </div>
              ${r.filters && Object.keys(r.filters).length ? `
                <p class="mt-1 font-mono text-[10px] text-muted break-all">${esc(JSON.stringify(r.filters))}</p>` : ""}
            </div>`).join("")}
        </div>`}
    </div>`);
}

// ========== CHECKPOINTS DRAWER (Story 2.6, FR-10.1) ==========
// The board header's restore-point control: create named checkpoints, preview
// a rollback (the orphaned list, never hidden), execute it, and read the
// rollback history — all over the checkpoint.* API commands.
export function renderCheckpointsDrawer(app) {
  const view = app.data.checkpoints;
  const confirming = app.state.rollbackConfirm;
  const outcome = app.data.rollbackOutcome;
  const overlay = document.createElement("div");
  overlay.className = "rc-modal fixed inset-0 z-[60] flex justify-end";
  const cpById = (id) => (view?.checkpoints || []).find((c) => c.id === id);
  overlay.innerHTML = `
    <div class="absolute inset-0 bg-black/30 backdrop-blur-sm" onclick="RC.closeCheckpoints()"></div>
    <div class="relative w-full max-w-md h-full bg-white border-l border-border shadow-xl p-6 overflow-y-auto animate-slide-right">
      <div class="flex items-center justify-between mb-1">
        <h2 class="font-serif text-2xl italic">${t("cp.title")}</h2>
        <button onclick="RC.closeCheckpoints()" class="p-1 rounded hover:bg-gray-100">${icon("close", "w-5 h-5")}</button>
      </div>
      <p class="text-xs text-muted mb-4">${t("cp.sub")}</p>
      ${!view ? `<p class="text-sm text-muted py-8 text-center">${t("rc.common.loading")}</p>` : `
      <div class="space-y-5">
        ${outcome ? `
          <div class="rounded-xl border border-emerald-200 bg-emerald-50 px-4 py-3 flex items-start gap-3">
            ${icon("check", "w-5 h-5 text-emerald-600 shrink-0 mt-0.5")}
            <p class="text-sm text-emerald-700 leading-snug">${t("cp.rolledBack", { name: outcome.rollback.name, count: outcome.rollback.orphanedCount })}</p>
          </div>` : ""}
        <div class="flex items-center justify-between text-xs">
          <span class="caption text-muted">${t("cp.head")}</span>
          <span class="font-mono text-xs tabular">e-${view.headSeq}</span>
        </div>
        <div class="rounded-xl border border-border bg-white p-4 space-y-3">
          <div class="flex gap-2">
            <input id="cp-name" placeholder="${t("cp.namePlaceholder")}" class="flex-1 rounded-lg border border-border bg-white px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30">
            ${btn({ label: t("cp.create"), variant: "default", size: "sm", iconName: "plus", onClick: "RC.createCheckpoint()" })}
          </div>
        </div>
        <div>
          <p class="caption text-muted mb-2">${t("cp.restorePoints")}</p>
          <div class="rounded-xl border border-border divide-y divide-border overflow-hidden bg-white">
            ${view.checkpoints.length ? view.checkpoints.slice().reverse().map((c) => `
              <div class="px-4 py-3">
                <div class="flex items-center justify-between gap-3">
                  <div class="min-w-0">
                    <p class="text-sm font-medium truncate">${esc(c.name)}</p>
                    <p class="font-mono text-[10px] text-muted tabular">e-${c.seq} · ${fmtTs(c.ts)}</p>
                  </div>
                  ${confirming && confirming.checkpointId === c.id ? "" : btn({ label: t("cp.rollback"), variant: "outline", size: "sm", onClick: `RC.confirmRollback('${esc(c.id)}')` })}
                </div>
                ${confirming && confirming.checkpointId === c.id ? (confirming.plan ? renderRollbackConfirm(confirming.plan) : `<p class="mt-3 text-xs text-muted">${t("rc.common.loading")}</p>`) : ""}
              </div>`).join("") : `<p class="px-4 py-6 text-sm text-muted text-center">${t("cp.empty")}</p>`}
          </div>
        </div>
        ${view.rollbacks.length ? `
        <div>
          <p class="caption text-muted mb-2">${t("cp.history")}</p>
          <div class="space-y-1">
            ${view.rollbacks.slice().reverse().map((r) => `
              <div class="flex items-center justify-between gap-2 text-xs">
                <span class="font-mono text-[10px] text-muted tabular">e-${r.seq}</span>
                <span class="text-muted truncate flex-1">${t("cp.historyEntry", { seq: r.seq, name: r.name, count: r.orphanedCount })}</span>
                <span class="font-mono text-[10px] text-muted tabular shrink-0">${fmtTs(r.ts)}</span>
              </div>`).join("")}
          </div>
        </div>` : ""}
      </div>`}
    </div>`;
  document.body.appendChild(overlay);
}

// The two-step confirmation (EXPERIENCE.md): every orphaned event and every
// orphaned proposal, by name, before the rollback is executed.
function renderRollbackConfirm(plan) {
  const cp = plan.checkpoint;
  const orphans = plan.orphanedEvents.length + plan.orphanedProposals.length;
  return `
    <div class="mt-3 rounded-xl border-2 border-amber-300 bg-amber-50 p-4 space-y-3">
      <div>
        <p class="text-xs font-semibold text-amber-700 uppercase tracking-wider mb-1">${t("cp.confirmTitle")}</p>
        <p class="text-xs text-amber-700 leading-relaxed">${t("cp.confirmBody", { name: cp.name, seq: cp.seq, count: plan.orphanedEvents.length + plan.orphanedProposals.length })}</p>
      </div>
      ${orphans === 0 ? `<p class="text-xs text-muted">${t("cp.orphanedNone")}</p>` : `
        <div class="space-y-2">
          ${plan.orphanedEvents.length ? `
            <div>
              <p class="caption text-muted mb-1">${t("cp.orphanedEvents")} · ${plan.orphanedEvents.length}</p>
              <div class="space-y-0.5">
                ${plan.orphanedEvents.map((e) => `
                  <div class="flex items-center justify-between gap-2 text-xs">
                    <span class="font-mono text-[10px] tabular text-muted">e-${e.seq}</span>
                    <span class="font-mono text-[10px] truncate">${esc(e.kind)}</span>
                    <span class="text-[10px] text-muted shrink-0">${esc(e.actor)}</span>
                  </div>`).join("")}
              </div>
            </div>` : ""}
          ${plan.orphanedProposals.length ? `
            <div>
              <p class="caption text-muted mb-1">${t("cp.orphanedProposals")} · ${plan.orphanedProposals.length}</p>
              <div class="space-y-1">
                ${plan.orphanedProposals.map((p) => `
                  <div class="text-xs">
                    <p class="truncate"><span class="font-mono text-[10px] text-muted tabular">pr-${p.seq}</span> ${esc(p.targetLabel || "—")} <span class="font-mono text-[10px]">${esc(p.proposedTo || "")}</span></p>
                    ${p.basis ? `<p class="text-[10px] text-muted truncate">${esc(p.basis)}</p>` : ""}
                  </div>`).join("")}
              </div>
            </div>` : ""}
        </div>`}
      <div class="flex items-center gap-2 pt-1">
        ${btn({ label: t("cp.cancel"), variant: "ghost", size: "sm", onClick: "RC.cancelRollback()" })}
        ${btn({ label: t("cp.confirm"), variant: "destructive", size: "sm", onClick: `RC.executeRollback('${esc(cp.id)}')` })}
      </div>
    </div>`;
}

// ========== READINESS DRAWER ==========
export function renderReadinessDrawer(app) {
  const missionId = app.state.readinessMissionId || app.state.boardMissionId;
  const r = app.data.readiness;
  const overlay = document.createElement("div");
  overlay.className = "rc-modal fixed inset-0 z-[60] flex justify-end";
  const threeState = r ? (r.verdict === "ready" ? "ready" : r.blockers.length === 0 && r.infos.length > 0 ? "near" : "not") : null;
  const stateChip = {
    ready: [t("rd.ready"), "success"],
    near: [t("rd.near"), "warning"],
    not: [t("rd.notReady"), "destructive"],
  };
  overlay.innerHTML = `
    <div class="absolute inset-0 bg-black/30 backdrop-blur-sm" onclick="RC.closeReadiness()"></div>
    <div class="relative w-full max-w-md h-full bg-white border-l border-border shadow-xl p-6 overflow-y-auto animate-slide-right">
      <div class="flex items-center justify-between mb-5">
        <h2 class="font-serif text-2xl italic">${t("rd.title")}</h2>
        <button onclick="RC.closeReadiness()" class="p-1 rounded hover:bg-gray-100">${icon("close", "w-5 h-5")}</button>
      </div>
      ${!r ? `<p class="text-sm text-muted py-8 text-center">${t("rc.common.loading")}</p>` : `
      <div class="space-y-5">
        <div class="flex items-center justify-between">
          <span class="text-sm font-medium">${t("rd.preprintReady")}</span>
          ${badge(stateChip[threeState][0], stateChip[threeState][1])}
        </div>
        <p class="text-xs text-muted">${t("rd.derived")}</p>
        ${r.blockers.length ? `
        <div>
          <p class="caption text-muted mb-2">${t("rd.blockers")}</p>
          <div class="space-y-2">
            ${r.blockers.map((b) => renderReadinessItem(b, true)).join("")}
          </div>
        </div>` : `<p class="text-sm text-emerald-700 font-medium">${t("rd.blockersNone")}</p>`}
        ${r.infos.length ? `
        <div>
          <p class="caption text-muted mb-2">${t("rd.infos")}</p>
          <div class="space-y-2">
            ${r.infos.map((b) => renderReadinessItem(b, false)).join("")}
          </div>
        </div>` : ""}
        <div>
          <p class="caption text-muted mb-2">${t("rd.trail")}</p>
          <div class="rounded-xl border border-border divide-y divide-border overflow-hidden">
            ${r.trail.map((row) => `
              <div class="px-4 py-3 bg-white">
                <div class="flex items-center justify-between gap-2">
                  <span class="text-xs font-medium">${trailLabel(row)}</span>
                  <span class="font-mono text-[10px] tabular text-muted">${row.kind === "merge_queue" ? (row.pending > 0 ? t("rd.t.queue.pending", { count: row.pending }) : t("rd.t.queue.empty")) : row.clean + "/" + row.total}</span>
                </div>
                ${row.refs.length ? `<div class="mt-1 flex flex-wrap gap-1">${row.refs.map((x) => `<span class="inline-flex items-center rounded bg-gray-100 px-1.5 py-0.5 font-mono text-[10px] text-muted">${esc(x)}</span>`).join("")}</div>` : ""}
              </div>
            `).join("")}
          </div>
          <p class="text-[11px] text-muted mt-2">${t("rd.trailNote")}</p>
        </div>
        ${renderTierTwoSection(app)}
      </div>`}
    </div>`;
  document.body.appendChild(overlay);
}

// ========== TIER TWO — the journal-ready verdict (Story 6.11, FR-19.1) ==========
// The drawer's second tier: a venue select over the bundled templates +
// the per-venue checklist verdict — every item referencing the specific
// board/manuscript object that blocks it; human-only items render
// flagged, never auto-passed (the bible's readiness-blocking-item idiom).
function renderTierTwoSection(app) {
  const venues = app.data.venues || [];
  const selected = app.state.readinessVenueId;
  const report = app.data.tierTwo;
  const lang = getLang();
  const venueOf = (id) => venues.find((v) => v.id === id);
  return `
    <div class="border-t border-border pt-4 space-y-3">
      <div class="flex items-center justify-between gap-2">
        <span class="text-sm font-medium">${t("rd.tier2.title")}</span>
        ${report ? badge(report.verdict === "ready" ? t("rd.ready") : t("rd.notReady"), report.verdict === "ready" ? "success" : "destructive") : ""}
      </div>
      <p class="text-[11px] text-muted">${t("rd.tier2.note")}</p>
      ${rcSelect({
        id: "readiness-venue",
        size: "sm",
        cls: "w-full",
        dir: "up",
        options: [{ value: "", label: t("rd.tier2.select") }, ...venues.map((v) => ({ value: v.id, label: v.name }))],
        value: selected || "",
        onChange: "RC.selectReadinessVenue(this.value)",
      })}
      ${selected && !report ? `<p class="text-sm text-muted py-2 text-center">${t("rc.common.loading")}</p>` : ""}
      ${selected && report ? `
      <div class="space-y-1.5">
        ${report.items.map((i) => renderTierTwoItem(i, venueOf(selected), lang)).join("")}
      </div>` : ""}
    </div>`;
}

function renderTierTwoItem(item, venue, lang) {
  // human-only items carry their label in the dataset (data-owned copy)
  const criterion = venue?.criteria.find((c) => c.id === item.criterionId);
  const label = item.kind === "human_only"
    ? ((lang === "en" ? criterion?.labelEn : criterion?.labelEs) || criterion?.labelEs || item.criterionId)
    : t("rd.c." + item.kind);
  const statusChip = {
    pass: [t("rd.tier2.pass"), "success"],
    fail: [t("rd.tier2.fail"), "destructive"],
    human_pending: [t("rd.tier2.humanPending"), "warning"],
    human_confirmed: [t("rd.tier2.humanConfirmed"), "success"],
  }[item.status] || [item.status, "muted"];
  const dot = { pass: "bg-emerald-500", fail: "bg-amber-500", human_pending: "bg-slate-400", human_confirmed: "bg-emerald-500" }[item.status] || "bg-slate-300";
  return `
    <div class="flex items-start gap-2.5 ${item.status === "fail" ? "" : "opacity-90"}">
      <span class="mt-1.5 h-2 w-2 rounded-full shrink-0 ${dot}"></span>
      <div class="min-w-0 flex-1">
        <div class="flex items-start justify-between gap-2">
          <p class="text-sm leading-snug ${item.kind === "human_only" ? "flex items-center gap-1" : ""}">${item.kind === "human_only" ? icon("pen", "w-3 h-3 inline text-muted") + " " : ""}${esc(label)}</p>
          ${badge(statusChip[0], statusChip[1])}
        </div>
        ${item.detail ? `<p class="font-mono text-[10px] text-muted mt-0.5">${esc(item.detail)}</p>` : ""}
        ${item.refs.length ? `<div class="mt-1 flex flex-wrap gap-1">${item.refs.map((x) => `<span class="inline-flex items-center rounded bg-gray-100 px-1.5 py-0.5 font-mono text-[10px] text-muted">${esc(x)}</span>`).join("")}</div>` : ""}
      </div>
    </div>`;
}

// The drawer's tier-2 read (Story 6.11): fold the journal-ready report of
// the selected venue for the drawer's current scope.
async function loadTierTwo(app) {
  const venueId = app.state.readinessVenueId;
  if (!venueId) return;
  try {
    app.data.tierTwo = await api.getJournalReadiness(
      app.state.readinessMissionId || null,
      venueId,
    );
  } catch (e) {
    console.error(e);
    app.data.tierTwo = null;
  }
  ctx.render();
}

function trailLabel(row) {
  if (row.kind === "claims_pinned") return t("rd.t.claims", { clean: row.clean, total: row.total }) + (row.verified ? ` · ${row.verified} ✓` : "");
  if (row.kind === "hypotheses_resolved") return t("rd.t.hyps", { clean: row.clean, total: row.total });
  if (row.kind === "nulls_disclosed") return t("rd.t.nulls", { clean: row.clean, total: row.total });
  if (row.kind === "manuscript_consistency") return t("rd.t.manuscript", { clean: row.clean, total: row.total });
  return t("rd.t.queue.empty");
}

function renderReadinessItem(b, blocking) {
  const objChip = (label, cls = "muted") => `<span class="inline-flex items-center rounded bg-gray-100 px-1.5 py-0.5 font-mono text-[10px] text-muted">${esc(label)}</span>`;
  let title = "";
  let chips = "";
  const msLoc = b.manuscriptFile ? objChip(b.manuscriptFile + ":" + b.manuscriptLine) : "";
  if (b.kind === "unpinned_claim") {
    title = t("rd.b.unpinned");
    chips = objChip("CLAIMS-" + b.claimSeq) + objChip("H-" + b.hypothesisSeq);
  } else if (b.kind === "load_bearing_unresolved") {
    title = t("rd.b.loadUnresolved");
    chips = objChip("H-" + b.hypothesisSeq) + objChip(t("hyp.status." + b.hypothesisStatus));
    if (b.claimTies?.length) chips += objChip(t("rd.ties.claims", { count: b.claimTies.length }));
  } else if (b.kind === "load_bearing_refuted") {
    title = t("rd.b.loadRefuted");
    chips = objChip("H-" + b.hypothesisSeq);
  } else if (b.kind === "unreckoned_null_result") {
    title = t("rd.b.null");
    chips = objChip("#" + b.searchSeq);
  } else if (b.kind === "pin_verification_failed") {
    title = t("rd.i.verificationFailed");
    chips = objChip("CLAIMS-" + b.claimSeq);
  } else if (b.kind === "pin_unsupported") {
    // Story 6.10: "pinned but unsupported by the citing source" — the
    // support verdict surfaces as an info, never a blocker (the pin stays).
    title = t("rd.i.supportUnsupported");
    chips = objChip("CLAIMS-" + b.claimSeq);
  } else if (b.kind === "pin_partially_supported") {
    title = t("rd.i.supportPartial");
    chips = objChip("CLAIMS-" + b.claimSeq);
  } else if (b.kind === "support_unchecked") {
    title = t("rd.i.supportUnchecked");
    chips = objChip("CLAIMS-" + b.claimSeq);
  } else if (b.kind === "merge_queue_pending") {
    title = t("rd.i.mergeQueue", { count: b.pendingCount });
  } else if (b.kind === "manuscript_hypothesis_unresolved" || b.kind === "manuscript_hypothesis_refuted") {
    // Story 6.8 (FR-20.4/20.5): the flag references the hypothesis card
    // AND the manuscript location (file:line + the raw marker)
    title = t(b.kind === "manuscript_hypothesis_unresolved" ? "rd.b.msHypUnresolved" : "rd.b.msHypRefuted");
    chips = objChip("H-" + b.hypothesisSeq) + objChip(t("hyp.status." + b.hypothesisStatus)) + objChip(b.marker) + msLoc;
  } else if (b.kind === "manuscript_claim_unpinned") {
    title = t("rd.b.msClaimUnpinned");
    chips = objChip("CLAIMS-" + b.claimSeq) + objChip("H-" + b.hypothesisSeq) + objChip(b.marker) + msLoc;
  } else if (b.kind === "manuscript_claim_unlinked") {
    title = t("rd.b.msClaimUnlinked");
    chips = objChip(b.marker) + msLoc;
  } else if (b.kind === "manuscript_unreadable") {
    title = t("rd.i.manuscriptUnreadable");
    chips = objChip(b.manuscriptFile || "");
  }
  return `
    <div class="flex items-start gap-2.5 ${blocking ? "" : "opacity-80"}">
      <span class="mt-1.5 h-2 w-2 rounded-full shrink-0 ${blocking ? "bg-amber-500" : "bg-slate-400"}"></span>
      <div class="min-w-0">
        <p class="text-sm leading-snug">${title}</p>
        <div class="mt-1 flex flex-wrap gap-1">${chips}</div>
      </div>
    </div>`;
}

// ========== RECEIPT DRAWER ==========
export function renderReceiptDrawer(app) {
  const runId = app.state.receiptRunId;
  const r = app.data.receipt;
  const overlay = document.createElement("div");
  overlay.className = "rc-modal fixed inset-0 z-[60] flex justify-end";
  const outcomeChip = {
    finished: [t("receipt.outcome.finished"), "success"],
    failed: [t("receipt.outcome.failed"), "destructive"],
    open: [t("receipt.outcome.open"), "warning"],
  };
  const chipClass = {
    run_start: "muted", search: "primary", call: "primary", claim: "medium",
    proposal: "warning", decision: "success", refused: "destructive", released: "low", run_end: "success",
  };
  overlay.innerHTML = `
    <div class="absolute inset-0 bg-black/30 backdrop-blur-sm" onclick="RC.closeReceipt()"></div>
    <div class="relative w-full max-w-md h-full bg-white border-l border-border shadow-xl p-6 overflow-y-auto animate-slide-right">
      <div class="flex items-center justify-between mb-5">
        <h2 class="font-serif text-2xl italic">${t("receipt.title")}</h2>
        <button onclick="RC.closeReceipt()" class="p-1 rounded hover:bg-gray-100">${icon("close", "w-5 h-5")}</button>
      </div>
      ${!r ? (r === null && app.data.receiptFetched ? `
        <div class="rounded-xl border border-border bg-gray-50 p-6 text-center">
          <p class="text-sm text-muted">${t("receipt.none")}</p>
        </div>` : `<p class="text-sm text-muted py-8 text-center">${t("rc.common.loading")}</p>`) : `
      <div class="space-y-4">
        <div class="flex items-center justify-between gap-2">
          <span class="font-mono text-xs text-muted tabular">M-${r.missionSeq} · ${esc(r.runId)}</span>
          ${badge(outcomeChip[r.outcome][0], outcomeChip[r.outcome][1])}
        </div>
        ${r.ceilingHit ? `<div class="rounded-lg border border-amber-300 bg-amber-50 px-3 py-2 flex items-center gap-2">${icon("danger", "w-4 h-4 text-amber-700 shrink-0")}<p class="text-xs font-medium text-amber-700">${t("receipt.ceilingHit")}</p></div>` : ""}
        <div class="grid grid-cols-2 gap-3">
          <div><p class="caption text-muted mb-0.5">${t("receipt.duration")}</p><p class="font-mono text-sm tabular">${r.durationSecs !== null ? r.durationSecs + "s" : "—"}</p></div>
          <div><p class="caption text-muted mb-0.5">${t("trust.spend")}</p><p class="font-mono text-sm tabular">${fmtCents(r.spendCents)} ${t("receipt.spendOf")} ${fmtCents(r.ceilingCents)}</p></div>
        </div>
        ${r.verdict ? `<p class="text-sm"><span class="font-medium">${t("digest.verdict.criterion")}:</span> ${esc(r.verdict)}</p>` : ""}
        ${r.models.length ? `<div class="flex flex-wrap gap-1.5">${r.models.map((m) => `<span class="inline-flex items-center gap-1 rounded-md bg-primary/10 px-2 py-1 text-xs font-medium text-primary font-mono">${esc(m)}</span>`).join("")}</div>` : ""}
        <div>
          <p class="caption text-muted mb-2">${t("receipt.rowsNote")}</p>
          <div class="rounded-xl border border-border divide-y divide-border overflow-hidden bg-white">
            ${r.rows.map((row) => receiptRow(row, chipClass)).join("")}
          </div>
        </div>
      </div>`}
    </div>`;
  document.body.appendChild(overlay);
}

function receiptRow(row, chipClass) {
  let line = "";
  const k = row.kind;
  if (k === "run_start") line = t("receipt.line.run_start", { step: row.step, schedule: row.schedule });
  else if (k === "search") line = t("receipt.line.search", { query: row.query });
  else if (k === "call") line = t("receipt.line.call", { provider: row.provider, model: row.model, in: row.inputTokens, out: row.outputTokens, cost: row.costCents });
  else if (k === "claim") line = t("receipt.line.claim", { text: row.text });
  else if (k === "proposal") line = t("receipt.line.proposal", { to: row.to, seq: row.proposalSeq, status: row.status });
  else if (k === "decision") line = t("receipt.line.decision." + row.decision, { seq: row.proposalSeq });
  else if (k === "refused") line = t("receipt.line.refused", { would: row.wouldBeCostCents, ceiling: row.ceilingCents, scope: row.scope });
  else if (k === "released") line = t("receipt.line.released", { reason: row.reason });
  else if (k === "run_end") line = t("receipt.line.run_end." + row.outcome, { verdict: row.verdict || "", reason: row.reason || "" });
  const refused = k === "refused";
  return `
    <div class="flex items-start justify-between gap-3 px-4 py-2.5 ${refused ? "bg-rose-50" : "bg-white"}">
      <div class="min-w-0">
        <p class="text-xs leading-snug ${refused ? "text-rose-700 font-medium" : "text-foreground"}">${line}</p>
        <p class="font-mono text-[10px] text-muted tabular mt-0.5">e-${row.seq} · ${fmtTs(row.ts)}</p>
      </div>
      <span class="shrink-0">${badge(t("receipt.chip." + k), chipClass[k] || "muted")}</span>
    </div>`;
}

// ========== BOARD BINDINGS (inline input focus preservation) ==========
export function bindBoard(app) {
  // interactions run through RC handlers; nothing to bind here
}

// ========== HANDLERS ==========
async function reloadBoard(app) {
  await ctx.loadBoard(app.state.boardMissionId);
}

Object.assign(RC, {
  toggleQuestionBox() {
    ctx.app.state.questionBoxOpen = !ctx.app.state.questionBoxOpen;
    ctx.renderMainOnly();
  },
  qboxAutonomy(v) {
    const s = ctx.app.state.qbox || (ctx.app.state.qbox = {});
    s.autonomy = v;
    ctx.renderMainOnly();
  },
  // The draft card's autonomy dial (Story 6.15): drafts keep their
  // in-progress form state per mission id.
  draftAutonomy(missionId, v) {
    const app = ctx.app;
    const s = app.state.drafts || (app.state.drafts = {});
    const d = s[missionId] || (s[missionId] = { autonomy: "suggest", ceiling: "10.00" });
    d.autonomy = v;
    ctx.renderMainOnly();
  },
  // Complete a quick-captured draft (Story 6.15, FR-21.3): the owner gives
  // the terminators — only then does the mission launch (FR-1.2).
  async completeCapture(missionId) {
    const app = ctx.app;
    const m = app.data.missions.find((x) => x.id === missionId);
    if (!m || m.status !== "draft") return;
    const s = app.state.drafts?.[missionId] || { autonomy: "suggest", ceiling: "10.00" };
    const read = (id) => document.getElementById(id)?.value.trim() || "";
    const stop = read(`dc-stop-${m.seq}`);
    const criterion = read(`dc-criterion-${m.seq}`);
    const ceiling = read(`dc-ceiling-${m.seq}`) || s.ceiling;
    if (!stop || !criterion) return;
    s.completing = true;
    ctx.renderMainOnly();
    try {
      await api.completeCapturedMission(missionId, stop, criterion, s.autonomy || "suggest", Math.round(parseFloat(ceiling || "10") * 100));
      await ctx.loadMissions();
    } catch (e) {
      alert(t("missions.draft.error") + (e?.message || e));
    }
    s.completing = false;
    ctx.renderMainOnly();
  },
  async launchMission() {
    const app = ctx.app;
    const s = app.state.qbox || {};
    s.question = document.getElementById("qb-question")?.value.trim() || s.question || "";
    s.stop = document.getElementById("qb-stop")?.value.trim() || s.stop || "";
    s.criterion = document.getElementById("qb-criterion")?.value.trim() || s.criterion || "";
    s.ceiling = document.getElementById("qb-ceiling")?.value || s.ceiling || "10.00";
    if (!s.question || !s.stop || !s.criterion) return;
    s.launching = true;
    ctx.renderMainOnly();
    try {
      await api.createMission({
        question: s.question,
        stopCondition: s.stop,
        successCriterion: s.criterion,
        autonomy: s.autonomy || "suggest",
        spendCeilingCents: Math.round(parseFloat(s.ceiling || "10") * 100),
      });
      app.state.questionBoxOpen = false;
      await ctx.loadMissions();
    } catch (e) {
      alert(t("missions.createError") + (e?.message || e));
    }
    s.launching = false;
    ctx.renderMainOnly();
  },
  async firstValue() {
    const app = ctx.app;
    const url = document.getElementById("first-value-url")?.value.trim();
    if (!url) return;
    app.state.wizard.initUrl = url;
    app.state.wizard.initLoading = true;
    ctx.renderMainOnly();
    try {
      app.state.wizard.initResult = await api.runFirstValue(url);
      await ctx.loadMissions();
    } catch (e) {
      alert(t("onb.error") + " " + (e?.message || e));
    }
    app.state.wizard.initLoading = false;
    ctx.renderMainOnly();
  },
  openBoard(missionId) {
    const app = ctx.app;
    app.state.boardMissionId = missionId;
    app.state.view = "board";
    ctx.render();
    ctx.loadBoard(missionId);
  },
  async toggleRuns(missionId) {
    const app = ctx.app;
    const open = !app.data.runsOpen[missionId];
    app.data.runsOpen[missionId] = open;
    if (open && !app.data.runs[missionId]) await ctx.loadRuns(missionId);
    else ctx.renderMainOnly();
  },
  async openReadiness(missionId) {
    const app = ctx.app;
    app.state.readinessOpen = true;
    app.state.readinessMissionId = missionId || null;
    app.data.readiness = undefined;
    ctx.render();
    // the venue select's options (Story 6.11): the bundled dataset, loaded
    // lazily the first time the drawer opens
    const venuesLoaded = app.data.venues !== null && app.data.venues !== undefined;
    const [report] = await Promise.all([
      api.getReadinessReport(missionId || null).catch((e) => {
        console.error(e);
        return null;
      }),
      venuesLoaded ? Promise.resolve(null) : api.listVenues().then((vs) => {
        app.data.venues = vs;
      }).catch((e) => {
        console.error(e);
        app.data.venues = [];
      }),
    ]);
    app.data.readiness = report;
    if (app.state.readinessVenueId) await loadTierTwo(app);
    else ctx.render();
  },
  // The Publication surface's affordance (Story 6.11): open the drawer with
  // the venue preselected — the two-tier verdict in one move.
  async openReadinessWithVenue(venueId) {
    const app = ctx.app;
    app.state.readinessVenueId = venueId || null;
    await RC.openReadiness(app.state.boardMissionId || null);
  },
  // The drawer's venue select (Story 6.11, FR-19.1): selecting a venue
  // folds the tier-2 report of the current scope for that venue.
  async selectReadinessVenue(venueId) {
    const app = ctx.app;
    app.state.readinessVenueId = venueId || null;
    app.data.tierTwo = undefined;
    ctx.render();
    if (venueId) await loadTierTwo(app);
    else ctx.render();
  },
  closeReadiness() {
    ctx.app.state.readinessOpen = false;
    ctx.render();
  },
  async openReceipt(runId) {
    const app = ctx.app;
    app.state.receiptRunId = runId;
    app.data.receipt = undefined;
    app.data.receiptFetched = false;
    ctx.render();
    try {
      app.data.receipt = await api.getRunReceipt(runId);
    } catch (e) {
      console.error(e);
      app.data.receipt = null;
    }
    app.data.receiptFetched = true;
    ctx.render();
  },
  closeReceipt() {
    ctx.app.state.receiptRunId = null;
    ctx.render();
  },
  // ---- Checkpoints (Story 2.6, FR-10.1): the restore-point control ----
  async openCheckpoints() {
    const app = ctx.app;
    app.state.checkpointsOpen = true;
    app.state.rollbackConfirm = null;
    app.data.rollbackOutcome = null;
    app.data.checkpoints = undefined;
    ctx.render();
    try {
      app.data.checkpoints = await api.listCheckpoints();
    } catch (e) {
      console.error(e);
      app.data.checkpoints = null;
    }
    ctx.render();
  },
  closeCheckpoints() {
    const app = ctx.app;
    app.state.checkpointsOpen = false;
    app.state.rollbackConfirm = null;
    app.data.rollbackOutcome = null;
    ctx.render();
  },
  async createCheckpoint() {
    const app = ctx.app;
    const name = document.getElementById("cp-name")?.value.trim() || "";
    if (!name) return;
    try {
      await api.createCheckpoint(name);
      app.data.checkpoints = await api.listCheckpoints();
    } catch (e) {
      alert(t("onb.error") + " " + (e?.message || e));
    }
    ctx.render();
    const input = document.getElementById("cp-name");
    if (input) input.value = "";
  },
  async confirmRollback(checkpointId) {
    const app = ctx.app;
    app.state.rollbackConfirm = { checkpointId, plan: null };
    ctx.render();
    try {
      app.state.rollbackConfirm = { checkpointId, plan: await api.previewRollback(checkpointId) };
    } catch (e) {
      console.error(e);
      app.state.rollbackConfirm = null;
      alert(t("onb.error") + " " + (e?.message || e));
    }
    ctx.render();
  },
  cancelRollback() {
    ctx.app.state.rollbackConfirm = null;
    ctx.render();
  },
  async executeRollback(checkpointId) {
    const app = ctx.app;
    try {
      app.data.rollbackOutcome = await api.rollbackToCheckpoint(checkpointId);
      app.state.rollbackConfirm = null;
      app.data.checkpoints = await api.listCheckpoints();
      // the board + missions are new folds of the restored state
      app.data.missionsLoaded = false;
      await ctx.loadMissions();
      if (app.state.boardMissionId) await ctx.loadBoard(app.state.boardMissionId);
    } catch (e) {
      alert(t("onb.error") + " " + (e?.message || e));
    }
    ctx.render();
  },
  async addHypothesis() {
    const app = ctx.app;
    const el = document.getElementById("new-hyp");
    const statement = el?.value.trim();
    if (!statement) return;
    try {
      await api.createHypothesis(statement, app.state.boardMissionId);
      el.value = "";
      await reloadBoard(app);
    } catch (e) {
      alert(t("hyp.createError") + (e?.message || e));
    }
  },
  async transitionHyp(hypothesisId, to) {
    if (to.startsWith("→ ")) to = to.slice(2);
    const app = ctx.app;
    const basis = window.prompt(t("hyp.basisPh"));
    if (basis === null) { ctx.renderMainOnly(); return; }
    if (!basis.trim()) { alert(t("hyp.basisRequired")); ctx.renderMainOnly(); return; }
    try {
      await api.transitionHypothesis(hypothesisId, to, basis.trim());
      await reloadBoard(app);
    } catch (e) {
      alert(t("hyp.transitionError") + (e?.message || e));
    }
  },
  async addClaim(hypothesisId) {
    const app = ctx.app;
    const el = document.getElementById("claim-" + hypothesisId);
    const text = el?.value.trim();
    if (!text) return;
    try {
      await api.registerClaim(hypothesisId, text, null);
      el.value = "";
      await reloadBoard(app);
    } catch (e) {
      alert(t("ev.claimError") + (e?.message || e));
    }
  },
  async verifyPins() {
    const app = ctx.app;
    try {
      await api.runPinVerification(app.state.boardMissionId);
      await reloadBoard(app);
    } catch (e) {
      alert(t("ev.verifyError") + (e?.message || e));
    }
  },
  // Story 6.9 (FR-23.1): run the entailment checks on the board's pins —
  // an LLM run through the provider layer (the SANCTIONED counterpart of
  // the no-LLM existence verifier), one pin.support_checked event per
  // judgment. The honest rollup surfaces skips (a pin with no different
  // model available is skipped, never faked).
  async checkSupport() {
    const app = ctx.app;
    try {
      const summary = await api.runPinSupportChecks(app.state.boardMissionId);
      const skipped = summary.records.filter((r) => r.skip).length;
      if (skipped > 0) {
        alert(t("ev.support.skippedToast", { count: skipped }));
      }
      await reloadBoard(app);
    } catch (e) {
      alert(t("ev.support.error") + (e?.message || e));
    }
  },
  async runStep(missionId, role) {
    const task = window.prompt(role === "drafter" ? t("missions.roles.drafter") : t("missions.roles.critic"), role === "drafter" ? "advance the mission" : "review the drafter's claims");
    if (!task || !task.trim()) return;
    try {
      await api.runAgentStep(missionId, role, task.trim());
      await reloadBoard(ctx.app);
      await ctx.loadMissions();
    } catch (e) {
      alert((e?.message || e));
    }
  },
  async approveProposal(proposalId, force) {
    try {
      await api.approveProposal(proposalId, force);
      await reloadBoard(ctx.app);
      await ctx.loadMissions();
    } catch (e) {
      alert(t("quarantine.actionError") + (e?.message || e));
    }
  },
  async rejectProposal(proposalId) {
    try {
      await api.rejectProposal(proposalId);
      await reloadBoard(ctx.app);
    } catch (e) {
      alert(t("quarantine.actionError") + (e?.message || e));
    }
  },
  // ---- Manuscript (Stories 6.6–6.7, FR-20) ----
  // Open one .tex source in the editor (the repo IS the manuscript — the
  // read serves the user's file; the save writes it in place).
  async msOpenFile(missionId, file) {
    const app = ctx.app;
    try {
      const view = await api.readManuscriptFile(missionId, file);
      app.state.ms = { missionId, file, content: view.content, draft: view.content, saving: false, savingFlag: false, compiling: false, saved: false };
      ctx.renderMainOnly();
    } catch (e) {
      alert((e?.message || e));
    }
  },
  // The draft updates WITHOUT a rerender — the cursor survives typing.
  msDraft(value) {
    const ms = ctx.app.state.ms;
    if (ms) ms.draft = value;
  },
  async msSave(missionId) {
    const app = ctx.app;
    const ms = app.state.ms;
    if (!ms || ms.saving) return;
    ms.saving = true;
    ctx.renderMainOnly();
    try {
      await api.writeManuscriptFile(missionId, ms.file, ms.draft);
      ms.content = ms.draft;
      ms.saved = true;
      await ctx.loadManuscript(missionId); // the scan's word counts refresh
    } catch (e) {
      alert((e?.message || e));
    }
    ms.saving = false;
    ctx.renderMainOnly();
  },
  async msCompile(missionId) {
    const app = ctx.app;
    if (app.state.ms) app.state.ms.compiling = true;
    ctx.renderMainOnly();
    try {
      await api.compileManuscript(missionId);
      await ctx.loadManuscript(missionId);
    } catch (e) {
      alert((e?.message || e));
    }
    if (app.state.ms) app.state.ms.compiling = false;
    ctx.renderMainOnly();
  },
  // Story 6.7 (FR-20.3): merge/reject an agent's quarantined LaTeX diff —
  // the merge applies the hunks to the user's file (a file backup + a log
  // checkpoint land first); the post-merge compile check is one click.
  async msDiffApprove(proposalId, force) {
    const app = ctx.app;
    try {
      await api.approveManuscriptDiff(proposalId, force);
      if (app.state.boardMissionId) await ctx.loadManuscript(app.state.boardMissionId);
    } catch (e) {
      alert(t("quarantine.actionError") + (e?.message || e));
    }
  },
  async msDiffReject(proposalId) {
    const app = ctx.app;
    try {
      await api.rejectManuscriptDiff(proposalId);
      if (app.state.boardMissionId) await ctx.loadManuscript(app.state.boardMissionId);
    } catch (e) {
      alert(t("quarantine.actionError") + (e?.message || e));
    }
  },
  // ---- Evidence pin composer (FR-3.3): citation (library ref) or
  // numerical (artifact ref) — the bible's modal idiom over the core
  // pin_claim_to_citation / pin_claim_to_numerical commands.
  async openPinForm(claimId, hypothesisId) {
    const app = ctx.app;
    const form = (app.state.pinForm = { claimId, hypothesisId, kind: "citation", saving: false });
    if (!app.data.refs.length) {
      try {
        app.data.refs = await api.listRefs(app.data.project?.id || "p1", null);
      } catch (e) {
        /* keep empty — the ref select falls back to a text input */
      }
    }
    RCPinModal(app, form);
  },
  setPinKind(kind) {
    const app = ctx.app;
    if (app.state.pinForm) app.state.pinForm.kind = kind;
    RCPinModal(app, app.state.pinForm);
  },
  async submitPin() {
    const app = ctx.app;
    const f = app.state.pinForm;
    if (!f || f.saving) return;
    const read = (id) => document.getElementById(id)?.value.trim() || "";
    const excerpt = f.kind === "citation" ? read("pin-excerpt") : read("pin-content");
    const confidence = parseFloat(read("pin-confidence")) || 0.5;
    const model = read("pin-model") || "GLM-5.3";
    if (!excerpt || !model) return;
    f.saving = true;
    try {
      if (f.kind === "citation") {
        const refId = read("pin-ref");
        if (!refId) return;
        await api.pinClaimToCitation(f.claimId, f.hypothesisId, refId, excerpt, confidence, model);
      } else {
        const artifact = read("pin-artifact");
        if (!artifact) return;
        await api.pinClaimToNumerical(f.claimId, f.hypothesisId, artifact, excerpt, confidence, model);
      }
      RC.closeRcModal();
      await reloadBoard(app);
    } catch (e) {
      f.saving = false;
      alert(t("ev.pinError") + (e?.message || e));
      RCPinModal(app, f);
    }
  },
  async openRelateForm(hypId) {
    const app = ctx.app;
    const b = app.data.board[app.state.boardMissionId];
    const others = (b?.hyps || []).filter((h) => h.id !== hypId);
    const overlay = document.createElement("div");
    overlay.className = "rc-modal fixed inset-0 z-[60] overflow-y-auto bg-black/40 backdrop-blur-sm";
    overlay.innerHTML = `
      <div class="min-h-full flex items-center justify-center p-6">
      <div class="w-full max-w-md rounded-xl border border-border bg-white shadow-xl p-6 animate-scale-in">
        <div class="flex items-center justify-between mb-5"><h3 class="font-semibold text-lg">${t("hyp.relate")}</h3><button onclick="RC.closeRcModal()" class="p-1 rounded hover:bg-gray-100">${icon("close", "w-5 h-5")}</button></div>
        ${others.length ? `
        <div class="space-y-4">
          <div><label class="block text-sm font-medium mb-1.5">${t("hyp.relateTo")}</label>${rcSelect({ id: "relate-to", options: others.map((o) => ({ value: o.id, label: `H-${o.seq} — ${o.statement.slice(0, 60)}` })) })}</div>
          <div><label class="block text-sm font-medium mb-1.5">${t("hyp.relationKind")}</label>${rcSelect({ id: "relate-kind", options: ["contradicts", "extends", "specializes", "supports_the_same_claim"].map((k) => ({ value: k, label: t("hyp.relKind." + k) })) })}</div>
        </div>
        <div class="mt-6 flex justify-end gap-2">
          ${btn({ label: t("rc.common.cancel"), variant: "ghost", onClick: "RC.closeRcModal()" })}
          ${btn({ label: t("hyp.relate"), variant: "default", onClick: `RC.submitRelation('${esc(hypId)}')` })}
        </div>` : `<p class="text-sm text-muted text-center py-6">${t("hyp.empty")}</p>`}
      </div>
      </div>`;
    document.body.appendChild(overlay);
    overlay.onclick = (e) => { if (e.target === overlay) RC.closeRcModal(); };
  },
  async submitRelation(fromId) {
    const to = document.getElementById("relate-to")?.value;
    const kind = document.getElementById("relate-kind")?.value;
    if (!to || !kind) return;
    try {
      await api.addRelation(fromId, to, kind);
      RC.closeRcModal();
      await reloadBoard(ctx.app);
    } catch (e) {
      alert(t("hyp.relationError") + (e?.message || e));
    }
  },
  closeRcModal() {
    document.querySelectorAll(".rc-modal").forEach((el) => el.remove());
    ctx.app.state.pinForm = null;
  },
});

function RCPinModal(app, f) {
  if (!f) return;
  document.querySelectorAll(".rc-modal").forEach((el) => el.remove());
  // FR-15.5 (Epic 5): a removed ref is never pinnable — the picker only
  // offers ACTIVE refs (the core refuses removed ids with the typed
  // `ref_removed:` error if one is forced through).
  const refs = (app.data.refs || []).filter((r) => !r.removed);
  const overlay = document.createElement("div");
  overlay.className = "rc-modal fixed inset-0 z-[60] overflow-y-auto bg-black/40 backdrop-blur-sm";
  const kindToggle = `
    <div class="grid grid-cols-2 gap-1 rounded-lg border border-border bg-gray-50 p-1">
      ${[["citation", t("ev.pin")], ["numerical", t("ev.pinArtifact")]].map(([k, l]) => `
        <button type="button" onclick="RC.setPinKind('${k}')" class="rounded-md py-1.5 text-xs font-medium transition ${f.kind === k ? "bg-white shadow-sm text-foreground" : "text-muted hover:text-foreground"}">${l}</button>`).join("")}
    </div>`;
  const body = f.kind === "citation"
    ? refs.length
      ? `<div><label class="block text-sm font-medium mb-1.5">${t("ev.refPick")}</label>${rcSelect({ id: "pin-ref", options: refs.map((r) => ({ value: r.id, label: `${r.year} — ${r.title.slice(0, 48)}` })) })}</div>`
      : `<div><label class="block text-sm font-medium mb-1.5">${t("ev.artifact")}</label><input id="pin-ref" placeholder="${t("ev.refPick")}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>`
    : `<div><label class="block text-sm font-medium mb-1.5">${t("ev.artifact")}</label><input id="pin-artifact" placeholder="${t("ev.artifactPh")}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>`;
  const contentField = f.kind === "citation"
    ? `<div><label class="block text-sm font-medium mb-1.5">${t("ev.excerpt")}</label><textarea id="pin-excerpt" rows="3" placeholder="${t("ev.excerptPh")}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"></textarea><p class="text-xs text-muted mt-1">${t("ev.excerptPrefill")}</p></div>`
    : `<div><label class="block text-sm font-medium mb-1.5">${t("ev.contentQuoted")}</label><textarea id="pin-content" rows="3" placeholder="${t("ev.excerptPh")}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"></textarea></div>`;
  overlay.innerHTML = `
    <div class="min-h-full flex items-center justify-center p-6">
    <div class="w-full max-w-md rounded-xl border border-border bg-white shadow-xl p-6 animate-scale-in">
      <div class="flex items-center justify-between mb-5"><h3 class="font-semibold text-lg">${t("ev.pin")}</h3><button onclick="RC.closeRcModal()" class="p-1 rounded hover:bg-gray-100">${icon("close", "w-5 h-5")}</button></div>
      <div class="space-y-4">
        ${kindToggle}
        ${body}
        ${contentField}
        <div class="grid grid-cols-2 gap-4">
          <div><label class="block text-sm font-medium mb-1.5">${t("ev.confidence")}</label>${rcSelect({ id: "pin-confidence", options: [{ value: "0.25", label: "0.25" }, { value: "0.5", label: "0.50" }, { value: "0.75", label: "0.75" }, { value: "0.9", label: "0.90" }], value: "0.5" })}</div>
          <div><label class="block text-sm font-medium mb-1.5">${t("ev.model")}</label><input id="pin-model" value="GLM-5.3" placeholder="${t("ev.modelPh")}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"></div>
        </div>
      </div>
      <div class="mt-6 flex justify-end gap-2">
        ${btn({ label: t("rc.common.cancel"), variant: "ghost", onClick: "RC.closeRcModal()" })}
        ${btn({ label: f.saving ? t("ev.pinning") : t("ev.pin"), variant: "default", onClick: "RC.submitPin()", disabled: f.saving })}
      </div>
    </div>
    </div>`;
  document.body.appendChild(overlay);
  overlay.onclick = (e) => { if (e.target === overlay) RC.closeRcModal(); };
  const claim = document.getElementById("pin-excerpt") || document.getElementById("pin-content");
  if (claim) {
    const src = app.data.board?.[app.state.boardMissionId]?.hyps
      ?.flatMap((h) => app.data.board[app.state.boardMissionId].evidence[h.id] || [])
      ?.find((c) => c.id === f.claimId);
    if (src && document.getElementById("pin-excerpt")) document.getElementById("pin-excerpt").value = src.text;
  }
}
