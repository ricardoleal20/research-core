# QA Manual — the guided UI walkthrough / Recorrido guiado de la interfaz

**Purpose / Propósito:** click through every screen of ResearchCore with the demo
workspace seeded, and record what looks wrong before deciding on fixes.
**Setup / Preparación:**

1. Run the app: `npm run dev` (browser mock) or `npm run tauri dev` (desktop, real core).
2. Seed the demo data: **Ajustes → Zona peligrosa → "Datos de ejemplo" → Poblar con datos de ejemplo**
   (or the subtle link under the empty missions home). You should see the green
   confirmation "generación 1, N eventos".
3. Keep this file open; paste a findings line under each section as you go.

> Copy this line under each section you test:
> `observaciones / findings:` …

---

## 1. Missions home / Inicio de misiones

**What to try / Qué probar:**
- Open **Misiones**: you should see demo missions with every status chip: `activa`,
  `en revisión`, `completada`, `detenida`, `fallida`, plus one captured **draft** card.
- Open one mission (e.g. the flagship "Does retrieval-augmented grounding…"):
  stop condition, success criterion, autonomy, spend meter, Night Shift schedule,
  and the **runs list** (mono receipt rows: `run.started`, `run.heartbeat`,
  `run.finished`, `spend.recorded`, `search.run`).
- The **stopped** mission ("…non-LLM fetchers…"): spend meter shows `blocked`
  (300/300) and the runs list carries the `spend.refused` row (`cost_ceiling_reached`).
- The **failed** mission: its runs list shows the honest `run.failed`
  (`provider_error`) row — never a bare "failed".
- Edge: click the draft card — it asks for its terminators, never runs.
- Error: try creating a mission with an empty stop condition — refused before anything lands (AD-12).

**Correct looks like / Lo correcto:** one card per mission, status chips derived
from events (FR-1.x); spend meter states `ok/near/blocked` (AD-10); every failed
row names its reason in code form (bilingual-safe).

`observaciones / findings:` …

---

## 2. Board + Quarantine / Tablero + Cuarentena

**What to try / Qué probar:**
- Open the flagship mission's board: hypotheses in `propuesta`, `en prueba`,
  `soportada`, `refutada`, `revisada` — each with its audit stamp.
- Relation chips: "X contradicts Y" renders on BOTH endpoints (FR-2.3).
- Claims: citation pin (sha-256 digest, confidence, model), numerical pin
  (artifact ref), one **unpinned** claim with the amber flag (FR-3.4).
- The pin with the failed-then-recovered verification: the LATEST result wins
  (verified); the failed history stays visible in receipts.
- **Cuarentena** (from the mission or dashboard): one `pending`, one `rejected`,
  one `merged` proposal. Approve the pending one → the board updates only after
  the merge. Try approving twice → refused `not_pending:`.
- Error: try an illegal transition from the board (e.g. propuesta → soportada) →
  refused `illegal_transition:` (FR-2.2).

**Correct looks like / Lo correcto:** quarantine changes nothing until the human
merges (AD-3); three signals never conflated: confidence (model), verification
(code), support (a different model) (NFR-3).

`observaciones / findings:` …

---

## 3. Morning digest / Resumen matutino

**What to try / Qué probar:**
- Open **Resumen**: rows for the demo night — a finished row, a **failed row**
  (`corrida fallida: provider_error`, receipt kept), and the outcome badge
  `éxito parcial`.
- The **dead-run alert** block (interruptor de hombre muerto): the `run.dead`
  detection with its heartbeat time, linking to the receipt.
- The **connection alert** row: `zotero` (seed) and/or `semantic_scholar` (demo)
  down — label + icon, never color alone.
- Edge: run **Correr ahora** (Night Shift) → new rows append on top, ≤10 rows.
- Receipt drill-down: click `recibos` on a row → the run's ledger.

**Correct looks like / Lo correcto:** ≤10 rows, one line per mission, failed runs
render honestly (FR-4.x); alerts present but separate from rows.

`observaciones / findings:` …

---

## 4. Receipts / Recibos

**What to try / Qué probar:**
- From a mission's runs list, open a receipt: `run_start`, `search`, `call`
  (provider, model, tokens, cost), `proposal`, `refused`, `run_end` rows in seq order.
- The ceiling-refused receipt: the `refused` row names the scope and the
  would-be cost (AD-10 — an event, never a silent no).
- Edge: a receipt for a dead run — it ends `run_end failed silently_dead`.

**Correct looks like / Lo correcto:** receipt voice (mono), every row timestamped,
spend attributable (provider + model, AD-10).

`observaciones / findings:` …

---

## 5. Readiness (tier 1 + 2) + Publication / Preparación (nivel 1 + 2) + Publicación

**What to try / Qué probar:**
- **Preparación** on the flagship mission: verdict `no listo`, with blockers
  naming their exact objects — the unpinned `CLAIMS-n` and the load-bearing
  `H-n` chips — plus info rows (unsupported pin, merge queue). No scores.
- The clean mission ("Does sparse attention match…"): verdict `listo`
  (preprint-ready) with the pinned/resolved trail.
- **Publicación**: the Journal Fit Finder ranking (siam-jsc 92), the tier-2
  machine checklist vs the registered demo manuscript, and the **in-flight
  submission checklist**: some machine items pre-checked (user-stamped), the
  human-only items flagged and unchecked.
- Check a human item → stamp lands; uncheck → removed. The agent pre-check
  refuses human items structurally (FR-19.4).
- Manuscript tab: the registered demo `.tex` — file list, markers (`\hyp`,
  `\claim`), compile state (12 pages), read/edit a file.

**Correct looks like / Lo correcto:** blockers reference exact board objects
(FR-13); confirmation flips tier-2 items, never adds or hides them; the choice
of venue is quarantined — nothing auto-applies.

`observaciones / findings:` …

---

## 6. Dashboard / Panel

**What to try / Qué probar:**
- Open **Panel** after seeding: status counts (every lifecycle represented),
  the active shortlist, the digest teaser (failed row + alerts), the
  spend-vs-ceiling meters, recent receipts (newest first), and the workspace
  readiness fold.
- Edge: before seeding (fresh workspace) the dashboard renders honest empty
  states — never zeros pretending to be data.

**Correct looks like / Lo correcto:** six read-only widgets over one fold
(FR-18); a composition, never a write.

`observaciones / findings:` …

---

## 7. Refs / Referencias

**What to try / Qué probar:**
- Open **Referencias**: the demo refs (arXiv, DOI, manual, S2 badges) plus the
  seeded library; one demo ref is **archived** (struck, restorable).
- Restore the archived ref → it returns (the log keeps the removal, FR-15.7).
- Add a ref (arXiv paste / manual) → lands with its source badge.
- Pin-refusal edge: a removed ref is never pinnable (FR-15.5/15.6).
- Search disclosure: the demo searches render — including the **null result**
  row (`0 resultados`), recorded identically, never hidden (FR-12.1).

**Correct looks like / Lo correcto:** every mutation is evented (archived, not
deleted); the mini-timeline (seq, ts, actor) renders per ref.

`observaciones / findings:` …

---

## 8. Assistant / Chat / Asistente

**What to try / Qué probar:**
- Open **Asistente**: create a chat, pick a skill (drafter, critic, librarian…),
  scope it to a mission, send a message.
- With no provider configured the send is REFUSED honestly (NFR-11) — configure
  one in Ajustes → IA first (or use the simulated/local provider).
- Attachments: attach a file, send — the reply's context echo shows the browser
  read it client-side.

**Correct looks like / Lo correcto:** the assistant never answers with canned
text out of the box; refusals are coded and bilingual.

`observaciones / findings:` …

---

## 9. Settings / Trust / IA / Ajustes

**What to try / Qué probar:**
- **Ajustes → IA**: configure a provider (BYOK / CLI bridge / local Ollama) —
  the key never renders back (NFR-10); "Probar conexión" gives an honest verdict.
- **Confianza**: the autonomy dial (watch/suggest/act with receipts), the spend
  ceiling, and the KILL switch with its confirm step; the health line shows the
  down connections (matching the digest alerts).
- **Datos de ejemplo** (danger tab): the confirmation state; "Forzar re-siembra"
  appends a NEW generation (confirm dialog first) — history never modified.
- Error: force re-seed without the first seed? It just seeds generation 1.

**Correct looks like / Lo correcto:** every dial change is an event; the kill
switch stops everything and says so.

`observaciones / findings:` …

---

## 10. Status / Estado

**What to try / Qué probar:**
- Open **Estado**: connection health (zotero down, arxiv recovered), heartbeats,
  the event log's shape — seq dense and increasing.
- Edge: with the demo seeded, the dead-run's heartbeat age vs the 30-minute
  threshold is visible.

**Correct looks like / Lo correcto:** code-form error codes (never translated),
health as label + icon (FR-9.1).

`observaciones / findings:` …

---

## 11. Bridge + Manuscrito / Puente + Manuscrito

**What to try / Qué probar:**
- **Ajustes → Puente**: OFF by default (RC_BRIDGE); enable, pair a device — the
  pairing ledger renders; unpaired remote verbs are refused `unpaired:` (401).
- **Manuscrito**: register a `.tex` directory (or browse the demo one), compile,
  edit a file; agent diffs land in quarantine — merge applies hunks, reject
  changes nothing, a stale basis renders the amber banner + force affordance.

**Correct looks like / Lo correcto:** one remote channel, one writer (AD-14);
diffs merge only through the human (FR-20.3).

`observaciones / findings:` …

---

## 12. Mobile companion `/m` / Acompañante móvil

**What to try / Qué probar:**
- Visit `/m` (served by the same app): the mobile companion renders — quick
  capture lands a DRAFT card on the home machine (FR-21.3), never a launch.
- Paired: approve/reject a pending proposal from the phone — the merge event
  carries `actor=user`, `surface=mobile`.
- Edge: unpaired, every verb refuses with the typed 401.

**Correct looks like / Lo correcto:** capture is a draft; decisions are evented
with their surface; read-only elsewhere.

`observaciones / findings:` …

---

## Wrap-up / Cierre

- Re-read your findings lines; anything that rendered wrong, leaked a language,
  or showed a bare "failed" with no reason is a bug — file it with the screen
  and the mission id (`M-n`, `H-n`, `CLAIMS-n` chips make every report precise).
- To reset and re-test: Ajustes → Zona peligrosa → Restablecer base de datos
  (desktop), or just reload (browser mock state is ephemeral).
