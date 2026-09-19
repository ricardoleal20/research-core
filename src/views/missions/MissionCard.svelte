<script lang="ts">
  import type { Mission, MissionRun } from "../../types";
  import { t } from "../../i18n";
  import { api } from "../../api";
  import HypothesesBoard from "./HypothesesBoard.svelte";

  let {
    mission,
    onopenreceipt = () => {},
  }: { mission: Mission; onopenreceipt?: (runId: string) => void } = $props();

  // Runs drill-down (FR-1.3): loaded lazily on first expand; the full
  // receipts timeline is a later story — this is the basic run list.
  let runsOpen = $state(false);
  let runs = $state<MissionRun[] | null>(null);
  let runsError = $state("");

  // Hypothesis board drill-down (Story 1.5): the mission's hypothesis
  // cards — lifecycle chips, relation chips, audit stamps. The full
  // board-at-a-glance surface is Story 1.8; this section suffices here.
  let boardOpen = $state(false);

  // Night Shift schedule (Story 2.3, FR-4.1): off | daily-HH:MM — the
  // compact editor in the card footer; a save appends mission.scheduled.
  let scheduleOpen = $state(false);
  let scheduleDraft = $state(mission.schedule);
  let scheduleError = $state("");
  let scheduleSaving = $state(false);

  const scheduleValid = $derived(
    scheduleDraft.trim().toLowerCase() === "off" ||
      (/^daily-\d{2}:\d{2}$/.test(scheduleDraft.trim()) &&
        Number(scheduleDraft.trim().slice(6, 8)) <= 23 &&
        Number(scheduleDraft.trim().slice(9, 11)) <= 59),
  );

  async function saveSchedule() {
    if (!scheduleValid) return;
    scheduleSaving = true;
    scheduleError = "";
    try {
      await api.setMissionSchedule(mission.id, scheduleDraft.trim());
      scheduleOpen = false;
    } catch (e) {
      scheduleError = t("missions.scheduleError") + e;
    } finally {
      scheduleSaving = false;
    }
  }

  const ceiling = $derived(`$${(mission.spendCeilingCents / 100).toFixed(2)}`);
  const autonomyLabel = $derived(t(`missions.autonomy.${mission.autonomy}`));
  const created = $derived(new Date(mission.ts).toLocaleString());
  const statusLabel = $derived(t(`missions.status.${mission.status}`));

  // DESIGN.md mission lifecycle tokens; awaiting-review reuses the amber
  // review hue (no dedicated token exists yet).
  const statusColor: Record<Mission["status"], string> = {
    active: "#3071B5",
    awaiting_review: "#B45309",
    completed: "#047857",
    stopped: "#334155",
    failed: "#BE123C",
  };

  // DESIGN.md components.spend-meter: thin track, fill colored by state,
  // amounts in mono tabular. Never color alone — amounts always render too.
  const spendLabel = $derived(`$${(mission.spendCents / 100).toFixed(2)}`);
  const spendPct = $derived(
    mission.spendCeilingCents > 0
      ? Math.min(100, Math.round((mission.spendCents / mission.spendCeilingCents) * 100))
      : mission.spendCents > 0
        ? 100
        : 0
  );
  const spendFill: Record<Mission["spendState"], string> = {
    ok: "#047857", // spend-ok
    near: "#B45309", // spend-near
    blocked: "#BE123C", // spend-blocked
  };

  async function toggleRuns() {
    runsOpen = !runsOpen;
    if (runsOpen && runs === null) {
      try {
        runs = await api.getMissionRuns(mission.id);
        runsError = "";
      } catch (e) {
        runsError = t("missions.loadRunsError") + e;
      }
    }
  }

  function toggleBoard() {
    boardOpen = !boardOpen;
  }
</script>

<!-- Mission card (DESIGN.md components.mission-card): status kicker, question
     in heading-3, stop condition + success criterion, thin spend meter, mono
     meta row, and the runs drill-down. Status is never color alone — the
     kicker carries text. -->
<article class="mission-card" id={`mission-${mission.id}`}>
  <header class="mc-head">
    <span class="mc-kicker" style={`--status:${statusColor[mission.status]}`}>
      <span class="mc-dot" aria-hidden="true"></span>
      {statusLabel}
    </span>
    <span class="mc-seq mono">seq {mission.seq}</span>
  </header>
  <h3 class="mc-question">{mission.question}</h3>
  <dl class="mc-terms">
    <div class="mc-term">
      <dt>{t("missions.stopCondition")}</dt>
      <dd>{mission.stopCondition}</dd>
    </div>
    <div class="mc-term">
      <dt>{t("missions.successCriterion")}</dt>
      <dd>{mission.successCriterion}</dd>
    </div>
  </dl>

  <!-- Spend meter: honest by construction — the blocked label says exactly
       what happened (ceiling reached, dispatch refused), nothing loud. -->
  <div class="mc-spend">
    <div
      class="spend-meter"
      role="meter"
      aria-valuemin={0}
      aria-valuemax={mission.spendCeilingCents}
      aria-valuenow={mission.spendCents}
      aria-label={t("missions.spend")}
    >
      <div
        class="spend-fill"
        class:blocked={mission.spendState === "blocked"}
        style={`width:${spendPct}%;background:${spendFill[mission.spendState]}`}
      ></div>
    </div>
    <span class="spend-amounts mono">
      {spendLabel} {t("missions.spendOf")} {ceiling}
    </span>
    {#if mission.spendState === "blocked"}
      <span class="spend-blocked-label">{t("missions.spendBlocked")}</span>
    {/if}
  </div>

  <!-- Night Shift schedule (FR-4.1): mono chip + inline editor; the hint
       names the wire form. Saving appends a mission.scheduled event. -->
  <div class="mc-schedule">
    {#if scheduleOpen}
      <input
        class="mc-schedule-input mono"
        type="text"
        bind:value={scheduleDraft}
        aria-label={t("missions.schedule")}
        aria-invalid={!scheduleValid}
      />
      <button
        class="mc-runs-toggle"
        type="button"
        onclick={saveSchedule}
        disabled={!scheduleValid || scheduleSaving}
      >
        {t("missions.scheduleSave")}
      </button>
      {#if scheduleError}
        <p class="mc-schedule-error" role="alert">{scheduleError}</p>
      {/if}
    {:else}
      <button
        class="mc-schedule-chip mono"
        type="button"
        onclick={() => {
          scheduleDraft = mission.schedule;
          scheduleOpen = true;
        }}
        title={t("missions.schedule")}
      >
        {t("digest.nightShift")}: {mission.schedule}
      </button>
    {/if}
  </div>

  <footer class="mc-meta mono">
    <span>{autonomyLabel}</span>
    <span aria-hidden="true">·</span>
    <span>{created}</span>
    <span class="mc-spacer"></span>
    <button class="mc-runs-toggle" type="button" onclick={toggleBoard} aria-expanded={boardOpen}>
      {t("hyp.show")}
    </button>
    <button class="mc-runs-toggle" type="button" onclick={toggleRuns} aria-expanded={runsOpen}>
      {runsOpen ? t("missions.hideRuns") : t("missions.showRuns")}
    </button>
  </footer>

  {#if boardOpen}
    <HypothesesBoard {mission} />
  {/if}

  {#if runsOpen}
    <div class="mc-runs">
      {#if runsError}
        <p class="mc-runs-error" role="alert">{runsError}</p>
      {:else if runs === null}
        <p class="mc-runs-empty">…</p>
      {:else if runs.length === 0}
        <p class="mc-runs-empty">{t("missions.runsEmpty")}</p>
      {:else}
        <ul class="mc-run-list">
          {#each runs as run (run.id)}
            <li class="mc-run">
              <span class="mc-run-seq mono">seq {run.seq}</span>
              <span class="mc-run-kind mono">{run.kind}</span>
              <span class="mc-run-actor">{run.actor}</span>
              <span class="mc-run-ts">{new Date(run.ts).toLocaleString()}</span>
              <!-- The receipts drill-down (Story 2.5, FR-6.2): run lifecycle
                   rows carry their run id — the row opens the run's receipt
                   drawer. The timeline is ONLY this drill-down, never a
                   parallel surface. -->
              {#if run.kind === "run.started" && run.runId}
                <button
                  class="mc-run-receipt mono"
                  type="button"
                  onclick={() => run.runId && onopenreceipt(run.runId)}
                >
                  {t("receipt.open")} →
                </button>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</article>

<style>
  .mission-card {
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
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    transition: transform 0.18s ease, box-shadow 0.18s ease, border-color 0.18s ease;
  }
  .mission-card:hover {
    transform: translateY(-2px);
    box-shadow: 0 8px 24px -6px rgba(0, 0, 0, 0.08);
    border-color: var(--rc-accent);
  }
  .mono {
    font-family: "JetBrains Mono", ui-monospace, monospace;
  }
  .mc-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .mc-kicker {
    /* status color flows from the mission lifecycle token via --status */
    display: inline-flex;
    align-items: center;
    gap: 7px;
    font-size: 12px;
    font-weight: 500;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--status);
    background: color-mix(in srgb, var(--status) 10%, transparent);
    border-radius: 9999px;
    padding: 2px 10px;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--status) 25%, transparent);
  }
  .mc-dot {
    width: 7px;
    height: 7px;
    border-radius: 9999px;
    background: var(--status);
  }
  .mc-seq {
    font-size: 12px;
    color: var(--rc-ink-muted);
  }
  .mc-question {
    font-family: Inter, system-ui, sans-serif;
    font-size: 16px;
    font-weight: 600;
    line-height: 1.35;
    color: var(--rc-ink);
    margin: 0;
  }
  .mc-terms {
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .mc-term dt {
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
    margin-bottom: 3px;
  }
  .mc-term dd {
    margin: 0;
    font-size: 14px;
    line-height: 1.55;
    color: var(--rc-ink);
  }

  /* Spend meter (DESIGN.md components.spend-meter): thin track surface-2,
     state-colored fill, mono tabular amounts. */
  .mc-spend {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .spend-meter {
    flex: 1 1 120px;
    min-width: 80px;
    height: 4px;
    border-radius: 9999px;
    background: var(--rc-surface-2);
    overflow: hidden;
  }
  .spend-fill {
    height: 100%;
    border-radius: 9999px;
    transition: width 0.3s ease;
  }
  .spend-amounts {
    font-size: 12.5px;
    color: var(--rc-ink-muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .spend-blocked-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--rc-danger-ink);
  }

  .mc-schedule {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .mc-schedule-chip {
    font-family: "JetBrains Mono", ui-monospace, monospace;
    font-size: 12px;
    color: var(--rc-ink-muted);
    background: var(--rc-surface-2);
    border: 1px solid var(--rc-border);
    border-radius: 9999px;
    padding: 3px 12px;
    cursor: pointer;
    transition: color 0.15s ease, border-color 0.15s ease;
  }
  .mc-schedule-chip:hover {
    color: var(--rc-accent);
    border-color: var(--rc-accent);
  }
  .mc-schedule-chip:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }
  .mc-schedule-input {
    font-family: "JetBrains Mono", ui-monospace, monospace;
    font-size: 12.5px;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 8px;
    padding: 6px 10px;
    outline: none;
    min-height: 34px;
    width: 150px;
  }
  .mc-schedule-input[aria-invalid="true"] {
    border-color: var(--rc-danger-ink);
  }
  .mc-schedule-error {
    margin: 0;
    font-size: 12px;
    color: var(--rc-danger-ink);
  }

  .mc-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    color: var(--rc-ink-muted);
    font-variant-numeric: tabular-nums;
    border-top: 1px solid var(--rc-border);
    padding-top: 12px;
  }
  .mc-spacer {
    flex: 1;
  }
  .mc-runs-toggle {
    font-family: inherit;
    font-size: 12.5px;
    font-weight: 500;
    color: var(--rc-accent);
    background: transparent;
    border: 1px solid var(--rc-border);
    border-radius: 8px;
    padding: 4px 10px;
    cursor: pointer;
    transition: background 0.15s ease;
  }
  .mc-runs-toggle:hover {
    background: var(--rc-accent-soft);
  }
  .mc-runs-toggle:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }

  /* Runs drill-down: mono receipt voice, seq order, quiet. */
  .mc-runs {
    border-top: 1px solid var(--rc-border);
    padding-top: 12px;
  }
  .mc-run-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .mc-run {
    display: grid;
    grid-template-columns: auto 1fr auto auto;
    gap: 12px;
    align-items: baseline;
    font-size: 12.5px;
  }
  .mc-run-seq,
  .mc-run-kind {
    color: var(--rc-ink);
    font-variant-numeric: tabular-nums;
  }
  .mc-run-actor {
    color: var(--rc-ink-muted);
  }
  .mc-run-ts {
    color: var(--rc-ink-muted);
    font-variant-numeric: tabular-nums;
  }
  .mc-run-receipt {
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
  .mc-run-receipt:hover {
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  .mc-run-receipt:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }
  .mc-runs-empty,
  .mc-runs-error {
    margin: 0;
    font-size: 12.5px;
    color: var(--rc-ink-muted);
  }
  .mc-runs-error {
    color: var(--rc-danger-ink);
  }

  @media (max-width: 640px) {
    .mc-run {
      grid-template-columns: auto 1fr;
    }
    .mc-run-ts {
      grid-column: 2;
    }
  }
</style>
