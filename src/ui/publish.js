// The Publicación / Publication surface (Stories 6.11–6.13, FR-19): the
// journal-targeting home — the bundled venue templates as data (local,
// community-extensible, NFR-1/NFR-7), the submission checklists as
// missions (choosing a venue spawns one, AD-12), and (Story 6.12) the
// Fit Finder's ranked candidates riding the quarantine. The tier-2
// "journal-ready" verdict renders in the readiness drawer (the bible's
// readiness idiom); the checklist mission cards render their own per-item
// state + the "ready to submit?" verdict via the same gate. All reads
// flow through api.ts; every mutation is a core command (AD-3/AD-15d).
import { t } from "../i18n";
import { api } from "../api";
import { icon, esc, badge, btn, card, pageHeader, rcSelect } from "./helpers";
import { RC, ctx } from "./rc";

// The venue card (the bible's card idiom): name + family badge, the
// scope tags (the Fit Finder's matching vocabulary), the machine/human
// criteria counts, and the honest human-items note (never auto-passed).
function renderVenueCard(venue) {
  const machine = venue.criteria.filter((c) => !c.human).length;
  const human = venue.criteria.filter((c) => c.human).length;
  return `
    <div class="rounded-xl border border-border bg-card shadow-sm p-5 flex flex-col gap-3">
      <div class="flex items-start justify-between gap-2">
        <div class="min-w-0">
          <h3 class="font-serif text-lg italic leading-tight">${esc(venue.name)}</h3>
          <p class="text-xs text-muted mt-1">${esc(venue.descriptionEs)}</p>
        </div>
        ${badge(venue.family, "primary")}
      </div>
      <div class="flex flex-wrap gap-1">
        ${venue.scope.map((s) => `<span class="inline-flex items-center rounded bg-gray-100 px-1.5 py-0.5 font-mono text-[10px] text-muted">${esc(s)}</span>`).join("")}
      </div>
      <div class="flex items-center justify-between gap-2 mt-auto pt-1">
        <span class="text-[11px] text-muted">${t("pub.venue.criteria", { machine, human })}</span>
        ${btn({ label: t("pub.venue.open"), variant: "secondary", size: "sm", iconName: "check", onClick: `RC.openReadinessWithVenue('${esc(venue.id)}')` })}
      </div>
      <div class="flex items-center justify-between gap-2">
        <p class="text-[11px] text-muted flex items-center gap-1">${icon("pen", "w-3 h-3")} ${t("pub.venue.humanFlag")}</p>
        ${btn({ label: t("pub.submission.create"), variant: "default", size: "sm", iconName: "plus", onClick: `RC.createChecklistForVenue('${esc(venue.id)}')` })}
      </div>
    </div>`;
}

const submissionStatusChip = {
  active: ["active", "primary"],
  completed: ["completed", "success"],
  stopped: ["stopped", "muted"],
  failed: ["failed", "destructive"],
};

// One checklist item row: the state chip (pass/fail/human pending), the
// label, the code-form detail, and the attribution stamp — agent vs
// human always visible (FR-2.2 discipline).
function renderChecklistItem(item, venue, lang) {
  const criterion = venue?.criteria.find((c) => c.id === item.itemId);
  const label = item.human
    ? ((lang === "en" ? criterion?.labelEn : criterion?.labelEs) || criterion?.labelEs || item.itemId)
    : t("rd.c." + item.kind, {});
  const byAgent = item.checked ? item.checked.actor.startsWith("agent:") : false;
  const attribution = item.checked ? (byAgent ? t("pub.submission.byAgent") : t("pub.submission.byUser")) : "";
  return `
    <div class="flex items-start gap-2.5 px-4 py-2.5">
      <span class="mt-1.5 h-2 w-2 rounded-full shrink-0 ${item.checked ? (byAgent ? "bg-emerald-500" : "bg-primary") : (item.human ? "bg-slate-400" : "bg-amber-400")}"></span>
      <div class="min-w-0 flex-1">
        <p class="text-sm leading-snug ${item.human ? "flex items-center gap-1" : ""}">${item.human ? icon("pen", "w-3 h-3 inline text-muted") + " " : ""}${esc(label)}</p>
        ${item.checked ? `<p class="text-[11px] text-muted mt-0.5">${esc(attribution)}${item.checked.note ? ` · <span class="font-mono">${esc(item.checked.note)}</span>` : ""} <span class="font-mono text-[10px]">e-${item.checked.seq}</span></p>` : (item.human ? `<p class="text-[11px] text-muted mt-0.5">${t("rd.tier2.humanPending")}</p>` : "")}
      </div>
    </div>`;
}

// The Fit Finder card (Story 6.12, FR-19.3): the advisory ranking as a
// reviewable proposal — each candidate's rationale cites its board
// objects, unpinned references are flagged, and the choice waits in
// quarantine (approve here or in the review surface; nothing submits
// itself, PRD §10).
function renderFitFinder(app, fit) {
  const loading = app.state.fitLoading;
  const title = fit ? t("pub.fit.refresh") : t("pub.fit.run");
  return card(`
    <div class="p-5 space-y-3">
      <div class="flex items-center justify-between gap-2">
        <h2 class="heading-3">${t("pub.fit.title")}</h2>
        ${btn({ label: loading ? t("rc.common.loading") : title, variant: "default", size: "sm", iconName: "sparkle", onClick: "RC.runJournalFit()", disabled: loading })}
      </div>
      <p class="text-[11px] text-muted">${t("pub.fit.note")}</p>
      ${fit ? `
      <div class="flex items-center gap-2">
        <span class="text-[11px] text-muted">${t("pub.fit.runsOn", { provider: fit.provider, model: fit.model })}</span>
        ${fit.proposal && fit.proposal.status === "pending"
          ? `<span class="inline-flex items-center rounded bg-gray-100 px-1.5 py-0.5 font-mono text-[10px] text-muted">pr-${fit.proposal.seq}</span>`
          : ""}
      </div>
      <div class="space-y-2">
        ${fit.candidates.map((c) => renderFitCandidate(c, app)).join("")}
      </div>
      ${fit.candidates.length === 0 ? `<p class="text-sm text-muted">${t("pub.fit.empty")}</p>` : ""}
      ${fit.proposal && fit.proposal.status === "pending" ? `
      <div class="rounded-xl border border-amber-300/60 bg-amber-50 px-3 py-2.5 space-y-2">
        <p class="text-xs font-medium text-amber-700 flex items-center gap-1.5">${icon("danger", "w-3.5 h-3.5 shrink-0")} ${t("pub.fit.proposes", { venue: fit.candidates[0] ? fit.candidates[0].venueId : "" })}</p>
        <div class="flex items-center gap-2">
          ${btn({ label: t("quarantine.reject"), variant: "secondary", size: "sm", onClick: `RC.rejectFitProposal('${esc(fit.proposal.id)}')` })}
          ${btn({ label: t("quarantine.approve"), variant: "default", size: "sm", onClick: `RC.approveFitProposal('${esc(fit.proposal.id)}')` })}
        </div>
      </div>` : ""}
      ` : `<p class="text-sm text-muted py-2">${t("pub.fit.run")}…</p>`}
    </div>`);
}

function renderFitCandidate(c, app) {
  const venue = app.data.venues?.find((v) => v.id === c.venueId);
  return `
    <div class="rounded-xl border border-border divide-y divide-border overflow-hidden bg-white p-3 space-y-1.5">
      <div class="flex items-center justify-between gap-2">
        <span class="font-medium text-sm">${esc(venue ? venue.name : c.venueId)} <span class="font-mono text-[10px] text-muted">${esc(c.venueId)}</span></span>
        <span class="inline-flex items-center gap-1 font-mono text-xs tabular text-primary">${c.score} <span class="text-[10px] text-muted">${t("pub.fit.score")}</span></span>
      </div>
      <p class="text-xs text-muted leading-relaxed">${esc(c.rationale)}</p>
      ${c.refs.length ? `<div class="flex flex-wrap gap-1">${c.refs.map((r) => `<span class="inline-flex items-center rounded bg-gray-100 px-1.5 py-0.5 font-mono text-[10px] text-muted">${esc(r)}</span>`).join("")}</div>` : ""}
      ${c.unverifiedRefs.length ? `<p class="text-[11px] text-amber-700 flex items-center gap-1">${icon("danger", "w-3 h-3")} ${t("pub.fit.unverified")}: ${c.unverifiedRefs.map((r) => `<span class="font-mono">${esc(r)}</span>`).join(", ")}</p>` : ""}
    </div>`;
}

// One checklist mission card: the venue-bound mission card (the bible's
// mission-card idiom) carrying its per-item state + the "ready to
// submit?" verdict via the tier-2 gate (Story 6.11) + the item actions.
function renderSubmissionCard(view, venue, lang) {
  const s = view.submission;
  const r = view.readiness;
  const chip = submissionStatusChip[s.status] || submissionStatusChip.active;
  const checkedCount = s.items.filter((i) => i.checked !== null).length;
  const done = s.status === "completed";
  return `
    <div class="rounded-xl border border-border bg-card shadow-sm p-5 space-y-3">
      <div class="flex items-start justify-between gap-2">
        <div class="min-w-0">
          <p class="font-mono text-[10px] text-muted tabular">M-${s.seq} · ${esc(s.venueId)}</p>
          <h3 class="font-serif text-lg italic leading-tight mt-0.5">${esc(s.venueName)}</h3>
          <p class="text-xs text-muted mt-1">${esc(s.question)}</p>
        </div>
        ${badge(t("missions.status." + chip[0]), chip[1])}
      </div>
      <div class="flex items-baseline justify-between gap-2">
        <span class="text-[11px] text-muted">${t("pub.submission.items", { checked: checkedCount, total: s.items.length })}</span>
        <span class="font-mono text-[10px] text-muted">${esc(s.stopCondition)}</span>
      </div>
      <div class="rounded-xl border border-border divide-y divide-border overflow-hidden bg-white">
        ${s.items.map((i) => renderChecklistItem(i, venue, lang)).join("")}
      </div>
      <div class="rounded-xl border ${r.verdict === "ready" ? "border-emerald-300 bg-emerald-50" : "border-border bg-gray-50"} px-3 py-2.5 space-y-1">
        <div class="flex items-center justify-between gap-2">
          <span class="text-xs font-medium ${r.verdict === "ready" ? "text-emerald-700" : "text-foreground"}">${t("pub.submission.ready")}</span>
          ${badge(r.verdict === "ready" ? t("rd.ready") : t("rd.notReady"), r.verdict === "ready" ? "success" : "destructive")}
        </div>
        <div class="flex flex-wrap gap-1">
          ${r.items.filter((i) => i.status === "fail").slice(0, 4).map((i) => `<span class="inline-flex items-center rounded bg-white px-1.5 py-0.5 font-mono text-[10px] text-muted">${esc(i.kind)}</span>`).join("")}
        </div>
      </div>
      <div class="flex items-center justify-end gap-2 pt-1">
        ${done ? "" : `${btn({ label: t("pub.submission.precheck"), variant: "secondary", size: "sm", iconName: "sparkle", onClick: `RC.precheckChecklist('${esc(s.id)}')` })}
        ${btn({ label: t("pub.submission.check"), variant: "secondary", size: "sm", onClick: `RC.uncheckNextItem('${esc(s.id)}')` })}
        ${btn({ label: t("pub.submission.complete"), variant: "default", size: "sm", iconName: "check", onClick: `RC.completeChecklist('${esc(s.id)}')`, cls: checkedCount === s.items.length ? "" : "opacity-40" })}`}
      </div>
      ${!done && checkedCount < s.items.length ? `<p class="text-[11px] text-muted">${t("pub.submission.itemsLeft", { count: s.items.length - checkedCount })}</p>` : ""}
    </div>`;
}

export function renderPublishHome(app) {
  const venues = app.data.venues;
  const submissions = app.data.submissions;
  const views = app.data.submissionViews || {};
  const header = pageHeader(t("pub.title"), t("pub.headline"));
  if (!venues || !submissions) {
    return `${header}${card(`<div class="p-12 text-center text-muted text-sm">${t("rc.common.loading")}</div>`)}`;
  }
  const scopeId = app.state.publishScopeMissionId || "";
  const scopeOptions = [
    { value: "", label: t("pub.submission.scope.workspace") },
    ...(app.data.missions || []).map((m) => ({ value: m.id, label: m.question.slice(0, 40) })),
  ];
  const venueOf = (id) => venues.find((v) => v.id === id);
  return `
    <div class="space-y-6">
      ${header}
      <p class="text-sm text-muted -mt-2">${t("pub.sub")}</p>
      ${renderFitFinder(app, app.data.fit)}
      <div>
        <div class="flex items-baseline justify-between gap-2 mb-3">
          <h2 class="heading-3">${t("pub.venues")}</h2>
          <p class="text-[11px] text-muted">${t("pub.venues.note")}</p>
        </div>
        <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
          ${venues.map((v) => renderVenueCard(v)).join("")}
        </div>
      </div>
      <div>
        <div class="flex items-baseline justify-between gap-2 mb-3">
          <h2 class="heading-3">${t("pub.submissions")}</h2>
          <p class="text-[11px] text-muted">${t("pub.submissions.note")}</p>
        </div>
        <div class="flex items-center gap-3 mb-3 max-w-xs">
          <span class="text-[11px] text-muted shrink-0">${t("pub.submission.scope")}</span>
          ${rcSelect({ id: "publish-scope", size: "sm", cls: "w-full", options: scopeOptions, value: scopeId, onChange: "RC.setPublishScope(this.value)" })}
        </div>
        ${submissions.length ? `<div class="grid grid-cols-1 lg:grid-cols-2 gap-4">${submissions.map((s) => renderSubmissionCard(views[s.id] || { submission: s, readiness: null, allChecked: false }, venueOf(s.venueId), app.state.lang || "es")).join("")}</div>` : `<p class="text-sm text-muted py-6">${t("pub.submission.empty")}</p>`}
      </div>
    </div>`;
}

// Submissions are fetched as full views (mission + "ready to submit?");
// the surface keeps the view data next to each mission for the cards.
Object.assign(RC, {
  // Fit Finder (Story 6.12): rank the venues of the bundled dataset for
  // the selected scope through the provider layer — advisory + quarantined.
  async runJournalFit() {
    const app = ctx.app;
    app.state.fitLoading = true;
    ctx.render();
    try {
      app.data.fit = await api.runJournalFit(app.state.publishScopeMissionId || null);
    } catch (e) {
      alert((e?.message || e));
      console.error(e);
    }
    app.state.fitLoading = false;
    ctx.render();
  },
  async approveFitProposal(proposalId) {
    try {
      await api.approveProposal(proposalId, false);
      await ctx.loadPublishData();
      ctx.render();
    } catch (e) {
      alert((e?.message || e));
      console.error(e);
    }
  },
  async rejectFitProposal(proposalId) {
    try {
      await api.rejectProposal(proposalId);
      const app = ctx.app;
      if (app.data.fit) app.data.fit.proposal = { ...app.data.fit.proposal, status: "rejected" };
      ctx.render();
    } catch (e) {
      alert((e?.message || e));
      console.error(e);
    }
  },
  async setPublishScope(missionId) {
    const app = ctx.app;
    app.state.publishScopeMissionId = missionId || null;
    ctx.render();
    await ctx.loadPublishData();
  },
  // Choose a venue (FR-19.4): spawn the submission checklist for the
  // selected scope.
  async createChecklistForVenue(venueId) {
    const app = ctx.app;
    const scope = app.state.publishScopeMissionId || null;
    try {
      await api.createSubmissionMission(scope, venueId);
      await ctx.loadPublishData();
    } catch (e) {
      alert((e?.message || e));
      console.error(e);
    }
  },
  // The agent pre-check (AD-3): propose the passing machine items —
  // nothing checks itself silently; the quarantine review merges them.
  async precheckChecklist(submissionId) {
    try {
      const proposals = await api.precheckSubmissionItems(submissionId);
      if (!proposals.length) alert(t("pub.submission.precheck") + " — 0");
      await ctx.loadPublishData();
    } catch (e) {
      alert((e?.message || e));
      console.error(e);
    }
  },
  // The human check: mark the next unchecked item (machine or human —
  // the user can check anything; the agent only machine items).
  async uncheckNextItem(submissionId) {
    const app = ctx.app;
    const view = app.data.submissionViews[submissionId];
    const next = view?.submission.items.find((i) => i.checked === null);
    if (!next) return;
    try {
      await api.checkSubmissionItem(submissionId, next.itemId, null);
      await ctx.loadPublishData();
    } catch (e) {
      alert((e?.message || e));
      console.error(e);
    }
  },
  async completeChecklist(submissionId) {
    try {
      await api.completeSubmissionMission(submissionId);
      await ctx.loadPublishData();
    } catch (e) {
      alert((e?.message || e));
      console.error(e);
    }
  },
});