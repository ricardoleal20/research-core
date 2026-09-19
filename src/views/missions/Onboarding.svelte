<script lang="ts">
  import { api } from "../../api";
  import { t, getLang } from "../../i18n";
  import type { FirstValueResult, Ref } from "../../types";

  // The sixty-second first value (FR-8.1, EXPERIENCE.md 60-second flow): the
  // missions tab's first-run state when the log holds no missions. Two quiet
  // actions — paste an arXiv URL or import Zotero — zero configuration, zero
  // jargon beyond the current layer (FR-8.2). The display-serif headline and
  // the two actions render bilingually (EN + ES) exactly as the DESIGN.md
  // onboarding frame does; operational strings follow the interface language.
  let { ondone }: { ondone: () => void } = $props();

  type Mode = "welcome" | "paste" | "zotero" | "generating" | "result";
  let mode = $state<Mode>("welcome");
  let url = $state("");
  let error = $state("");
  let result = $state<FirstValueResult | null>(null);
  let elapsed = $state(0);
  let refs = $state<Ref[] | null>(null);
  let running = $state(false);

  const urlMissing = $derived(url.trim() === "");

  /** The confidence dot's color (DESIGN.md citation-pin anatomy — same
   *  thresholds as the evidence pins): green 0.75+, amber 0.5–0.75, red
   *  below. Never a "verified" label. */
  function confidenceColor(c: number): string {
    if (c >= 0.75) return "var(--st-read)";
    if (c >= 0.5) return "var(--st-reading)";
    return "var(--destructive)";
  }
  const pct = (c: number) => `${Math.round(c * 100)}%`;

  // The generation receipt (mono, honesty — EXPERIENCE.md): the provider and
  // model that answered, with the cost of a real call or an explicit
  // "simulated provider" note when no key is configured.
  const receiptLine = $derived.by(() => {
    if (!result) return "";
    const r = result.receipt;
    if (r.simulated) {
      return `receipt · ${t("onb.simulatedNote")} · ${elapsed.toFixed(0)} s`;
    }
    return `receipt · ${r.provider} / ${r.model} · $${(r.costCents / 100).toFixed(2)} · ${elapsed.toFixed(0)} s`;
  });

  // The streaming-style receipt log: fetch → extract claims → score
  // confidence, each line labeled with what actually happened.
  const logLines = $derived.by(() => {
    if (!result) return [];
    return [
      `fetch arXiv:${result.paper.arxivId || result.paper.url}`,
      t("onb.logClaims", { n: result.candidates.length }),
      t("onb.logScore", { model: result.receipt.model }),
    ];
  });

  // The Spanish subline under the display-serif headline (DESIGN.md onboarding
  // frame renders both languages) shows only when the interface is not Spanish.
  const showEsSubline = $derived(getLang() !== "es");

  function openPaste() {
    mode = "paste";
    error = "";
  }

  async function openZotero() {
    mode = "zotero";
    error = "";
    refs = null;
    try {
      const project = await api.getActiveProject();
      refs = project ? await api.listRefs(project.id) : [];
    } catch {
      refs = [];
    }
  }

  function back() {
    mode = "welcome";
    error = "";
  }

  async function generate(kind: "url" | "ref", id: string) {
    running = true;
    error = "";
    mode = "generating";
    const started = performance.now();
    try {
      result =
        kind === "url"
          ? await api.runFirstValue(id)
          : await api.runFirstValueFromRef(id);
      elapsed = (performance.now() - started) / 1000;
      mode = "result";
    } catch (e) {
      // Coded errors (`invalid_url:` …) render verbatim — codes are never
      // translated (bilingual-safe by construction).
      error = t("onb.error") + e;
      mode = kind === "url" ? "paste" : "zotero";
    } finally {
      running = false;
    }
  }

  function submitUrl() {
    if (urlMissing || running) return;
    generate("url", url.trim());
  }
</script>

<div class="onboarding">
  {#if mode === "welcome"}
    <!-- The quiet welcome (DESIGN.md onboarding frame A): display-serif
         headline, two big quiet actions, zero jargon, bilingual EN/ES. -->
    <section class="welcome" aria-label="Onboarding">
      <div class="aurora" aria-hidden="true"></div>
      <p class="kicker mono">First run · <span lang="es">Primera ejecución</span></p>
      <h2 class="display">The exact editor for your research</h2>
      <p class="display-es" lang="es">El editor exacto para tu investigación</p>
      <p class="sub">
        Paste an arXiv URL or import Zotero — that's it.
        <span class="es" lang="es">· Pega un enlace de arXiv o importa Zotero — eso es todo.</span>
      </p>
      <div class="actions">
        <button class="btn-primary btn-lg" type="button" onclick={openPaste}>
          Paste arXiv URL <span class="es" lang="es">· Pegar enlace de arXiv</span>
        </button>
        <button class="btn-ghost btn-lg" type="button" onclick={openZotero}>
          Import Zotero <span class="es" lang="es">· Importar Zotero</span>
        </button>
      </div>
      <p class="calm">
        No setup, no accounts. Your research lives on your disk.
        <span lang="es">· Sin configuración, sin cuentas. Tu investigación vive en tu disco.</span>
      </p>
    </section>
  {:else if mode === "paste"}
    <section class="panel" aria-label={t("onb.pasteTitle")}>
      <p class="kicker">{t("onb.pasteTitle")}</p>
      <div class="paste-row">
        <input
          class="mono"
          type="url"
          bind:value={url}
          placeholder={t("onb.urlPh")}
          aria-label={t("onb.pasteTitle")}
          autofocus
          onkeydown={(e) => { if (e.key === "Enter") submitUrl(); }}
        />
        <button class="btn-primary" type="button" onclick={submitUrl} disabled={urlMissing}>
          {t("onb.generate")}
        </button>
      </div>
      {#if error}
        <p class="error mono" role="alert">{error}</p>
      {/if}
      <button class="btn-ghost btn-sm" type="button" onclick={back}>{t("onb.back")}</button>
    </section>
  {:else if mode === "zotero"}
    <!-- The Zotero door: the connector's minimal stub — the already-migrated
         library is its output; picking a reference starts the same flow. -->
    <section class="panel" aria-label={t("onb.zoteroTitle")}>
      <p class="kicker">{t("onb.zoteroTitle")}</p>
      <p class="hint">{t("onb.zoteroHint")}</p>
      {#if refs === null}
        <p class="hint mono">…</p>
      {:else if refs.length === 0}
        <p class="hint">{t("onb.zoteroEmpty")}</p>
      {:else}
        <div class="ref-list">
          {#each refs.slice(0, 8) as ref (ref.id)}
            <button class="ref-row" type="button" onclick={() => generate("ref", ref.id)}>
              <span class="ref-title">{ref.title}</span>
              <span class="ref-meta mono">{ref.authors} · {ref.year}</span>
            </button>
          {/each}
        </div>
      {/if}
      {#if error}
        <p class="error mono" role="alert">{error}</p>
      {/if}
      <button class="btn-ghost btn-sm" type="button" onclick={back}>{t("onb.back")}</button>
    </section>
  {:else if mode === "generating"}
    <section class="panel" aria-label={t("onb.generating")}>
      <p class="kicker">{t("onb.genTitle")}</p>
      <div class="log mono" aria-hidden="true">
        <p class="log-line"><span class="log-cursor"></span></p>
      </div>
      <p class="hint">{t("onb.generating")}</p>
    </section>
  {:else if mode === "result" && result}
    <!-- The result moment (DESIGN.md onboarding frame B): the streaming-style
         receipt, the candidates with confidence dots labeled with the
         assessing model, and the starter mission card animating in with its
         stop condition + criterion pre-filled. -->
    <section class="result" aria-label={t("onb.resultTitle")}>
      <header class="rep-head">
        <p class="kicker">{t("onb.resultKicker")}</p>
        <h3 class="display">{t("onb.resultTitle")}</h3>
        {#if showEsSubline}
          <p class="display-es" lang="es">Tu primera misión está lista</p>
        {/if}
      </header>
      <div class="grid">
        <div class="gen-card">
          <div class="gen-head">
            <h4>{t("onb.genTitle")}</h4>
            <p class="receipt mono">{receiptLine}</p>
          </div>
          <div class="log mono">
            {#each logLines as line, i (line)}
              <p class="log-line" style={`animation-delay:${150 + i * 350}ms`}>{line}</p>
            {/each}
            <p class="log-line log-done" style={`animation-delay:${150 + logLines.length * 350}ms`}>
              receipt rc-onb · {result.receipt.provider} · {result.paper.title}
            </p>
          </div>
          <p class="kicker candidates-kicker">{t("onb.candidatesTitle")}</p>
          <ul class="candidates">
            {#each result.candidates as candidate, i (candidate.hypothesisId)}
              <li class="cand" style={`animation-delay:${700 + i * 220}ms`}>
                <p class="cand-statement">{candidate.statement}</p>
                <div class="cand-meta">
                  <span class="chip">H-{candidate.seq} · {t("onb.proposed")}</span>
                  <span
                    class="conf-dot"
                    style={`background:${confidenceColor(candidate.confidence)}`}
                    title={`${candidate.assessingModel} · ${pct(candidate.confidence)}`}
                  ></span>
                  <span class="mono cand-conf">{candidate.assessingModel} · {pct(candidate.confidence)}</span>
                </div>
              </li>
            {/each}
          </ul>
        </div>
        <article class="mission-card" style="animation-delay:1200ms">
          <div class="m-top">
            <span class="m-id mono">M-{result.mission.seq}</span>
            <span class="chip chip-active"><span class="dot"></span>{t("onb.active")}</span>
          </div>
          <p class="kicker">{t("onb.missionLabel")}</p>
          <h4 class="m-title">{result.mission.question}</h4>
          <dl class="m-decl">
            <div class="row">
              <dt class="kicker">{t("onb.stop")}</dt>
              <dd>{result.mission.stopCondition}</dd>
            </div>
            <div class="row">
              <dt class="kicker">{t("onb.criterion")}</dt>
              <dd>{result.mission.successCriterion}</dd>
            </div>
            <div class="row">
              <dt class="kicker">{t("onb.spend")}</dt>
              <dd class="mono">
                {t("onb.ceiling", { amount: `$${(result.mission.spendCeilingCents / 100).toFixed(2)}` })}
              </dd>
            </div>
          </dl>
          <div class="meter" role="img" aria-label="Spend 0%"><i style="width:0%"></i></div>
          <button class="btn-primary" type="button" onclick={ondone}>{t("onb.continue")}</button>
        </article>
      </div>
    </section>
  {/if}
</div>

<style>
  .onboarding {
    /* DESIGN.md tokens (light surface) — same token block as the missions
       surface so the first-run state reads as the same product. */
    --rc-surface: var(--surface);
    --rc-surface-2: var(--surface-2);
    --rc-ink: var(--fg);
    --rc-ink-muted: var(--muted);
    --rc-border: var(--border);
    --rc-accent: var(--accent);
    --rc-danger-ink: var(--destructive-tx);
    --rc-ok: var(--st-read);
    width: 100%;
    display: flex;
    flex-direction: column;
    font-family: var(--font-body);
    color: var(--rc-ink);
  }
  .mono {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }
  .kicker {
    font-size: 12.5px;
    font-weight: 500;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
    margin: 0 0 8px;
  }
  .display {
    font-family: var(--font-display);
    font-style: italic;
    font-weight: 400;
    font-size: clamp(30px, 4vw, 44px);
    line-height: 1.08;
    letter-spacing: -0.01em;
    margin: 0 0 6px;
    color: var(--rc-ink);
  }
  .display-es {
    font-family: var(--font-display);
    font-style: italic;
    font-weight: 400;
    font-size: clamp(19px, 2.4vw, 26px);
    line-height: 1.2;
    margin: 0 0 18px;
    color: var(--rc-ink-muted);
  }

  /* The quiet welcome: centered, aurora backing, two big quiet actions. */
  .welcome {
    position: relative;
    isolation: isolate;
    text-align: center;
    padding: 48px 16px 32px;
  }
  .aurora {
    position: absolute;
    inset: -24px -24px auto -24px;
    height: 85%;
    z-index: -1;
    filter: blur(26px);
    opacity: 0.5;
    background:
      radial-gradient(42% 62% at 26% 38%, color-mix(in oklch, var(--accent) 20%, transparent), transparent 70%),
      radial-gradient(40% 60% at 74% 30%, color-mix(in oklch, var(--accent) 10%, transparent), transparent 70%);
    pointer-events: none;
  }
  .sub {
    font-size: 16px;
    line-height: 1.6;
    margin: 0 0 28px;
    color: var(--rc-ink);
  }
  .sub .es,
  .actions .es,
  .calm {
    color: var(--rc-ink-muted);
  }
  .actions {
    display: flex;
    gap: 12px;
    justify-content: center;
    flex-wrap: wrap;
    margin: 0 0 20px;
  }
  .calm {
    font-size: 13px;
    line-height: 1.55;
    margin: 0;
  }

  .btn-primary,
  .btn-ghost {
    font-family: inherit;
    font-size: 14px;
    font-weight: 500;
    border-radius: var(--r-input);
    padding: 11px 18px;
    min-height: 42px;
    cursor: pointer;
    transition: transform 0.15s ease, box-shadow 0.15s ease, background 0.15s ease;
  }
  .btn-lg {
    font-size: 15.5px;
    padding: 14px 26px;
    min-height: 52px;
  }
  .btn-sm {
    font-size: 13px;
    padding: 7px 12px;
    min-height: 34px;
    align-self: flex-start;
  }
  .btn-primary {
    background: var(--rc-accent);
    color: var(--surface);
    border: 1px solid var(--rc-accent);
  }
  .btn-primary:hover:not(:disabled) {
    transform: translateY(-2px);
    box-shadow: 0 4px 12px -2px color-mix(in oklch, var(--accent) 35%, transparent);
  }
  .btn-primary:active:not(:disabled) {
    transform: scale(0.98);
  }
  .btn-primary:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .btn-ghost {
    background: var(--rc-surface);
    color: var(--rc-ink);
    border: 1px solid var(--rc-border);
  }
  .btn-ghost:hover {
    background: var(--rc-surface-2);
    transform: translateY(-2px);
  }
  button:focus-visible,
  input:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }

  /* Paste / Zotero panels */
  .panel {
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 14px;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    animation: rise 0.25s ease both;
  }
  .paste-row {
    display: flex;
    gap: 10px;
  }
  .paste-row input {
    flex: 1;
    min-width: 0;
    font-size: 14.5px;
    line-height: 1.6;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: var(--r-input);
    padding: 10px 14px;
    outline: none;
  }
  .hint {
    margin: 0;
    font-size: 13.5px;
    line-height: 1.55;
    color: var(--rc-ink-muted);
  }
  .ref-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 280px;
    overflow-y: auto;
  }
  .ref-row {
    font-family: inherit;
    text-align: left;
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: var(--r-input);
    padding: 10px 12px;
    cursor: pointer;
  }
  .ref-row:hover {
    background: var(--rc-surface-2);
    transform: translateY(-1px);
  }
  .ref-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--rc-ink);
  }
  .ref-meta {
    font-size: 12px;
    color: var(--rc-ink-muted);
  }
  .error {
    margin: 0;
    font-size: 12.5px;
    color: var(--rc-danger-ink);
    word-break: break-word;
  }

  /* Generating state: a quiet log with a blinking cursor. */
  .log {
    background: var(--rc-surface-2);
    border: 1px solid var(--rc-border);
    border-radius: var(--r-card);
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--rc-ink);
    overflow-x: auto;
  }
  .log-line {
    margin: 0;
    white-space: nowrap;
    animation: rise 0.3s ease both;
  }
  .log-done {
    color: var(--rc-ink-muted);
  }
  .log-cursor {
    display: inline-block;
    width: 8px;
    height: 15px;
    vertical-align: text-bottom;
    background: var(--rc-accent);
    animation: blink 1s steps(1) infinite;
  }
  @keyframes blink {
    50% { opacity: 0; }
  }

  /* The result moment */
  .result {
    display: flex;
    flex-direction: column;
    gap: 20px;
    animation: rise 0.3s ease both;
  }
  .rep-head .display {
    margin-bottom: 2px;
  }
  .rep-head .display-es {
    margin-bottom: 0;
  }
  .grid {
    display: grid;
    grid-template-columns: 1.2fr 1fr;
    gap: 16px;
    align-items: start;
  }
  .gen-card,
  .mission-card {
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 14px;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
    padding: 22px;
  }
  .gen-head h4 {
    margin: 0 0 4px;
    font-size: 16px;
    font-weight: 600;
  }
  .receipt {
    margin: 0 0 14px;
    font-size: 12px;
    color: var(--rc-ink-muted);
  }
  .candidates-kicker {
    margin-top: 16px;
  }
  .candidates {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .cand {
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: var(--r-card);
    padding: 12px 14px;
    animation: rise 0.35s ease both;
  }
  .cand-statement {
    margin: 0 0 8px;
    font-size: 14px;
    line-height: 1.5;
    color: var(--rc-ink);
  }
  .cand-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .chip {
    font-size: 11.5px;
    font-weight: 500;
    color: var(--rc-ink-muted);
    background: var(--rc-surface-2);
    border: 1px solid var(--rc-border);
    border-radius: 999px;
    padding: 2px 9px;
  }
  .conf-dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    flex: none;
  }
  .cand-conf {
    font-size: 11.5px;
    color: var(--rc-ink-muted);
  }

  /* The starter mission card, animating in. */
  .mission-card {
    display: flex;
    flex-direction: column;
    gap: 10px;
    animation: rise 0.4s ease both;
  }
  .m-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .m-id {
    font-size: 12.5px;
    color: var(--rc-ink-muted);
  }
  .chip-active {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--rc-accent);
    background: color-mix(in oklch, var(--accent) 8%, var(--surface));
    border-color: color-mix(in oklch, var(--accent) 25%, var(--border));
  }
  .chip-active .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--rc-accent);
  }
  .m-title {
    font-family: var(--font-display);
    font-style: italic;
    font-weight: 400;
    font-size: 21px;
    line-height: 1.25;
    margin: 0;
  }
  .m-decl {
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .m-decl .row {
    display: grid;
    grid-template-columns: 88px 1fr;
    gap: 10px;
    align-items: baseline;
  }
  .m-decl .kicker {
    margin: 0;
  }
  .m-decl dd {
    margin: 0;
    font-size: 13.5px;
    line-height: 1.5;
    color: var(--rc-ink);
  }
  .meter {
    height: 4px;
    border-radius: 999px;
    background: var(--rc-surface-2);
    overflow: hidden;
  }
  .meter i {
    display: block;
    height: 100%;
    background: var(--rc-ok);
    border-radius: 999px;
  }
  .mission-card .btn-primary {
    align-self: flex-start;
    margin-top: 4px;
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  @media (max-width: 640px) {
    .grid {
      grid-template-columns: 1fr;
    }
    .paste-row {
      flex-direction: column;
    }
  }
</style>
