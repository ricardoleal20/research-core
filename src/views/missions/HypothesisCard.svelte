<script lang="ts">
  import { api } from "../../api";
  import { t } from "../../i18n";
  import type { Hypothesis, HypothesisStatus, RelationChip, RelationKind } from "../../types";

  // Hypothesis card (DESIGN.md components.hypothesis-card): statement,
  // lifecycle chip top-right, relation chips inline, audit-stamp strip in
  // mono. Read-model only (AD-8): mutations go through invoke; after each
  // mutation the parent board re-folds via `refresh`.
  let {
    hypothesis,
    siblings,
    refresh,
  }: {
    hypothesis: Hypothesis;
    siblings: Hypothesis[];
    refresh: () => Promise<void>;
  } = $props();

  // DESIGN.md lifecycle tokens: per-state ink + soft background.
  const lifecycle: Record<HypothesisStatus, { ink: string; soft: string }> = {
    proposed: { ink: "#334155", soft: "#F1F5F9" },
    testing: { ink: "#0369A1", soft: "#F0F9FF" },
    supported: { ink: "#047857", soft: "#ECFDF5" },
    refuted: { ink: "#BE123C", soft: "#FFF1F2" },
    revised: { ink: "#B45309", soft: "#FFFBEB" },
  };

  // FR-2.2 transition table — only legal next statuses ever render; the
  // illegal ones are not hidden behind a disabled state, they are absent.
  const allowedNext: Record<HypothesisStatus, HypothesisStatus[]> = {
    proposed: ["testing"],
    testing: ["supported", "refuted"],
    supported: ["revised"],
    refuted: ["revised"],
    revised: ["testing"],
  };

  const relationKinds: RelationKind[] = [
    "contradicts",
    "extends",
    "specializes",
    "supports_the_same_claim",
  ];

  // Transition control state — a basis is required (FR-2.2).
  let basis = $state("");
  let submitted = $state(false);
  let transitioning = $state(false);
  let transitionError = $state("");

  // Add-relation control state (FR-2.3): pick second hypothesis + kind.
  let relTarget = $state("");
  let relKind = $state<RelationKind>("contradicts");
  let relating = $state(false);
  let relationError = $state("");

  const basisMissing = $derived(basis.trim() === "");
  const shortId = $derived(`H-${hypothesis.seq}`);
  const statusLabel = $derived(t(`hyp.status.${hypothesis.status}`));
  const tokens = $derived(lifecycle[hypothesis.status]);
  const next = $derived(allowedNext[hypothesis.status]);
  const stampTs = $derived(fmtStamp(new Date(hypothesis.audit.ts)));

  function fmtStamp(d: Date): string {
    const p = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
  }

  /** Chip label for one relation (FR-2.3 vocabulary): outgoing reads
   *  "⟶ contradicts H-7"; incoming reads "⟵ contradicted-by H-3". */
  function relLabel(rel: RelationChip): string {
    if (rel.kind === "supports_the_same_claim") {
      return t("hyp.rel.supports_the_same_claim");
    }
    return t(`hyp.rel.${rel.kind}.${rel.direction === "outgoing" ? "out" : "in"}`);
  }

  async function transition(to: HypothesisStatus) {
    submitted = true;
    if (basisMissing) return;
    transitioning = true;
    transitionError = "";
    try {
      await api.transitionHypothesis(hypothesis.id, to, basis.trim());
      basis = "";
      submitted = false;
      await refresh();
    } catch (e) {
      transitionError = t("hyp.transitionError") + e;
    } finally {
      transitioning = false;
    }
  }

  async function relate() {
    if (!relTarget) return;
    relating = true;
    relationError = "";
    try {
      await api.addRelation(hypothesis.id, relTarget, relKind);
      relTarget = "";
      await refresh();
    } catch (e) {
      relationError = t("hyp.relationError") + e;
    } finally {
      relating = false;
    }
  }
</script>

<article class="hyp-card">
  <header class="hc-head">
    <span class="hc-id mono">{shortId}</span>
    <!-- Lifecycle chip (DESIGN.md components.lifecycle-chip): pill, soft
         background + ink text + 20% ring inset — never color alone, the
         label always renders. -->
    <span
      class="hc-chip"
      style={`--ink:${tokens.ink};--soft:${tokens.soft}`}
    >
      {statusLabel}
    </span>
  </header>
  <p class="hc-statement">{hypothesis.statement}</p>

  {#if hypothesis.relations.length > 0}
    <!-- Typed-relation chips (FR-2.3): inline pills, H-n in mono — chips
         only, never a graph canvas. -->
    <ul class="hc-relations" aria-label={t("hyp.relationKind")}>
      {#each hypothesis.relations as rel (rel.seq)}
        <li
          class="hc-relation"
          title={rel.otherStatement}
        >
          <span aria-hidden="true">{rel.direction === "incoming" ? "⟵" : "⟶"}</span>
          {relLabel(rel)}
          <span class="mono">H-{rel.otherSeq}</span>
        </li>
      {/each}
    </ul>
  {/if}

  <!-- Transition control (FR-2.2): a basis is required for every
       transition; only legal next statuses render as buttons. -->
  <div class="hc-transition">
    <input
      class="hc-basis"
      class:invalid={submitted && basisMissing}
      type="text"
      bind:value={basis}
      placeholder={t("hyp.basisPh")}
      aria-label={t("hyp.basis")}
      aria-invalid={submitted && basisMissing}
    />
    <div class="hc-next">
      {#each next as to (to)}
        <button
          class="hc-next-btn"
          type="button"
          onclick={() => transition(to)}
          disabled={transitioning}
        >
          {t(`hyp.status.${to}`)}
        </button>
      {/each}
    </div>
    {#if submitted && basisMissing}
      <p class="hc-error" role="alert">{t("hyp.basisRequired")}</p>
    {/if}
    {#if transitionError}
      <p class="hc-error" role="alert">{transitionError}</p>
    {/if}
  </div>

  {#if siblings.length > 0}
    <!-- Add-relation control (FR-2.3): pick the second hypothesis and the
         typed relation kind. -->
    <div class="hc-relate">
      <span class="hc-relate-label">{t("hyp.relate")}</span>
      <select class="hc-select" bind:value={relTarget} aria-label={t("hyp.relateTo")}>
        <option value="" disabled selected>{t("hyp.relationPick")}</option>
        {#each siblings as s (s.id)}
          <option value={s.id}>H-{s.seq} · {s.statement}</option>
        {/each}
      </select>
      <select class="hc-select" bind:value={relKind} aria-label={t("hyp.relationKind")}>
        {#each relationKinds as kind (kind)}
          <option value={kind}>{t(`hyp.relKind.${kind}`)}</option>
        {/each}
      </select>
      <button
        class="hc-relate-btn"
        type="button"
        onclick={relate}
        disabled={!relTarget || relating}
      >
        {t("hyp.relate")}
      </button>
      {#if relationError}
        <p class="hc-error" role="alert">{relationError}</p>
      {/if}
    </div>
  {/if}

  <!-- Audit-stamp strip (FR-2.2): caption + mono — actor, ts, basis of the
       last lifecycle event. No status ever changes silently. -->
  <footer class="hc-stamp mono">
    {stampTs} · {hypothesis.audit.actor} · {hypothesis.audit.basis}
  </footer>
</article>

<style>
  .hyp-card {
    --rc-surface: #ffffff;
    --rc-ink: #151519;
    --rc-ink-muted: #71717a;
    --rc-border: #eaeaec;
    --rc-accent: #3071b5;
    --rc-accent-soft: rgba(48, 113, 181, 0.1);
    --rc-danger-ink: #be123c;
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 12px;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    transition: transform 0.18s ease, box-shadow 0.18s ease, border-color 0.18s ease;
  }
  .hyp-card:hover {
    transform: translateY(-2px);
    box-shadow: 0 8px 24px -6px rgba(0, 0, 0, 0.08);
    border-color: var(--rc-accent);
  }
  .mono {
    font-family: "JetBrains Mono", ui-monospace, monospace;
    font-variant-numeric: tabular-nums;
  }

  .hc-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .hc-id {
    font-size: 12px;
    color: var(--rc-ink-muted);
  }
  /* Lifecycle chip: badge anatomy — pill, 12px medium, soft background +
     ink text + 20% ring inset (DESIGN.md components.badge). */
  .hc-chip {
    font-size: 12px;
    font-weight: 500;
    letter-spacing: 0.02em;
    color: var(--ink);
    background: var(--soft);
    border-radius: 9999px;
    padding: 2px 10px;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--ink) 20%, transparent);
    white-space: nowrap;
  }
  .hc-statement {
    font-family: Inter, system-ui, sans-serif;
    font-size: 15px;
    font-weight: 600;
    line-height: 1.45;
    color: var(--rc-ink);
    margin: 0;
  }

  /* Relation chips: inline pills, wrap; H-n label in mono. */
  .hc-relations {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .hc-relation {
    display: inline-flex;
    align-items: baseline;
    gap: 5px;
    font-size: 12px;
    font-weight: 500;
    color: var(--rc-ink-muted);
    background: var(--rc-accent-soft);
    border-radius: 9999px;
    padding: 2px 10px;
    cursor: default;
  }
  .hc-relation .mono {
    font-size: 11.5px;
    color: var(--rc-ink);
  }

  /* Transition control: basis input + one button per legal next status. */
  .hc-transition {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }
  .hc-basis {
    flex: 1 1 160px;
    min-width: 0;
    font-family: inherit;
    font-size: 13px;
    line-height: 1.5;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 8px;
    padding: 7px 10px;
    outline: none;
    min-height: 34px;
  }
  .hc-basis::placeholder {
    color: var(--rc-ink-muted);
  }
  .hc-basis.invalid {
    border-color: var(--rc-danger-ink);
    background: #fff1f2;
  }
  .hc-next {
    display: flex;
    gap: 6px;
  }
  .hc-next-btn {
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
    transition: background 0.15s ease, transform 0.15s ease;
  }
  .hc-next-btn:hover:not(:disabled) {
    background: var(--rc-accent-soft);
    transform: translateY(-1px);
  }
  .hc-next-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  /* Add-relation control. */
  .hc-relate {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }
  .hc-relate-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--rc-ink-muted);
  }
  .hc-select {
    font-family: inherit;
    font-size: 12.5px;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 8px;
    padding: 6px 8px;
    min-height: 34px;
    max-width: 220px;
    outline: none;
    cursor: pointer;
  }
  .hc-relate-btn {
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
  }
  .hc-relate-btn:hover:not(:disabled) {
    background: var(--rc-accent-soft);
  }
  .hc-relate-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .hc-error {
    margin: 0;
    flex-basis: 100%;
    font-size: 12px;
    font-weight: 500;
    color: var(--rc-danger-ink);
  }

  /* Audit-stamp strip: caption-size mono — the receipt voice. */
  .hc-stamp {
    font-size: 11px;
    letter-spacing: 0.02em;
    color: var(--rc-ink-muted);
    border-top: 1px solid var(--rc-border);
    padding-top: 8px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  button:focus-visible,
  input:focus-visible,
  select:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }
</style>
