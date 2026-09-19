<script lang="ts">
  import { api } from "../../api";
  import { t } from "../../i18n";
  import type { Hypothesis, Mission } from "../../types";
  import HypothesisCard from "./HypothesisCard.svelte";

  // The hypothesis board of one mission (Story 1.5; the full board-at-a-
  // glance surface is Story 1.8 — this section lists the focused mission's
  // cards). Read-model only (AD-8): `hypotheses` holds exactly what
  // `list_hypotheses` folded; every mutation re-folds from the log.
  let { mission }: { mission: Mission } = $props();

  let hypotheses = $state<Hypothesis[] | null>(null);
  let loadError = $state("");

  // Create-hypothesis control: a falsifiable statement, non-empty to add.
  let statement = $state("");
  let creating = $state(false);
  let createError = $state("");
  let submitted = $state(false);

  const statementMissing = $derived(statement.trim() === "");
  const canAdd = $derived(!statementMissing && !creating);
  // Newest first, matching the missions list; H-n labels derive from seq.
  const ordered = $derived([...(hypotheses ?? [])].sort((a, b) => b.seq - a.seq));

  $effect(() => {
    load();
  });

  async function load() {
    try {
      hypotheses = await api.listHypotheses(mission.id);
      loadError = "";
    } catch (e) {
      loadError = t("hyp.loadError") + e;
    }
  }

  async function add() {
    submitted = true;
    if (!canAdd) return;
    creating = true;
    createError = "";
    try {
      await api.createHypothesis(statement.trim(), mission.id);
      statement = "";
      submitted = false;
      await load();
    } catch (e) {
      createError = t("hyp.createError") + e;
    } finally {
      creating = false;
    }
  }
</script>

<section class="board" aria-label={t("hyp.board")}>
  <header class="board-head">
    <p class="board-kicker">{t("hyp.board")}</p>
    <!-- Create hypothesis from the mission: a statement input, validated
         inline (blank never reaches the command). -->
    <div class="board-add">
      <input
        class="board-input"
        class:invalid={submitted && statementMissing}
        type="text"
        bind:value={statement}
        placeholder={t("hyp.statementPh")}
        aria-label={t("hyp.statement")}
        aria-invalid={submitted && statementMissing}
        onkeydown={(e) => { if (e.key === "Enter") add(); }}
      />
      <button class="board-btn" type="button" onclick={add} disabled={!canAdd}>
        {t("hyp.add")}
      </button>
    </div>
    {#if createError}
      <p class="board-error" role="alert">{createError}</p>
    {/if}
  </header>

  {#if loadError}
    <p class="board-error" role="alert">{loadError}</p>
  {:else if hypotheses === null}
    <p class="board-empty">…</p>
  {:else if hypotheses.length === 0}
    <!-- Empty board (EXPERIENCE.md state patterns): appears only once the
         mission exists — the mission will propose its first. -->
    <p class="board-empty">{t("hyp.empty")}</p>
  {:else}
    <div class="board-cards">
      {#each ordered as hypothesis (hypothesis.id)}
        {@const siblings = ordered.filter((h) => h.id !== hypothesis.id)}
        <HypothesisCard {hypothesis} {siblings} refresh={load} />
      {/each}
    </div>
  {/if}
</section>

<style>
  .board {
    --rc-surface: #ffffff;
    --rc-surface-2: #f4f4f6;
    --rc-ink: #151519;
    --rc-ink-muted: #71717a;
    --rc-border: #eaeaec;
    --rc-accent: #3071b5;
    --rc-accent-soft: rgba(48, 113, 181, 0.1);
    --rc-danger-ink: #be123c;
    background: var(--rc-surface-2);
    border: 1px solid var(--rc-border);
    border-radius: 12px;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .board-kicker {
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
    margin: 0 0 8px;
  }
  .board-head {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .board-add {
    display: flex;
    gap: 8px;
  }
  .board-input {
    flex: 1;
    min-width: 0;
    font-family: inherit;
    font-size: 13.5px;
    line-height: 1.5;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 8px;
    padding: 8px 12px;
    outline: none;
    min-height: 36px;
  }
  .board-input::placeholder {
    color: var(--rc-ink-muted);
  }
  .board-input.invalid {
    border-color: var(--rc-danger-ink);
    background: #fff1f2;
  }
  .board-btn {
    font-family: inherit;
    font-size: 13px;
    font-weight: 500;
    color: #ffffff;
    background: var(--rc-accent);
    border: 1px solid var(--rc-accent);
    border-radius: 8px;
    padding: 8px 14px;
    min-height: 36px;
    cursor: pointer;
    transition: transform 0.15s ease, box-shadow 0.15s ease;
  }
  .board-btn:hover:not(:disabled) {
    transform: translateY(-2px);
    box-shadow: 0 4px 12px -2px rgba(48, 113, 181, 0.35);
  }
  .board-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .board-error {
    margin: 0;
    font-size: 12.5px;
    color: var(--rc-danger-ink);
  }
  .board-empty {
    margin: 0;
    font-size: 13px;
    line-height: 1.55;
    color: var(--rc-ink-muted);
    padding: 4px 0;
  }
  .board-cards {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  button:focus-visible,
  input:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }
</style>
