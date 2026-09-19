<script lang="ts">
  import { api } from "../../api";
  import { t } from "../../i18n";
  import type { ReceiptRow, RunReceipt } from "../../types";

  // Run Timeline Receipts (Story 2.5, FR-6.1/6.2): the drill-down drawer for
  // one agent run — an ordered ledger of every autonomous action, replayed
  // identically from the event log. This component renders ONLY as a drawer
  // opened from a mission card's runs list or a digest row (FR-6.2): it is
  // never a nav item, never a parallel surface. Read-model only (AD-8).
  let { runId = null, onclose = () => {} }: { runId?: string | null; onclose?: () => void } =
    $props();

  let receipt = $state<RunReceipt | null>(null);
  let loading = $state(false);
  let loadError = $state("");

  $effect(() => {
    const id = runId;
    if (!id) {
      receipt = null;
      loadError = "";
      return;
    }
    loading = true;
    loadError = "";
    receipt = null;
    api
      .getRunReceipt(id)
      .then((r) => {
        receipt = r;
        loading = false;
      })
      .catch((e) => {
        loadError = t("receipt.loadError") + e;
        loading = false;
      });
  });

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }

  // Mono receipt voice: UTC HH:MM:SS — deterministic, the same on every
  // replay (the ledger's timestamps are the log's).
  const time = (ts: string) => new Date(ts).toISOString().slice(11, 19);
  const date = (ts: string) => new Date(ts).toISOString().slice(0, 10);

  const durationLabel = $derived(
    receipt?.durationSecs != null
      ? t("receipt.duration", { min: Math.max(1, Math.round(receipt.durationSecs / 60)) })
      : "",
  );

  // The outcome chip (receipt frame): finished/failed/open, with the honest
  // "partial — ceiling hit" variant when a ceiling refused a dispatch.
  const outcomeLabel = $derived(
    receipt ? t(`receipt.outcome.${receipt.outcome}`) : "",
  );
  const outcomeClass = $derived(
    !receipt ? "" : receipt.ceilingHit ? "hit" : receipt.outcome,
  );

  // The chip vocabulary (receipt frame): neutral run mechanics, info search,
  // success claims and merges, warning proposals, destructive refusals and
  // failures. Never color alone — every chip carries its text.
  function chip(row: ReceiptRow): string {
    switch (row.kind) {
      case "search":
        return "info";
      case "claim":
        return "success";
      case "proposal":
        return "warning";
      case "decision":
        return row.decision === "merged" ? "success" : "warning";
      case "refused":
        return "destructive";
      case "run_end":
        return row.outcome === "failed" ? "destructive" : "neutral";
      default:
        return "neutral";
    }
  }

  function chipKey(row: ReceiptRow): string {
    return `receipt.chip.${row.kind}`;
  }

  // The one-line description each row renders — composed from the row's
  // typed atoms, bilingual through the i18n layer (values stay mono).
  function line(row: ReceiptRow): string {
    switch (row.kind) {
      case "run_start":
        return t("receipt.line.run_start", { step: row.step, schedule: row.schedule });
      case "search":
        return t("receipt.line.search", { query: row.query });
      case "call":
        return t("receipt.line.call", {
          provider: row.provider,
          model: row.model,
          in: row.inputTokens.toLocaleString(),
          out: row.outputTokens.toLocaleString(),
          cost: row.costCents,
        });
      case "claim":
        return t("receipt.line.claim", { text: row.text });
      case "proposal":
        return t("receipt.line.proposal", {
          to: row.to,
          seq: row.proposalSeq,
          status: row.status,
        });
      case "decision":
        return t(`receipt.line.decision.${row.decision}`, { seq: row.proposalSeq });
      case "refused":
        return t("receipt.line.refused", {
          would: row.wouldBeCostCents,
          ceiling: row.ceilingCents,
          scope: row.scope,
        });
      case "released":
        return t("receipt.line.released", { reason: row.reason });
      case "run_end":
        return row.outcome === "failed"
          ? t("receipt.line.run_end.failed", { reason: row.reason ?? "unknown" })
          : t("receipt.line.run_end.finished", { verdict: row.verdict ?? "" });
    }
  }
</script>

<svelte:window onkeydown={runId ? onKeydown : undefined} />

{#if runId}
  <!-- The receipt drawer (FR-6.2): reachable ONLY from a mission card's runs
       list or a digest row — the breadcrumb names the drill-down path, and
       there is no other surface that opens it. -->
  <div class="drawer-root" role="presentation" onclick={onclose}>
    <aside
      class="drawer"
      role="dialog"
      aria-modal="true"
      aria-label={t("receipt.title")}
      onclick={(e) => e.stopPropagation()}
    >
      <header class="d-head">
        <!-- Breadcrumb (receipt frame): Missions → M-n → run -->
        <nav class="crumbs" aria-label={t("missions.title")}>
          <span>{t("missions.title")}</span>
          <span class="sep" aria-hidden="true">›</span>
          <span class="mono">M-{receipt?.missionSeq ?? "—"}</span>
          <span class="sep" aria-hidden="true">›</span>
          <span class="cur mono">{runId}</span>
        </nav>
        <button class="d-close" type="button" onclick={onclose} aria-label={t("receipt.close")}>
          ✕
        </button>
      </header>

      {#if loadError}
        <p class="error" role="alert">{loadError}</p>
      {:else if loading}
        <p class="d-empty">…</p>
      {:else if !receipt}
        <!-- An honest no-receipt: the run id has no run.started — never an
             empty ledger pretending to be one -->
        <p class="d-empty">{t("receipt.none")}</p>
      {:else}
        <div class="d-body">
          <!-- The run header (receipt frame): micro label, run display,
               outcome chip + mono meta line -->
          <div class="run-head">
            <p class="micro">{t("receipt.title")}</p>
            <h3 class="run-display mono">run · {receipt.runId}</h3>
            <div class="run-meta">
              <span class="chip {outcomeClass}">{outcomeLabel}</span>
              <span class="mono meta-line">
                {durationLabel}
                {#if receipt.endedTs}
                  · {time(receipt.startedTs)}–{time(receipt.endedTs)} UTC · {date(receipt.startedTs)}
                {/if}
              </span>
              <span class="mono meta-line">
                {#if receipt.models.length > 0}{receipt.models.join(" · ")} · {/if}
                {t("receipt.spendOf", { spend: receipt.spendCents, ceiling: receipt.ceilingCents })}
              </span>
            </div>
          </div>

          <!-- The ordered ledger: mono timestamp, action chip, one line,
               e-seq ref — the honest refused row renders as the alert line -->
          <div class="ledger">
            {#each receipt.rows as row (row.seq)}
              <div class="row" class:alert={row.kind === "refused"}>
                <span class="row-time mono">{time(row.ts)}</span>
                <span class="row-chip chip {chip(row)}">{t(chipKey(row))}</span>
                <p class="row-desc">{line(row)}</p>
                <span class="row-seq mono">e-{row.seq}</span>
              </div>
            {/each}
          </div>

          <p class="d-foot mono">
            {t("receipt.rowsNote", { count: receipt.rows.length })}
          </p>
        </div>
      {/if}
    </aside>
  </div>
{/if}

<style>
  .drawer-root {
    position: fixed;
    inset: 0;
    z-index: 80;
    background: color-mix(in oklch, var(--fg) 32%, transparent);
    display: flex;
    justify-content: flex-end;
  }
  .drawer {
    --rc-surface: var(--surface);
    --rc-surface-2: var(--surface-2);
    --rc-ink: var(--fg);
    --rc-ink-muted: var(--muted);
    --rc-border: var(--border);
    --rc-accent: var(--accent);
    --rc-danger-ink: var(--destructive-tx);
    --rc-danger-bg: color-mix(in oklch, var(--destructive) 6%, var(--surface));
    --rc-success-ink: var(--st-read-tx);
    --rc-success-bg: color-mix(in oklch, var(--st-read) 14%, var(--surface));
    --rc-info-ink: var(--accent-text);
    --rc-info-bg: var(--accent-soft);
    --rc-attention-ink: var(--st-reading-tx);
    --rc-attention-bg: color-mix(in oklch, var(--st-reading) 14%, var(--surface));
    width: min(680px, 100vw);
    height: 100%;
    background: var(--rc-surface);
    border-left: 1px solid var(--rc-border);
    box-shadow: -12px 0 40px -18px rgba(0, 0, 0, 0.25);
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    font-family: var(--font-body);
    color: var(--rc-ink);
    animation: drawer-in 0.22s ease;
  }
  @keyframes drawer-in {
    from {
      transform: translateX(24px);
      opacity: 0.6;
    }
    to {
      transform: translateX(0);
      opacity: 1;
    }
  }
  .mono {
    font-family: var(--font-mono);
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

  .d-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 16px 24px;
    border-bottom: 1px solid var(--rc-border);
    position: sticky;
    top: 0;
    background: var(--rc-surface);
    z-index: 1;
  }
  .crumbs {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    color: var(--rc-ink-muted);
    min-width: 0;
    flex-wrap: wrap;
  }
  .crumbs .cur {
    color: var(--rc-ink);
    font-size: 12px;
  }
  .crumbs .sep {
    color: var(--rc-ink-muted);
  }
  .d-close {
    font-family: inherit;
    font-size: 13px;
    line-height: 1;
    color: var(--rc-ink-muted);
    background: transparent;
    border: 1px solid var(--rc-border);
    border-radius: var(--r-input);
    padding: 6px 10px;
    cursor: pointer;
  }
  .d-close:hover {
    color: var(--rc-ink);
    background: var(--rc-surface-2);
  }
  .d-close:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }

  .error {
    margin: 16px 24px;
    font-size: 13px;
    color: var(--rc-danger-ink);
  }
  .d-empty {
    margin: 24px;
    font-size: 13.5px;
    line-height: 1.6;
    color: var(--rc-ink-muted);
  }

  .d-body {
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  /* The run header (receipt frame). */
  .run-head {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .run-display {
    margin: 0;
    font-size: 19px;
    font-weight: 600;
    letter-spacing: -0.01em;
    word-break: break-all;
  }
  .run-meta {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }
  .meta-line {
    font-size: 12px;
    color: var(--rc-ink-muted);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    font-size: 11.5px;
    font-weight: 500;
    border-radius: 9999px;
    padding: 2px 10px;
    white-space: nowrap;
  }
  .chip.finished {
    color: var(--rc-success-ink);
    background: var(--rc-success-bg);
    box-shadow: inset 0 0 0 1px color-mix(in oklch, var(--st-read) 25%, transparent);
  }
  .chip.failed {
    color: var(--rc-danger-ink);
    background: var(--rc-danger-bg);
    box-shadow: inset 0 0 0 1px color-mix(in oklch, var(--destructive) 25%, transparent);
  }
  .chip.open {
    color: var(--rc-ink-muted);
    background: var(--rc-surface-2);
    box-shadow: inset 0 0 0 1px var(--rc-border);
  }
  .chip.hit {
    color: var(--rc-attention-ink);
    background: var(--rc-attention-bg);
    box-shadow: inset 0 0 0 1px color-mix(in oklch, var(--st-reading) 25%, transparent);
  }

  /* The ledger: grid [time | chip | line | seq], rows in seq order. */
  .ledger {
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: var(--r-card);
    overflow: hidden;
  }
  .row {
    display: grid;
    grid-template-columns: 64px auto 1fr auto;
    gap: 12px;
    align-items: start;
    padding: 12px 18px;
    border-bottom: 1px solid var(--rc-border);
  }
  .row:last-child {
    border-bottom: none;
  }
  /* the honest refused row renders as the alert line (receipt frame) */
  .row.alert {
    background: var(--rc-danger-bg);
  }
  .row.alert .row-desc {
    color: var(--rc-danger-ink);
    font-weight: 500;
  }
  .row-time {
    font-size: 11.5px;
    color: var(--rc-ink-muted);
    line-height: 1.9;
    padding-top: 2px;
  }
  .row-chip {
    justify-self: start;
    margin-top: 2px;
  }
  .row-chip.neutral {
    color: var(--rc-ink-muted);
    background: var(--rc-surface-2);
  }
  .row-chip.info {
    color: var(--rc-info-ink);
    background: var(--rc-info-bg);
  }
  .row-chip.success {
    color: var(--rc-success-ink);
    background: var(--rc-success-bg);
  }
  .row-chip.warning {
    color: var(--rc-attention-ink);
    background: var(--rc-attention-bg);
  }
  .row-chip.destructive {
    color: var(--rc-danger-ink);
    background: var(--rc-danger-bg);
  }
  .row-desc {
    margin: 0;
    font-size: 13px;
    line-height: 1.6;
    min-width: 0;
    padding-top: 1px;
  }
  .row-seq {
    font-size: 11px;
    color: var(--rc-ink-muted);
    padding-top: 4px;
    white-space: nowrap;
  }
  .d-foot {
    margin: 0;
    font-size: 11px;
    color: var(--rc-ink-muted);
  }

  @media (max-width: 640px) {
    /* reflow, never hide: the chip carries the row's kind text (never color
       alone) and the e-seq ref is the audit anchor — both stay visible */
    .row {
      grid-template-columns: 64px 1fr auto;
      gap: 8px 10px;
    }
    .row-desc {
      grid-column: 2 / -1;
    }
  }
</style>
