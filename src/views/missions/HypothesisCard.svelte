<script lang="ts">
  import { api } from "../../api";
  import { t } from "../../i18n";
  import type { Claim, Hypothesis, HypothesisStatus, PinKind, PinVerification, RelationChip, RelationKind, Ref } from "../../types";

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
    proposed: { ink: "var(--lifecycle-proposed-tx)", soft: "color-mix(in oklch, var(--lifecycle-proposed) 14%, var(--surface))" },
    testing: { ink: "var(--lifecycle-testing-tx)", soft: "color-mix(in oklch, var(--lifecycle-testing) 14%, var(--surface))" },
    supported: { ink: "var(--lifecycle-supported-tx)", soft: "color-mix(in oklch, var(--lifecycle-supported) 14%, var(--surface))" },
    refuted: { ink: "var(--lifecycle-refuted-tx)", soft: "color-mix(in oklch, var(--lifecycle-refuted) 14%, var(--surface))" },
    revised: { ink: "var(--lifecycle-revised-tx)", soft: "color-mix(in oklch, var(--lifecycle-revised) 14%, var(--surface))" },
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

  // ---- Evidence pins (FR-3, Story 1.7) ----
  // Read-model only: `claims` holds exactly what `list_evidence` folded
  // (pinned + unpinned, FR-3.4); every mutation re-reads from the log.

  let claims = $state<Claim[]>([]);
  let evidenceError = $state("");

  // Register-claim control: attach an AI output fragment as a claim.
  let claimText = $state("");
  let claiming = $state(false);
  let claimError = $state("");
  let claimSubmitted = $state(false);

  // Pin control (FR-3.1 citation, FR-3.3 numerical): pick the source (a
  // library ref or an artifact), confirm the pinned content, state the
  // agent-assessed confidence + assessing model.
  let pinningClaim = $state<{ id: string; kind: PinKind } | null>(null);
  let refs = $state<Ref[]>([]);
  let refsLoaded = $state(false);
  let pinRefId = $state("");
  let pinArtifactRef = $state("");
  let pinExcerpt = $state("");
  let pinConfidence = $state("0.8");
  let pinModel = $state("");
  let pinning = $state(false);
  let pinError = $state("");
  let pinSubmitted = $state(false);

  const claimTextMissing = $derived(claimText.trim() === "");
  const canClaim = $derived(!claimTextMissing && !claiming);
  const unpinnedCount = $derived(claims.filter((c) => !c.pinned).length);
  const pinConfidenceNum = $derived(Number(pinConfidence));
  const pinValid = $derived(
    (pinningClaim?.kind === "citation" ? pinRefId !== "" : pinArtifactRef.trim() !== "") &&
      pinExcerpt.trim() !== "" &&
      pinModel.trim() !== "" &&
      !Number.isNaN(pinConfidenceNum) &&
      pinConfidenceNum >= 0 &&
      pinConfidenceNum <= 1,
  );

  $effect(() => {
    loadEvidence();
    // Prefill the assessing model from the configured provider model —
    // attribution with a sensible default, still editable.
    api.getSettings().then((s) => {
      if (!pinModel && s.model) pinModel = s.model;
    }).catch(() => {});
  });

  async function loadEvidence() {
    try {
      claims = await api.listEvidence(hypothesis.id);
      evidenceError = "";
    } catch (e) {
      evidenceError = t("ev.loadError") + e;
    }
  }

  async function addClaim() {
    claimSubmitted = true;
    if (!canClaim) return;
    claiming = true;
    claimError = "";
    try {
      await api.registerClaim(hypothesis.id, claimText.trim(), null);
      claimText = "";
      claimSubmitted = false;
      await loadEvidence();
    } catch (e) {
      claimError = t("ev.claimError") + e;
    } finally {
      claiming = false;
    }
  }

  /** Open the pin form for one claim: `kind` picks the anatomy (citation:
   *  a library ref; numerical: an artifact ref). The content prefills
   *  with the claim text — the user confirms or edits it. Citation pins
   *  load the library refs (once). */
  async function openPin(claim: Claim, kind: PinKind) {
    pinningClaim = { id: claim.id, kind };
    pinRefId = "";
    pinArtifactRef = "";
    pinExcerpt = claim.text;
    pinError = "";
    pinSubmitted = false;
    if (kind === "citation" && !refsLoaded) {
      try {
        const project = await api.getActiveProject();
        refs = await api.listRefs(project.id);
        refsLoaded = true;
      } catch (e) {
        pinError = t("ev.loadError") + e;
      }
    }
  }

  async function pin() {
    if (!pinningClaim) return;
    pinSubmitted = true;
    if (!pinValid) return;
    pinning = true;
    pinError = "";
    try {
      if (pinningClaim.kind === "citation") {
        await api.pinClaimToCitation(
          pinningClaim.id,
          hypothesis.id,
          pinRefId,
          pinExcerpt,
          pinConfidenceNum,
          pinModel.trim(),
        );
      } else {
        await api.pinClaimToNumerical(
          pinningClaim.id,
          hypothesis.id,
          pinArtifactRef.trim(),
          pinExcerpt,
          pinConfidenceNum,
          pinModel.trim(),
        );
      }
      pinningClaim = null;
      await loadEvidence();
    } catch (e) {
      pinError = t("ev.pinError") + e;
    } finally {
      pinning = false;
    }
  }

  /** The confidence dot's color (DESIGN.md citation-pin anatomy): green at
   *  0.75+, amber 0.5–0.75, red below — never a "verified" label. */
  function confidenceColor(c: number): string {
    if (c >= 0.75) return "var(--st-read)";
    if (c >= 0.5) return "var(--st-reading)";
    return "var(--destructive)";
  }

  // ---- Pin verification (FR-14.1, Story 4.2) ----
  // The machine axis: `run_pin_verification` re-checks every pin of this
  // hypothesis against its source with NO LLM call; the read model then
  // carries the latest status on each pin. Read-model only (AD-8): the
  // action invokes the command and re-reads from the log. Failures mark
  // the pin visibly and never delete it — re-verify is always offered.

  let verifying = $state(false);

  async function verifyPins() {
    verifying = true;
    try {
      await api.runPinVerification(hypothesis.id);
      evidenceError = "";
      await loadEvidence();
    } catch (e) {
      evidenceError = t("ev.verifyError") + e;
    } finally {
      verifying = false;
    }
  }

  /** The verification chip's label — the machine axis's own vocabulary,
   *  never the confidence's: "verified by code" is existence by code, not
   *  a model's judgment (FR-3.6 separation). */
  function verifLabel(v: PinVerification): string {
    if (v.status === "verified") return t("ev.verified");
    if (v.status === "failed") return t("ev.verificationFailed");
    return t("ev.stale");
  }

  /** The chip's tooltip: the label always renders (status is never color
   *  alone); the tooltip adds the machine detail code + the consulted
   *  source — code form, bilingual-safe. */
  function verifTitle(v: PinVerification | null): string {
    if (!v) return t("ev.unverified");
    return `${verifLabel(v)} · ${v.detail} · ${t("ev.verifSource")}${v.source}`;
  }

  const pct = (c: number) => `${Math.round(c * 100)}%`;
</script>

<article class="hyp-card">
  <header class="hc-head">
    <span class="hc-id mono">{shortId}</span>
    <span class="hc-head-chips">
      <!-- Card-level unpinned flag (FR-3.4): amber chip whenever this
           hypothesis carries claims without evidence pins. -->
      {#if unpinnedCount > 0}
        <span class="hc-chip hc-unpinned-card" title={t("ev.unpinned")}>
          {unpinnedCount} · {t("ev.unpinned")}
        </span>
      {/if}
      <!-- Lifecycle chip (DESIGN.md components.lifecycle-chip): pill, soft
           background + ink text + 20% ring inset — never color alone, the
           label always renders. -->
      <span
        class="hc-chip"
        style={`--ink:${tokens.ink};--soft:${tokens.soft}`}
      >
        {statusLabel}
      </span>
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

  <!-- Evidence claims (FR-3, Story 1.7): AI-generated claims with inline
       pin state. Pinned claims render the citation pin anatomy (source
       icon + author-year + confidence dot labeled with the assessing
       model); unpinned claims render the amber chip (FR-3.4) wherever
       they appear — claim level and card level. -->
  <section class="hc-claims" aria-label={t("ev.claims")}>
    <p class="hc-claims-label">{t("ev.claims")}</p>
    {#if evidenceError}
      <p class="hc-error" role="alert">{evidenceError}</p>
    {:else if claims.length === 0}
      <p class="hc-claims-empty">{t("ev.empty")}</p>
    {:else}
      {#each claims as claim (claim.id)}
        <div class="hc-claim">
          <p class="hc-claim-text">{claim.text}</p>
          {#if claim.pinned && claim.pin}
            {@const pin = claim.pin}
            {#if pin.kind === "numerical"}
              <!-- Numerical pin (FR-3.3, DESIGN.md numerical-pin anatomy):
                   chart icon, artifact name, confidence dot labeled with
                   the assessing model, digest fragment in mono — the same
                   attribution rules as a citation pin (FR-3.6). -->
              <div class="hc-pin">
                <span class="hc-pin-main">
                  <svg class="hc-pin-icon" viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
                    <path
                      d="M2.5 13.5h11M3.5 13V8.5m3 4.5V5.5m3 7.5v-6m3 6V3.5"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="1.5"
                      stroke-linecap="round"
                    />
                  </svg>
                  <span class="hc-pin-pinned">{t("ev.pinnedToArtifact")}</span>
                  <span class="hc-pin-ref">{pin.artifactRef}</span>
                </span>
                <span
                  class="hc-pin-conf"
                  title={`${pin.assessingModel} · ${pct(pin.confidence)}`}
                >
                  <span
                    class="hc-dot"
                    style={`background:${confidenceColor(pin.confidence)}`}
                    aria-hidden="true"
                  ></span>
                  {pin.assessingModel} · {pct(pin.confidence)}
                </span>
                <!-- The computed digest, in mono (AD-5): the pin is bound
                     to exactly the content it anchors. -->
                <span class="hc-pin-digest mono" title={pin.digest}>
                  {t("ev.digest")} {pin.digest}
                </span>
                <!-- Verification chip (FR-14.1, Story 4.2): the machine
                     axis — label + icon, never color alone; a separate
                     field from the confidence dot above ("verified" never
                     drives the model's assessment). null = unverified. -->
                <span
                  class="hc-verif hc-verif--{pin.verification?.status ?? 'unverified'}"
                  title={verifTitle(pin.verification)}
                >
                  {#if pin.verification?.status === "verified"}
                    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
                      <path
                        d="M3 8.5 6.5 12 13 4.5"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                      />
                    </svg>
                  {:else if pin.verification?.status === "failed"}
                    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
                      <path
                        d="M4 4l8 8M12 4l-8 8"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                      />
                    </svg>
                  {:else}
                    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
                      <circle cx="8" cy="8" r="5.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-dasharray="2 2" />
                    </svg>
                  {/if}
                  {pin.verification ? verifLabel(pin.verification) : t("ev.unverified")}
                </span>
                <!-- The re-verifiable affordance: every pin can be
                     (re-)verified by code — a stale or failed result is
                     never a dead end. -->
                <button
                  class="hc-verif-btn"
                  type="button"
                  onclick={verifyPins}
                  disabled={verifying}
                >
                  {verifying ? t("ev.verifying") : pin.verification ? t("ev.reverify") : t("ev.verify")}
                </button>
              </div>
              <blockquote class="hc-excerpt" title={t("ev.contentQuoted")}>
                {pin.excerpt}
              </blockquote>
            {:else}
              <!-- Citation pin (DESIGN.md citation-pin anatomy): source icon,
                   author-year, confidence dot labeled with the assessing
                   model — "GLM-5.3 · 82%", never "verified" (FR-3.6). -->
              <div class="hc-pin">
                <span class="hc-pin-main">
                  <svg class="hc-pin-icon" viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
                    <path
                      d="M3 2.5A1.5 1.5 0 0 0 1.5 4v3A1.5 1.5 0 0 0 3 8.5h1.5a4.5 4.5 0 0 1-3 4.16V14c3.9-.35 6.5-3.3 6.5-7.5V4A1.5 1.5 0 0 0 6.5 2.5H3Zm8.5 0A1.5 1.5 0 0 0 10 4v3a1.5 1.5 0 0 0 1.5 1.5H13a4.5 4.5 0 0 1-3 4.16V14c3.9-.35 6.5-3.3 6.5-7.5V4A1.5 1.5 0 0 0 15 2.5h-3.5Z"
                      fill="currentColor"
                    />
                  </svg>
                  <span class="hc-pin-pinned">{t("ev.pinnedTo")}</span>
                  <span class="hc-pin-ref">{pin.refLabel ?? pin.refId}</span>
                </span>
                <span
                  class="hc-pin-conf"
                  title={`${pin.assessingModel} · ${pct(pin.confidence)}`}
                >
                  <span
                    class="hc-dot"
                    style={`background:${confidenceColor(pin.confidence)}`}
                    aria-hidden="true"
                  ></span>
                  {pin.assessingModel} · {pct(pin.confidence)}
                </span>
                <!-- The computed digest, in mono (AD-5): the pin is bound to
                     exactly the excerpt it quotes. -->
                <span class="hc-pin-digest mono" title={pin.digest}>
                  {t("ev.digest")} {pin.digest}
                </span>
                <!-- Verification chip (FR-14.1, Story 4.2): the machine
                     axis — same anatomy as the numerical pin above; a
                     separate field from the confidence dot ("verified"
                     never drives the model's assessment). -->
                <span
                  class="hc-verif hc-verif--{pin.verification?.status ?? 'unverified'}"
                  title={verifTitle(pin.verification)}
                >
                  {#if pin.verification?.status === "verified"}
                    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
                      <path
                        d="M3 8.5 6.5 12 13 4.5"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                      />
                    </svg>
                  {:else if pin.verification?.status === "failed"}
                    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
                      <path
                        d="M4 4l8 8M12 4l-8 8"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                      />
                    </svg>
                  {:else}
                    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
                      <circle cx="8" cy="8" r="5.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-dasharray="2 2" />
                    </svg>
                  {/if}
                  {pin.verification ? verifLabel(pin.verification) : t("ev.unverified")}
                </span>
                <!-- The re-verifiable affordance — same action as the
                     numerical pin. -->
                <button
                  class="hc-verif-btn"
                  type="button"
                  onclick={verifyPins}
                  disabled={verifying}
                >
                  {verifying ? t("ev.verifying") : pin.verification ? t("ev.reverify") : t("ev.verify")}
                </button>
              </div>
              <blockquote class="hc-excerpt" title={t("ev.excerptQuoted")}>
                {pin.excerpt}
              </blockquote>
            {/if}
          {:else}
            <div class="hc-claim-foot">
              <!-- Unpinned amber chip (FR-3.4): the claim carries no
                   evidence pin — flagged, never hidden. -->
              <span class="hc-unpinned">{t("ev.unpinned")}</span>
              <span class="hc-pin-actions">
                <button
                  class="hc-pin-btn"
                  type="button"
                  onclick={() => openPin(claim, "citation")}
                >
                  {t("ev.pin")}
                </button>
                <button
                  class="hc-pin-btn"
                  type="button"
                  onclick={() => openPin(claim, "numerical")}
                >
                  {t("ev.pinArtifact")}
                </button>
              </span>
            </div>
            {#if pinningClaim?.id === claim.id}
              <!-- Pin form (FR-3.1 citation / FR-3.3 numerical): pick the
                   source (library ref or artifact), confirm the pinned
                   content (prefilled with the claim text), state confidence
                   + assessing model. The digest is computed in the core —
                   never sent from here. -->
              <form
                class="hc-pin-form"
                onsubmit={(e) => {
                  e.preventDefault();
                  pin();
                }}
              >
                {#if pinningClaim.kind === "citation"}
                  <select
                    class="hc-select hc-pin-ref"
                    bind:value={pinRefId}
                    aria-label={t("ev.refPick")}
                  >
                    <option value="" disabled selected>{t("ev.refPick")}</option>
                    {#each refs as r (r.id)}
                      <option value={r.id}>{r.authors} {r.year} · {r.title}</option>
                    {/each}
                  </select>
                {:else}
                  <input
                    class="hc-artifact-input"
                    class:invalid={pinSubmitted && pinArtifactRef.trim() === ""}
                    type="text"
                    bind:value={pinArtifactRef}
                    placeholder={t("ev.artifactPh")}
                    aria-label={t("ev.artifact")}
                    aria-invalid={pinSubmitted && pinArtifactRef.trim() === ""}
                  />
                {/if}
                <textarea
                  class="hc-excerpt-input"
                  class:invalid={pinSubmitted && pinExcerpt.trim() === ""}
                  bind:value={pinExcerpt}
                  rows="3"
                  placeholder={t("ev.excerptPh")}
                  aria-label={t("ev.excerpt")}
                ></textarea>
                <p class="hc-pin-hint">{t("ev.excerptPrefill")}</p>
                <div class="hc-pin-row">
                  <input
                    class="hc-conf-input"
                    class:invalid={pinSubmitted && (Number.isNaN(pinConfidenceNum) || pinConfidenceNum < 0 || pinConfidenceNum > 1)}
                    type="number"
                    min="0"
                    max="1"
                    step="0.01"
                    bind:value={pinConfidence}
                    aria-label={t("ev.confidence")}
                  />
                  <input
                    class="hc-model-input"
                    class:invalid={pinSubmitted && pinModel.trim() === ""}
                    type="text"
                    bind:value={pinModel}
                    placeholder={t("ev.modelPh")}
                    aria-label={t("ev.model")}
                  />
                  <button
                    class="hc-pin-submit"
                    type="submit"
                    disabled={!pinValid || pinning}
                  >
                    {pinning ? t("ev.pinning") : t("ev.confirmExcerpt")}
                  </button>
                </div>
                {#if pinSubmitted && !pinValid}
                  <p class="hc-error" role="alert">{t("ev.required")}</p>
                {/if}
                {#if pinError}
                  <p class="hc-error" role="alert">{pinError}</p>
                {/if}
              </form>
            {/if}
          {/if}
        </div>
      {/each}
    {/if}
    <!-- Attach an AI output fragment as a claim (it registers unpinned). -->
    <div class="hc-claim-add">
      <input
        class="hc-claim-input"
        class:invalid={claimSubmitted && claimTextMissing}
        type="text"
        bind:value={claimText}
        placeholder={t("ev.addPh")}
        aria-label={t("ev.claims")}
        aria-invalid={claimSubmitted && claimTextMissing}
        onkeydown={(e) => { if (e.key === "Enter") addClaim(); }}
      />
      <button
        class="hc-pin-btn"
        type="button"
        onclick={addClaim}
        disabled={!canClaim}
      >
        {t("ev.add")}
      </button>
    </div>
    {#if claimSubmitted && claimTextMissing}
      <p class="hc-error" role="alert">{t("ev.claimRequired")}</p>
    {/if}
    {#if claimError}
      <p class="hc-error" role="alert">{claimError}</p>
    {/if}
  </section>

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
    --rc-surface: var(--surface);
    --rc-ink: var(--fg);
    --rc-ink-muted: var(--muted);
    --rc-border: var(--border);
    --rc-accent: var(--accent);
    --rc-accent-soft: var(--accent-soft);
    --rc-danger-ink: var(--destructive-tx);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: var(--r-card);
    box-shadow: var(--shadow-card);
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    transition: transform 0.18s ease, box-shadow 0.18s ease, border-color 0.18s ease;
  }
  .hyp-card:hover {
    transform: translateY(-2px);
    box-shadow: var(--shadow-pop);
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
    font-family: var(--font-body);
    font-size: 15px;
    font-weight: 600;
    line-height: 1.45;
    color: var(--rc-ink);
    margin: 0;
  }

  .hc-head-chips {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  /* Card-level unpinned chip (FR-3.4): amber — claims without pins are
     flagged, never hidden. */
  .hc-unpinned-card {
    --ink: var(--st-reading-tx);
    --soft: color-mix(in oklch, var(--st-reading) 14%, var(--surface));
  }

  /* Evidence claims: one bordered row per claim, pin state inline. */
  .hc-claims {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-top: 2px;
  }
  .hc-claims-label {
    margin: 0;
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--rc-ink-muted);
  }
  .hc-claims-empty {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--rc-ink-muted);
  }
  .hc-claim {
    border: 1px solid var(--rc-border);
    border-radius: var(--r-card);
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 7px;
    background: var(--rc-surface);
  }
  .hc-claim-text {
    margin: 0;
    font-size: 13.5px;
    line-height: 1.5;
    color: var(--rc-ink);
  }
  .hc-claim-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex-wrap: wrap;
  }
  /* Unpinned amber chip (FR-3.4, claim level). */
  .hc-unpinned {
    font-size: 11.5px;
    font-weight: 500;
    letter-spacing: 0.02em;
    color: var(--st-reading-tx);
    background: color-mix(in oklch, var(--st-reading) 14%, var(--surface));
    border-radius: 9999px;
    padding: 2px 10px;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--st-reading) 25%, transparent);
    white-space: nowrap;
  }

  /* Citation pin anatomy (DESIGN.md): source icon + author-year +
     confidence dot labeled with the assessing model, digest in mono. */
  .hc-pin {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 12px;
  }
  .hc-pin-main {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    font-weight: 500;
    color: var(--rc-ink);
  }
  .hc-pin-icon {
    color: var(--rc-accent);
    flex-shrink: 0;
  }
  .hc-pin-pinned {
    color: var(--rc-ink-muted);
  }
  .hc-pin-ref {
    color: var(--rc-accent);
  }
  .hc-pin-conf {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    font-weight: 500;
    color: var(--rc-ink);
    white-space: nowrap;
  }
  .hc-dot {
    width: 8px;
    height: 8px;
    border-radius: 9999px;
    display: inline-block;
    flex-shrink: 0;
  }
  .hc-pin-digest {
    font-size: 10.5px;
    letter-spacing: 0.02em;
    color: var(--rc-ink-muted);
    word-break: break-all;
    flex-basis: 100%;
  }
  .hc-excerpt {
    margin: 0;
    font-size: 12px;
    line-height: 1.55;
    color: var(--rc-ink-muted);
    border-left: 2px solid var(--rc-accent-soft);
    border-left-color: color-mix(in srgb, var(--accent) 35%, transparent);
    padding-left: 10px;
    white-space: pre-wrap;
  }

  /* Verification chip (FR-14.1, Story 4.2): the machine axis — pill with
     label + icon, per-status ink + soft background. Status never reads
     color alone: the label always renders. */
  .hc-verif {
    --ink: var(--muted);
    --soft: var(--surface-2);
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11.5px;
    font-weight: 500;
    letter-spacing: 0.02em;
    color: var(--ink);
    background: var(--soft);
    border-radius: 9999px;
    padding: 2px 9px;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--ink) 20%, transparent);
    white-space: nowrap;
  }
  .hc-verif svg {
    flex-shrink: 0;
  }
  .hc-verif--verified {
    --ink: var(--st-read-tx);
    --soft: color-mix(in oklch, var(--st-read) 14%, var(--surface));
  }
  .hc-verif--failed {
    --ink: var(--destructive-tx);
    --soft: color-mix(in oklch, var(--destructive) 6%, var(--surface));
  }
  .hc-verif--stale {
    --ink: var(--st-reading-tx);
    --soft: color-mix(in oklch, var(--st-reading) 14%, var(--surface));
  }
  .hc-verif-btn {
    font-family: inherit;
    font-size: 11.5px;
    font-weight: 500;
    color: var(--rc-accent);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: var(--r-input);
    padding: 3px 9px;
    min-height: 26px;
    cursor: pointer;
    transition: background 0.15s ease;
  }
  .hc-verif-btn:hover:not(:disabled) {
    background: var(--rc-accent-soft);
  }
  .hc-verif-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  /* Pin-to-citation form: ref picker, excerpt confirmation, confidence +
     assessing model. */
  .hc-pin-form {
    display: flex;
    flex-direction: column;
    gap: 7px;
    border-top: 1px dashed var(--rc-border);
    padding-top: 8px;
  }
  .hc-pin-ref {
    max-width: 100%;
    width: 100%;
  }
  .hc-excerpt-input {
    font-family: inherit;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: var(--r-input);
    padding: 7px 10px;
    outline: none;
    resize: vertical;
  }
  .hc-excerpt-input::placeholder,
  .hc-claim-input::placeholder {
    color: var(--rc-ink-muted);
  }
  .hc-excerpt-input.invalid,
  .hc-claim-input.invalid,
  .hc-conf-input.invalid,
  .hc-model-input.invalid,
  .hc-artifact-input.invalid {
    border-color: var(--rc-danger-ink);
    background: color-mix(in oklch, var(--destructive) 6%, var(--surface));
  }
  .hc-artifact-input {
    width: 100%;
    box-sizing: border-box;
    font-family: "JetBrains Mono", ui-monospace, monospace;
    font-size: 12.5px;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: var(--r-input);
    padding: 7px 10px;
    min-height: 34px;
    outline: none;
  }
  .hc-artifact-input::placeholder {
    color: var(--rc-ink-muted);
  }
  .hc-pin-hint {
    margin: 0;
    font-size: 11px;
    color: var(--rc-ink-muted);
  }
  .hc-pin-row {
    display: flex;
    flex-wrap: wrap;
    gap: 7px;
    align-items: center;
  }
  .hc-conf-input {
    width: 84px;
    font-family: inherit;
    font-size: 12.5px;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: var(--r-input);
    padding: 7px 9px;
    min-height: 34px;
    outline: none;
  }
  .hc-model-input {
    flex: 1 1 140px;
    min-width: 0;
    font-family: inherit;
    font-size: 12.5px;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: var(--r-input);
    padding: 7px 10px;
    min-height: 34px;
    outline: none;
  }
  .hc-pin-submit {
    font-family: inherit;
    font-size: 12.5px;
    font-weight: 500;
    color: var(--surface);
    background: var(--rc-accent);
    border: 1px solid var(--rc-accent);
    border-radius: var(--r-input);
    padding: 7px 12px;
    min-height: 34px;
    cursor: pointer;
    transition: background 0.15s ease;
  }
  .hc-pin-submit:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .hc-pin-submit:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  /* Pin / add-claim buttons: quiet secondary actions. */
  .hc-pin-actions {
    display: inline-flex;
    gap: 6px;
  }
  .hc-pin-btn {
    font-family: inherit;
    font-size: 12px;
    font-weight: 500;
    color: var(--rc-accent);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: var(--r-input);
    padding: 5px 11px;
    min-height: 30px;
    cursor: pointer;
    transition: background 0.15s ease;
  }
  .hc-pin-btn:hover:not(:disabled) {
    background: var(--rc-accent-soft);
  }
  .hc-pin-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .hc-claim-add {
    display: flex;
    gap: 7px;
  }
  .hc-claim-input {
    flex: 1;
    min-width: 0;
    font-family: inherit;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--rc-ink);
    background: var(--rc-surface);
    border: 1px solid var(--rc-border);
    border-radius: var(--r-input);
    padding: 7px 10px;
    outline: none;
    min-height: 32px;
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
    border-radius: var(--r-input);
    padding: 7px 10px;
    outline: none;
    min-height: 34px;
  }
  .hc-basis::placeholder {
    color: var(--rc-ink-muted);
  }
  .hc-basis.invalid {
    border-color: var(--rc-danger-ink);
    background: color-mix(in oklch, var(--destructive) 6%, var(--surface));
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
    border-radius: var(--r-input);
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
    border-radius: var(--r-input);
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
    border-radius: var(--r-input);
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
  select:focus-visible,
  textarea:focus-visible {
    outline: 2px solid var(--rc-accent);
    outline-offset: 2px;
  }
</style>
