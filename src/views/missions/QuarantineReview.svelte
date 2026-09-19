<script lang="ts">
  import { api } from "../../api";
  import { t } from "../../i18n";
  import type { HypothesisStatus, Proposal, ProposalStatus } from "../../types";

  // Quarantine review (Story 2.2, AD-3/AD-13): agent proposals wait here as
  // diff cards — nothing changes until a human merges. The basis-stale
  // variant carries the bilingual warning banner and the force-approve
  // secondary action; decided proposals stay visible in the history view
  // with their chips + receipt stamps (mono seq/time) — nothing disappears.
  // Read-model only (AD-8): every decision re-folds via `load`, then
  // `ondecided` re-folds the board (a merge is what applies the change).
  let { ondecided }: { ondecided: () => void } = $props();

  let proposals = $state<Proposal[] | null>(null);
  let loadError = $state("");
  let historyOpen = $state(false);
  let actingOn = $state<string | null>(null);
  let actionError = $state("");

  // DESIGN.md lifecycle tokens (same values as the hypothesis cards).
  const lifecycle: Record<HypothesisStatus, { ink: string; soft: string }> = {
    proposed: { ink: "#334155", soft: "#F1F5F9" },
    testing: { ink: "#0369A1", soft: "#F0F9FF" },
    supported: { ink: "#047857", soft: "#ECFDF5" },
    refuted: { ink: "#BE123C", soft: "#FFF1F2" },
    revised: { ink: "#B45309", soft: "#FFFBEB" },
  };

  // DESIGN.md quarantine tokens: per-status ink + soft background.
  const statusTokens: Record<ProposalStatus, { ink: string; soft: string }> = {
    pending: { ink: "#334155", soft: "#F1F5F9" },
    merged: { ink: "#047857", soft: "#ECFDF5" },
    rejected: { ink: "#BE123C", soft: "#FFF1F2" },
    superseded: { ink: "#71717A", soft: "#F4F4F6" },
    voided: { ink: "#71717A", soft: "#F4F4F6" },
  };

  const pending = $derived(
    (proposals ?? []).filter((p) => p.status === "pending").sort((a, b) => b.seq - a.seq),
  );
  const decided = $derived(
    (proposals ?? []).filter((p) => p.status !== "pending").sort((a, b) => (b.decided?.seq ?? 0) - (a.decided?.seq ?? 0)),
  );

  $effect(() => {
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
           receipt stamps — nothing disappears. -->
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
              {#if proposal.status === "merged" && proposal.basisStale}
                <p class="q-stale q-stale-marker">
                  <span class="q-stale-icon" aria-hidden="true">⚠</span>
                  {t("quarantine.mergedStale")}
                </p>
              {/if}
              {#if proposal.status === "superseded"}
                <p class="q-note">{t("quarantine.supersededNote")}</p>
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
    border-radius: 6px;
    padding: 2px 6px;
  }
  .q-history-btn {
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
    border-radius: 10px;
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: var(--rc-surface);
  }
  .q-card.stale {
    border-color: #b45309;
    background: #fffbeb;
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
  /* The basis-stale warning banner (AD-13) — amber, never silent. */
  .q-stale {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    font-weight: 500;
    line-height: 1.45;
    color: #92400e;
    background: #fffbeb;
    border: 1px solid #b45309;
    border-radius: 8px;
    padding: 8px 10px;
  }
  .q-card.stale .q-stale {
    background: #ffffff;
  }
  .q-stale-icon {
    font-size: 13px;
  }
  .q-stale-seq {
    font-size: 11px;
    color: #92400e;
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
    border-radius: 8px;
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
    color: #ffffff;
    border: 1px solid var(--rc-accent);
  }
  .btn-approve:hover:not(:disabled) {
    transform: translateY(-2px);
    box-shadow: 0 4px 12px -2px rgba(48, 113, 181, 0.35);
  }
  .btn-force {
    background: #ffffff;
    color: #92400e;
    border: 1px solid #b45309;
  }
  .btn-force:hover:not(:disabled) {
    background: #fffbeb;
    transform: translateY(-2px);
  }
  .btn-reject {
    margin-right: auto;
    background: var(--rc-surface);
    color: var(--rc-danger-ink);
    border: 1px solid var(--rc-border);
  }
  .btn-reject:hover:not(:disabled) {
    background: #fff1f2;
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
