// (m) Bilingual: the ES/EN toggle sanity across the shell, the dashboard,
// and the digest.
import { test, expect, nav, pickRc } from "./helpers";

test("ES/EN toggle switches the shell, the dashboard, and the digest", async ({ page }) => {
  await nav(page, "dashboard");
  await expect(page.locator("#main-content")).toContainText("Panel");
  await expect(page.locator("#sidebar")).toContainText("Misiones");

  await test.step("flip the language in Ajustes → Interfaz", async () => {
    await nav(page, "settings");
    await pickRc(page, "set-lang", "en");
    await expect(page.locator("#sidebar")).toContainText("Missions");
  });

  await test.step("the dashboard re-renders in English", async () => {
    await nav(page, "dashboard");
    await expect(page.locator("#main-content")).toContainText("Dashboard");
    await expect(page.locator("#main-content")).toContainText("Board health");
    await expect(page.locator("#main-content")).toContainText("Spend vs ceiling");
  });

  await test.step("the digest screen names itself in English too", async () => {
    await nav(page, "digest");
    await expect(page.locator("#main-content")).toContainText("Morning Digest");
    await expect(page.locator("#main-content")).not.toContainText("Resumen Matutino");
  });

  await test.step("flipping back restores Spanish", async () => {
    await nav(page, "settings");
    await pickRc(page, "set-lang", "es");
    await expect(page.locator("#sidebar")).toContainText("Misiones");
    await expect(page.locator("#sidebar")).not.toContainText("Missions");
  });
});