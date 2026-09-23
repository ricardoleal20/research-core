// The notification bell + push (Story 6.16, FR-21.4, NFR-13): the desktop
// shell's bell polls the notifications read — verdict summaries only, in
// code form — and every new pending proposal fires a system push through
// the Tauri notification plugin (with the in-app bell + toast as the
// always-present surface). One-tap Approve/Reject routes through the SAME
// typed merge commands the board's quarantine review calls (single writer,
// AD-14); a basis-stale proposal refuses one-tap — the desktop warning +
// the explicit force-confirm surface, never a blind merge (AD-13). Failed
// polls leave the last known state visible; nothing is silently missed.
import { t } from "../i18n";
import { api } from "../api";
import { mockActive } from "../mock-backend";
import { icon, esc, badge, btn, fmtTs } from "./helpers";
import { RC, ctx } from "./rc";

const POLL_MS = 30000;
const SEEN_KEY = "rc-seen-notifications";
let pollTimer = null;

// ========== SYSTEM PUSH ==========
// System push through the Tauri notification plugin (Story 6.16): fires
// once per NEW pending proposal. In the plain browser (vite dev) there is
// no system surface — the in-app bell + toast carry it, same actions.
async function systemPush(item) {
  if (mockActive) return;
  try {
    const mod = await import("@tauri-apps/plugin-notification");
    let granted = await mod.isPermissionGranted();
    if (!granted) granted = (await mod.requestPermission()) === "granted";
    if (granted) {
      mod.sendNotification({
        title: t("bell.proposal"),
        body: item.summary,
      });
    }
  } catch (e) {
    // honest fallback — the in-app bell + toast are the surface
    console.warn("[bell] system push unavailable:", e);
  }
}

function loadSeen() {
  try {
    return JSON.parse(localStorage.getItem(SEEN_KEY) || "[]");
  } catch (e) {
    return [];
  }
}

function saveSeen(seen) {
  // bounded — the last 200 seen proposal ids
  localStorage.setItem(SEEN_KEY, JSON.stringify(seen.slice(-200)));
}

// ========== POLL ==========
async function pollBell() {
  const app = ctx.app;
  if (!app || app.state.locked) return;
  try {
    const items = await api.listNotifications();
    app.data.notifications = items;
    // a NEW pending proposal pushes once (system + toast); the digest row
    // only lands in the bell's list
    const seen = loadSeen();
    let fresh = false;
    for (const n of items) {
      if (n.kind !== "proposal" || !n.proposalId) continue;
      if (!seen.includes(n.proposalId)) {
        seen.push(n.proposalId);
        fresh = true;
        await systemPush(n);
        showToast(n);
      }
    }
    if (fresh) saveSeen(seen);
    paintBell();
  } catch (e) {
    // a failed poll keeps the last known state visible — never a crash,
    // never a silent miss (the next successful poll repaints)
    console.warn("[bell] poll failed:", e);
  }
}

export function startBellPolling() {
  if (pollTimer) return;
  pollBell();
  pollTimer = setInterval(pollBell, POLL_MS);
}

// ========== RENDER ==========
// The header's bell button (badge = pending proposal count) + dropdown.
// Painted without a full re-render so open inputs never lose focus.
export function renderBell(app) {
  const n = (app.data.notifications || []).filter((x) => x.kind === "proposal").length;
  return `
    <div class="relative">
      <button onclick="RC.bellToggle()" class="relative rounded-lg border border-border bg-white p-2 text-muted hover:text-foreground hover:border-primary/30 transition ring-focus" aria-label="${t("bell.title")}">
        ${icon("bell", "w-4 h-4")}
        <span id="bell-badge" class="absolute -top-1.5 -right-1.5 min-w-[16px] h-4 px-1 rounded-full bg-rose-500 text-white text-[10px] font-semibold items-center justify-center" style="display:${n ? "inline-flex" : "none"}">${n}</span>
      </button>
      ${app.state.bellOpen ? `<div id="bell-dropdown" class="absolute right-0 top-full mt-1 w-80 max-w-[calc(100vw-2rem)] rounded-xl border border-border bg-white shadow-lg z-40 overflow-hidden animate-fade-up">${dropdownBody()}</div>` : ""}
    </div>`;
}

function dropdownBody() {
  const app = ctx.app;
  const items = app.data.notifications || [];
  const proposals = items.filter((n) => n.kind === "proposal");
  const digest = items.find((n) => n.kind === "digest");
  return `
    <div class="px-4 py-3 border-b border-border flex items-center justify-between">
      <p class="text-sm font-semibold">${t("bell.title")}</p>
      ${btn({ label: t("bell.markSeen"), variant: "ghost", size: "sm", onClick: "RC.bellMarkSeen()" })}
    </div>
    <div class="max-h-[60vh] overflow-y-auto divide-y divide-border">
      ${proposals.map((n) => proposalRow(n)).join("")}
      ${digest ? `
        <div class="px-4 py-3 flex items-center justify-between gap-2">
          <div class="min-w-0">
            <p class="text-xs font-medium">${t("bell.digestReady")}</p>
            <p class="font-mono text-[11px] text-muted mt-0.5 truncate">${esc(digest.summary)}</p>
          </div>
          ${btn({ label: t("rc.nav.digest"), variant: "ghost", size: "sm", onClick: "RC.navigate('digest')" })}
        </div>` : ""}
      ${!proposals.length && !digest ? `<p class="px-4 py-6 text-sm text-muted text-center">${t("bell.empty")}</p>` : ""}
    </div>`;
}

// A pending proposal's one-tap row: the verdict summary in code form, the
// same typed commands as the board's quarantine review. Basis-stale rows
// refuse one-tap — the warning + the explicit force-confirm appear.
function proposalRow(n) {
  const forcing = ctx.app.state.bellForceId === n.proposalId;
  return `
    <div class="px-4 py-3 space-y-2 ${n.basisStale ? "bg-amber-50/50" : ""}">
      <div class="flex items-center justify-between gap-2">
        <span class="font-mono text-[10px] text-amber-700 tabular">${esc(n.summary)}</span>
        <span class="font-mono text-[10px] text-muted tabular shrink-0">${fmtTs(n.ts)}</span>
      </div>
      ${n.basisStale ? `
        <div class="rounded-lg border-2 border-amber-300 bg-amber-50 px-3 py-2 flex items-start gap-2">
          ${icon("danger", "w-4 h-4 text-amber-700 shrink-0 mt-0.5")}
          <p class="text-[11px] font-medium text-amber-700 leading-snug">${t("quarantine.basisStale")}</p>
        </div>` : ""}
      <div class="flex items-center gap-2">
        ${btn({ label: t("quarantine.reject"), variant: "secondary", size: "sm", onClick: `RC.bellReject('${esc(n.proposalId || "")}')` })}
        ${n.basisStale
          ? (forcing
              ? btn({ label: t("quarantine.forceApprove"), variant: "destructive", size: "sm", onClick: `RC.bellApprove('${esc(n.proposalId || "")}', true)` })
              : btn({ label: t("quarantine.approve"), variant: "outline", size: "sm", onClick: `RC.bellApprove('${esc(n.proposalId || "")}', false)` }))
          : btn({ label: t("quarantine.approve"), variant: "default", size: "sm", onClick: `RC.bellApprove('${esc(n.proposalId || "")}', false)` })}
      </div>
    </div>`;
}

// Repaint the badge + an open dropdown in place — no full re-render.
function paintBell() {
  const app = ctx.app;
  if (!app || app.state.locked) return;
  const n = (app.data.notifications || []).filter((x) => x.kind === "proposal").length;
  const badgeEl = document.getElementById("bell-badge");
  if (badgeEl) {
    badgeEl.textContent = String(n);
    badgeEl.style.display = n ? "inline-flex" : "none";
  }
  const dd = document.getElementById("bell-dropdown");
  if (dd && app.state.bellOpen) dd.innerHTML = dropdownBody();
}

// The in-app toast: a new notification while the dropdown is closed still
// reaches the owner with the same one-tap actions.
function showToast(n) {
  dismissToast();
  const toast = document.createElement("div");
  toast.id = "bell-toast";
  toast.className = "fixed bottom-5 right-5 z-[70] w-80 max-w-[calc(100vw-2.5rem)] rounded-xl border border-border bg-white shadow-xl animate-fade-up";
  toast.innerHTML = `
    <div class="p-4 space-y-2.5">
      <div class="flex items-start justify-between gap-2">
        <p class="text-xs font-semibold">${t("bell.proposal")}</p>
        <button onclick="RC.bellDismissToast()" class="p-0.5 rounded hover:bg-gray-100 text-muted">${icon("close", "w-4 h-4")}</button>
      </div>
      <p class="font-mono text-[11px] text-muted">${esc(n.summary)}</p>
      <div class="flex items-center gap-2">
        ${btn({ label: t("quarantine.reject"), variant: "secondary", size: "sm", onClick: `RC.bellReject('${esc(n.proposalId || "")}')` })}
        ${n.basisStale
          ? ""
          : btn({ label: t("quarantine.approve"), variant: "default", size: "sm", onClick: `RC.bellApprove('${esc(n.proposalId || "")}', false)` })}
      </div>
    </div>`;
  document.body.appendChild(toast);
  setTimeout(dismissToast, 8000);
}

function dismissToast() {
  document.getElementById("bell-toast")?.remove();
}

// ========== HANDLERS ==========
Object.assign(RC, {
  bellToggle() {
    const app = ctx.app;
    app.state.bellOpen = !app.state.bellOpen;
    app.state.bellForceId = null;
    ctx.render();
  },
  bellClose() {
    const app = ctx.app;
    if (!app.state.bellOpen) return;
    app.state.bellOpen = false;
    app.state.bellForceId = null;
    ctx.render();
  },
  bellMarkSeen() {
    const app = ctx.app;
    const items = app.data.notifications || [];
    saveSeen(items.filter((n) => n.kind === "proposal" && n.proposalId).map((n) => n.proposalId));
    dismissToast();
    paintBell();
  },
  // One-tap approve (Story 6.16): the same typed merge command as the
  // board's review — a basis-stale refusal surfaces the force-confirm.
  async bellApprove(proposalId, force) {
    const app = ctx.app;
    try {
      await api.approveProposal(proposalId, force);
      app.state.bellForceId = null;
      dismissToast();
    } catch (e) {
      const msg = String(e?.message || e);
      if (msg.startsWith("basis_stale:")) {
        app.state.bellForceId = proposalId;
        alert(msg);
      } else {
        alert(t("quarantine.actionError") + msg);
      }
    }
    await pollBell();
    if (app.state.view === "missions") await ctx.loadMissions();
    if (app.state.view === "board") await ctx.loadBoard(app.state.boardMissionId);
    paintBell();
  },
  async bellReject(proposalId) {
    const app = ctx.app;
    try {
      await api.rejectProposal(proposalId);
      dismissToast();
    } catch (e) {
      alert(t("quarantine.actionError") + (e?.message || e));
    }
    await pollBell();
    if (app.state.view === "missions") await ctx.loadMissions();
    if (app.state.view === "board") await ctx.loadBoard(app.state.boardMissionId);
    paintBell();
  },
  bellDismissToast: dismissToast,
});

// Escape closes the bell dropdown like every other overlay.
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") dismissToast();
});
