// (h) Dashboard: the six widgets render — honest per-widget state, empty
// and seeded — over the one read-only aggregate.
import { test, expect, nav } from "./helpers";

test("the six dashboard widgets render their honest seeded state", async ({ page }) => {
  await nav(page, "dashboard");

  const titles = [
    "Misiones", // missions overview
    "Salud de los tableros", // board health
    "Último digesto", // digest teaser
    "Gasto vs tope", // spend vs ceiling
    "Recibos recientes", // recent receipts
    "Listo para preprint", // readiness teaser
  ];
  for (const title of titles) {
    await expect(page.locator("#main-content")).toContainText(title);
  }

  await test.step("missions widget: the seeded draft + no active section (honest empty)", async () => {
    await expect(page.locator("#main-content")).toContainText("Does attention sparsity hold at 32k context?");
    await expect(page.locator("#main-content")).toContainText("$0.00");
    // no active missions exist — the "Activas" section must not render
    await expect(page.locator("#main-content").getByText("Activas").first()).not.toBeVisible();
  });

  await test.step("digest teaser: the seeded night's rows", async () => {
    await expect(page.locator("#main-content")).toContainText("M-21");
    await expect(page.locator("#main-content")).toContainText("M-26");
  });

  await test.step("readiness widget: the workspace's honest not-ready chip", async () => {
    await expect(page.locator("#main-content")).toContainText("no listo");
  });
});