<script lang="ts">
  import type { ComputeTargetView, Job, JobSpec, Mission, MissionRun, FetchedJobResults } from "../../types";
  import { t } from "../../i18n";
  import { api } from "../../api";
  import HypothesesBoard from "./HypothesesBoard.svelte";
  import SearchDisclosure from "./SearchDisclosure.svelte";

  let {
    mission,
    onopenreceipt = () => {},
  }: { mission: Mission; onopenreceipt?: (runId: string) => void } = $props();

  // Runs drill-down (FR-1.3): loaded lazily on first expand; the full
  // receipts timeline is a later story — this is the basic run list.
  let runsOpen = $state(false);
  let runs = $state<MissionRun[] | null>(null);
  let runsError = $state("");

  // Compute jobs (Story 3.2, FR-11.1/11.2/11.4): the target row + the
  // typed spec composer + the queued/running/terminal monitor. The spec
  // is COMPOSED from typed fields with a live JSON preview — never a
  // freeform shell box (EXPERIENCE.md, AD-6); the core rejects shell
  // syntax before submit regardless.
  let jobsOpen = $state(false);
  let targets = $state<ComputeTargetView[] | null>(null);
  let selectedTarget = $state("");
  let jobs = $state<Job[]>([]);
  let jobsError = $state("");
  // composer fields — each typed, each named (inline validation, EXPERIENCE.md)
  let specCmd = $state("");
  let specArgs = $state("");
  let specEnv = $state("");
  let specWorkdir = $state("");
  let specCpus = $state("");
  let specMemory = $state("");
  let submitting = $state(false);
  let submitError = $state("");
  // Fetched results per job id (Story 3.4, FR-11.5): the fetch lands the
  // artifacts as QUARANTINED pin-candidate proposals — the note names the
  // count, the captured output previews below. Idempotent: a second fetch
  // appends nothing.
  let fetched = $state<Record<string, FetchedJobResults>>({});
  let fetching = $state<string | null>(null);

  // The spec as typed JSON — composed from the fields above, never typed
  // as text. This is exactly what submit_job receives and validates.
  const draftSpec = $derived.by(() => {
    const env: Record<string, string> = {};
    for (const line of specEnv.split("\n")) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      const eq = trimmed.indexOf("=");
      if (eq > 0) env[trimmed.slice(0, eq).trim()] = trimmed.slice(eq + 1).trim();
    }
    const spec: JobSpec = {
      cmd: specCmd.trim(),
      args: specArgs.trim() ? specArgs.trim().split(/\s+/) : [],
      env,
    };
    if (specCpus.trim() || specMemory.trim()) {
      spec.resources = {
        ...(specCpus.trim() ? { cpus: Number(specCpus) } : {}),
        ...(specMemory.trim() ? { memoryMb: Number(specMemory) } : {}),
      };
    }
    if (specWorkdir.trim()) spec.workdir = specWorkdir.trim();
    return spec;
  });

  // Phase chips: the queued/running/terminal lifecycle (FR-11.4) — text
  // always (never color alone), terminal states carry ts + reason below.
  const phaseColor: Record<Job["phase"], string> = {
    queued: "#71717a",
    running: "#3071b5",
    finished: "#047857",
    failed: "#be123c",
  };

  async function toggleJobs() {
    jobsOpen = !jobsOpen;
    if (!jobsOpen) return;
    if (targets === null) {
      try {
        targets = await api.listComputeTargets();
        if (!selectedTarget) selectedTarget = targets[0]?.name ?? "local";
      } catch (e) {
        jobsError = t("missions.jobs.loadError") + e;
      }
    }
    try {
      jobs = await api.pollJobs(mission.id);
      jobsError = "";
    } catch (e) {
      jobsError = t("missions.jobs.loadError") + e;
    }
  }

  // The monitor loop, per surface: while the jobs area is open and any
  // job is live, poll once per 1.5s — each poll appends the observed
  // transitions (the runtime's poll loop, invoked per command).
  $effect(() => {
    if (!jobsOpen) return;
    const anyLive = jobs.some((j) => j.phase === "queued" || j.phase === "running");
    if (!anyLive) return;
    const timer = setInterval(async () => {
      try {
        jobs = await api.pollJobs(mission.id);
      } catch {
        /* a failed poll keeps the last observed state — the next one retries */
      }
    }, 1500);
    return () => clearInterval(timer);
  });

  async function submitJob() {
    if (!draftSpec.cmd || submitting) return;
    submitting = true;
    submitError = "";
    try {
      const job = await api.submitJob(mission.id, selectedTarget, draftSpec);
      jobs = [job, ...jobs.filter((j) => j.id !== job.id)];
    } catch (e) {
      submitError = String(e);
    } finally {
      submitting = false;
    }
  }

  // The fetch affordance (Story 3.4): fetching a finished job's results
  // sends each meaningful artifact to quarantine as a numerical pin
  // candidate — the proposals appear in Pending review; approving pins.
  async function fetchResults(job: Job) {
    if (fetched[job.id] || fetching) return;
    fetching = job.id;
    submitError = "";
    try {
      fetched[job.id] = await api.fetchJobResults(job.id);
    } catch (e) {
      submitError = t("missions.jobs.fetchError") + e;
    } finally {
      fetching = null;
    }
  }


  // Hypothesis board drill-down (Story 1.5): the mission's hypothesis
  // cards — lifecycle chips, relation chips, audit stamps. The full
  // board-at-a-glance surface is Story 1.8; this section suffices here.
  let boardOpen = $state(false);

  // Search disclosure drill-down (Story 4.1, FR-12.1): the mission's
  // PRISMA-style Divulgación — every search the runs and the user
  // performed, null results visibly marked. The section self-loads on
  // first open; read-model only.
  let disclosureOpen = $state(false);

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

  function toggleDisclosure() {
    disclosureOpen = !disclosureOpen;
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
    <button class="mc-runs-toggle" type="button" onclick={toggleJobs} aria-expanded={jobsOpen}>
      {jobsOpen ? t("missions.jobs.hide") : t("missions.jobs.show")}
    </button>
    <button
      class="mc-runs-toggle"
      type="button"
      onclick={toggleDisclosure}
      aria-expanded={disclosureOpen}
    >
      {disclosureOpen ? t("missions.disclosure.hide") : t("missions.disclosure.show")}
    </button>
  </footer>

  {#if boardOpen}
    <HypothesesBoard {mission} />
  {/if}

  {#if disclosureOpen}
    <SearchDisclosure missionId={mission.id} />
  {/if}

  {#if jobsOpen}
    <!-- Compute jobs area (Story 3.2, FR-11.1/11.2/11.4): the target row,
         the typed spec composer (structured JSON preview ONLY — never a
         freeform shell box, AD-6), and the queued/running/terminal monitor
         with timestamped, reasoned terminals (EXPERIENCE.md). -->
    <div class="mc-jobs">
      {#if jobsError}
        <p class="mc-jobs-error" role="alert">{jobsError}</p>
      {/if}

      <!-- Target row: every known compute target as a name chip + its kind
           (Local in v1; SSH registers in Story 3.3); the selected target is
           where the composed spec submits. -->
      <div class="mc-target-row" role="group" aria-label={t("missions.jobs.target")}>
        <span class="mc-target-label">{t("missions.jobs.target")}</span>
        {#each targets ?? [] as target (target.name)}
          <button
            class="mc-target-chip mono"
            class:selected={target.name === selectedTarget}
            type="button"
            onclick={() => (selectedTarget = target.name)}
            aria-pressed={target.name === selectedTarget}
          >
            {target.name}
            <span class="mc-target-kind">
              {target.kind === "local" ? "Local" : target.kind.toUpperCase()}
              {#if target.kind === "ssh" && target.host}· {target.host}{/if}
            </span>
          </button>
        {/each}
      </div>

      <!-- Job spec composer: typed fields ONLY (EXPERIENCE.md) — cmd is one
           executable, arguments are a list, env is KEY=VALUE lines. The
           preview is composed from these fields, never typed as text. -->
      <div class="mc-composer">
        <div class="mc-composer-fields">
          <label class="mc-field mc-field-cmd">
            <span>{t("missions.jobs.cmd")}</span>
            <input
              class="mono"
              type="text"
              bind:value={specCmd}
              placeholder={t("missions.jobs.cmdPh")}
              aria-invalid={submitError.includes("freeform_shell")}
            />
          </label>
          <label class="mc-field mc-field-args">
            <span>{t("missions.jobs.args")}</span>
            <input
              class="mono"
              type="text"
              bind:value={specArgs}
              placeholder="train.py --epochs 10"
            />
            <small class="mc-field-hint">{t("missions.jobs.argsHint")}</small>
          </label>
          <label class="mc-field mc-field-env">
            <span>{t("missions.jobs.env")}</span>
            <textarea class="mono" rows="2" bind:value={specEnv} placeholder="EPOCHS=10&#10;LR=3e-4"></textarea>
            <small class="mc-field-hint">{t("missions.jobs.envHint")}</small>
          </label>
          <label class="mc-field">
            <span>{t("missions.jobs.workdir")}</span>
            <input class="mono" type="text" bind:value={specWorkdir} placeholder="/tmp/experiment" />
          </label>
          <label class="mc-field mc-field-narrow">
            <span>{t("missions.jobs.cpus")}</span>
            <input class="mono" type="number" min="1" bind:value={specCpus} placeholder="4" />
          </label>
          <label class="mc-field mc-field-narrow">
            <span>{t("missions.jobs.memory")}</span>
            <input class="mono" type="number" min="1" bind:value={specMemory} placeholder="2048" />
          </label>
        </div>
        <div class="mc-composer-preview">
          <span class="mc-preview-label">{t("missions.jobs.preview")}</span>
          <pre class="mono">{JSON.stringify(draftSpec, null, 2)}</pre>
        </div>
      </div>
      <div class="mc-composer-actions">
        <button
          class="mc-submit"
          type="button"
          onclick={submitJob}
          disabled={!draftSpec.cmd || !selectedTarget || submitting}
        >
          {submitting ? t("missions.jobs.submitting") : t("missions.jobs.submit")}
        </button>
        {#if submitError}
          <p class="mc-jobs-error" role="alert">{submitError}</p>
        {/if}
      </div>

      <!-- Job monitor: queued / running / terminal per job — every terminal
           carries its timestamp and (on failure) its reason; no job ends
           silently (AD-12). Fetch results is the explicit action (the
           quarantine flow for results is Story 3.4). -->
      {#if jobs.length === 0}
        <p class="mc-jobs-empty">{t("missions.jobs.empty")}</p>
      {:else}
        <ul class="mc-job-list">
          {#each jobs as job (job.id)}
            <li class="mc-job" class:live={job.phase === "queued" || job.phase === "running"}>
              <div class="mc-job-head">
                <span
                  class="mc-job-chip"
                  style={`--ink:${phaseColor[job.phase]}`}
                >{t(`missions.jobs.phase.${job.phase}`)}</span>
                <span class="mc-job-cmd mono">{job.spec.cmd} {job.spec.args.join(" ")}</span>
                <span class="mc-job-target mono">{job.target}</span>
                <span class="mc-job-ts mono">
                  {t("missions.jobs.submittedAt")} {new Date(job.ts).toLocaleTimeString()}
                  {#if job.runningTs}
                    · {t("missions.jobs.runningAt")} {new Date(job.runningTs).toLocaleTimeString()}
                  {/if}
                  {#if job.finishedTs}
                    · {t("missions.jobs.finishedAt")} {new Date(job.finishedTs).toLocaleTimeString()}
                  {/if}
                </span>
                {#if job.exitCode != null}
                  <span class="mc-job-code mono">{t("missions.jobs.exit")} {job.exitCode}</span>
                {/if}
                {#if job.phase === "finished"}
                  <!-- The fetch affordance (Story 3.4, FR-11.5): fetching
                       lands the artifacts as quarantined pin-candidate
                       proposals — a failed job keeps its stamped reason. -->
                  <button
                    class="mc-job-fetch mono"
                    type="button"
                    disabled={fetching === job.id}
                    onclick={() => fetchResults(job)}
                  >
                    {fetching === job.id ? "…" : t("missions.jobs.fetch")} →
                  </button>
                {/if}
              </div>
              {#if job.reason}
                <p class="mc-job-reason mono">{t("missions.jobs.reason")}: {job.reason}</p>
              {/if}
              {#if fetched[job.id]}
                <div class="mc-job-results">
                  <span class="mc-results-label">{t("missions.jobs.results")}</span>
                  {#if fetched[job.id].proposals.length > 0}
                    <!-- The quarantine note: the artifacts left as pin
                         candidates, awaiting review (AD-3 — nothing pins
                         without the human's merge). -->
                    <p class="mc-quarantine-note">
                      {t("missions.jobs.quarantined", { count: fetched[job.id].proposals.length })}
                    </p>
                  {/if}
                  {#if fetched[job.id].results.stdout}
                    <div class="mc-result-block">
                      <span class="mono">{t("missions.jobs.stdout")}</span>
                      <pre class="mono">{fetched[job.id].results.stdout}</pre>
                    </div>
                  {/if}
                  {#if fetched[job.id].results.stderr}
                    <div class="mc-result-block">
                      <span class="mono">{t("missions.jobs.stderr")}</span>
                      <pre class="mono">{fetched[job.id].results.stderr}</pre>
                    </div>
                  {/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
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

  /* Compute jobs area (Story 3.2, FR-11.1/11.2/11.4): target row, typed
     spec composer with a live JSON preview, and the job monitor — chip
     anatomy per DESIGN.md components.badge (soft bg + ink + 20% ring). */
  .mc-jobs {
    border-top: 1px solid var(--rc-border);
    padding-top: 12px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .mc-jobs-empty,
  .mc-jobs-error {
    margin: 0;
    font-size: 12.5px;
    color: var(--rc-ink-muted);
  }
  .mc-jobs-error {
    color: var(--rc-danger-ink);
    word-break: break-word;
  }
  .mc-target-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .mc-target-label {
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
  }
  .mc-target-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--rc-ink);
    background: var(--rc-surface-2);
    border: 1px solid transparent;
    border-radius: 9999px;
    padding: 3px 10px;
    cursor: pointer;
    transition: border-color 0.15s ease, background 0.15s ease;
  }
  .mc-target-chip:hover {
    border-color: var(--rc-accent);
  }
  .mc-target-chip:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }
  .mc-target-chip.selected {
    background: var(--rc-accent-soft);
    border-color: var(--rc-accent);
  }
  .mc-target-kind {
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
  }
  .mc-target-chip.selected .mc-target-kind {
    color: var(--rc-accent);
  }

  /* Composer: typed fields left, structured JSON preview right — the
     preview is composed from the fields, never typed as text (AD-6). */
  .mc-composer {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
    background: var(--rc-surface-2);
    border: 1px solid var(--rc-border);
    border-radius: 10px;
    padding: 12px;
  }
  .mc-composer-fields {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .mc-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .mc-field span {
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
  }
  .mc-field input,
  .mc-field textarea {
    font-size: 12.5px;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 8px;
    padding: 6px 10px;
    outline: none;
    min-height: 32px;
    width: 100%;
    resize: vertical;
  }
  .mc-field input:focus-visible,
  .mc-field textarea:focus-visible {
    border-color: var(--rc-accent);
  }
  .mc-field input[aria-invalid="true"] {
    border-color: var(--rc-danger-ink);
  }
  .mc-field-cmd,
  .mc-field-args,
  .mc-field-env {
    grid-column: 1 / -1;
  }
  .mc-field-narrow input {
    min-height: 32px;
  }
  .mc-field-hint {
    font-size: 11px;
    color: var(--rc-ink-muted);
  }
  .mc-composer-preview {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .mc-preview-label {
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
  }
  .mc-composer-preview pre {
    margin: 0;
    font-size: 11.5px;
    line-height: 1.55;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: 8px;
    padding: 10px;
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-word;
    flex: 1;
  }
  .mc-composer-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .mc-submit {
    font-family: inherit;
    font-size: 12.5px;
    font-weight: 500;
    color: #ffffff;
    background: var(--rc-accent);
    border: 1px solid var(--rc-accent);
    border-radius: 8px;
    padding: 6px 14px;
    cursor: pointer;
    transition: opacity 0.15s ease;
  }
  .mc-submit:hover:not(:disabled) {
    opacity: 0.9;
  }
  .mc-submit:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .mc-submit:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }

  /* Job monitor rows: phase chip + argv in mono + stamps; live rows get a
     quiet accent edge so motion is findable without color alone. */
  .mc-job-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .mc-job {
    border: 1px solid var(--rc-border);
    border-radius: 10px;
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .mc-job.live {
    border-color: color-mix(in srgb, var(--rc-accent) 35%, var(--rc-border));
  }
  .mc-job-head {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .mc-job-chip {
    font-size: 11.5px;
    font-weight: 500;
    color: var(--ink);
    background: color-mix(in srgb, var(--ink) 10%, transparent);
    border-radius: 9999px;
    padding: 2px 10px;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--ink) 20%, transparent);
  }
  .mc-job-cmd {
    font-size: 12px;
    color: var(--rc-ink);
    word-break: break-all;
  }
  .mc-job-target {
    font-size: 11px;
    color: var(--rc-ink-muted);
  }
  .mc-job-ts {
    font-size: 11px;
    color: var(--rc-ink-muted);
    font-variant-numeric: tabular-nums;
  }
  .mc-job-code {
    font-size: 11px;
    color: var(--rc-ink-muted);
    font-variant-numeric: tabular-nums;
  }
  .mc-job-fetch {
    font-size: 11.5px;
    font-weight: 500;
    color: var(--rc-accent);
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    white-space: nowrap;
  }
  .mc-job-fetch:hover {
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  .mc-job-fetch:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }
  .mc-job-reason {
    margin: 0;
    font-size: 11.5px;
    color: var(--rc-danger-ink);
    word-break: break-word;
  }
  .mc-job-results {
    display: flex;
    flex-direction: column;
    gap: 6px;
    border-top: 1px dashed var(--rc-border);
    padding-top: 6px;
  }
  /* The quarantine note (Story 3.4): where the artifacts went — pin
     candidates waiting for the human's merge (AD-3). */
  .mc-quarantine-note {
    margin: 0;
    font-size: 12px;
    line-height: 1.45;
    color: var(--rc-accent);
    background: var(--rc-accent-soft);
    border-radius: 6px;
    padding: 5px 8px;
  }
  .mc-results-label {
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
  }
  .mc-result-block {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .mc-result-block > span {
    font-size: 10.5px;
    color: var(--rc-ink-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .mc-result-block pre {
    margin: 0;
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--rc-ink);
    background: var(--rc-surface-2);
    border-radius: 8px;
    padding: 8px 10px;
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-word;
  }

  @media (max-width: 640px) {
    .mc-run {
      grid-template-columns: auto 1fr;
    }
    .mc-run-ts {
      grid-column: 2;
    }
    .mc-composer {
      grid-template-columns: 1fr;
    }
  }
</style>
