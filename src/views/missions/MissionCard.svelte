<script lang="ts">
  import type { Mission } from "../../types";
  import { t } from "../../i18n";

  let { mission }: { mission: Mission } = $props();

  const ceiling = $derived(`$${(mission.spendCeilingCents / 100).toFixed(2)}`);
  const autonomyLabel = $derived(t(`missions.autonomy.${mission.autonomy}`));
  const created = $derived(new Date(mission.ts).toLocaleString());
</script>

<!-- Mission card (DESIGN.md components.mission-card): card anatomy, status
     kicker, question in heading-3, stop condition + success criterion, mono
     meta row. Status is never color alone — the kicker carries text. -->
<article class="mission-card">
  <header class="mc-head">
    <span class="mc-kicker">
      <span class="mc-dot" aria-hidden="true"></span>
      {t("missions.active")}
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
  <footer class="mc-meta mono">
    <span>{autonomyLabel}</span>
    <span aria-hidden="true">·</span>
    <span>{ceiling}</span>
    <span aria-hidden="true">·</span>
    <span>{created}</span>
  </footer>
</article>

<style>
  .mission-card {
    --rc-surface: #ffffff;
    --rc-ink: #151519;
    --rc-ink-muted: #71717a;
    --rc-border: #eaeaec;
    --rc-accent: #3071b5;
    --rc-accent-soft: rgba(48, 113, 181, 0.1);
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
  .mc-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .mc-kicker {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    font-size: 12px;
    font-weight: 500;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--rc-accent);
    background: var(--rc-accent-soft);
    border-radius: 9999px;
    padding: 2px 10px;
    box-shadow: inset 0 0 0 1px rgba(48, 113, 181, 0.2);
  }
  .mc-dot {
    width: 7px;
    height: 7px;
    border-radius: 9999px;
    background: var(--rc-accent);
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
  .mono {
    font-family: "JetBrains Mono", ui-monospace, monospace;
  }
</style>
