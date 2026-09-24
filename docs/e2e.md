# End-to-end test suites

ResearchCore ships two E2E layers — one for the browser UI over `vite dev`
+ the in-memory mock, one for the real Rust core over the in-process server
shell. Both are fully deterministic: no network, no external providers.

## 1. Browser E2E — Playwright over the real UI (`npm run test:e2e`)

The real UI served by the Vite dev server on a fixed free port
(`playwright.config.ts`: port 5188, CLI-overridden), driven with the
in-memory mock active. The mock activates in plain-browser `vite dev`
(`mockActive` in `src/mock-backend.ts`) — the E2E fixtures additionally
abort every `/api/**` request so the app's `servedByCore` probe stays
false and the UI never flips into the read-only served-by-core view.

Each test gets a fresh browser context, so the mock's in-memory state
(missions, digest, readiness board, refs, AI config) starts from the fixed
seed on every test — no seeding API needed, a reload is the reset.

Suites (in `e2e/`), mapping onto the biblical journeys:

| file | journey |
|---|---|
| `onboarding.spec.ts` | first-run wizard: arXiv paste → starter mission + candidates (runFirstValue) |
| `missions.spec.ts` | question box: inline empty-refuse, launch with terminators, spend meter; draft completion |
| `board.spec.ts` | hypothesis board: claims, unpinned amber flag, citation + numerical pins (confidence/mono digest), legal-only transitions, relation chips |
| `quarantine.spec.ts` | agent proposal → approve merges / reject discards; basis-stale manuscript-diff banner + force merge |
| `digest.spec.ts` | morning digest: seeded rows, honest failed row, dead-run + connection alerts |
| `receipts.spec.ts` | receipts drawer: audit-ledger rows replayed over the log |
| `readiness.spec.ts` | not-ready blockers with CLAIMS/H chips; clean board → preprint-ready + trail |
| `dashboard.spec.ts` | six widgets, honest seeded state |
| `refs.spec.ts` | library CRUD: arXiv + manual adds, two-step archive, pin-refusal, restore |
| `assistant.spec.ts` | configure-provider gate; configured chat with scope/skill/model/attachments |
| `settings-trust.spec.ts` | autonomy dial, hard ceiling, kill-switch confirm |
| `publish.spec.ts` | fit finder → quarantined venue choice → checklist mission |

Notes for writers:

- The suites use ES-language selectors by default (the app defaults to
  Spanish); switchable via `pickRc(page, "set-lang", "en")`.
- The UI's `window.prompt` / `confirm` / `alert` calls are load-bearing —
  the fixtures auto-accept dialogs and let tests override the prompt text
  with `setPromptText(page, "...")`.
- All mutations settle through `settle()`/`pickRc()` — the views re-render
  on every mock read, which would otherwise wipe un-submitted inputs.
- rcSelect options are resolved through the UI's own `RC.pickRcSelect`
  handler (some top-row menus open outside the viewport).
- Run just one file: `npx playwright test e2e/board.spec.ts --headed`.

## 2. Core journey E2E — the real core + the real server (`cargo test --test journey`)

`src-tauri/tests/journey.rs` boots the real in-process server shell
(`server::router` / `bridge::bridge_router`, same axum app the desktop
spawns) over a real temp workspace (`Db::open`: migrate + seed +
eventstore) and drives the writer via the domain seam the Tauri commands
delegate to — the typed `NewEvent` constructors and the domain mutations
(`missions::quick_capture`, `proposals::propose_transition/approve/reject`,
`bridge::pair_device`). No mock anywhere.

- `served_ui_and_read_only_api` — `GET /` serves the built UI (run
  `npm run build` first — `dist/` is the server's `ServeDir`), the
  read-only API folds the core, and mutation verbs are structurally refused
  (405).
- `writer_journey_reaches_preprint_ready` — mission → hypothesis → claim →
  citation pin (real library ref) → resolved hypothesis → night run; the
  projected reads (missions → hypotheses → evidence) and the end state
  `verdict: "ready"` with the evidence trail over HTTP.
- `closed_remote_vocabulary_enforces_pairing` — the bridge's closed remote
  vocabulary: unpaired 401 (`unpaired:`), paired quick-capture lands a
  pending card, token approve/reject are evented `actor=user,
  surface=mobile`, and the basis-stale rule refuses (409) then force-merges
  with the marker.

Deviation: the server binds an OS-assigned ephemeral port instead of
`RC_PORT` (the fixed-port spawner is process-global and would collide
across parallel tests); the router is the identical app the spawner runs.

## Gates

```sh
# browser journeys (vite dev + mock)
npm run test:e2e                # 20 tests, deterministic

# real-core journeys (requires the built UI for the / GET assertion)
cd src-tauri && cargo test --test journey

# full suites
cargo test && npm run build && npx tsc --noEmit
```