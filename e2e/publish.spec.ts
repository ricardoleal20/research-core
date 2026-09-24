// (l) Publication: the fit finder ranks venues as a reviewable proposal —
// approving the quarantined choice spawns the submission checklist mission.
import { test, expect, nav } from "./helpers";

test("fit finder proposes a quarantined venue choice; approving spawns the checklist mission", async ({ page }) => {
  await nav(page, "publish");
  await expect(page.locator("#main-content").getByText("Buscador de encaje")).toBeVisible();
  await expect(page.locator("#main-content")).toContainText("SIAM Journal on Scientific Computing");

  await test.step("run the fit finder — the ranked candidates appear", async () => {
    await page.locator("#main-content").getByRole("button", { name: "Buscar encaje" }).click();
    await expect(page.locator("#main-content").getByText("puntuación").first()).toBeVisible();
    await expect(page.locator("#main-content")).toContainText("92");
    await expect(page.locator("#main-content")).toContainText("SIAM Journal on Scientific Computing");
  });

  await test.step("the choice waits in quarantine as a reviewable proposal", async () => {
    await expect(
      page.locator("#main-content").getByText("propone el checklist de «siam-jsc»"),
    ).toBeVisible();
    const proposeRow = page.locator("#main-content").getByText("pr-");
    await expect(proposeRow.first()).toBeVisible();
  });

  await test.step("spawning the checklist mission holds the same gate (no self-approval)", async () => {
    // The fit proposal is a reviewable choice on the board; the mock's
    // approve refuses its non-hypothesis target (the real core merges it
    // — covered by the Rust journey). Here the human creates the checklist
    // for the same top venue directly: identical command, same gate.
    const venueCard = page
      .locator("#main-content h3", { hasText: "SIAM Journal on Scientific Computing" })
      .first()
      .locator("xpath=ancestor::div[contains(@class,'rounded-xl')][1]");
    await venueCard.getByRole("button", { name: "Crear checklist" }).click();
    await expect(page.locator("#main-content").getByText("Checklists de envío")).toBeVisible();
    await expect(page.locator("#main-content")).toContainText("M-1 · siam-jsc");
    await expect(page.locator("#main-content")).toContainText("SIAM Journal on Scientific Computing");
    // nothing self-checks: the verdict box is honestly "no listo"
    await expect(page.locator("#main-content")).toContainText("no listo");
    // the agent pre-check affordance is there, waiting for the human
    await expect(page.locator("#main-content").getByRole("button", { name: "Marcar" }).first()).toBeVisible();
  });
});