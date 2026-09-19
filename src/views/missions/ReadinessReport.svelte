<script lang="ts">
  import { api } from "../../api";
  import { t } from "../../i18n";
  import type {
    Mission,
    ReadinessItem,
    ReadinessItemKind,
    ReadinessReport,
    ReadinessTrailRow,
  } from "../../types";

  // The readiness report (Story 4.3, FR-13 — the final PRD story): the
  // preprint-tier gate, derived from board state. Read-model only (AD-8):
  // the panel holds exactly what get_readiness_report folded; every fix
  // happens on the board (pin a claim, resolve a hypothesis, answer a null)
  // and the panel re-folds — the gate cannot be "run" into readiness, only
  // asked again. NO scores, only the specific objects (FR-13.1).
  let {
    missions,
    revision,
  }: { missions: Mission[]; revision: number } = $props();

  // The scope: the whole workspace (default) or one mission's board.
  let scope = $state<string | null>(null);
  let report = $state<ReadinessReport | null>(null);
  let loadError = $state("");

  // The three-state presentation (the frame's state strip), derived — never
  // scored: not_ready = ≥1 blocker; near = 0 blockers but ≥1 advisory info
  // row; ready = neither. States, never percentages.
  let state = $derived<"not_ready" | "near" | "ready">(
    !report || report.blockers.length > 0
      ? "not_ready"
      : report.infos.length > 0
        ? "near"
        : "ready",
  );

  // Blockers render grouped by category (the fold emits them kind-sorted):
  // one row per category, every object chipped — the frame's three rows
  // (CLAIMS-2/5/9 chips, H-4 with lifecycle + contradicted-by chip, search
  // log #12 chip).
  interface BlockerGroup {
    kind: ReadinessItemKind;
    items: ReadinessItem[];
  }
  let blockerGroups = $derived.by<BlockerGroup[]>(() => {
    const groups: BlockerGroup[] = [];
    for (const b of report?.blockers ?? []) {
      const last = groups[groups.length - 1];
      if (last && last.kind === b.kind) last.items.push(b);
      else groups.push({ kind: b.kind, items: [b] });
    }
    return groups;
  });

  $effect(() => {
    // Re-fold when the scope changes or the board changed underneath (a
    // quarantine merge, a rollback — the parent's revision signal).
    void scope;
    void revision;
    load();
  });

  async function load() {
    try {
      report = await api.getReadinessReport(scope);
      loadError = "";
    } catch (e) {
      loadError = t("rd.loadError") + e;
    }
  }

  function groupTitle(g: BlockerGroup): string {
    switch (g.kind) {
      case "unpinned_claim":
        return t("rd.b.unpinned", { count: g.items.length });
      case "load_bearing_unresolved":
        return t("rd.b.loadUnresolved");
      case "load_bearing_refuted":
        return t("rd.b.loadRefuted");
      default:
        return t("rd.b.null");
    }
  }

  function groupDesc(g: BlockerGroup): string {
    switch (g.kind) {
      case "unpinned_claim":
        return t("rd.b.unpinned.d");
      case "load_bearing_unresolved": {
        const it = g.items[0];
        return t("rd.b.loadUnresolved.d", {
          seq: it.hypothesisSeq ?? "?",
          status: it.hypothesisStatus ? t(`hyp.status.${it.hypothesisStatus}`) : "?",
        });
      }
      case "load_bearing_refuted": {
        const it = g.items[0];
        return t("rd.b.loadRefuted.d", { seq: it.hypothesisSeq ?? "?" });
      }
      default:
        return t("rd.b.null.d", { seq: g.items[0].searchSeq ?? "?" });
    }
  }

  function trailTitle(row: ReadinessTrailRow): string {
    switch (row.kind) {
      case "claims_pinned":
        return t("rd.t.claims", {
          clean: row.clean, total: row.total, verified: row.verified,
        });
      case "hypotheses_resolved":
        return t("rd.t.hyps", { clean: row.clean, total: row.total });
      case "nulls_disclosed":
        return t("rd.t.nulls", { clean: row.clean, total: row.total });
      default:
        return row.pending > 0
          ? t("rd.t.queue.pending", { count: row.pending })
          : t("rd.t.queue.empty");
    }
  }

  // The relation tie chip label: the board's own vocabulary
  // ("contradicted-by H-3" / "contradicts H-3").
  function tieLabel(kind: string, otherSeq: number, incoming: boolean): string {
    return `${t(`hyp.rel.${kind}.${incoming ? "in" : "out"}`)} H-${otherSeq}`;
  }
</script>

<div class="rd-panel" role="region" aria-label={t("rd.report")}>
  <header class="rd-head">
    <div class="rd-head-title">
      <p class="rd-kicker">{t("rd.derived")}</p>
      <h4 class="rd-question">{t("rd.question")}</h4>
    </div>
    <div class="rd-head-controls">
      <!-- Scope: the whole workspace or one mission's board. -->
      <select
        class="rd-select"
        bind:value={scope}
        aria-label={t("rd.scope")}
      >
        <option value={null}>{t("rd.scope.workspace")}</option>
        {#each missions as mission (mission.id)}
          <option value={mission.id}>M-{mission.seq} · {mission.question}</option>
        {/each}
      </select>
    </div>
  </header>

  {#if loadError}
    <p class="rd-error" role="alert">{loadError}</p>
  {:else if report === null}
    <p class="rd-empty">…</p>
  {:else}
    <!-- The three-state strip (the frame): the active state lit with its
         dot, the others dimmed — shown honestly, never scored. -->
    <div class="rd-strip" role="status">
      <span class="rd-chip rd-chip--destructive" class:dim={state !== "not_ready"}>
        {#if state === "not_ready"}<span class="rd-dot"></span>{/if}
        {t("rd.notReady")}
      </span>
      <span class="rd-chip rd-chip--warning" class:dim={state !== "near"}>
        {#if state === "near"}<span class="rd-dot"></span>{/if}
        {t("rd.near")}
      </span>
      <span class="rd-chip rd-chip--success" class:dim={state !== "ready"}>
        {#if state === "ready"}<span class="rd-dot"></span>{/if}
        {t("rd.ready")}
      </span>
    </div>
    {#if state === "ready"}
      <p class="rd-verdict">{t("rd.preprintReady")}</p>
    {/if}

    <!-- Blocking items: one card row per category, every blocking object
         chipped — the specific board object, never a summary (FR-13.1). -->
    {#if report.blockers.length > 0}
      <div class="rd-card">
        <div class="rd-card-h">
          <h5 class="rd-card-title">{t("rd.blockers")}</h5>
          <span class="rd-chip rd-chip--neutral rd-count">{report.blockers.length}</span>
        </div>
        {#each blockerGroups as group (group.kind)}
          <div class="rd-row">
            <div class="rd-row-body">
              <p class="rd-row-t">{groupTitle(group)}</p>
              <p class="rd-row-d">{groupDesc(group)}</p>
              <div class="rd-chips">
                {#each group.items as item (item.claimId ?? item.hypothesisId ?? item.searchSeq)}
                  {#if item.kind === "unpinned_claim"}
                    <span class="rd-obj mono">CLAIMS-{item.claimSeq}</span>
                  {:else if item.kind === "unreckoned_null_result"}
                    <span class="rd-obj mono">#{item.searchSeq}</span>
                  {:else}
                    <span class="rd-obj rd-obj--status mono">
                      H-{item.hypothesisSeq} · {item.hypothesisStatus ? t(`hyp.status.${item.hypothesisStatus}`) : ""}
                    </span>
                    {#each item.relationTies as tie (tie.otherSeq)}
                      <span class="rd-obj rd-obj--rel mono">
                        {tieLabel(tie.kind, tie.otherSeq, tie.incoming)}
                      </span>
                    {/each}
                    {#if item.claimTies.length > 0}
                      <span class="rd-obj rd-obj--muted">
                        {t("rd.ties.claims", { count: item.claimTies.length })}
                      </span>
                    {/if}
                  {/if}
                {/each}
              </div>
            </div>
          </div>
        {/each}
      </div>
    {:else}
      <p class="rd-clean">{t("rd.blockersNone")}</p>
    {/if}

    <!-- Advisory rows (never blockers — the decided separation): the
         verified-failed pins and a non-empty merge queue stay visible. -->
    {#if report.infos.length > 0}
      <div class="rd-card rd-card--info">
        <div class="rd-card-h">
          <h5 class="rd-card-title">{t("rd.infos")}</h5>
          <span class="rd-chip rd-chip--neutral rd-count">{report.infos.length}</span>
        </div>
        {#each report.infos as info (info.claimId ?? info.pendingCount)}
          <div class="rd-row">
            <div class="rd-row-body">
              <p class="rd-row-t">
                {#if info.kind === "pin_verification_failed"}
                  {t("rd.i.verificationFailed")}
                {:else}
                  {t("rd.i.mergeQueue", { count: info.pendingCount })}
                {/if}
              </p>
              <p class="rd-row-d">
                {#if info.kind === "pin_verification_failed"}
                  {t("rd.i.verificationFailed.d", { seq: info.claimSeq ?? "?" })}
                {:else}
                  {t("rd.i.mergeQueue.d")}
                {/if}
              </p>
              <div class="rd-chips">
                {#if info.kind === "pin_verification_failed"}
                  <span class="rd-obj mono">CLAIMS-{info.claimSeq}</span>
                {:else if info.pendingCount > 0}
                  <span class="rd-obj mono">{info.pendingCount} × pr</span>
                {/if}
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    <!-- The evidence trail (the clean board's justification): four rows,
         each citing the objects that satisfied it — defensible line by
         line. Rendered when nothing blocks (near keeps it too: the counts
         are the facts, the advisory rows ride beside). -->
    {#if report.blockers.length === 0}
      <div class="rd-card rd-card--trail">
        <div class="rd-card-h">
          <h5 class="rd-card-title">{t("rd.trail")}</h5>
          <span class="rd-chip rd-chip--success rd-count">{report.trail.length}</span>
        </div>
        {#each report.trail as row (row.kind)}
          <div class="rd-row">
            <span class="rd-check" aria-hidden="true">✓</span>
            <div class="rd-row-body">
              <p class="rd-row-t">{trailTitle(row)}</p>
              <div class="rd-chips">
                {#each row.refs as ref (ref)}
                  <span class="rd-obj mono">{ref}</span>
                {/each}
              </div>
            </div>
          </div>
        {/each}
        <p class="rd-trail-note">{t("rd.trailNote")}</p>
      </div>
    {/if}
  {/if}
</div>

<style>
  .rd-panel {
    background: var(--rc-surface, #ffffff);
    border: 1px solid var(--rc-border, #eaeaec);
    border-radius: 12px;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .mono {
    font-family: "JetBrains Mono", ui-monospace, monospace;
    font-variant-numeric: tabular-nums;
  }
  .rd-head {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 12px;
    flex-wrap: wrap;
  }
  .rd-kicker {
    font-size: 12px;
    font-weight: 500;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--rc-ink-muted, #71717a);
    margin: 0 0 6px;
  }
  .rd-question {
    font-family: "Instrument Serif", Georgia, serif;
    font-style: italic;
    font-weight: 400;
    font-size: 26px;
    line-height: 1.1;
    margin: 0;
  }
  .rd-select {
    font-family: inherit;
    font-size: 13px;
    color: inherit;
    background: var(--rc-surface, #ffffff);
    border: 1px solid var(--rc-border, #eaeaec);
    border-radius: 8px;
    padding: 8px 10px;
    min-height: 38px;
    max-width: 320px;
    outline: none;
  }
  .rd-select:focus-visible,
  button:focus-visible {
    outline: 2px solid var(--rc-accent, #3071b5);
    outline-offset: 2px;
  }

  /* The three-state strip: the active chip lit + dotted, others dimmed. */
  .rd-strip {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .rd-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    font-weight: 600;
    border-radius: 999px;
    padding: 4px 12px;
    border: 1px solid transparent;
  }
  .rd-chip.dim {
    opacity: 0.45;
    font-weight: 500;
  }
  .rd-chip--destructive {
    color: #be123c;
    background: #fff1f2;
    border-color: #fecdd3;
  }
  .rd-chip--warning {
    color: #92400e;
    background: #fffbeb;
    border-color: #fde68a;
  }
  .rd-chip--success {
    color: #166534;
    background: #f0fdf4;
    border-color: #bbf7d0;
  }
  .rd-chip--neutral {
    color: var(--rc-ink-muted, #71717a);
    background: var(--rc-surface-2, #f4f4f6);
    border-color: var(--rc-border, #eaeaec);
  }
  .rd-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: currentColor;
  }
  .rd-verdict {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    color: #166534;
  }
  .rd-clean {
    margin: 0;
    font-size: 13.5px;
    line-height: 1.5;
    color: var(--rc-ink-muted, #71717a);
  }

  .rd-card {
    border: 1px solid var(--rc-border, #eaeaec);
    border-radius: 10px;
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .rd-card--info {
    background: var(--rc-surface-2, #f4f4f6);
  }
  .rd-card--trail {
    border-color: #bbf7d0;
  }
  .rd-card-h {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .rd-card-title {
    margin: 0;
    font-size: 14.5px;
    font-weight: 600;
  }
  .rd-count {
    padding: 2px 10px;
    font-size: 12px;
  }

  .rd-row {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 8px 0;
    border-top: 1px solid var(--rc-border, #eaeaec);
  }
  .rd-row:first-of-type {
    border-top: 0;
  }
  .rd-check {
    color: #166534;
    font-weight: 700;
    font-size: 14px;
    line-height: 1.5;
  }
  .rd-row-body {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .rd-row-t {
    margin: 0;
    font-size: 13.5px;
    font-weight: 600;
  }
  .rd-row-d {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--rc-ink-muted, #71717a);
  }
  .rd-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 2px;
  }
  .rd-obj {
    font-size: 11.5px;
    font-weight: 500;
    border-radius: 6px;
    padding: 2px 8px;
    background: var(--rc-surface-2, #f4f4f6);
    border: 1px solid var(--rc-border, #eaeaec);
    color: var(--rc-ink, #151519);
  }
  .rd-obj--status {
    background: #fff1f2;
    border-color: #fecdd3;
    color: #be123c;
  }
  .rd-obj--rel {
    background: #eff6ff;
    border-color: #bfdbfe;
    color: #1d4ed8;
  }
  .rd-obj--muted {
    color: var(--rc-ink-muted, #71717a);
  }
  .rd-trail-note {
    margin: 4px 0 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--rc-ink-muted, #71717a);
  }
  .rd-error {
    margin: 0;
    font-size: 13px;
    color: #be123c;
  }
  .rd-empty {
    margin: 0;
    color: var(--rc-ink-muted, #71717a);
  }
</style>
