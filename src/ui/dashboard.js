// The dashboard home panel (Story 5.10, FR-18): a READ-ONLY composition
// over the one aggregated `getDashboardSummary` fold — six widgets in the
// bible card idiom, each with an honest empty state (FR-18.8, NFR-9). No
// new domain model, no writes: a dashboard render never creates events
// (FR-18.1, AD-1); every drill-down reuses the existing surfaces (board,
// digest, receipts drawer, readiness drawer).
import { t } from "../i18n";
import { icon, esc, badge, btn, card, pageHeader, fmtCents, fmtTs } from "./helpers";
import { lifecycleChip, statusColor, spendMeter } from "./missions";

const MISSION_STATUSES = ["active", "awaiting_review", "completed", "stopped", "failed"];
const HYP_STATUSES = ["proposed", "testing", "supported", "refuted", "revised"];

// The trust center's ceiling meter idiom (FR-18.5): ok/near/blocked fill,
// the honest "sin techo" chip when no ceiling is configured.
const ceilingMeter = (spendCents, ceilingCents) => {
  if (ceilingCents === null || ceilingCents === undefined) {
    return `<span class="text-xs text-muted font-mono">${t("trust.unset")}</span>`;
  }
  const pct = ceilingCents > 0 ? Math.min(100, Math.round((spendCents / ceilingCents) * 100)) : 0;
  const cls = pct >= 100 ? "bg-rose-500" : pct >= 80 ? "bg-amber-500" : "bg-emerald-500";
  return `
    <div class="flex-1 min-w-[100px]">
      <div class="flex items-baseline justify-between gap-2 mb-1">
        <span class="font-mono text-[10px] tabular text-muted">${fmtCents(spendCents)} ${t("trust.of")} ${fmtCents(ceilingCents)}</span>
      </div>
      <div class="h-1.5 rounded-full bg-border overflow-hidden"><div class="h-full rounded-full ${cls}" style="width:${pct}%"></div></div>
    </div>`;
};

// The digest row's one-line verdict (FR-18.4): the same composition the
// digest screen renders, compacted for the teaser.
const digestVerdict = (r) => {
  if (r.failed > 0) return t("digest.verdict.failed", { reason: r.failureReason || "?" });
  if (r.ceilingReached) return t("digest.verdict.ceiling");
  if (r.jobsFinished || r.jobsFailed)
    return r.jobsFailed
      ? t("digest.job.failed", { target: r.jobVerdict?.target || "", job: (r.jobVerdict?.jobId || "").slice(0, 8), reason: r.jobVerdict?.reason || "?" })
      : t("digest.verdict.jobs", { count: r.jobsFinished });
  return t("digest.verdict.ok", { runs: r.runs, proposals: r.proposalsPending });
};

const widgetTitle = (label) => `<h3 class="heading-3 mb-3">${esc(label)}</h3>`;

const emptyState = (key) =>
  `<p class="text-sm text-muted py-6 text-center leading-relaxed">${t(key)}</p>`;

// ========== WIDGET 1 (FR-18.2): missions overview ==========
function renderMissionsWidget(summary) {
  const missions = summary.missions;
  const counts = MISSION_STATUSES.map((s) => ({ s, n: missions.filter((m) => m.status === s).length }));
  const active = missions.filter((m) => m.status === "active" || m.status === "awaiting_review");
  return card(`
    <div class="p-6 space-y-4">
      <div class="flex items-start justify-between gap-3">
        ${widgetTitle(t("dash.missions.title"))}
        <button onclick="RC.navigate('missions')" class="text-xs font-medium text-primary hover:underline shrink-0">${t("dash.missions.viewAll")}</button>
      </div>
      ${missions.length ? `
        <div class="flex flex-wrap gap-2">
          ${counts.map(({ s, n }) => `
            <span class="inline-flex items-center gap-1.5">
              ${badge(t("missions.status." + s), statusColor[s] || "muted")}
              <span class="font-mono text-xs tabular text-muted">${n}</span>
            </span>`).join("")}
        </div>
        ${active.length ? `
          <div class="space-y-3 pt-1">
            <p class="caption text-muted">${t("dash.missions.active")}</p>
            ${active.map((m) => `
              <div class="rounded-xl border border-border bg-gray-50/50 p-4 space-y-2">
                <div class="flex items-center gap-2">
                  <span class="font-mono text-[10px] text-muted tabular shrink-0">M-${m.seq}</span>
                  <p class="text-sm font-medium truncate cursor-pointer hover:text-primary transition" onclick="RC.openBoard('${esc(m.id)}')">${esc(m.question)}</p>
                  ${badge(t("missions.status." + m.status), statusColor[m.status] || "muted")}
                </div>
                ${spendMeter(m.spendCents, m.spendCeilingCents, m.spendState)}
              </div>`).join("")}
          </div>` : ""}` : emptyState("dash.missions.empty")}
    </div>`, "lg:col-span-2");
}

// ========== WIDGET 2 (FR-18.3): board health ==========
function renderBoardWidget(summary) {
  const hyps = summary.hypotheses;
  const byStatus = HYP_STATUSES.map((s) => ({ s, n: hyps.filter((h) => h.status === s).length }));
  const byMission = new Map();
  for (const h of hyps) {
    if (!byMission.has(h.missionId)) byMission.set(h.missionId, []);
    byMission.get(h.missionId).push(h);
  }
  const missionById = new Map(summary.missions.map((m) => [m.id, m]));
  return card(`
    <div class="p-6 space-y-4">
      ${widgetTitle(t("dash.board.title"))}
      ${hyps.length ? `
        <div class="flex flex-wrap gap-2">
          ${byStatus.map(({ s, n }) => `
            <span class="inline-flex items-center gap-1.5">
              ${badge(t("hyp.status." + s), lifecycleChip[s]?.[0] || "muted")}
              <span class="font-mono text-xs tabular text-muted">${n}</span>
            </span>`).join("")}
        </div>
        <div class="space-y-2 pt-1">
          ${[...byMission.entries()].map(([missionId, mh]) => {
            const m = missionById.get(missionId);
            return `
            <div class="flex items-center justify-between gap-3 rounded-lg px-2 py-1.5 hover:bg-gray-50 transition">
              <div class="min-w-0 flex items-center gap-2">
                ${m ? `<span class="font-mono text-[10px] text-muted tabular shrink-0">M-${m.seq}</span>` : ""}
                <p class="text-xs text-muted truncate">${esc(m ? m.question : missionId)}</p>
              </div>
              <button onclick="RC.openBoard('${esc(missionId)}')" class="text-xs font-medium text-primary hover:underline shrink-0">${t("dash.board.viewBoard")}</button>
            </div>`;
          }).join("")}
        </div>` : emptyState("dash.board.empty")}
    </div>`);
}

// ========== WIDGET 3 (FR-18.4): latest digest teaser ==========
function renderDigestWidget(summary) {
  const d = summary.digest;
  return card(`
    <div class="p-6 space-y-3">
      <div class="flex items-start justify-between gap-3">
        ${widgetTitle(t("dash.digest.title"))}
        <button onclick="RC.navigate('digest')" class="text-xs font-medium text-primary hover:underline shrink-0">${t("dash.digest.viewFull")}</button>
      </div>
      ${d.rows.length ? `
        <div class="divide-y divide-border -mx-2">
          <div class="px-2 pb-2 flex items-center justify-between gap-2">
            ${badge(t("digest.outcome." + d.outcome), d.outcome === "all_finished" ? "success" : d.outcome === "partial_success" ? "warning" : d.outcome === "all_failed" ? "destructive" : "muted")}
            <span class="font-mono text-xs tabular text-muted">${fmtCents(d.spendCents)} ${t("digest.ofCeiling", { ceiling: fmtCents(d.ceilingCents) })}</span>
            <span class="caption text-muted ml-auto">${fmtTs(d.generatedAt)}</span>
          </div>
          ${d.rows.slice(0, 3).map((r) => `
            <div class="px-2 py-3">
              <div class="flex items-center justify-between gap-3">
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    <span class="font-mono text-[10px] text-muted tabular shrink-0">M-${r.missionSeq}</span>
                    <p class="text-sm font-medium truncate">${esc(r.question)}</p>
                  </div>
                  <p class="text-xs text-muted mt-0.5 truncate">${digestVerdict(r)}</p>
                </div>
                <div class="flex items-center gap-2 shrink-0">
                  ${badge(t("missions.status." + r.status), statusColor[r.status] || "muted")}
                  <button onclick="RC.openReceipt('${esc(r.runId)}')" class="text-xs font-medium text-primary hover:underline">${t("digest.receipts")}</button>
                </div>
              </div>
            </div>`).join("")}
        </div>` : emptyState("dash.digest.empty")}
    </div>`);
}

// ========== WIDGET 4 (FR-18.5): spend vs ceiling ==========
function renderSpendWidget(summary) {
  const trust = summary.trust;
  const hasAny = trust.globalCeilingCents !== null || trust.missions.length > 0;
  return card(`
    <div class="p-6 space-y-4">
      ${widgetTitle(t("dash.spend.title"))}
      ${hasAny ? `
        <div class="space-y-3">
          <div class="flex items-center justify-between gap-3">
            <span class="text-sm font-medium">${t("dash.spend.global")}</span>
            <div class="flex-1 flex justify-end">${ceilingMeter(trust.globalSpendCents, trust.globalCeilingCents)}</div>
          </div>
          ${trust.missions.map((m) => `
            <div class="pt-2 border-t border-border space-y-1.5">
              <div class="flex items-center gap-2">
                <p class="text-xs font-medium truncate">${esc(m.question)}</p>
                ${badge(t("missions.autonomy." + m.dial), "muted")}
              </div>
              ${ceilingMeter(m.spendCents, m.ceilingCents)}
            </div>`).join("")}
        </div>` : emptyState("dash.spend.empty")}
    </div>`);
}

// ========== WIDGET 5 (FR-18.6): recent run receipts ==========
function renderReceiptsWidget(summary) {
  const receipts = summary.recentReceipts;
  const outcomeChip = {
    finished: [t("receipt.outcome.finished"), "success"],
    failed: [t("receipt.outcome.failed"), "destructive"],
    open: [t("receipt.outcome.open"), "warning"],
  };
  return card(`
    <div class="p-6 space-y-3">
      ${widgetTitle(t("dash.receipts.title"))}
      ${receipts.length ? `
        <div class="divide-y divide-border -mx-2">
          ${receipts.map((r) => `
            <div class="px-2 py-3">
              <div class="flex items-center justify-between gap-3">
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    <span class="font-mono text-[10px] text-muted tabular shrink-0">M-${r.missionSeq}</span>
                    <span class="font-mono text-[10px] text-muted tabular truncate">${esc(r.runId)}</span>
                    ${badge(outcomeChip[r.outcome][0], outcomeChip[r.outcome][1])}
                  </div>
                  <p class="text-xs text-muted mt-0.5 truncate">${esc(r.verdict || r.reason || "—")}</p>
                  <p class="font-mono text-[10px] text-muted tabular mt-0.5">${fmtCents(r.spendCents)} ${t("missions.spendOf")} ${fmtCents(r.ceilingCents)} · ${fmtTs(r.startedTs)}</p>
                </div>
                <button onclick="RC.openReceipt('${esc(r.runId)}')" class="text-xs font-medium text-primary hover:underline shrink-0">${t("digest.receipts")}</button>
              </div>
            </div>`).join("")}
        </div>` : emptyState("dash.receipts.empty")}
    </div>`);
}

// ========== WIDGET 6 (FR-18.7): readiness teaser ==========
function renderReadinessWidget(summary) {
  const r = summary.readiness;
  const threeState = r.verdict === "ready" ? "ready" : r.blockers.length === 0 && r.infos.length > 0 ? "near" : "not";
  const stateChip = {
    ready: [t("rd.ready"), "success"],
    near: [t("rd.near"), "warning"],
    not: [t("rd.notReady"), "destructive"],
  };
  return card(`
    <div class="p-6 space-y-3">
      <div class="flex items-start justify-between gap-3">
        ${widgetTitle(t("dash.readiness.title"))}
        <button onclick="RC.openReadiness('')" class="text-xs font-medium text-primary hover:underline shrink-0">${t("dash.readiness.viewReport")}</button>
      </div>
      <div class="flex items-center justify-between gap-3">
        <span class="text-sm font-medium">${t("rd.preprintReady")}</span>
        ${badge(stateChip[threeState][0], stateChip[threeState][1])}
      </div>
      ${r.blockers.length
        ? `<p class="text-xs text-muted">${t("dash.readiness.blockers", { count: r.blockers.length })}</p>`
        : `<p class="text-sm text-emerald-700 font-medium">${t("rd.blockersNone")}</p>`}
      <p class="text-[11px] text-muted">${t("rd.derived")}</p>
    </div>`, "lg:col-span-2");
}

// ========== THE PANEL ==========
export function renderDashboard(app) {
  const summary = app.data.dashboard;
  const header = pageHeader(t("dash.title"), t("dash.sub"));
  if (!summary) {
    return `${header}${card(`<div class="p-12 text-center text-muted text-sm">${t("rc.common.loading")}</div>`)}`;
  }
  return `
    <div class="space-y-6">
      ${header}
      <div class="grid grid-cols-1 lg:grid-cols-2 gap-6 items-start">
        ${renderMissionsWidget(summary)}
        ${renderBoardWidget(summary)}
        ${renderDigestWidget(summary)}
        ${renderSpendWidget(summary)}
        ${renderReceiptsWidget(summary)}
        ${renderReadinessWidget(summary)}
      </div>
      <p class="text-xs text-muted flex items-center gap-1.5">${icon("history", "w-3.5 h-3.5 shrink-0")} ${t("dash.note")}</p>
    </div>`;
}
