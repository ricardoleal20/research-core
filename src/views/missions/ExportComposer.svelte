<script lang="ts">
  import { api } from "../../api";
  import { t } from "../../i18n";
  import type { ExportInspect, ExportOutcome } from "../../types";

  // The export composer (Story 3.1, FR-7.1/7.2, EXPERIENCE.md "Own your
  // research / Tu investigación es tuya"): a header action — scope picker
  // (whole workspace or one entity family), destination folder (native
  // dialog when the Tauri runtime is present, typed path otherwise), and
  // the result moment: the written folder, the mono manifest cut, and the
  // stale-warning states (Story 2.6's signal — both cuts in mono). Read
  // model only: exporting never mutates the log; it renders the folds.
  let { onclose = () => {} }: { onclose?: () => void } = $props();

  type ScopeId = "all" | "missions" | "hypotheses" | "evidence" | "timeline" | "search_log";
  const scopes: ScopeId[] = ["all", "missions", "hypotheses", "evidence", "timeline", "search_log"];

  let scope = $state<ScopeId>("all");
  let dir = $state("");
  let exporting = $state(false);
  let outcome = $state<ExportOutcome | null>(null);
  let exportError = $state("");
  // The at-open staleness read (EXPERIENCE.md): when the chosen folder
  // already holds a stale export, the composer warns BEFORE exporting.
  let staleAtOpen = $state<ExportInspect | null>(null);

  const canExport = $derived(dir.trim() !== "" && !exporting);

  async function pickFolder() {
    try {
      const picked = await api.pickFolder();
      if (picked) {
        dir = picked;
        await checkStale();
      }
    } catch {
      // the native dialog is unavailable (plain browser) — the typed path
      // input remains the way in
    }
  }

  async function checkStale() {
    staleAtOpen = null;
    const trimmed = dir.trim();
    if (trimmed === "") return;
    try {
      staleAtOpen = await api.inspectExport(trimmed);
    } catch {
      staleAtOpen = null; // an unreadable folder exports fresh
    }
  }

  async function runExport() {
    if (!canExport) return;
    exporting = true;
    exportError = "";
    try {
      outcome = await api.exportWorkspace(dir.trim(), scope);
      staleAtOpen = null; // the fresh render supersedes whatever was there
    } catch (e) {
      exportError = t("export.error") + e;
      outcome = null;
    } finally {
      exporting = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="drawer-root" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}>
  <section
    class="composer"
    role="dialog"
    aria-modal="true"
    aria-labelledby="export-heading"
  >
    <header class="head">
      <p class="kicker">{t("export.action")}</p>
      <h3 class="title" id="export-heading">{t("export.heading")}</h3>
      <p class="sub">{t("export.sub")}</p>
    </header>

    <div class="field">
      <span class="field-label" id="export-scope-label">{t("export.scope")}</span>
      <div class="scopes" role="radiogroup" aria-labelledby="export-scope-label">
        {#each scopes as s (s)}
          <button
            type="button"
            role="radio"
            aria-checked={scope === s}
            class="scope"
            class:active={scope === s}
            onclick={() => (scope = s)}
          >
            <span class="scope-label">{t(`export.scope.${s}`)}</span>
            <span class="scope-desc">{t(`export.scope.desc.${s}`)}</span>
          </button>
        {/each}
      </div>
    </div>

    <div class="field">
      <label for="export-dir">{t("export.path")}</label>
      <div class="path-row">
        <input
          id="export-dir"
          class="mono"
          type="text"
          bind:value={dir}
          placeholder="~/research/my-export"
          onchange={() => { void checkStale(); }}
        />
        <button class="btn-secondary" type="button" onclick={pickFolder}>
          {t("export.pick")}
        </button>
      </div>
      {#if staleAtOpen?.stale}
        <p class="stale" role="alert">
          {t("export.staleAtOpen", { seq: staleAtOpen.rollbackSeq ?? 0 })}
        </p>
      {/if}
    </div>

    <footer class="foot">
      <button class="btn-secondary" type="button" onclick={onclose}>
        {t("export.close")}
      </button>
      <button class="btn-primary" type="button" onclick={runExport} disabled={!canExport}>
        {exporting ? t("export.running") : t("export.run")}
      </button>
    </footer>

    {#if exportError}
      <p class="error" role="alert">{exportError}</p>
    {/if}

    {#if outcome}
      <!-- The result moment: the written folder, the mono manifest cut, the
           stale-warning state when the render superseded a stale export. -->
      <div class="result" role="status">
        <p class="result-line">{t("export.result")} <span class="mono">{outcome.dir}</span></p>
        <p class="result-line">
          {t("export.resultCut", { count: outcome.manifest.fileCount })}
          <span class="mono">e-{outcome.manifest.cutSeq}</span>
          {#if outcome.manifest.cutTs}
            <span class="muted">· {outcome.manifest.cutTs}</span>
          {/if}
        </p>
        {#if outcome.manifest.staleNotice}
          <p class="stale" role="alert">
            {t("export.staleSuperseded", {
              prev: outcome.manifest.staleNotice.previousCut,
              seq: outcome.manifest.staleNotice.rollbackSeq,
              cut: outcome.manifest.cutSeq,
            })}
          </p>
        {/if}
        <button
          class="btn-secondary reveal"
          type="button"
          onclick={() => { try { void api.revealPath(outcome!.dir); } catch { /* non-desktop */ } }}
        >
          {t("export.reveal")}
        </button>
      </div>
    {/if}
  </section>
</div>

<style>
  .composer {
    /* DESIGN.md tokens (light surface) — same family as the missions views. */
    --rc-surface: #ffffff;
    --rc-surface-2: #f4f4f6;
    --rc-ink: #151519;
    --rc-ink-muted: #71717a;
    --rc-border: #eaeaec;
    --rc-accent: #3071b5;
    --rc-danger-ink: #be123c;
    --rc-warn-ink: #92400e;
    --rc-warn-bg: #fffbeb;
    --rc-warn-border: #b45309;
  }
  .drawer-root {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgba(21, 21, 25, 0.35);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding: 8vh 16px 16px;
    overflow-y: auto;
  }
  .composer {
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 14px;
    box-shadow: 0 12px 40px -12px rgba(0, 0, 0, 0.25);
    padding: 24px;
    width: 100%;
    max-width: 560px;
    height: fit-content;
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
  .kicker {
    font-size: 13px;
    font-weight: 500;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
    margin: 0 0 6px;
  }
  .title {
    font-family: "Instrument Serif", Georgia, serif;
    font-style: italic;
    font-weight: 400;
    font-size: 26px;
    line-height: 1.15;
    margin: 0 0 8px;
  }
  .sub {
    margin: 0;
    font-size: 13.5px;
    line-height: 1.5;
    color: var(--rc-ink-muted);
  }
  .head {
    border-bottom: 1px solid var(--rc-border);
    padding-bottom: 14px;
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

  /* Scope picker: a two-column grid of named stops (the autonomy dial's
     pattern — never a free-form multiselect). */
  .scopes {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 4px;
    background: var(--rc-surface-2);
    border-radius: 12px;
    padding: 4px;
  }
  .scope {
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
  .scope.active {
    background: var(--rc-surface);
    color: var(--rc-ink);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.06);
  }
  .scope-label {
    font-size: 13.5px;
    font-weight: 600;
  }
  .scope.active .scope-label {
    color: var(--rc-accent);
  }
  .scope-desc {
    font-size: 11.5px;
    line-height: 1.4;
  }

  .path-row {
    display: flex;
    gap: 8px;
    align-items: stretch;
  }
  .path-row input {
    flex: 1;
    min-width: 0;
    font-family: "JetBrains Mono", ui-monospace, monospace;
    font-size: 13px;
    line-height: 1.5;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 8px;
    padding: 9px 12px;
    outline: none;
    min-height: 40px;
  }
  .path-row input::placeholder {
    color: var(--rc-ink-muted);
  }
  .path-row input:focus-visible,
  .scope:focus-visible,
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

  .foot {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    border-top: 1px solid var(--rc-border);
    padding-top: 16px;
  }

  /* The stale warning (EXPERIENCE.md): amber inline banner, both cuts in
     mono — never color alone (the text carries the meaning). */
  .stale {
    margin: 4px 0 0;
    padding: 9px 12px;
    font-size: 12.5px;
    font-weight: 500;
    line-height: 1.45;
    color: var(--rc-warn-ink);
    background: var(--rc-warn-bg);
    border: 1px solid var(--rc-warn-border);
    border-radius: 8px;
  }

  .result {
    border: 1px solid var(--rc-border);
    border-radius: 10px;
    background: var(--rc-surface-2);
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .result-line {
    margin: 0;
    font-size: 13.5px;
    line-height: 1.5;
  }
  .muted {
    color: var(--rc-ink-muted);
    font-size: 12px;
  }
  .reveal {
    align-self: flex-start;
    margin-top: 4px;
    font-size: 13px;
    padding: 7px 12px;
    min-height: 34px;
  }

  .error {
    margin: 0;
    font-size: 13px;
    color: var(--rc-danger-ink);
  }

  @media (max-width: 560px) {
    .scopes {
      grid-template-columns: 1fr;
    }
    .path-row {
      flex-direction: column;
    }
  }
</style>
