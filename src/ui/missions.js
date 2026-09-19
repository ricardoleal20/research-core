// The v1 agent surfaces, built in the bible's exact design language:
// missions home (question box + mission cards), the hypothesis board with
// evidence pins and quarantine review, the readiness drawer, and the run
// receipt drawer. All data flows through api.ts; every mutation is a core
// command. Visual anatomy per DESIGN.md: mission-card, hypothesis-card,
// evidence-pin, quarantine-diff, spend-meter, digest-item,
// readiness-blocking-item.
import { t } from "../i18n";
import { api } from "../api";
import { icon, esc, badge, btn, card, rcSelect, pageHeader, fmtCents, fmtTs } from "./helpers";
import { RC, ctx } from "./rc";

const lifecycleChip = {
  proposed: ["primary", "proposed"],
  testing: ["warning", "testing"],
  supported: ["success", "supported"],
  refuted: ["destructive", "refuted"],
  revised: ["primary", "revised"],
};
const statusColor = {
  active: "primary",
  awaiting_review: "warning",
  completed: "success",
  stopped: "muted",
  failed: "destructive",
};

const spendMeterFill = (state) =>
  state === "blocked" ? "bg-rose-500" : state === "near" ? "bg-amber-500" : "bg-primary";

const spendMeter = (spendCents, ceilingCents, state) => {
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
              <input id="first-value-url" value="${esc(w.initUrl)}" placeholder="https://arxiv.org/abs/1706.03762" class="flex-1 rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30">
              ${btn({ label: w.initLoading ? t("onb.generating") : t("onb.generate"), variant: "default", iconName: "sparkle", onClick: "RC.firstValue()", disabled: w.initLoading })}
            </div>
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

function renderRunsDrill(app, missionId, runs) {
  return `
    <div class="mt-3 rounded-xl border border-border bg-gray-50/50 p-4 animate-fade-up">
      <p class="caption text-muted mb-2">${t("missions.runs")}</p>
      ${runs.length ? `
        <div class="space-y-1">
          ${runs.map((r) => `
            <div class="flex items-center justify-between gap-3 rounded-lg px-2 py-1.5 hover:bg-gray-100 transition">
              <span class="font-mono text-xs tabular text-muted truncate">e-${r.seq} · ${esc(r.kind)} · ${esc(r.actor)}${r.role ? " · " + esc(r.role) : ""}</span>
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
            </div>
          </div>
          ${renderAddHypothesis()}
          ${b.hyps.length ? b.hyps.map((h, i) => renderHypothesisCard(app, h, b.evidence[h.id] || [], i)).join("") : card(`<div class="p-8 text-center text-muted text-sm">${t("hyp.empty")}</div>`)}
        </div>
        <div class="space-y-5">
          ${renderQuarantine(app, pending, decided)}
          ${renderMissionMeta(mission)}
        </div>
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

function renderEvidenceRows(h, claims) {
  return `
    <div class="space-y-1.5 rounded-xl border border-border bg-gray-50/50 p-3">
      <p class="caption text-muted">${t("ev.claims")}</p>
      ${claims.map((c) => `
        <div class="rounded-lg bg-white border border-border px-3 py-2">
          <div class="flex items-start justify-between gap-2">
            <p class="text-xs leading-snug"><span class="font-mono text-[10px] text-muted tabular">CLAIMS-${c.seq}</span> ${esc(c.text)}</p>
            ${c.pinned ? "" : badge(t("ev.unpinned"), "warning")}
          </div>
          ${c.pinned && c.pin ? `
            <div class="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1">
              ${c.pin.kind === "citation"
                ? `<span class="inline-flex items-center gap-1 rounded-md bg-primary/10 px-2 py-1 text-xs font-medium text-primary">${icon("book", "w-3 h-3")} ${esc(c.pin.refLabel || c.pin.refId || "")}</span>`
                : `<span class="inline-flex items-center gap-1 rounded-md bg-slate-100 px-2 py-1 text-xs font-medium text-slate-700">${icon("fileText", "w-3 h-3")} <span class="font-mono">${esc((c.pin.digest || "").slice(0, 10))}…</span></span>`}
              <span class="text-[10px] text-muted">${t("ev.confidence")} <span class="font-mono tabular">${(c.pin.confidence * 100).toFixed(0)}%</span> · ${esc(c.pin.assessingModel)}</span>
              ${verificationChip(c.pin.verification)}
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
      ` : `
        <p class="text-sm"><span class="font-mono text-xs text-muted">${esc(target)}</span> <span class="font-mono text-xs">${esc(payload.from)}</span> <span class="text-muted">→</span> <span class="font-mono text-xs font-medium">${esc(payload.to)}</span></p>
      `}
      <p class="text-xs text-muted leading-relaxed"><span class="font-medium text-foreground">${t("quarantine.basis")}:</span> ${esc(isPin ? (payload.assessing_model || "") : (payload.basis || ""))}</p>
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
      </div>`}
    </div>`;
  document.body.appendChild(overlay);
}

function trailLabel(row) {
  if (row.kind === "claims_pinned") return t("rd.t.claims", { clean: row.clean, total: row.total }) + (row.verified ? ` · ${row.verified} ✓` : "");
  if (row.kind === "hypotheses_resolved") return t("rd.t.hyps", { clean: row.clean, total: row.total });
  if (row.kind === "nulls_disclosed") return t("rd.t.nulls", { clean: row.clean, total: row.total });
  return t("rd.t.queue.empty");
}

function renderReadinessItem(b, blocking) {
  const objChip = (label, cls = "muted") => `<span class="inline-flex items-center rounded bg-gray-100 px-1.5 py-0.5 font-mono text-[10px] text-muted">${esc(label)}</span>`;
  let title = "";
  let chips = "";
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
  } else if (b.kind === "merge_queue_pending") {
    title = t("rd.i.mergeQueue", { count: b.pendingCount });
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
    try {
      app.data.readiness = await api.getReadinessReport(missionId || null);
    } catch (e) {
      console.error(e);
      app.data.readiness = null;
    }
    ctx.render();
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
});
