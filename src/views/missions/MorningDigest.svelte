<script lang="ts">
  import { api } from "../../api";
  import { t } from "../../i18n";
  import type { DigestRow, MissionStatus, MorningDigest } from "../../types";

  // The Morning Digest (Story 2.3, FR-4.1/4.4 — the OpenDesign digest frame):
  // the Night Shift result, skimmable in ninety seconds. Read-model only
  // (AD-8): the digest holds exactly what get_morning_digest returned; the
  // manual trigger is the one action, and it re-folds everything after.
  // Story 2.5 (FR-6.2): each row's "receipts →" link drills into its run's
  // receipt drawer — the run timeline is never a parallel surface.
  let {
    onran = () => {},
    onopenreceipt = () => {},
  }: { onran?: () => void; onopenreceipt?: (runId: string) => void } = $props();

  let digest = $state<MorningDigest | null>(null);
  let loadError = $state("");
  let running = $state(false);
  let runError = $state("");

  $effect(() => {
    load();
  });

  async function load() {
    try {
      digest = await api.getMorningDigest();
      loadError = "";
    } catch (e) {
      loadError = t("digest.loadError") + e;
    }
  }

  async function runNow() {
    running = true;
    runError = "";
    try {
      digest = await api.runNightShiftNow();
      onran(); // spend, proposals, and statuses may all have moved
    } catch (e) {
      runError = String(e);
    } finally {
      running = false;
    }
  }

  const dollars = (cents: number) => `$${(cents / 100).toFixed(2)}`;
  const timeLabel = $derived(
    digest ? new Date(digest.generatedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "",
  );
  const dateLabel = $derived(
    digest ? new Date(digest.generatedAt).toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" }) : "",
  );
  // The gradient ring (DESIGN.md digest frame): spend vs ceiling at a glance.
  const ringPct = $derived(
    digest && digest.ceilingCents > 0
      ? Math.min(1, digest.spendCents / digest.ceilingCents)
      : 0,
  );
  const CIRC = 2 * Math.PI * 22;
  const dashOffset = $derived(CIRC * (1 - ringPct));

  // DESIGN.md mission lifecycle tokens (same mapping as the mission card).
  const statusColor: Record<MissionStatus, string> = {
    active: "#3071B5",
    awaiting_review: "#B45309",
    completed: "#047857",
    stopped: "#334155",
    failed: "#BE123C",
  };

  // The one-line verdict of a row (FR-4.4 — ≤2 lines rendered: the verdict
  // plus its translation). The honest failure row carries its reason (FR-4.3).
  // Remote job completions (Story 3.4) join the line — a jobs-only night
  // still names what the cluster did.
  function verdict(row: DigestRow): string {
    const label = `M-${row.missionSeq}`;
    if (row.failed > 0 && row.finished === 0) {
      return `${label} · ${t("digest.verdict.failed", { reason: row.failureReason ?? "unknown" })}`;
    }
    let line = `${label} · ${t("digest.verdict.ok", { runs: row.runs, proposals: row.proposalsPending })}`;
    if (row.jobsFinished > 0) {
      line += ` · ${t("digest.verdict.jobs", { count: row.jobsFinished })}`;
    }
    if (row.ceilingReached) {
      line += ` · ${t("digest.verdict.ceiling")}`;
    } else if (row.status === "completed") {
      line += ` · ${t("digest.verdict.criterion")}`;
    }
    return line;
  }

  // The latest completed job's one-line verdict (Story 3.4, FR-11.5):
  // target · job · finished/failed — the short id is the mono anchor.
  function jobVerdict(row: DigestRow): string {
    if (!row.jobVerdict) return "";
    const job = row.jobVerdict.jobId.slice(0, 8);
    return row.jobVerdict.failed
      ? t("digest.job.failed", {
          target: row.jobVerdict.target,
          job,
          reason: row.jobVerdict.reason ?? "unknown",
        })
      : t("digest.job.finished", { target: row.jobVerdict.target, job });
  }
</script>

{#if loadError}
  <p class="error" role="alert">{loadError}</p>
{/if}

{#if digest && (digest.rows.length > 0 || digest.alerts.length > 0 || digest.connectionAlerts.length > 0)}
<section class="digest" aria-label={t("digest.title")}>
  <!-- The digest hero (digest frame): outcome badge, mono spend line, and
       the gradient spend ring. Never color alone — the mono line always
       renders the amounts too. -->
  <div class="d-hero">
    <div class="d-hero-text">
      <p class="micro">{t("digest.micro")}</p>
      <h3 class="d-date">{dateLabel}</h3>
      <p class="d-meta mono">
        {timeLabel} · {t("digest.nightShift")} ·
        {dollars(digest.spendCents)}
        {t("digest.ofCeiling", { ceiling: dollars(digest.ceilingCents) })}
      </p>
      <span
        class="d-chip"
        class:failed={digest.outcome === "all_failed"}
        class:partial={digest.outcome === "partial_success"}
      >
        {t(`digest.outcome.${digest.outcome}`)}
      </span>
    </div>
    <div class="d-ring">
      <svg width="56" height="56" viewBox="0 0 56 56" role="img"
        aria-label={`${dollars(digest.spendCents)} / ${dollars(digest.ceilingCents)}`}>
        <defs>
          <linearGradient id="ringGrad" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stop-color="#3071B5" />
            <stop offset="1" stop-color="#32838F" />
          </linearGradient>
        </defs>
        <circle cx="28" cy="28" r="22" fill="none" stroke="#E4E4E7" stroke-width="5" />
        <circle
          cx="28" cy="28" r="22" fill="none" stroke="url(#ringGrad)" stroke-width="5"
          stroke-linecap="round" stroke-dasharray={CIRC} stroke-dashoffset={dashOffset}
          transform="rotate(-90 28 28)"
        />
        <text x="28" y="31.5" text-anchor="middle" class="ring-txt">{dollars(digest.spendCents)}</text>
      </svg>
      <p class="micro">{t("digest.ofCeiling", { ceiling: dollars(digest.ceilingCents) })}</p>
    </div>
  </div>

  <!-- The digest rows: ≤10, one line per mission, status chip + receipts
       link; the honest failed row and the dead-man alert row. -->
  <div class="d-card">
    {#each digest.rows as row (row.missionId)}
      <div class="d-row" class:failed={row.failed > 0 && row.finished === 0}>
        <p class="d-line mono" title={row.question}>{verdict(row)}</p>
        {#if row.jobVerdict}
          <!-- The job completion's one-line verdict (Story 3.4, FR-11.5):
               target · job · finished/failed — its results wait in
               quarantine as pin candidates. -->
          <p class="d-line d-job mono">{jobVerdict(row)}</p>
        {/if}
        <div class="d-side">
          <span class="status-chip" style={`color:${statusColor[row.status]};background:color-mix(in srgb, ${statusColor[row.status]} 10%, transparent)`}>
            {t(`missions.status.${row.status}`)}
          </span>
          <!-- The receipts drill-down (FR-6.2): opens the run's receipt
               drawer — the row carries its run id for exactly this -->
          <button
            class="d-link mono"
            type="button"
            onclick={() => row.runId && onopenreceipt(row.runId)}
            disabled={!row.runId}
          >
            {t("digest.receipts")} →
          </button>
        </div>
      </div>
    {/each}
    {#each digest.alerts as alert (alert.runId)}
      <!-- The dead-man-switch alert row (FR-9.1/FR-4.3, Story 2.6): renders
           from the run.dead detection event — label + icon (amber), never
           color alone. -->
      <div class="d-row d-row--alert" role="alert">
        <p class="d-line mono">
          <span class="d-label">{t("digest.alertLabel")}</span>
          {t("digest.alertBody", {
            runId: alert.runId,
            time: new Date(alert.heartbeatTs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }),
          })}
        </p>
        <div class="d-side">
          <!-- the dead run's receipt drills down the same way (FR-6.2) -->
          <button
            class="d-link mono"
            type="button"
            onclick={() => alert.runId && onopenreceipt(alert.runId)}
          >
            {t("digest.receipts")} →
          </button>
        </div>
      </div>
    {/each}
    {#each digest.connectionAlerts as ca (ca.connection)}
      <!-- Connection alert row (FR-9.1, Story 2.6): a research connection
           (Zotero, arXiv, Semantic Scholar) is down — the ⚠ lives in the
           label (label + icon, never color alone). -->
      <div class="d-row d-row--alert" role="alert">
        <p class="d-line mono">
          <span class="d-label">{t("digest.connAlertLabel")}</span>
          {t("digest.connAlertBody", {
            connection: ca.connection,
            code: ca.errorCode,
            time: new Date(ca.failedTs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }),
          })}
        </p>
      </div>
    {/each}
    <p class="d-note mono">{t("digest.rowsNote")}</p>
    <div class="d-foot">
      <span class="mono">
        {t("digest.foot", { rows: digest.rows.length, alerts: digest.alerts.length })}
      </span>
      <button class="d-run" type="button" onclick={runNow} disabled={running}>
        {running ? t("digest.running") : t("digest.runNow")}
      </button>
    </div>
  </div>
  {#if runError}
    <p class="error" role="alert">{runError}</p>
  {/if}
</section>
{/if}
{#if digest && digest.rows.length === 0 && digest.alerts.length === 0 && digest.connectionAlerts.length === 0}
  <section class="digest digest--empty" aria-label={t("digest.title")}>
    <p class="micro">{t("digest.micro")}</p>
    <p class="d-empty">{t("digest.empty")}</p>
    <button class="d-run" type="button" onclick={runNow} disabled={running}>
      {running ? t("digest.running") : t("digest.runNow")}
    </button>
  </section>
{/if}

<style>
  .digest {
    --rc-surface: #ffffff;
    --rc-surface-2: #f4f4f6;
    --rc-ink: #151519;
    --rc-ink-muted: #71717a;
    --rc-border: #eaeaec;
    --rc-accent: #3071b5;
    --rc-danger-ink: #be123c;
    --rc-attention-ink: #92400e;
    --rc-attention-bg: #fef3c7;
    display: flex;
    flex-direction: column;
    gap: 18px;
    font-family: Inter, system-ui, sans-serif;
    color: var(--rc-ink);
  }
  .mono {
    font-family: "JetBrains Mono", ui-monospace, monospace;
    font-variant-numeric: tabular-nums;
  }
  .micro {
    margin: 0;
    font-size: 12px;
    font-weight: 500;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
  }
  .error {
    margin: 0;
    font-size: 13px;
    color: var(--rc-danger-ink);
  }

  /* The hero: date + mono spend line + outcome chip, with the ring. */
  .d-hero {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 28px;
  }
  .d-date {
    font-family: "Instrument Serif", Georgia, serif;
    font-style: italic;
    font-weight: 400;
    font-size: 26px;
    line-height: 1.15;
    margin: 6px 0 10px;
  }
  .d-meta {
    margin: 0 0 12px;
    font-size: 12.5px;
    color: var(--rc-ink-muted);
  }
  .d-chip {
    display: inline-flex;
    align-items: center;
    font-size: 12px;
    font-weight: 500;
    color: #047857;
    background: rgba(4, 120, 87, 0.1);
    border-radius: 9999px;
    padding: 2px 10px;
    box-shadow: inset 0 0 0 1px rgba(4, 120, 87, 0.25);
  }
  .d-chip.partial {
    color: var(--rc-attention-ink);
    background: var(--rc-attention-bg);
    box-shadow: inset 0 0 0 1px rgba(146, 64, 14, 0.25);
  }
  .d-chip.failed {
    color: var(--rc-danger-ink);
    background: rgba(190, 18, 60, 0.08);
    box-shadow: inset 0 0 0 1px rgba(190, 18, 60, 0.25);
  }
  .d-ring {
    text-align: center;
    flex-shrink: 0;
    padding-top: 6px;
  }
  .d-ring .micro {
    margin-top: 8px;
  }
  .ring-txt {
    font-family: "JetBrains Mono", ui-monospace, monospace;
    font-size: 10px;
    fill: var(--rc-ink);
  }

  /* The rows card (digest frame). */
  .d-card {
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 14px;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
    overflow: hidden;
  }
  .d-row {
    display: flex;
    gap: 16px;
    align-items: flex-start;
    justify-content: space-between;
    padding: 13px 20px;
    border-bottom: 1px solid var(--rc-border);
  }
  .d-row.failed .d-line {
    color: var(--rc-danger-ink);
  }
  .d-row--alert {
    background: var(--rc-attention-bg);
  }
  .d-line {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.7;
    min-width: 0;
  }
  /* The job completion verdict (Story 3.4): the second line of a row that
     ran remote jobs — quieter than the mission verdict, still mono. */
  .d-job {
    font-size: 11.5px;
    color: var(--rc-ink-muted);
  }
  .d-label {
    font-weight: 500;
    color: var(--rc-attention-ink);
    margin-right: 10px;
  }
  .d-side {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-shrink: 0;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .status-chip {
    font-size: 11.5px;
    font-weight: 500;
    border-radius: 9999px;
    padding: 2px 10px;
  }
  .d-link {
    /* a button styled as the row's quiet mono link (FR-6.2 drill-down) */
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-family: "JetBrains Mono", ui-monospace, monospace;
    font-size: 11.5px;
    font-weight: 500;
    color: var(--rc-accent);
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    white-space: nowrap;
  }
  .d-link:hover:not(:disabled) {
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  .d-link:disabled {
    color: var(--rc-ink-muted);
    opacity: 0.6;
    cursor: default;
  }
  .d-link:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }
  .d-note {
    margin: 12px 20px;
    background: rgba(48, 113, 181, 0.08);
    color: var(--rc-accent);
    border-radius: 8px;
    padding: 7px 12px;
    font-size: 11px;
  }
  .d-foot {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 14px;
    padding: 10px 20px 12px;
    font-size: 11px;
    color: var(--rc-ink-muted);
    flex-wrap: wrap;
  }
  .d-run {
    font-family: inherit;
    font-size: 12px;
    font-weight: 500;
    color: var(--rc-accent);
    background: transparent;
    border: 1px solid var(--rc-border);
    border-radius: 8px;
    padding: 4px 12px;
    cursor: pointer;
    transition: background 0.15s ease;
  }
  .d-run:hover:not(:disabled) {
    background: rgba(48, 113, 181, 0.08);
  }
  .d-run:disabled {
    opacity: 0.55;
    cursor: wait;
  }
  .d-run:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }

  .digest--empty {
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 14px;
    padding: 20px;
    gap: 10px;
  }
  .d-empty {
    margin: 0;
    font-size: 14px;
    color: var(--rc-ink-muted);
  }

  @media (max-width: 640px) {
    .d-hero {
      flex-direction: column;
      gap: 14px;
    }
    .d-row {
      flex-direction: column;
      gap: 8px;
    }
    .d-side {
      justify-content: flex-start;
    }
  }
</style>
