// (f) The receipts drawer: the drill-down (board run → receipt, digest row
// → receipt) replays the ordered audit ledger over the event log.
import { test, expect, nav } from "./helpers";

test("the receipts drawer replays the audit ledger rows", async ({ page }) => {
  await nav(page, "digest");
  await expect(page.locator("#main-content")).toContainText("M-21");

  await test.step("drill from the M-21 digest row into its run receipt", async () => {
    await page
      .locator("#main-content .px-6.py-4.hover-row", { hasText: "M-21" })
      .first()
      .getByRole("button", { name: "recibos" })
      .click();
    const drawer = page.locator(".rc-modal");
    await expect(drawer).toBeVisible();
    await expect(drawer).toContainText("Recibo de corrida");
    await expect(drawer).toContainText("M-21");
    await expect(drawer).toContainText("nightshift-21");
  });

  await test.step("the ledger rows and their chips render", async () => {
    const drawer = page.locator(".rc-modal");
    // ordered mono rows e-{seq} · timestamp
    await expect(drawer.locator(".font-mono", { hasText: /e-\d+/ }).first()).toBeVisible();
    // the refused row (ceiling refusal) carries the rose treatment + chip
    await expect(drawer).toContainText("techo");
    await expect(drawer.locator("div", { hasText: /e-\d+ · \d{4}-\d{2}-\d{2}/ }).first()).toBeVisible();
  });

  await test.step("Escape closes the drawer", async () => {
    await page.keyboard.press("Escape");
    await expect(page.locator(".rc-modal")).not.toBeVisible();
  });
});