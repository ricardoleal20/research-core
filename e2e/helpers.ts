// Shared E2E fixtures + helpers (docs/e2e.md). Two page flavors:
//   - `test`  — onboarding already completed (`rc-onboarding=1`), the lock
//     shows the returning-user variant and `#lock-btn` unlocks straight to
//     Missions.
//   - `freshTest` — no onboarding key: the first-run lock + wizard render.
// Both register a dialog handler (the UI's `window.prompt`/`confirm`/`alert`
// calls are load-bearing: hypothesis transition bases, agent-step tasks,
// the kill-switch confirm, error alerts) and disable CSS animations for
// stable assertions.
import { test as base, expect } from "@playwright/test";
import type { Page } from "@playwright/test";

type PromptState = { text: string };

function withDialogs(page: Page) {
  const prompt: PromptState = { text: "e2e basis — from the trial evidence" };
  (page as unknown as { __rcPrompt: PromptState }).__rcPrompt = prompt;
  page.on("dialog", (d) => {
    if (d.type() === "prompt") void d.accept(prompt.text);
    else void d.accept();
  });
}

// The suites run the REAL UI over the in-memory mock. Vite dev's SPA
// fallback answers `/api/*` with index.html (HTTP 200), which would make
// the app's `servedByCore` probe resolve true and flip the whole UI into
// the read-only served-by-core view. The E2E layer aborts every `/api/**`
// request so the probe fails and `mockActive` takes over — the same
// process sel-container "plain vite dev" detection inside the app.
function forceMockTransport(page: Page) {
  page.route("**/api/**", (route) => route.abort());
}

/** The seeded-mock "onboarded" flavor: fresh page per test, onboarding done. */
export const test = base.extend({
  onboarded: [
    async ({ page }, use) => {
      withDialogs(page);
      forceMockTransport(page);
      await page.addInitScript(() => {
        localStorage.setItem("rc-onboarding", "1");
        localStorage.setItem("rc-animations", "0");
      });
      await use();
    },
    { auto: true },
  ],
});

/** The first-run flavor: the wizard must render, so no onboarding key. */
export const freshTest = base.extend({
  fresh: [
    async ({ page }, use) => {
      withDialogs(page);
      forceMockTransport(page);
      await page.addInitScript(() => {
        localStorage.removeItem("rc-onboarding");
        localStorage.setItem("rc-animations", "0");
      });
      await use();
    },
    { auto: true },
  ],
});

/** Make the dialog handler return `text` on the next prompt (transition
 *  basis, agent-step task, ...). */
export function setPromptText(page: Page, text: string) {
  (page as unknown as { __rcPrompt: PromptState }).__rcPrompt.text = text;
}

/** Boot onto the lock, unlock, and settle on the Missions home. */
export async function unlock(page: Page, unlockLabel = "Desbloquear") {
  await page.goto("/");
  await page.locator("#lock-btn").click();
  await expect(page.locator("#sidebar")).toBeVisible();
}

/** A quiet beat after a mutation: the views re-render on every async read
 *  (each mockApi call settles in ~60-120ms), and a re-render wipes the
 *  raw, not-yet-submitted inputs the tests type into. Settling after each
 *  mutation keeps the next fill atomic against that churn. */
export async function settle(page: Page, ms = 250) {
  await page.waitForTimeout(ms);
}

/** Idempotent boot: if the shell is not up yet, load + unlock first, then
 *  wait for the boot's async reads (loadBase + the initial missions load)
 *  to land so their re-renders settle before the test starts typing. */
export async function ensureApp(page: Page) {
  const shell = page.locator("#sidebar");
  const up = await shell.count().then((n) => n > 0);
  if (!up) {
    await page.goto("/");
    await page.locator("#lock-btn").click();
    await expect(shell).toBeVisible();
    // the boot renders twice more once the mock answers (loadBase + the
    // missions read); settle on both signals before letting tests type
    const header = page.locator("header").getByText("Optimización de Modelos de Atención");
    await expect(header.first()).toBeVisible();
    await expect(page.locator("#main-content").getByText("M-1").first()).toBeVisible();
    await settle(page);
  }
}

/** Click a sidebar nav item (language-independent: rides the `onclick`),
 *  then let the view's read loads settle. */
export async function nav(page: Page, view: string) {
  await ensureApp(page);
  await page.locator(`#sidebar nav button[onclick="RC.navigate('${view}')"]`).click();
  await expect(page.locator("#main-content")).toBeVisible();
  await settle(page);
}

/** Resolve one select option through the UI's own handler. The rendered
 *  option's `onclick` points at `RC.pickRcSelect(id, value)`; the menus of
 *  the top-row selects (dir="up") open outside the viewport or under the
 *  sticky shell header, where a real pointer click can land on the wrong
 *  element even with `force`. Dispatching the same handler straight from
 *  the DOM exercises the identical state path (sync → change → the app's
 *  onChange) deterministically, independent of popover geometry. The menu
 *  trigger itself IS clicked for real, so the open interaction is covered. */
async function clickOption(option: ReturnType<Page["locator"]>, id: string, value: string) {
  await option.waitFor({ state: "attached", timeout: 5_000 });
  await option.evaluate(
    (el, { selectId, v }) => {
      const rc = (window as unknown as { RC?: { pickRcSelect?: (i: string, v: string, e: unknown) => void } }).RC;
      const wrapId = el.closest(".rc-select")?.getAttribute("data-select-id") ?? selectId;
      if (rc?.pickRcSelect) rc.pickRcSelect(wrapId, v, { stopPropagation: () => {} });
    },
    { selectId: id, v: value },
  );
}

/** Pick an rcSelect option by its data-value (the shadcn-style select). */
export async function pickRc(page: Page, id: string, value: string) {
  const wrap = page.locator(`.rc-select[data-select-id="${id}"]`);
  await wrap.locator("> button").click();
  await clickOption(wrap.locator(`.rc-select-option[data-value="${value}"]`), id, value);
  await settle(page, 350);
}

/** Pick an rcSelect option by visible label text (unknown-value selects,
 *  e.g. a mission list whose options carry server-generated ids). */
export async function pickRcByText(page: Page, id: string, label: string) {
  const wrap = page.locator(`.rc-select[data-select-id="${id}"]`);
  await wrap.locator("> button").click();
  const option = wrap.locator(".rc-select-option", { hasText: label }).first();
  const value = (await option.getAttribute("data-value")) ?? "";
  await clickOption(option, id, value);
  await settle(page, 350);
}

/** Close any open rcSelect menu. */
export async function closeRcMenus(page: Page) {
  await page.keyboard.press("Escape");
}

/** Drive the question-box composer end-to-end; waits for the mission card. */
export async function createMission(
  page: Page,
  question: string,
  opts: { stop?: string; criterion?: string } = {},
) {
  await nav(page, "missions");
  await page
    .locator("#main-content")
    .getByRole("button", { name: "Nueva misión" })
    .click();
  await expect(page.locator("#qb-question")).toBeVisible();
  await page.locator("#qb-question").fill(question);
  await page.locator("#qb-stop").fill(opts.stop ?? "Stop after 3 runs or 20 sources.");
  await page.locator("#qb-criterion").fill(opts.criterion ?? "A blind rater agrees with the claim.");
  await page.locator("#main-content").getByRole("button", { name: "Lanzar misión" }).click();
  await expect(page.locator("h3", { hasText: question }).first()).toBeVisible();
  await settle(page);
}

/** Open the board of a mission by clicking its card title. */
export async function openBoard(page: Page, question: string) {
  await nav(page, "missions");
  await page.locator("h3", { hasText: question }).first().click();
  await expect(page.locator("#new-hyp")).toBeVisible();
  await expect(page.locator("h1.font-serif", { hasText: question })).toBeVisible();
  await settle(page); // the board's disclosure/manuscript reads re-render after
}

/** Add a hypothesis on the current board. */
export async function addHypothesis(page: Page, statement: string, label = "H-1") {
  await page.locator("#new-hyp").fill(statement);
  await page.getByRole("button", { name: "Añadir hipótesis" }).click();
  await expect(
    page.locator("#main-content").getByText(label, { exact: true }).first(),
  ).toBeVisible();
  await settle(page);
}

/** Register a claim on a hypothesis card. */
export async function addClaim(page: Page, text: string, clSeq = "CLAIMS-1") {
  await page.locator('input[id^="claim-"]').fill(text);
  await page.getByRole("button", { name: "Añadir afirmación" }).click();
  await expect(
    page.locator("#main-content").getByText(clSeq, { exact: true }).first(),
  ).toBeVisible();
  await settle(page);
}

export { expect };