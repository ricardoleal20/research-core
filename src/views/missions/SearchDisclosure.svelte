<script lang="ts">
  import type { SearchDisclosure as SearchDisclosureView } from "../../types";
  import { t } from "../../i18n";
  import { api } from "../../api";
  import { openExport } from "./export";

  // Search protocol disclosure (Story 4.1, FR-12.1): the mission's
  // PRISMA-style read model — every search the runs and the user
  // performed, null results visibly marked with the info chip, never as
  // "nothing happened". Read-model only: it re-queries on every open
  // (replay = re-query); the export button opens the composer whose
  // search_log scope writes the same disclosure at its single seq cut
  // (FR-12.2).
  let { missionId }: { missionId: string } = $props();

  let disclosure = $state<SearchDisclosureView | null>(null);
  let loading = $state(true);
  let loadError = $state("");

  $effect(() => {
    loading = true;
    loadError = "";
    api
      .getSearchDisclosure(missionId)
      .then((d) => {
        disclosure = d;
        loading = false;
      })
      .catch((e) => {
        loadError = t("disclosure.error") + e;
        loading = false;
      });
  });

  const filtersLabel = (filters: Record<string, unknown>) =>
    Object.entries(filters).length === 0
      ? "—"
      : Object.entries(filters)
          .map(([k, v]) => `${k}=${String(v)}`)
          .join(", ");
</script>

<section class="sd" aria-label={t("disclosure.title")}>
  <header class="sd-head">
    <h4 class="sd-title">{t("disclosure.title")}</h4>
    {#if disclosure}
      <span class="sd-summary mono">
        {t("disclosure.summary", { total: disclosure.total, nulls: disclosure.nullResultCount })}
      </span>
    {/if}
    <button class="sd-export" type="button" onclick={openExport}>
      {t("disclosure.export")}
    </button>
  </header>

  {#if loadError}
    <p class="sd-error" role="alert">{loadError}</p>
  {:else if loading}
    <p class="sd-empty">…</p>
  {:else if !disclosure || disclosure.rows.length === 0}
    <!-- the honest empty state: no searches ran yet — never a blank panel
         pretending the disclosure doesn't exist -->
    <p class="sd-empty">{t("disclosure.empty")}</p>
  {:else}
    <div class="sd-table" role="table" aria-label={t("disclosure.title")}>
      <div class="sd-row sd-head-row" role="row">
        <span role="columnheader">{t("disclosure.date")}</span>
        <span role="columnheader">{t("disclosure.query")}</span>
        <span role="columnheader">{t("disclosure.database")}</span>
        <span role="columnheader">{t("disclosure.filters")}</span>
        <span role="columnheader">{t("disclosure.order")}</span>
        <span role="columnheader">{t("disclosure.resultsHeader")}</span>
      </div>
      {#each disclosure.rows as row (row.seq)}
        <div class="sd-row" role="row" class:sd-null={row.nullResult}>
          <span class="mono sd-date" role="cell">
            {new Date(row.startedAt).toLocaleString()} · e-{row.seq}
          </span>
          <span class="sd-query" role="cell">“{row.query}”</span>
          <span class="mono" role="cell">{row.database}</span>
          <span class="mono sd-filters" role="cell">{filtersLabel(row.filters)}</span>
          <span class="mono" role="cell">{row.order ?? "—"}</span>
          <span role="cell">
            {#if row.nullResult}
              <!-- the null result: info-style chip, text always (never
                   color alone) — the search happened and found nothing -->
              <span class="sd-chip info">{t("disclosure.nullResult")}</span>
            {:else}
              <span class="sd-count mono">
                {t("disclosure.results", { count: row.resultCount })}
              </span>
            {/if}
            {#if row.firstPage}
              <span class="sd-first mono">· {t("disclosure.firstPage")}</span>
            {/if}
          </span>
        </div>
      {/each}
    </div>
  {/if}
</section>

<style>
  /* DESIGN.md tokens (local): the info/telemetry hue the receipt drawer's
     chips use — null results render informational, never alarming. */
  .sd {
    --rc-info-ink: #3071b5;
    --rc-info-bg: #eff6fc;
    --rc-ink: #0f172a;
    --rc-muted: #64748b;
    --rc-line: #e2e8f0;
    margin-top: 12px;
    border: 1px solid var(--rc-line);
    border-radius: 10px;
    padding: 12px 14px;
    background: #fff;
  }

  .sd-head {
    display: flex;
    align-items: baseline;
    gap: 10px;
    flex-wrap: wrap;
    margin-bottom: 8px;
  }

  .sd-title {
    margin: 0;
    font-size: 0.95rem;
    font-weight: 600;
    color: var(--rc-ink);
  }

  .sd-summary {
    font-size: 0.78rem;
    color: var(--rc-muted);
    font-variant-numeric: tabular-nums;
  }

  .sd-export {
    margin-left: auto;
    font-size: 0.78rem;
    color: var(--rc-info-ink);
    background: var(--rc-info-bg);
    border: 1px solid transparent;
    border-radius: 999px;
    padding: 3px 10px;
    cursor: pointer;
  }

  .sd-export:hover { text-decoration: underline; }
  .sd-export:focus-visible { outline: 2px solid var(--rc-info-ink); outline-offset: 1px; }

  .sd-table {
    display: flex;
    flex-direction: column;
    font-size: 0.82rem;
  }

  .sd-row {
    display: grid;
    grid-template-columns: 185px minmax(160px, 1fr) 110px 140px 80px 170px;
    gap: 8px;
    padding: 7px 6px;
    border-top: 1px solid var(--rc-line);
    align-items: center;
  }

  .sd-head-row {
    border-top: none;
    color: var(--rc-muted);
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .sd-row.sd-null {
    /* the null-result row: info tint — visible, informational, never hidden */
    background: var(--rc-info-bg);
  }

  .mono {
    font-family: ui-monospace, "SF Mono", SFMono-Regular, Menlo, monospace;
    font-variant-numeric: tabular-nums;
  }

  .sd-date { color: var(--rc-muted); font-size: 0.76rem; }
  .sd-query { color: var(--rc-ink); }
  .sd-filters { color: var(--rc-muted); font-size: 0.76rem; overflow-wrap: anywhere; }
  .sd-count { color: var(--rc-ink); }

  .sd-chip {
    display: inline-block;
    font-size: 0.72rem;
    border-radius: 999px;
    padding: 1px 8px;
    border: 1px solid transparent;
  }

  .sd-chip.info {
    color: var(--rc-info-ink);
    background: #fff;
    border-color: var(--rc-info-ink);
  }

  .sd-first { color: var(--rc-muted); font-size: 0.72rem; }

  .sd-empty {
    margin: 4px 0 0;
    font-size: 0.82rem;
    color: var(--rc-muted);
  }

  .sd-error {
    margin: 4px 0 0;
    font-size: 0.82rem;
    color: #be123c;
  }

  @media (max-width: 860px) {
    .sd-row {
      grid-template-columns: 1fr 1fr;
    }
    .sd-head-row { display: none; }
  }
</style>
