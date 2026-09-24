// (b) Question box → mission: the composer refuses empty terminators
// (inline empty-refuse — nothing launches, no card appears), and a filled
// launch lands an Active mission card with its spend meter.
import { test, expect, nav, createMission } from "./helpers";

test("question box refuses empty terminators and launches a mission with a spend meter", async ({ page }) => {
  await test.step("open the composer", async () => {
    await nav(page, "missions");
    await page
      .locator("#main-content")
      .getByRole("button", { name: "Nueva misión" })
      .click();
    await expect(page.locator("#qb-question")).toBeVisible();
  });

  await test.step("launch with empty fields is refused inline — no card, composer stays", async () => {
    await page.locator("#main-content").getByRole("button", { name: "Lanzar misión" }).click();
    await expect(page.locator("#qb-question")).toBeVisible();
    await expect(page.locator("#main-content")).not.toContainText("Does sparse attention hold?");
  });

  await test.step("filled terminators launch the mission card", async () => {
    await page.locator("#qb-question").fill("Does sparse attention hold?");
    await page.locator("#qb-stop").fill("Stop after 3 runs or 20 sources.");
    await page.locator("#qb-criterion").fill("A blind rater agrees with the claim.");
    await page.locator("#main-content").getByRole("button", { name: "Lanzar misión" }).click();
    await expect(page.locator("h3", { hasText: "Does sparse attention hold?" })).toBeVisible();
    await expect(page.locator("#main-content")).toContainText("M-2");
  });

  await test.step("the card carries its terminators and the spend meter", async () => {
    const card = page.locator("h3", { hasText: "Does sparse attention hold?" }).locator("..").locator("..");
    await expect(card).toContainText("Condición de paro:");
    await expect(card).toContainText("Stop after 3 runs or 20 sources.");
    await expect(card).toContainText("Criterio de éxito (falsable):");
    await expect(card).toContainText("Gasto");
    await expect(card).toContainText("$0.00");
    await expect(card).toContainText("$10.00");
    await expect(card).toContainText("Activa");
  });
});

test("a captured draft completes into an active mission with its spend meter", async ({ page }) => {
  await nav(page, "missions");

  // The seeded quick-captured draft renders its pending card (border-l-amber).
  const draft = page.locator("h3", { hasText: "Does attention sparsity hold at 32k context?" });
  await expect(draft).toBeVisible();
  await expect(page.locator("#main-content")).toContainText("Borrador");
  await expect(page.locator("#main-content")).toContainText("Capturada en el móvil");

  // The owner gives it terminators; only then does the mission launch.
  await page.locator("#dc-stop-1").fill("Stop after 5 runs.");
  await page.locator("#dc-criterion-1").fill("The table reproduces across five seeds.");
  await page.locator("#main-content").getByRole("button", { name: "Completar y lanzar" }).click();

  await expect(draft).toBeVisible();
  await expect(page.locator("#main-content")).toContainText("Activa");
  await expect(page.locator("#main-content")).toContainText("Stop after 5 runs.");
  await expect(page.locator("#main-content")).toContainText("$0.00");
  await expect(page.locator("#main-content")).toContainText("$10.00");
  await expect(page.locator("#main-content")).not.toContainText("Completar y lanzar");
});