<script lang="ts">
  import { api } from "../../api";
  import { t } from "../../i18n";
  import type {
    CheckpointsView,
    Hypothesis,
    HypothesisStatus,
    Mission,
    RollbackPlan,
    RollbackOutcome,
  } from "../../types";
  import HypothesisCard from "./HypothesisCard.svelte";
  import ReadinessReport from "./ReadinessReport.svelte";

  // The board at a glance (FR-2.4, Story 1.8): ALL hypotheses and their
  // states in one view — a board-level grid, not per-mission sections
  // (those stay on each mission card). Read-model only (AD-8): the grid
  // holds exactly what list_hypotheses folded per mission; every mutation
  // inside a card re-folds via `refresh`. `revision` (Story 2.2): bumped by
  // the quarantine surface when a merge applies a change — the mission set
  // is unchanged, the hypotheses are not. Story 2.6: `onrollback` fires
  // when a rollback returns the board to a checkpoint — the parent re-folds
  // missions (a post-checkpoint mission disappears) and bumps the revision.
  let {
    missions,
    revision,
    onrollback = () => {},
  }: { missions: Mission[]; revision: number; onrollback?: () => void } = $props();

  // One grid entry: the hypothesis plus the mission it belongs to (the
  // mission label renders on the card so the flat grid stays navigable).
  interface Entry {
    mission: Mission;
    hypothesis: Hypothesis;
    siblings: Hypothesis[];
  }

  let entries = $state<Entry[] | null>(null);
  let loadError = $state("");

  // Status filter (FR-2.4): trivial select over the five lifecycle states.
  let statusFilter = $state<"all" | HypothesisStatus>("all");
  const statuses: HypothesisStatus[] = [
    "proposed",
    "testing",
    "supported",
    "refuted",
    "revised",
  ];

  // Checkpoints control (FR-10.1, Story 2.6): the restore-point list, the
  // create form, and the rollback confirm flow that names every orphaned
  // proposal (EXPERIENCE.md — never a summary).
  let checkpointsOpen = $state(false);
  let checkpoints = $state<CheckpointsView | null>(null);
  let checkpointsError = $state("");
  let newName = $state("");
  let creating = $state(false);
  let createError = $state("");
  // The confirm step: the plan of the rollback being considered.
  let pendingPlan = $state<RollbackPlan | null>(null);
  let rolling = $state(false);
  let rollbackError = $state("");
  // The last executed outcome — the post-rollback note (superseded history
  // lives in the quarantine view; this names what just happened).
  let lastOutcome = $state<RollbackOutcome | null>(null);

  // Readiness report (Story 4.3, FR-13): the preprint-tier gate, opened
  // from the board header — a derived view, re-folded on every revision.
  let readinessOpen = $state(false);

  const filtered = $derived(
    entries === null
      ? []
      : entries.filter(
          (e) => statusFilter === "all" || e.hypothesis.status === statusFilter,
        ),
  );

  $effect(() => {
    // Re-fold whenever the mission set changes (a new mission appears) or a
    // quarantine decision applied a change (the revision bump).
    void missions;
    void revision;
    load();
  });

  async function load() {
    try {
      const perMission = await Promise.all(
        missions.map(async (mission) => ({
          mission,
          hyps: await api.listHypotheses(mission.id),
        })),
      );
      entries = perMission
        .flatMap(({ mission, hyps }) =>
          hyps.map((hypothesis) => ({
            mission,
            hypothesis,
            siblings: hyps.filter((h) => h.id !== hypothesis.id),
          })),
        )
        .sort((a, b) => b.hypothesis.seq - a.hypothesis.seq);
      loadError = "";
    } catch (e) {
      loadError = t("board.loadError") + e;
    }
  }

  function toggleCheckpoints() {
    checkpointsOpen = !checkpointsOpen;
    pendingPlan = null;
    rollbackError = "";
    if (checkpointsOpen) {
      void loadCheckpoints();
    }
  }

  async function loadCheckpoints() {
    try {
      checkpoints = await api.listCheckpoints();
      checkpointsError = "";
    } catch (e) {
      checkpointsError = t("cp.loadError") + e;
    }
  }

  async function createCheckpoint() {
    const name = newName.trim();
    if (!name || creating) return;
    creating = true;
    createError = "";
    try {
      await api.createCheckpoint(name);
      newName = "";
      await loadCheckpoints();
    } catch (e) {
      createError = t("cp.createError") + e;
    } finally {
      creating = false;
    }
  }

  // The confirm flow (EXPERIENCE.md): the preview names every orphaned
  // proposal by its target's statement — the user confirms exactly what
  // will be superseded, never a summary.
  async function askRollback(checkpointId: string) {
    rollbackError = "";
    try {
      pendingPlan = await api.previewRollback(checkpointId);
    } catch (e) {
      rollbackError = t("cp.rollbackError") + e;
    }
  }

  async function confirmRollback() {
    if (!pendingPlan || rolling) return;
    rolling = true;
    rollbackError = "";
    try {
      lastOutcome = await api.rollbackToCheckpoint(pendingPlan.checkpoint.id);
      pendingPlan = null;
      await loadCheckpoints();
      onrollback(); // the parent re-folds missions + bumps the revision
    } catch (e) {
      rollbackError = t("cp.rollbackError") + e;
    } finally {
      rolling = false;
    }
  }

  const timeLabel = (ts: string) =>
    new Date(ts).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
</script>

<section class="glance" aria-label={t("board.title")}>
  <header class="glance-head">
    <div class="glance-title">
      <p class="glance-kicker">{t("board.title")}</p>
      <p class="glance-sub">{t("board.sub")}</p>
    </div>
    <div class="glance-controls">
      <!-- Status filter (FR-2.4): one select over the lifecycle states. -->
      <select
        class="glance-select"
        bind:value={statusFilter}
        aria-label={t("board.filter")}
      >
        <option value="all">{t("board.all")}</option>
        {#each statuses as s (s)}
          <option value={s}>{t(`hyp.status.${s}`)}</option>
        {/each}
      </select>
      <!-- Checkpoint control (FR-10.1, Story 2.6): the board header's entry
           point for restore points and rollbacks. -->
      <button
        class="glance-cp-btn"
        type="button"
        aria-expanded={checkpointsOpen}
        onclick={toggleCheckpoints}
      >
        {t("cp.title")}
      </button>
      <!-- Readiness gate (FR-13, Story 4.3): the board header's entry to
           the preprint-tier report — what blocks, item by object. -->
      <button
        class="glance-cp-btn"
        type="button"
        aria-expanded={readinessOpen}
        onclick={() => (readinessOpen = !readinessOpen)}
      >
        {t("rd.title")}
      </button>
    </div>
  </header>

  {#if checkpointsOpen}
    <!-- The checkpoint panel (FR-10.1): create form, restore points with
         the rollback confirm flow, and the rollback history. -->
    <div class="glance-cp-panel" role="region" aria-label={t("cp.title")}>
      {#if checkpointsError}
        <p class="glance-cp-error" role="alert">{checkpointsError}</p>
      {:else if checkpoints === null}
        <p class="glance-cp-empty">…</p>
      {:else if pendingPlan}
        <!-- The confirm step (EXPERIENCE.md): every orphaned proposal BY
             NAME, never a summary; nothing is executed yet. -->
        <div class="glance-cp-confirm" role="alertdialog" aria-label={t("cp.confirmTitle")}>
          <p class="glance-cp-confirm-title">{t("cp.confirmTitle")}</p>
          <p class="glance-cp-confirm-body">
            {t("cp.confirmBody", {
              name: pendingPlan.checkpoint.name,
              seq: pendingPlan.checkpoint.seq,
              count: pendingPlan.orphanedEvents.length,
            })}
          </p>
          {#if pendingPlan.orphanedProposals.length > 0}
            <p class="glance-cp-sub">{t("cp.orphanedProposals")}</p>
            <ul class="glance-cp-orphans">
              {#each pendingPlan.orphanedProposals as op (op.proposalId)}
                <li>
                  <span class="mono">{op.targetLabel ?? `H-${op.targetSeq ?? "?"}`}</span>
                  {#if op.proposedTo}
                    <span class="glance-cp-to"> {t("cp.orphanedTo", { to: t(`hyp.status.${op.proposedTo}`) })}</span>
                  {/if}
                </li>
              {/each}
            </ul>
          {:else}
            <p class="glance-cp-sub">{t("cp.orphanedNone")}</p>
          {/if}
          <div class="glance-cp-actions">
            <button
              class="glance-cp-btn"
              type="button"
              onclick={() => (pendingPlan = null)}
              disabled={rolling}
            >
              {t("cp.cancel")}
            </button>
            <button
              class="glance-cp-btn glance-cp-btn--confirm"
              type="button"
              onclick={confirmRollback}
              disabled={rolling}
            >
              {rolling ? "…" : t("cp.confirm")}
            </button>
          </div>
          {#if rollbackError}
            <p class="glance-cp-error" role="alert">{rollbackError}</p>
          {/if}
        </div>
      {:else}
        <!-- Create form: name the current head as a restore point. -->
        <form
          class="glance-cp-form"
          onsubmit={(e) => {
            e.preventDefault();
            void createCheckpoint();
          }}
        >
          <input
            class="glance-cp-input"
            type="text"
            bind:value={newName}
            placeholder={t("cp.namePlaceholder")}
            aria-label={t("cp.create")}
            maxlength={60}
          />
          <button class="glance-cp-btn glance-cp-btn--confirm" type="submit" disabled={creating || !newName.trim()}>
            {t("cp.create")}
          </button>
        </form>
        {#if createError}
          <p class="glance-cp-error" role="alert">{createError}</p>
        {/if}
        {#if checkpoints.checkpoints.length === 0}
          <p class="glance-cp-empty">{t("cp.empty")}</p>
        {:else}
          <p class="glance-cp-sub">{t("cp.restorePoints")}</p>
          <ul class="glance-cp-list">
            {#each checkpoints.checkpoints as cp (cp.id)}
              <li class="glance-cp-item">
                <span class="glance-cp-name">{cp.name}</span>
                <span class="glance-cp-meta mono">
                  {t("cp.atSeq", { seq: cp.seq })} · {timeLabel(cp.ts)}
                </span>
                <button
                  class="glance-cp-btn"
                  type="button"
                  onclick={() => void askRollback(cp.id)}
                  disabled={rolling}
                >
                  {t("cp.rollback")}
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if checkpoints.rollbacks.length > 0}
          <p class="glance-cp-sub">{t("cp.history")}</p>
          <ul class="glance-cp-list">
            {#each checkpoints.rollbacks as rb (rb.seq)}
              <li class="glance-cp-item glance-cp-item--history">
                <span class="glance-cp-meta mono">
                  {t("cp.historyEntry", { seq: rb.seq, name: rb.name, count: rb.orphanedCount })}
                </span>
              </li>
            {/each}
          </ul>
        {/if}
        {#if lastOutcome}
          <p class="glance-cp-rolled mono" role="status">
            {t("cp.rolledBack", {
              name: lastOutcome.rollback.name,
              count: lastOutcome.rollback.orphanedCount,
            })}
          </p>
        {/if}
        {#if rollbackError}
          <p class="glance-cp-error" role="alert">{rollbackError}</p>
        {/if}
      {/if}
    </div>
  {/if}

  {#if readinessOpen}
    <!-- The readiness panel (FR-13.1/13.2): derived from board state, no
         scores — every blocking item references its specific board object;
         a clean board shows the trail that justifies it. Re-folds on every
         board revision (a merge, a rollback — the same signal the grid
         re-folds on). -->
    <ReadinessReport {missions} revision={revision} />
  {/if}

  {#if loadError}
    <p class="glance-error" role="alert">{loadError}</p>
  {:else if entries === null}
    <p class="glance-empty">…</p>
  {:else if entries.length === 0}
    <p class="glance-empty">{t("board.empty")}</p>
  {:else if filtered.length === 0}
    <p class="glance-empty">{t("board.filterEmpty")}</p>
  {:else}
    <div class="glance-grid">
      {#each filtered as { mission, hypothesis, siblings } (hypothesis.id)}
        <div class="glance-cell">
          <!-- Mission label: keeps the flat grid navigable — the H-n card
               names the mission it belongs to. -->
          <p class="glance-mission" title={mission.question}>
            <span class="glance-mission-label">{t("board.mission")}</span>
            {mission.question}
          </p>
          <HypothesisCard {hypothesis} {siblings} refresh={load} />
        </div>
      {/each}
    </div>
  {/if}
</section>

<style>
  .glance {
    --rc-surface: #ffffff;
    --rc-surface-2: #f4f4f6;
    --rc-ink: #151519;
    --rc-ink-muted: #71717a;
    --rc-border: #eaeaec;
    --rc-accent: #3071b5;
    --rc-accent-soft: rgba(48, 113, 181, 0.1);
    --rc-danger-ink: #be123c;
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 14px;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .glance-head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }
  .glance-kicker {
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
    margin: 0 0 3px;
  }
  .glance-sub {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--rc-ink-muted);
  }
  .glance-controls {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .glance-select {
    font-family: inherit;
    font-size: 12.5px;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 8px;
    padding: 6px 8px;
    min-height: 34px;
    outline: none;
    cursor: pointer;
  }
  /* Checkpoint control: quiet secondary button — the board header's entry
     point for restore points (Story 2.6). */
  .glance-cp-btn {
    font-family: inherit;
    font-size: 12.5px;
    font-weight: 500;
    color: var(--rc-accent);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 8px;
    padding: 7px 12px;
    min-height: 34px;
    cursor: pointer;
    transition: background 0.15s ease;
    white-space: nowrap;
  }
  .glance-cp-btn:hover {
    background: var(--rc-accent-soft);
  }
  .glance-cp-btn[aria-expanded="true"] {
    background: var(--rc-accent-soft);
    border-color: var(--rc-accent);
  }
  .glance-cp-panel {
    border: 1px dashed var(--rc-border);
    border-radius: 10px;
    padding: 12px 14px;
    background: var(--rc-surface-2);
  }
  .glance-cp-empty {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--rc-ink-muted);
  }
  .mono {
    font-family: "JetBrains Mono", ui-monospace, monospace;
    font-variant-numeric: tabular-nums;
  }
  .glance-cp-sub {
    margin: 10px 0 6px;
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
  }
  .glance-cp-error {
    margin: 6px 0 0;
    font-size: 12.5px;
    color: var(--rc-danger-ink);
  }
  .glance-cp-form {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .glance-cp-input {
    flex: 1;
    min-width: 180px;
    font-family: inherit;
    font-size: 12.5px;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 8px;
    padding: 7px 10px;
    min-height: 34px;
    outline: none;
  }
  .glance-cp-input:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 1px;
  }
  .glance-cp-btn--confirm {
    color: #ffffff;
    background: var(--rc-accent);
    border-color: var(--rc-accent);
  }
  .glance-cp-btn--confirm:hover:not(:disabled) {
    background: #285f97;
  }
  .glance-cp-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .glance-cp-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 0;
    border-bottom: 1px dashed var(--rc-border);
    flex-wrap: wrap;
  }
  .glance-cp-item:last-child {
    border-bottom: none;
  }
  .glance-cp-item--history {
    padding: 4px 0;
  }
  .glance-cp-name {
    font-size: 13px;
    font-weight: 500;
    color: var(--rc-ink);
  }
  .glance-cp-meta {
    font-size: 11.5px;
    color: var(--rc-ink-muted);
  }
  .glance-cp-item .glance-cp-btn {
    margin-left: auto;
  }
  /* The confirm step (EXPERIENCE.md): names every orphaned proposal. */
  .glance-cp-confirm {
    border: 1px solid rgba(180, 83, 9, 0.35);
    background: #fffbeb;
    border-radius: 10px;
    padding: 12px 14px;
  }
  .glance-cp-confirm-title {
    margin: 0 0 6px;
    font-size: 13px;
    font-weight: 600;
    color: #92400e;
  }
  .glance-cp-confirm-body {
    margin: 0 0 4px;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--rc-ink);
  }
  .glance-cp-orphans {
    list-style: none;
    margin: 4px 0 8px;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .glance-cp-orphans li {
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--rc-ink);
  }
  .glance-cp-orphans .mono {
    font-size: 12px;
  }
  .glance-cp-to {
    color: var(--rc-ink-muted);
  }
  .glance-cp-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    flex-wrap: wrap;
  }
  .glance-cp-rolled {
    margin: 10px 0 0;
    font-size: 11.5px;
    color: #047857;
    background: #ecfdf5;
    border-radius: 8px;
    padding: 6px 10px;
  }
  .glance-error {
    margin: 0;
    font-size: 13px;
    color: var(--rc-danger-ink);
  }
  .glance-empty {
    margin: 0;
    font-size: 13px;
    line-height: 1.55;
    color: var(--rc-ink-muted);
    padding: 4px 0;
  }
  /* The at-a-glance grid (FR-2.4): responsive columns of hypothesis
     cards — every mission's, every state, one view. */
  .glance-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: 12px;
    align-items: start;
  }
  .glance-cell {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .glance-mission {
    margin: 0;
    font-size: 11.5px;
    line-height: 1.4;
    color: var(--rc-ink-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .glance-mission-label {
    font-weight: 500;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    font-size: 10.5px;
    margin-right: 4px;
  }
  button:focus-visible,
  select:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }
</style>
