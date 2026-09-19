<script lang="ts">
  import { api } from "../../api";
  import { t } from "../../i18n";
  import type { HypothesisStatus, Proposal, ProposalStatus, ProposedPin } from "../../types";

  // Quarantine review (Story 2.2, AD-3/AD-13): agent proposals wait here as
  // diff cards — nothing changes until a human merges. The basis-stale
  // variant carries the bilingual warning banner and the force-approve
  // secondary action; decided proposals stay visible in the history view
  // with their chips + receipt stamps (mono seq/time) — nothing disappears.
  // Read-model only (AD-8): every decision re-folds via `load`, then
  // `ondecided` re-folds the board (a merge is what applies the change).
  // Story 2.6: `revision` re-folds this surface too (a rollback orphans
  // proposals — they land in the superseded-history view, never hidden).
  let {
    ondecided,
    revision = 0,
  }: { ondecided: () => void; revision?: number } = $props();

  let proposals = $state<Proposal[] | null>(null);
  let loadError = $state("");
  let historyOpen = $state(false);
  let actingOn = $state<string | null>(null);
  let actionError = $state("");

  // DESIGN.md lifecycle tokens (same values as the hypothesis cards).
  const lifecycle: Record<HypothesisStatus, { ink: string; soft: string }> = {
    proposed: { ink: "#5A6373", soft: "color-mix(in oklch,var(--lifecycle-proposed) 14%,var(--surface))" },
    testing: { ink: "#8A5214", soft: "color-mix(in oklch,var(--lifecycle-testing) 14%,var(--surface))" },
    supported: { ink: "#1A6B47", soft: "color-mix(in oklch,var(--lifecycle-supported) 14%,var(--surface))" },
    refuted: { ink: "#C0392B", soft: "color-mix(in oklch,var(--lifecycle-refuted) 14%,var(--surface))" },
    revised: { ink: "#2B46B8", soft: "color-mix(in oklch,var(--lifecycle-revised) 14%,var(--surface))" },
  };

  // DESIGN.md quarantine tokens: per-status ink + soft background.
  const statusTokens: Record<ProposalStatus, { ink: string; soft: string }> = {
    pending: { ink: "#8A5214", soft: "color-mix(in oklch,var(--quarantine-pending) 14%,var(--surface))" },
    merged: { ink: "#1A6B47", soft: "color-mix(in oklch,var(--quarantine-merged) 14%,var(--surface))" },
    rejected: { ink: "#5A6373", soft: "color-mix(in oklch,var(--quarantine-rejected) 14%,var(--surface))" },
    superseded: { ink: "var(--muted)", soft: "color-mix(in oklch,var(--quarantine-superseded) 14%,var(--surface))" },
    voided: { ink: "var(--muted)", soft: "color-mix(in oklch,var(--quarantine-void) 14%,var(--surface))" },
  };

  const pending = $derived(
    (proposals ?? []).filter((p) => p.status === "pending").sort((a, b) => b.seq - a.seq),
  );
  const decided = $derived(
    (proposals ?? []).filter((p) => p.status !== "pending").sort((a, b) => (b.decided?.seq ?? 0) - (a.decided?.seq ?? 0)),
  );
  // Story 2.6 (AD-1): proposals a rollback orphaned — superseded history,
  // never hidden. They render in the history view with their superseded
  // chips plus the rollback stamp naming the event that orphaned them.
  const orphaned = $derived(
    (proposals ?? []).filter((p) => p.orphanedByRollback).sort((a, b) => b.seq - a.seq),
  );

  $effect(() => {
    // Re-fold when the board revision moves — a rollback orphans proposals
    // (they land here as superseded history) and a merge decides them.
    void revision;
    load();
  });

  async function load() {
    try {
      proposals = await api.listProposals(null);
      loadError = "";
    } catch (e) {
      loadError = t("quarantine.loadError") + e;
    }
  }

  async function approve(proposal: Proposal, force: boolean) {
    actingOn = proposal.id;
    actionError = "";
    try {
      await api.approveProposal(proposal.id, force);
      await load();
      ondecided(); // a merge applied a change — the board re-folds
    } catch (e) {
      actionError = t("quarantine.actionError") + e;
      // a basis_stale refusal surfaced late: re-fold — the pending card now
      // reads stale and renders the warning variant with force-approve
      await load();
    } finally {
      actingOn = null;
    }
  }

  async function reject(proposal: Proposal) {
    actingOn = proposal.id;
    actionError = "";
    try {
      await api.rejectProposal(proposal.id);
      await load();
      ondecided();
    } catch (e) {
      actionError = t("quarantine.actionError") + e;
      await load();
    } finally {
      actingOn = null;
    }
  }

  const fmtTs = (ts: string) => new Date(ts).toLocaleString();

  // Result-pin proposals (Story 3.4, FR-11.5, AD-5): a fetched job result
  // arrives as a numerical pin CANDIDATE — same Approve/Reject as every
  // proposal; the merge is what pins it (auto-pinning is v0.2.0).
  const isPin = (p: Proposal) => p.proposedKind === "evidence.pinned";
  const asPin = (p: Proposal): ProposedPin => p.proposedPayload as ProposedPin;
  // The confidence dot's color (same scale as the hypothesis cards): green
  // at ≥0.8, amber at ≥0.5, red below — labeled with the assessing model,
  // never "verified" (FR-3.6).
  const confidenceColor = (c: number) =>
    c >= 0.8 ? "var(--st-read)" : c >= 0.5 ? "var(--st-reading)" : "var(--destructive)";
  const pct = (c: number) => `${Math.round(c * 100)}%`;
</script>

{#if proposals !== null && proposals.length > 0}
  <section class="quarantine" aria-label={t("quarantine.title")}>
    <header class="q-head">
      <div class="q-title">
        <p class="q-kicker">{t("quarantine.title")}</p>
        <p class="q-sub">
          {t("quarantine.sub")}
          {#if pending.length > 0}
            <span class="q-count mono">{pending.length} {t("quarantine.pending")}</span>
          {/if}
        </p>
      </div>
      {#if decided.length > 0}
        <button
          class="q-history-btn"
          type="button"
          aria-expanded={historyOpen}
          onclick={() => (historyOpen = !historyOpen)}
        >
          {t("quarantine.history")}
        </button>
      {/if}
    </header>

    {#if loadError}
      <p class="q-error" role="alert">{loadError}</p>
    {/if}
    {#if actionError}
      <p class="q-error" role="alert">{actionError}</p>
    {/if}

    {#if pending.length === 0}
      <p class="q-empty">{t("quarantine.empty")}</p>
    {:else}
      <div class="q-list">
        {#each pending as proposal (proposal.id)}
          <article class="q-card" class:stale={proposal.basisStale}>
            <header class="q-card-head">
              <span class="q-target" title={proposal.targetLabel ?? ""}>
                {#if proposal.targetSeq !== null}
                  <span class="mono q-h">H-{proposal.targetSeq}</span>
                {/if}
                {proposal.targetLabel ?? proposal.targetEntity}
              </span>
              <span class="q-run mono" title={t("quarantine.run")}>run {proposal.runId}</span>
            </header>

            {#if isPin(proposal)}
              <!-- The numerical pin candidate's anatomy (AD-5, FR-3.3): the
                   artifact it anchors to, the sha-256 digest computed at
                   proposal time (mono, full digest on hover), the confidence
                   dot labeled with the assessing model (FR-3.6 — attributed,
                   never "verified"), and the captured content it anchors. -->
              {@const pin = asPin(proposal)}
              <p class="q-diff">
                <span class="q-verb">{t("quarantine.pinProposes")}</span>
              </p>
              <div class="q-pin">
                <span class="q-pin-main">
                  <span class="q-pin-label">{t("quarantine.artifact")}</span>
                  <span class="q-pin-ref mono">{pin.artifact_ref}</span>
                </span>
                <span
                  class="q-pin-conf"
                  title={`${pin.assessing_model} · ${pct(pin.confidence)}`}
                >
                  <span
                    class="q-dot"
                    style={`background:${confidenceColor(pin.confidence)}`}
                    aria-hidden="true"
                  ></span>
                  {pin.assessing_model} · {pct(pin.confidence)}
                </span>
                <span class="q-pin-digest mono" title={pin.digest}>
                  sha-256 {pin.digest.slice(0, 16)}…
                </span>
              </div>
              <blockquote class="q-excerpt" title={pin.digest}>
                {pin.excerpt}
              </blockquote>
              <p class="q-note">{t("quarantine.awaitingMerge")}</p>
            {:else}
              <p class="q-diff">
                <span class="q-verb">{t("quarantine.proposes")}</span>
                <span class="q-change">{t("quarantine.change")}:</span>
                <span class="status-chip" style={`color:${lifecycle[proposal.proposedPayload.from].ink};background:${lifecycle[proposal.proposedPayload.from].soft}`}>
                  {t(`hyp.status.${proposal.proposedPayload.from}`)}
                </span>
                <span class="q-arrow" aria-hidden="true">→</span>
                <span class="status-chip" style={`color:${lifecycle[proposal.proposedPayload.to].ink};background:${lifecycle[proposal.proposedPayload.to].soft}`}>
                  {t(`hyp.status.${proposal.proposedPayload.to}`)}
                </span>
              </p>

              <p class="q-basis">“{proposal.proposedPayload.basis}”</p>
            {/if}

            {#if proposal.basisStale}
              <!-- The basis-stale variant (AD-13): the bilingual warning
                   banner + force-approve secondary action. -->
              <p class="q-stale" role="alert">
                <span class="q-stale-icon" aria-hidden="true">⚠</span>
                {t("quarantine.basisStale")}
                <span class="mono q-stale-seq">basis {proposal.basisSeq}</span>
              </p>
            {/if}

            <footer class="q-actions">
              <button
                class="btn-reject"
                type="button"
                disabled={actingOn === proposal.id}
                onclick={() => reject(proposal)}
              >
                {actingOn === proposal.id ? t("quarantine.rejecting") : t("quarantine.reject")}
              </button>
              {#if proposal.basisStale}
                <button
                  class="btn-force"
                  type="button"
                  disabled={actingOn === proposal.id}
                  onclick={() => approve(proposal, true)}
                >
                  {actingOn === proposal.id ? t("quarantine.approving") : t("quarantine.forceApprove")}
                </button>
              {:else}
                <button
                  class="btn-approve"
                  type="button"
                  disabled={actingOn === proposal.id}
                  onclick={() => approve(proposal, false)}
                >
                  {actingOn === proposal.id ? t("quarantine.approving") : t("quarantine.approve")}
                </button>
              {/if}
            </footer>
          </article>
        {/each}
      </div>
    {/if}

    {#if historyOpen}
      <!-- The history view (AD-13): decided proposals with their chips +
           receipt stamps — nothing disappears. Story 2.6: proposals a
           rollback orphaned render here as visibly SUPERSEDED history with
           the rollback stamp — excluded from every projection, never
           hidden. -->
      {#if orphaned.length > 0}
        <p class="q-kicker q-superseded-kicker">{t("quarantine.superseded")}</p>
        <p class="q-sub q-superseded-sub">{t("quarantine.supersededSub")}</p>
      {/if}
      {#if decided.length === 0}
        <p class="q-empty">{t("quarantine.historyEmpty")}</p>
      {:else}
        <div class="q-list q-history">
          {#each decided as proposal (proposal.id)}
            <article class="q-card q-card-decided">
              <header class="q-card-head">
                <span class="q-target" title={proposal.targetLabel ?? ""}>
                  {#if proposal.targetSeq !== null}
                    <span class="mono q-h">H-{proposal.targetSeq}</span>
                  {/if}
                  {proposal.targetLabel ?? proposal.targetEntity}
                </span>
                <span
                  class="status-chip"
                  style={`color:${statusTokens[proposal.status].ink};background:${statusTokens[proposal.status].soft}`}
                >
                  {t(`quarantine.status.${proposal.status}`)}
                </span>
              </header>
              {#if isPin(proposal)}
                <!-- Decided result pins render the same candidate anatomy
                     (the artifact + digest + attribution), with the decision
                     receipt below — nothing disappears. -->
                {@const pin = asPin(proposal)}
                <p class="q-diff">
                  <span class="q-verb">{t("quarantine.pinProposes")}</span>
                </p>
                <div class="q-pin">
                  <span class="q-pin-main">
                    <span class="q-pin-label">{t("quarantine.artifact")}</span>
                    <span class="q-pin-ref mono">{pin.artifact_ref}</span>
                  </span>
                  <span class="q-pin-digest mono" title={pin.digest}>
                    sha-256 {pin.digest.slice(0, 16)}…
                  </span>
                </div>
              {:else}
                <p class="q-diff">
                  <span class="q-change">{t("quarantine.change")}:</span>
                  <span class="status-chip" style={`color:${lifecycle[proposal.proposedPayload.from].ink};background:${lifecycle[proposal.proposedPayload.from].soft}`}>
                    {t(`hyp.status.${proposal.proposedPayload.from}`)}
                  </span>
                  <span class="q-arrow" aria-hidden="true">→</span>
                  <span class="status-chip" style={`color:${lifecycle[proposal.proposedPayload.to].ink};background:${lifecycle[proposal.proposedPayload.to].soft}`}>
                    {t(`hyp.status.${proposal.proposedPayload.to}`)}
                  </span>
                </p>
              {/if}
              {#if proposal.status === "merged" && proposal.basisStale}
                <p class="q-stale q-stale-marker">
                  <span class="q-stale-icon" aria-hidden="true">⚠</span>
                  {t("quarantine.mergedStale")}
                </p>
              {/if}
              {#if proposal.status === "superseded"}
                <p class="q-note">{t("quarantine.supersededNote")}</p>
              {/if}
              {#if proposal.orphanedByRollback}
                <!-- The rollback stamp (Story 2.6, AD-1): the event that
                     orphaned this proposal — superseded history, never hidden. -->
                <p class="q-receipt mono">
                  {t("quarantine.rolledBackAt", { seq: proposal.rolledBackSeq ?? 0 })}
                </p>
              {/if}
              <!-- Receipt stamp (mono seq/time) — never silent, never anonymous. -->
              {#if proposal.decided}
                <p class="q-receipt mono">
                  {t("quarantine.decided")} · seq {proposal.decided.seq} · {fmtTs(proposal.decided.ts)} · {proposal.decided.actor}
                </p>
              {/if}
            </article>
          {/each}
        </div>
      {/if}
    {/if}
  </section>
{/if}

<style>
  .quarantine {
    --rc-surface: var(--surface);
    --rc-surface-2: var(--surface-2);
    --rc-ink: var(--fg);
    --rc-ink-muted: var(--muted);
    --rc-border: var(--border);
    --rc-accent: var(--accent);
    --rc-accent-soft: var(--accent-soft);
    --rc-danger-ink: var(--destructive-tx);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: var(--r-card);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .mono {
    font-family: "JetBrains Mono", ui-monospace, monospace;
    font-variant-numeric: tabular-nums;
  }
  .q-head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }
  .q-kicker {
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
    margin: 0 0 3px;
  }
  /* The superseded-history banner (Story 2.6): quiet gray — history, not an
     alert; the rollback stamps carry the specifics. */
  .q-superseded-kicker {
    margin-top: 12px;
  }
  .q-superseded-sub {
    margin-bottom: 8px;
  }
  .q-sub {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--rc-ink-muted);
  }
  .q-count {
    margin-left: 6px;
    font-size: 11.5px;
    color: var(--rc-ink);
    background: var(--rc-accent-soft);
    border-radius: var(--r-input);
    padding: 2px 6px;
  }
  .q-history-btn {
    font-family: inherit;
    font-size: 12.5px;
    font-weight: 500;
    color: var(--rc-accent);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: var(--r-input);
    padding: 7px 12px;
    min-height: 34px;
    cursor: pointer;
    transition: background 0.15s ease;
    white-space: nowrap;
  }
  .q-history-btn:hover {
    background: var(--rc-accent-soft);
  }
  .q-history-btn[aria-expanded="true"] {
    background: var(--rc-accent-soft);
    border-color: var(--rc-accent);
  }
  .q-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .q-history {
    border-top: 1px dashed var(--rc-border);
    padding-top: 12px;
  }
  .q-card {
    border: 1px solid var(--rc-border);
    border-radius: var(--r-card);
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: var(--rc-surface);
  }
  .q-card.stale {
    border-color: var(--st-reading);
    background: color-mix(in oklch, var(--st-reading) 14%, var(--surface));
  }
  .q-card-decided {
    background: var(--rc-surface-2);
  }
  .q-card-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
  }
  .q-target {
    font-size: 13.5px;
    font-weight: 500;
    line-height: 1.45;
    color: var(--rc-ink);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .q-h {
    font-size: 11.5px;
    color: var(--rc-ink-muted);
    margin-right: 6px;
  }
  .q-run {
    font-size: 11px;
    color: var(--rc-ink-muted);
    white-space: nowrap;
  }
  .q-diff {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 7px;
    flex-wrap: wrap;
    font-size: 13px;
    color: var(--rc-ink);
  }
  .q-verb {
    font-style: italic;
    color: var(--rc-ink-muted);
  }
  .q-change {
    color: var(--rc-ink-muted);
  }
  .status-chip {
    font-size: 12px;
    font-weight: 500;
    border-radius: 999px;
    padding: 3px 10px;
    line-height: 1.3;
  }
  .q-arrow {
    color: var(--rc-ink-muted);
  }
  .q-basis {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--rc-ink-muted);
    font-style: italic;
  }
  /* The numerical pin candidate's anatomy (Story 3.4, AD-5 — same tokens
     as the hypothesis cards' pin rows). */
  .q-pin {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    font-size: 12.5px;
    color: var(--rc-ink);
    background: var(--rc-surface-2);
    border-radius: var(--r-input);
    padding: 7px 10px;
  }
  .q-pin-main {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .q-pin-label {
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
  }
  .q-pin-ref {
    font-size: 12px;
    color: var(--rc-ink);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .q-pin-conf {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--rc-ink);
    white-space: nowrap;
  }
  .q-dot {
    width: 8px;
    height: 8px;
    border-radius: 9999px;
    flex: none;
  }
  .q-pin-digest {
    font-size: 11px;
    color: var(--rc-ink-muted);
    letter-spacing: 0.01em;
    margin-left: auto;
    white-space: nowrap;
  }
  .q-excerpt {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--rc-ink-muted);
    font-family: "JetBrains Mono", ui-monospace, monospace;
    background: var(--rc-surface-2);
    border-left: 3px solid var(--rc-border);
    border-radius: 0 var(--r-input) var(--r-input) 0;
    padding: 8px 10px;
    max-height: 120px;
    overflow: hidden;
    white-space: pre-wrap;
    word-break: break-word;
  }
  /* The basis-stale warning banner (AD-13) — amber, never silent. */
  .q-stale {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    font-weight: 500;
    line-height: 1.45;
    color: var(--st-reading-tx);
    background: color-mix(in oklch, var(--st-reading) 14%, var(--surface));
    border: 1px solid var(--st-reading);
    border-radius: var(--r-input);
    padding: 8px 10px;
  }
  .q-card.stale .q-stale {
    background: var(--surface);
  }
  .q-stale-icon {
    font-size: 13px;
  }
  .q-stale-seq {
    font-size: 11px;
    color: var(--st-reading-tx);
    margin-left: auto;
  }
  .q-stale-marker {
    background: transparent;
    border-style: dashed;
    padding: 4px 8px;
  }
  .q-note {
    margin: 0;
    font-size: 12px;
    line-height: 1.45;
    color: var(--rc-ink-muted);
  }
  .q-receipt {
    margin: 0;
    font-size: 11px;
    color: var(--rc-ink-muted);
    letter-spacing: 0.01em;
  }
  .q-actions {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    border-top: 1px solid var(--rc-border);
    padding-top: 10px;
  }
  .q-actions button {
    font-family: inherit;
    font-size: 13.5px;
    font-weight: 500;
    border-radius: var(--r-input);
    padding: 8px 14px;
    min-height: 38px;
    cursor: pointer;
    transition: transform 0.15s ease, box-shadow 0.15s ease, background 0.15s ease;
  }
  .q-actions button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .btn-approve {
    background: var(--rc-accent);
    color: var(--surface);
    border: 1px solid var(--rc-accent);
  }
  .btn-approve:hover:not(:disabled) {
    transform: translateY(-2px);
    box-shadow: 0 4px 12px -2px rgba(59, 91, 219, 0.35);
  }
  .btn-force {
    background: var(--surface);
    color: var(--st-reading-tx);
    border: 1px solid var(--st-reading);
  }
  .btn-force:hover:not(:disabled) {
    background: color-mix(in oklch, var(--st-reading) 14%, var(--surface));
    transform: translateY(-2px);
  }
  .btn-reject {
    margin-right: auto;
    background: var(--rc-surface);
    color: var(--rc-danger-ink);
    border: 1px solid var(--rc-border);
  }
  .btn-reject:hover:not(:disabled) {
    background: color-mix(in oklch, var(--destructive) 6%, var(--surface));
    border-color: var(--rc-danger-ink);
    transform: translateY(-2px);
  }
  .q-error {
    margin: 0;
    font-size: 13px;
    color: var(--rc-danger-ink);
  }
  .q-empty {
    margin: 0;
    font-size: 13px;
    line-height: 1.55;
    color: var(--rc-ink-muted);
    padding: 4px 0;
  }
  button:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }
</style>
