// The mobile-web companion (Story 6.15, FR-21.2, NFR-5/NFR-13): a
// mobile-first, read-only status surface served at `/m` by the bridge —
// mission status chips, dashboard_summary widgets (digest teaser,
// trust/spend), recent receipts, and the pending-proposals list with
// one-tap approve/reject (Story 6.16's actions). The depth divide is
// enforced: the ONLY creation verb is quick-capture (FR-21.3) — no board
// editing, no manuscript editing, no settings; deep work stays on the
// laptop. All traffic flows through the one bridge (mobileApi carries the
// pairing token); a down bridge renders the honest offline state, never
// stale data presented as live (FR-9.1 spirit).
import { t, setLang, getLang } from "../i18n";
import { mobileApi, setPairingToken, mobileServedByCore } from "../api";
import { icon, esc, badge, btn, card, fmtCents, fmtTs } from "./helpers";
import { spendMeter, statusColor } from "./missions";

// The companion's own small state — it never mounts the desktop shell.
const m = {
  probing: true, // true until the first probe answers
  paired: false,
  offline: false, // honest offline state — a down bridge, never stale-as-live
  served: false, // a core answers (false = plain vite dev → mock)
  missions: [],
  summary: null,
  proposals: [],
  capture: { sending: false, sent: false },
  forceProposalId: null, // the basis-stale force surface (Story 6.16)
  error: null,
};

const POLL_MS = 30000;
let pollTimer = null;

function render() {
  const root = document.getElementById("app");
  if (!root) return;
  root.innerHTML = m.probing
    ? probeShell()
    : m.paired
      ? companion()
      : pairingForm();
}

function probeShell() {
  return `
    <div class="min-h-screen bg-background flex items-center justify-center p-6">
      <p class="text-sm text-muted">${t("rc.common.loading")}</p>
    </div>`;
}

function shell(body) {
  return `
    <div class="min-h-screen bg-background">
      <header class="sticky top-0 z-30 bg-card/80 backdrop-blur border-b border-border">
        <div class="max-w-md mx-auto px-4 h-14 flex items-center justify-between">
          <div class="flex items-center gap-2.5">
            <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-primary text-white">${icon("sparkle", "w-3.5 h-3.5")}</div>
            <span class="font-serif text-lg italic">${t("mobile.title")}</span>
          </div>
          <div class="flex items-center gap-2">
            <span class="inline-flex items-center gap-1.5 text-[11px] font-medium ${m.offline ? "text-rose-600" : "text-emerald-600"}">
              <span class="h-1.5 w-1.5 rounded-full ${m.offline ? "bg-rose-500" : "bg-emerald-500"}"></span>
              ${m.offline ? t("mobile.offline").split("—")[0].trim() : t("mobile.paired")}
            </span>
            <button onclick="RCM.setLang()" class="rounded-lg border border-border bg-white px-2 py-1 text-[11px] font-medium text-muted ring-focus">${getLang().toUpperCase()}</button>
          </div>
        </div>
      </header>
      <main class="max-w-md mx-auto px-4 py-5 space-y-5">${body}</main>
    </div>`;
}

// The pairing form (NFR-13): an unpaired surface gets the shell and this
// one form — no data until the token verifies.
function pairingForm() {
  return shell(`
    <div class="pt-8">
      ${card(`
        <div class="p-6 space-y-4">
          <div class="flex items-center gap-3">
            <div class="flex h-9 w-9 items-center justify-center rounded-xl bg-amber-500/10 text-amber-600">${icon("key", "w-4 h-4")}</div>
            <h2 class="heading-2">${t("mobile.unpaired")}</h2>
          </div>
          <p class="text-sm text-muted leading-relaxed">${t("mobile.unpairedHint")}</p>
          <input id="pair-token" placeholder="${t("mobile.pairPh")}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30">
          ${m.error ? `<div class="rounded-lg border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-700 font-medium">${esc(m.error)}</div>` : ""}
          ${btn({ label: t("mobile.pair"), variant: "default", cls: "w-full", onClick: "RCM.pair()" })}
        </div>`)}
    </div>`);
}

function companion() {
  if (m.offline) {
    return shell(`
      <div class="pt-10">
        ${card(`
          <div class="p-6 space-y-3 text-center">
            <div class="mx-auto flex h-10 w-10 items-center justify-center rounded-xl bg-rose-500/10 text-rose-600">${icon("danger", "w-5 h-5")}</div>
            <p class="text-sm text-muted leading-relaxed">${t("mobile.offline")}</p>
            ${btn({ label: t("rc.common.loading"), variant: "secondary", size: "sm", onClick: "RCM.refresh()" })}
          </div>`)}
      </div>`);
  }
  const summary = m.summary;
  const drafts = m.missions.filter((x) => x.status === "draft");
  return shell(`
    ${statusChips(m.missions)}
    ${digestTeaser(summary)}
    ${trustWidget(summary)}
    ${receiptsWidget(summary)}
    ${drafts.length ? draftNote(drafts) : ""}
    ${proposalsWidget()}
    ${captureBox()}
  `);
}

// Mission status chips (FR-21.2): the one-line health of the workspace.
function statusChips(missions) {
  const order = ["draft", "active", "awaiting_review", "completed", "stopped", "failed"];
  const counts = order.map((k) => [k, missions.filter((x) => x.status === k).length]);
  return card(`
    <div class="p-4">
      <p class="caption text-muted mb-2.5">${t("mobile.status")}</p>
      <div class="flex flex-wrap gap-2">
        ${counts.filter(([, n]) => n > 0).map(([k, n]) => `
          <span class="inline-flex items-center gap-1.5">
            ${badge(`${t("missions.status." + k)} ${n}`, statusColor[k] || "muted")}
          </span>
        `).join("") || `<span class="text-sm text-muted">${t("missions.emptyHint")}</span>`}
      </div>
    </div>`);
}

// The latest digest teaser (FR-21.2): outcome + spend, code-form.
function digestTeaser(summary) {
  const d = summary?.digest;
  if (!d) return "";
  const outcomeColor = {
    all_finished: "success",
    partial_success: "warning",
    all_failed: "destructive",
    no_runs: "muted",
  };
  return card(`
    <div class="p-4 space-y-2">
      <div class="flex items-center justify-between gap-2">
        <p class="caption text-muted">${t("mobile.digest")}</p>
        <span class="font-mono text-[10px] tabular text-muted">${fmtTs(d.generatedAt)}</span>
      </div>
      <div class="flex items-center justify-between gap-2">
        ${badge(t("digest.outcome." + d.outcome), outcomeColor[d.outcome] || "muted")}
        <span class="font-mono text-xs tabular text-muted">${fmtCents(d.spendCents)} ${t("digest.ofCeiling", { ceiling: fmtCents(d.ceilingCents) })}</span>
      </div>
    </div>`);
}

// The trust/spend meter (FR-21.2): runtime state + the global ceiling.
function trustWidget(summary) {
  const trust = summary?.trust;
  if (!trust) return "";
  const killed = trust.runtimeState === "killed";
  return card(`
    <div class="p-4 space-y-3">
      <div class="flex items-center justify-between gap-2">
        <p class="caption text-muted">${t("mobile.trust")}</p>
        ${badge(killed ? t("trust.killOff") : t("trust.autonomousWork"), killed ? "destructive" : "success")}
      </div>
      ${spendMeter(trust.globalSpendCents, trust.globalCeilingCents ?? 0, trust.globalCeilingCents == null ? "ok" : (trust.globalSpendCents >= trust.globalCeilingCents ? "blocked" : trust.globalSpendCents * 5 >= trust.globalCeilingCents * 4 ? "near" : "ok"))}
    </div>`);
}

// Recent receipts (FR-21.2): the last runs' outcomes, code-form rows.
function receiptsWidget(summary) {
  const receipts = summary?.recentReceipts || [];
  if (!receipts.length) return "";
  const outcomeColor = { finished: "success", failed: "destructive", open: "warning" };
  return card(`
    <div class="p-4 space-y-2">
      <p class="caption text-muted">${t("mobile.receipts")}</p>
      <div class="divide-y divide-border -mx-1">
        ${receipts.slice(0, 4).map((r) => `
          <div class="px-1 py-2 flex items-center justify-between gap-2">
            <span class="font-mono text-[11px] tabular text-muted truncate">M-${r.missionSeq} · ${esc(r.runId)}</span>
            <span class="flex items-center gap-2 shrink-0">
              <span class="font-mono text-[10px] tabular text-muted">${fmtCents(r.spendCents)}</span>
              ${badge(t("receipt.outcome." + r.outcome), outcomeColor[r.outcome] || "muted")}
            </span>
          </div>
        `).join("")}
      </div>
    </div>`);
}

// Captured drafts visible on mobile: the card landed — completing it is
// the laptop's work (the depth divide, NFR-5).
function draftNote(drafts) {
  return card(`
    <div class="p-4 space-y-1.5">
      <p class="caption text-muted">${t("missions.status.draft")}</p>
      ${drafts.slice(0, 3).map((d) => `
        <div class="flex items-start gap-2">
          ${icon("history", "w-3.5 h-3.5 text-amber-600 shrink-0 mt-0.5")}
          <p class="text-xs leading-snug">${esc(d.question)} <span class="text-muted">— ${t("missions.draft.awaiting")}</span></p>
        </div>
      `).join("")}
    </div>`);
}

// The pending-proposals list with one-tap approve/reject (Story 6.16,
// FR-21.4): verdict lines in code form. A basis-stale proposal refuses
// one-tap — the desktop-required warning + the explicit force-confirm
// surface appear; never a blind merge (AD-13).
function proposalsWidget() {
  const pending = m.proposals.filter((p) => p.status === "pending");
  return card(`
    <div class="p-4 space-y-3">
      <p class="caption text-muted">${t("mobile.proposals")}</p>
      ${pending.length ? pending.map((p) => proposalRow(p)).join("") : `<p class="text-sm text-muted py-2 text-center">${t("mobile.noProposals")}</p>`}
    </div>`);
}

function proposalRow(p) {
  const isPin = p.proposedKind === "evidence.pinned";
  const payload = p.proposedPayload || {};
  const target = p.targetSeq ? `H-${p.targetSeq}` : "";
  const line = isPin
    ? `${t("quarantine.pinProposes")} · ${esc(payload.artifact_ref || "")}`
    : `${target} ${esc(payload.from || "")} → ${esc(payload.to || "")}`;
  const forcing = m.forceProposalId === p.id;
  return `
    <div class="rounded-xl border ${p.basisStale ? "border-amber-300" : "border-border"} bg-white p-3 space-y-2">
      <div class="flex items-center justify-between gap-2">
        <span class="font-mono text-[10px] text-amber-700 tabular">pr-${p.seq}</span>
        ${badge(isPin ? t("quarantine.pinProposes") : t("quarantine.change"), "warning")}
      </div>
      <p class="text-xs font-mono">${line}</p>
      ${p.basisStale ? `
        <div class="rounded-lg border-2 border-amber-300 bg-amber-50 px-3 py-2 flex items-start gap-2">
          ${icon("danger", "w-4 h-4 text-amber-700 shrink-0 mt-0.5")}
          <p class="text-[11px] font-medium text-amber-700 leading-snug">${t("mobile.desktopRequired")}</p>
        </div>` : ""}
      <div class="flex items-center gap-2">
        ${btn({ label: t("quarantine.reject"), variant: "secondary", size: "sm", onClick: `RCM.reject('${esc(p.id)}')` })}
        ${p.basisStale
          ? (forcing
              ? btn({ label: t("quarantine.forceApprove"), variant: "destructive", size: "sm", onClick: `RCM.approve('${esc(p.id)}', true)` })
              : btn({ label: t("quarantine.approve"), variant: "outline", size: "sm", onClick: `RCM.approve('${esc(p.id)}', false)` }))
          : btn({ label: t("quarantine.approve"), variant: "default", size: "sm", onClick: `RCM.approve('${esc(p.id)}', false)` })}
      </div>
    </div>`;
}

// Quick-capture (FR-21.3): the companion's one creation verb — a question
// captured here lands on the home machine as a pending mission card.
function captureBox() {
  const c = m.capture;
  return card(`
    <div class="p-4 space-y-3">
      <div>
        <p class="caption text-muted">${t("mobile.capture.title")}</p>
        <p class="text-[11px] text-muted mt-0.5 leading-snug">${t("mobile.capture.hint")}</p>
      </div>
      ${c.sent ? `
        <div class="rounded-lg border border-emerald-200 bg-emerald-50 px-3 py-2 flex items-center gap-2">
          ${icon("check", "w-4 h-4 text-emerald-600 shrink-0")}
          <p class="text-xs font-medium text-emerald-700">${t("mobile.capture.sent")}</p>
        </div>` : `
        <textarea id="capture-question" rows="2" placeholder="${t("mobile.capture.ph")}" class="w-full rounded-lg border border-border bg-white px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"></textarea>
        ${btn({ label: c.sending ? t("mobile.capture.sending") : t("mobile.capture.send"), variant: "default", cls: "w-full", onClick: "RCM.capture()", disabled: c.sending })}`}
    </div>`);
}

// ========== DATA ==========
async function loadAll() {
  try {
    const [missions, summary, proposals] = await Promise.all([
      mobileApi.listMissions(),
      mobileApi.getDashboardSummary(),
      mobileApi.listProposals(),
    ]);
    m.missions = missions;
    m.summary = summary;
    m.proposals = proposals;
    m.offline = false;
  } catch (e) {
    const msg = String(e?.message || e);
    if (msg.startsWith("unpaired:")) {
      m.paired = false;
      stopPolling();
    } else {
      // a down bridge (or a failed read) is the honest offline state —
      // never stale data presented as live (FR-9.1 spirit)
      m.offline = true;
    }
  }
  render();
}

function startPolling() {
  stopPolling();
  pollTimer = setInterval(loadAll, POLL_MS);
}

function stopPolling() {
  if (pollTimer) clearInterval(pollTimer);
  pollTimer = null;
}

// ========== HANDLERS ==========
// The companion mounts its own namespace — it never touches the desktop
// shell's RC bus.
window.RCM = {
  setLang() {
    const order = ["es", "en", "pt", "fr"];
    const next = order[(order.indexOf(getLang()) + 1) % order.length];
    setLang(next);
    localStorage.setItem("rc-lang", next);
    render();
  },
  async pair() {
    const token = document.getElementById("pair-token")?.value.trim();
    if (!token) return;
    setPairingToken(token);
    m.error = null;
    try {
      await mobileApi.listMissions();
      m.paired = true;
      m.probing = false;
      render();
      await loadAll();
      startPolling();
    } catch (e) {
      m.error = String(e?.message || e);
      render();
    }
  },
  async refresh() {
    await loadAll();
  },
  async capture() {
    const el = document.getElementById("capture-question");
    const question = el?.value.trim();
    if (!question || m.capture.sending) return;
    m.capture.sending = true;
    render();
    try {
      await mobileApi.quickCapture(question);
      m.capture = { sending: false, sent: true };
    } catch (e) {
      m.capture = { sending: false, sent: false };
      alert(t("mobile.capture.error") + (e?.message || e));
    }
    render();
    // the draft lands on the home machine — refresh the status chips
    await loadAll();
    setTimeout(() => { m.capture.sent = false; render(); }, 4000);
  },
  async approve(proposalId, force) {
    try {
      await mobileApi.approveProposal(proposalId, force);
      m.forceProposalId = null;
    } catch (e) {
      const msg = String(e?.message || e);
      if (msg.startsWith("basis_stale:")) {
        // one-tap refused — surface the force-confirm (never a blind merge)
        m.forceProposalId = proposalId;
        alert(msg);
      } else {
        alert(t("mobile.actionError") + msg);
      }
    }
    await loadAll();
  },
  async reject(proposalId) {
    try {
      await mobileApi.rejectProposal(proposalId);
    } catch (e) {
      alert(t("mobile.actionError") + (e?.message || e));
    }
    await loadAll();
  },
};

// ========== BOOT ==========
// Mounted by app.js when the page is served at `/m` — the companion never
// mounts the lock, the wizard, or the desktop shell.
export function bootMobile() {
  const savedLang = localStorage.getItem("rc-lang");
  if (savedLang) setLang(savedLang);
  document.documentElement.lang = getLang();
  m.probing = true;
  render();
  (async () => {
    m.served = await mobileServedByCore;
    m.probing = false;
    if (!m.served) {
      // plain vite dev — the mock answers, no pairing to speak of
      m.paired = true;
      render();
      await loadAll();
      startPolling();
      return;
    }
    // a core serves /m — the pairing token decides what renders
    try {
      await mobileApi.listMissions();
      m.paired = true;
      render();
      await loadAll();
      startPolling();
    } catch (e) {
      const msg = String(e?.message || e);
      m.paired = false;
      m.error = msg.startsWith("unpaired:") ? null : msg;
      render();
    }
  })();
}
