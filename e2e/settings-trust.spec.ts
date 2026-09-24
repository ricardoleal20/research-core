// (k) Settings → Trust: the autonomy dial, the hard ceiling, and the
// kill-switch confirm step.
import { test, expect, nav } from "./helpers";

test("trust center: autonomy dial, ceiling save, and the kill-switch confirm step", async ({ page }) => {
  await nav(page, "settings");
  await page.locator('[onclick="RC.setSettingsTab(\'trust\')"]').click();

  await test.step("the global autonomy dial moves and persists visually", async () => {
    const dial = page.locator('[onclick="RC.dial(\'global\',\'\',\'watch\')"]');
    await expect(dial).toBeVisible();
    await dial.click();
    // the selected stop renders the active (white/foreground) treatment
    await expect(dial).toHaveClass(/bg-white/);
  });

  await test.step("a new hard ceiling saves and the meter re-renders", async () => {
    await expect(page.locator("#main-content")).toContainText("Techo mensual");
    await page.locator("#trust-global-ceiling").fill("30.00");
    await page.locator("#main-content").getByRole("button", { name: "Guardar cambios" }).click();
    await expect(page.locator("#main-content")).toContainText("$30.00");
  });

  await test.step("the kill switch requires its confirm step, then kills", async () => {
    await page.locator("#main-content").getByRole("button", { name: "Interruptor de apagado" }).click();
    // the confirm dialog is auto-accepted by the fixture — the runtime flips
    await expect(
      page.locator("#main-content").getByText("Detenido — todo despacho se rechaza hasta reanudar"),
    ).toBeVisible();
    await expect(
      page.locator("#main-content").getByRole("button", { name: "Reanudar" }),
    ).toBeVisible();
  });
});