// (m) The demo workspace seed (the user-testing harness): the "Datos de
// ejemplo" control seeds one rich generation through the real UI — the
// dashboard populates with the demo missions and the digest renders its
// demo rows (a finished night + the honest failed row).
import { test, expect, nav } from "./helpers";

test("demo seed: the settings control populates the workspace end to end", async ({ page }) => {
  await nav(page, "settings");
  await page.locator('[onclick="RC.setSettingsTab(\'danger\')"]').click();

  await test.step("the honest demo-data card seeds one generation", async () => {
    await expect(page.locator("#main-content")).toContainText("Datos de ejemplo");
    await expect(page.locator("#main-content")).toContainText("Puebla este workspace con datos de ejemplo");
    await page
      .locator("#main-content")
      .getByRole("button", { name: "Poblar con datos de ejemplo" })
      .click();
    // the confirmation state: generation 1, N events, honest and visible
    await expect(page.locator("#main-content")).toContainText("generación 1");
  });

  await test.step("the dashboard populates with the demo missions", async () => {
    await nav(page, "dashboard");
    await expect(page.locator("#main-content")).toContainText("M-201"); // the active flagship
    await expect(page.locator("#main-content")).toContainText("M-205"); // failed honestly
    await expect(page.locator("#main-content")).toContainText("M-206"); // the dead-run mission
  });

  await test.step("the digest renders the demo night after the seed", async () => {
    await nav(page, "digest");
    await expect(page.locator("#main-content")).toContainText("Resumen Matutino");
    await expect(page.locator("#main-content")).toContainText("M-201");
    await expect(page.locator("#main-content")).toContainText("M-206"); // the dead-run mission's row
  });
});
