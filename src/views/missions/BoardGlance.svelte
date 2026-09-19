<script lang="ts">
  import { api } from "../../api";
  import { t } from "../../i18n";
  import type { Hypothesis, HypothesisStatus, Mission } from "../../types";
  import HypothesisCard from "./HypothesisCard.svelte";

  // The board at a glance (FR-2.4, Story 1.8): ALL hypotheses and their
  // states in one view — a board-level grid, not per-mission sections
  // (those stay on each mission card). Read-model only (AD-8): the grid
  // holds exactly what list_hypotheses folded per mission; every mutation
  // inside a card re-folds via `refresh`.
  let { missions }: { missions: Mission[] } = $props();

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

  // Checkpoints control (entry point for Story 2.6): presence and UI only
  // — the empty restore-points list state. Creation/rollback events are
  // Story 2.6; nothing is wired here.
  let checkpointsOpen = $state(false);

  const filtered = $derived(
    entries === null
      ? []
      : entries.filter(
          (e) => statusFilter === "all" || e.hypothesis.status === statusFilter,
        ),
  );

  $effect(() => {
    // Re-fold whenever the mission set changes (a new mission appears).
    void missions;
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
      <!-- Checkpoint control (entry point for Story 2.6): presence and UI
           only — the button opens the restore-points list, which reads
           its empty state until checkpoints exist. -->
      <button
        class="glance-cp-btn"
        type="button"
        aria-expanded={checkpointsOpen}
        onclick={() => (checkpointsOpen = !checkpointsOpen)}
      >
        {t("cp.title")}
      </button>
    </div>
  </header>

  {#if checkpointsOpen}
    <!-- Empty restore-points list (Story 2.6 fills this): the state the
         board header offers today. -->
    <div class="glance-cp-panel" role="region" aria-label={t("cp.title")}>
      <p class="glance-cp-empty">{t("cp.empty")}</p>
    </div>
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
