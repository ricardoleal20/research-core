// (e) Morning digest: the seeded night renders its rows, the honest failed
// row names its reason, and the dead-run alert row is present.
import { test, expect, nav } from "./helpers";

test("morning digest renders the seeded night: rows, honest failure, dead-run alert", async ({ page }) => {
  await nav(page, "digest");
  await expect(page.locator("#main-content")).toContainText("Resumen Matutino");
  await expect(page.locator("#main-content")).toContainText("éxito parcial");

  await test.step("the four seeded rows render with their verdicts", async () => {
    await expect(page.locator("#main-content")).toContainText("M-21");
    await expect(page.locator("#main-content")).toContainText("M-22");
    await expect(page.locator("#main-content")).toContainText("M-24");
    await expect(page.locator("#main-content")).toContainText("M-26");
  });

  await test.step("the honest failed row names its reason (never hidden)", async () => {
    await expect(page.locator("#main-content")).toContainText("corrida fallida: provider_error");
  });

  await test.step("the dead-run alert row is present and links its receipt", async () => {
    await expect(page.locator("#main-content")).toContainText("interruptor de hombre muerto");
    await expect(page.locator("#main-content")).toContainText("nightshift-17");
    await expect(page.locator("#main-content").getByRole("button", { name: "recibos" }).first()).toBeVisible();
  });

  await test.step("the seeded connection alert (Zotero unreachable) shows honestly", async () => {
    await expect(page.locator("#main-content")).toContainText("zotero");
    await expect(page.locator("#main-content")).toContainText("unreachable");
  });
});