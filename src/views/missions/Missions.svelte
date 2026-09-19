<script lang="ts">
  import { api } from "../../api";
  import { t } from "../../i18n";
  import type { Autonomy, Mission } from "../../types";
  import MissionCard from "./MissionCard.svelte";

  // Read-model only (AD-8): `missions` holds exactly what commands returned;
  // every change flows through create_mission / list_missions.
  let missions = $state<Mission[]>([]);
  let loaded = $state(false);
  let loadError = $state("");

  // Question box
  let question = $state("");
  let composing = $state(false);

  // Composer fields (FR-1.2): stop condition + falsifiable success criterion
  // are required to launch; autonomy is one of three named stops; spend
  // ceiling is a hard pre-dispatch limit (AD-10), entered in dollars and
  // stored as cents.
  let stopCondition = $state("");
  let successCriterion = $state("");
  let autonomy = $state<Autonomy>("watch");
  let spendCeiling = $state(5);
  let submitted = $state(false);
  let launching = $state(false);
  let launchError = $state("");

  const stops: Autonomy[] = ["watch", "suggest", "act_with_receipts"];
  const newestFirst = $derived([...missions].sort((a, b) => b.seq - a.seq));
  const questionMissing = $derived(question.trim() === "");
  const stopMissing = $derived(stopCondition.trim() === "");
  const successMissing = $derived(successCriterion.trim() === "");
  const canLaunch = $derived(!questionMissing && !stopMissing && !successMissing && !launching);

  $effect(() => {
    load();
  });

  async function load() {
    try {
      missions = await api.listMissions();
      loaded = true;
      loadError = "";
    } catch (e) {
      loadError = t("missions.loadError") + e;
    }
  }

  function openComposer() {
    if (questionMissing) return;
    composing = true;
  }

  function cancelComposer() {
    composing = false;
    submitted = false;
    launchError = "";
  }

  async function launch() {
    submitted = true;
    if (!canLaunch) return;
    launching = true;
    launchError = "";
    try {
      const mission = await api.createMission({
        question: question.trim(),
        stopCondition: stopCondition.trim(),
        successCriterion: successCriterion.trim(),
        autonomy,
        spendCeilingCents: Math.round(spendCeiling * 100),
      });
      missions.push(mission);
      composing = false;
      submitted = false;
      question = "";
      stopCondition = "";
      successCriterion = "";
    } catch (e) {
      launchError = t("missions.createError") + e;
    } finally {
      launching = false;
    }
  }
</script>

<div class="missions">
  <!-- Question box (DESIGN.md components.question-box): the day-one hero —
       card anatomy with aurora backing at reduced opacity. -->
  <section class="hero" aria-label={t("missions.title")}>
    <div class="aurora" aria-hidden="true"></div>
    <div class="hero-card">
      <p class="kicker">{t("missions.title")}</p>
      <h2 class="greeting">{t("missions.greeting")}</h2>
      <div class="ask">
        <input
          class="ask-input"
          type="text"
          bind:value={question}
          placeholder={t("missions.questionPh")}
          aria-label={t("missions.greeting")}
          onkeydown={(e) => { if (e.key === "Enter") openComposer(); }}
        />
        <button class="btn-primary" onclick={openComposer} disabled={questionMissing}>
          {t("missions.turnIntoMission")}
        </button>
      </div>
    </div>
  </section>

  {#if loadError}
    <p class="error" role="alert">{loadError}</p>
  {/if}

  <!-- Progressive disclosure (FR-1.4, EXPERIENCE.md ladder layer 0): on a
       fresh workspace the view is the question box and its empty state —
       nothing else. Mission cards, spend meters, and runs appear only when
       the log actually holds missions; no layer reveals itself before the
       user's research opens it. -->
  {#if loaded && missions.length === 0}
    <section class="empty-state" aria-label={t("missions.empty")}>
      <p class="empty-title">{t("missions.empty")}</p>
      <p class="empty-hint">{t("missions.emptyHint")}</p>
    </section>
  {/if}

  {#if composing}
    <!-- Mission composer: the UI never lets a mission exist that cannot end —
         launch is disabled without both terminator fields (FR-1.2, AD-12). -->
    <section class="composer" aria-label={t("missions.composer.title")}>
      <header class="composer-head">
        <p class="kicker">{t("missions.composer.title")}</p>
        <h3 class="composer-question">{question.trim()}</h3>
      </header>

      <div class="field" class:invalid={submitted && stopMissing}>
        <label for="stop-condition">{t("missions.stopCondition")}</label>
        <input
          id="stop-condition"
          type="text"
          bind:value={stopCondition}
          placeholder={t("missions.stopConditionPh")}
          aria-invalid={submitted && stopMissing}
        />
        {#if submitted && stopMissing}
          <p class="field-error" role="alert">{t("missions.required")}</p>
        {/if}
      </div>

      <div class="field" class:invalid={submitted && successMissing}>
        <label for="success-criterion">{t("missions.successCriterion")}</label>
        <input
          id="success-criterion"
          type="text"
          bind:value={successCriterion}
          placeholder={t("missions.successCriterionPh")}
          aria-invalid={submitted && successMissing}
        />
        {#if submitted && successMissing}
          <p class="field-error" role="alert">{t("missions.required")}</p>
        {/if}
      </div>

      <div class="field">
        <span class="field-label" id="autonomy-label">{t("missions.autonomy")}</span>
        <!-- Autonomy dial: a segmented control, never a free-slider (DESIGN.md
             components.autonomy-dial) — three named contracts. -->
        <div class="segmented" role="radiogroup" aria-labelledby="autonomy-label">
          {#each stops as stop (stop)}
            <button
              type="button"
              role="radio"
              aria-checked={autonomy === stop}
              class="segment"
              class:active={autonomy === stop}
              onclick={() => (autonomy = stop)}
            >
              <span class="segment-label">{t(`missions.autonomy.${stop}`)}</span>
              <span class="segment-desc">{t(`missions.autonomyDesc.${stop}`)}</span>
            </button>
          {/each}
        </div>
        <p class="field-hint">{t("missions.autonomyNote")}</p>
      </div>

      <div class="field">
        <label for="spend-ceiling">{t("missions.spendCeiling")}</label>
        <div class="ceiling">
          <span class="ceiling-unit mono" aria-hidden="true">$</span>
          <input
            id="spend-ceiling"
            class="mono"
            type="number"
            min="0"
            step="0.5"
            bind:value={spendCeiling}
          />
          <span class="ceiling-cents mono">= {Math.round(spendCeiling * 100)}¢</span>
        </div>
        <p class="field-hint">{t("missions.spendCeilingHint")}</p>
      </div>

      <footer class="composer-foot">
        <button class="btn-secondary" type="button" onclick={cancelComposer}>
          {t("missions.cancel")}
        </button>
        <button class="btn-primary" type="button" onclick={launch} disabled={!canLaunch}>
          {launching ? t("missions.landing") : t("missions.launch")}
        </button>
      </footer>
      {#if launchError}
        <p class="error" role="alert">{launchError}</p>
      {/if}
    </section>
  {/if}

  {#if newestFirst.length > 0}
    <section class="missions-list" aria-label={t("missions.title")}>
      {#each newestFirst as mission (mission.id)}
        <MissionCard {mission} />
      {/each}
    </section>
  {/if}
</div>

<style>
  .missions {
    /* DESIGN.md tokens (light surface): values from the token tables. */
    --rc-surface: #ffffff;
    --rc-surface-2: #f4f4f6;
    --rc-ink: #151519;
    --rc-ink-muted: #71717a;
    --rc-border: #eaeaec;
    --rc-accent: #3071b5;
    --rc-accent-soft: rgba(48, 113, 181, 0.1);
    --rc-aurora: #32838f;
    --rc-danger-ink: #be123c;
    max-width: 720px;
    margin: 0 auto;
    width: 100%;
    padding: 32px 24px 48px;
    display: flex;
    flex-direction: column;
    gap: 24px;
    font-family: Inter, system-ui, sans-serif;
    color: var(--rc-ink);
  }
  .mono {
    font-family: "JetBrains Mono", ui-monospace, monospace;
    font-variant-numeric: tabular-nums;
  }
  .kicker {
    font-size: 14px;
    font-weight: 500;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
    margin: 0 0 8px;
  }
  .greeting {
    font-family: "Instrument Serif", Georgia, serif;
    font-style: italic;
    font-weight: 400;
    font-size: clamp(28px, 3vw, 40px);
    line-height: 1.1;
    letter-spacing: -0.01em;
    margin: 0 0 20px;
  }

  /* Question box hero: card anatomy + aurora backing at reduced opacity. */
  .hero {
    position: relative;
    isolation: isolate;
  }
  .aurora {
    position: absolute;
    inset: -12px -12px auto -12px;
    height: 78%;
    z-index: -1;
    filter: blur(24px);
    opacity: 0.55;
    background:
      radial-gradient(42% 62% at 24% 38%, rgba(48, 113, 181, 0.18), transparent 70%),
      radial-gradient(40% 60% at 76% 30%, rgba(50, 131, 143, 0.09), transparent 70%);
    pointer-events: none;
  }
  .hero-card {
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 14px;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
    padding: 28px 24px 24px;
  }
  .ask {
    display: flex;
    gap: 10px;
    align-items: stretch;
  }
  .ask-input {
    flex: 1;
    min-width: 0;
    font-family: inherit;
    font-size: 16px;
    line-height: 1.65;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 8px;
    padding: 10px 14px;
    outline: none;
  }
  .ask-input::placeholder {
    color: var(--rc-ink-muted);
  }
  .ask-input:focus-visible,
  .composer input:focus-visible,
  .segment:focus-visible,
  button:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }

  .btn-primary,
  .btn-secondary {
    font-family: inherit;
    font-size: 14px;
    font-weight: 500;
    border-radius: 8px;
    padding: 10px 16px;
    min-height: 40px;
    cursor: pointer;
    transition: transform 0.15s ease, box-shadow 0.15s ease, background 0.15s ease;
  }
  .btn-primary {
    background: var(--rc-accent);
    color: #ffffff;
    border: 1px solid var(--rc-accent);
  }
  .btn-primary:hover:not(:disabled) {
    transform: translateY(-2px);
    box-shadow: 0 4px 12px -2px rgba(48, 113, 181, 0.35);
  }
  .btn-primary:active:not(:disabled) {
    transform: scale(0.98);
  }
  .btn-primary:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .btn-secondary {
    background: var(--rc-surface);
    color: var(--rc-ink);
    border: 1px solid var(--rc-border);
  }
  .btn-secondary:hover {
    background: var(--rc-surface-2);
    transform: translateY(-2px);
  }

  /* Mission composer */
  .composer {
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 14px;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  .composer-head {
    border-bottom: 1px solid var(--rc-border);
    padding-bottom: 14px;
  }
  .composer-question {
    font-family: "Instrument Serif", Georgia, serif;
    font-style: italic;
    font-weight: 400;
    font-size: 22px;
    line-height: 1.2;
    margin: 0;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .field label,
  .field-label {
    font-size: 13px;
    font-weight: 500;
    color: var(--rc-ink);
  }
  .field input {
    font-family: inherit;
    font-size: 14.5px;
    line-height: 1.55;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 8px;
    padding: 10px 12px;
    outline: none;
    min-height: 40px;
  }
  .field input::placeholder {
    color: var(--rc-ink-muted);
  }
  .field.invalid input {
    border-color: var(--rc-danger-ink);
    background: #fff1f2;
  }
  .field-error {
    margin: 0;
    font-size: 12.5px;
    font-weight: 500;
    color: var(--rc-danger-ink);
  }
  .field-hint {
    margin: 0;
    font-size: 12px;
    line-height: 1.45;
    color: var(--rc-ink-muted);
  }

  /* Autonomy dial: soft track, white active segment, shadow-sm. */
  .segmented {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 4px;
    background: var(--rc-surface-2);
    border-radius: 12px;
    padding: 4px;
  }
  .segment {
    font-family: inherit;
    display: flex;
    flex-direction: column;
    gap: 3px;
    align-items: flex-start;
    text-align: left;
    background: transparent;
    border: 0;
    border-radius: 8px;
    padding: 10px 12px;
    min-height: 40px;
    cursor: pointer;
    color: var(--rc-ink-muted);
  }
  .segment.active {
    background: var(--rc-surface);
    color: var(--rc-ink);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.06);
  }
  .segment-label {
    font-size: 13.5px;
    font-weight: 600;
  }
  .segment.active .segment-label {
    color: var(--rc-accent);
  }
  .segment-desc {
    font-size: 11.5px;
    line-height: 1.4;
  }

  .ceiling {
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 280px;
  }
  .ceiling input {
    flex: 1;
    min-width: 0;
    font-size: 14.5px;
  }
  .ceiling-unit,
  .ceiling-cents {
    font-size: 13px;
    color: var(--rc-ink-muted);
  }

  .composer-foot {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    border-top: 1px solid var(--rc-border);
    padding-top: 16px;
  }

  .missions-list {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  /* Day-one empty state (ladder layer 0): quiet, centered, mission
     vocabulary only — no boards, targets, or publication layers. */
  .empty-state {
    text-align: center;
    padding: 40px 16px 24px;
  }
  .empty-title {
    font-family: "Instrument Serif", Georgia, serif;
    font-style: italic;
    font-weight: 400;
    font-size: clamp(22px, 2.6vw, 30px);
    line-height: 1.2;
    margin: 0 0 10px;
    color: var(--rc-ink);
  }
  .empty-hint {
    margin: 0;
    font-size: 14px;
    line-height: 1.6;
    color: var(--rc-ink-muted);
  }

  .error {
    margin: 0;
    font-size: 13px;
    color: var(--rc-danger-ink);
  }

  @media (max-width: 640px) {
    .ask {
      flex-direction: column;
    }
    .segmented {
      grid-template-columns: 1fr;
    }
  }
</style>
