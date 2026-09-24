// (g) Readiness: the not-ready blockers surface with their CLAIMS/H
// object chips; a clean live board reaches preprint-ready with the
// evidence trail.
import { test, expect, nav, createMission, openBoard, addHypothesis, addClaim, pickRc, setPromptText } from "./helpers";

test("workspace readiness shows not-ready blockers with object chips", async ({ page }) => {
  await nav(page, "dashboard");
  await expect(page.locator("#main-content")).toContainText("Listo para preprint");

  await test.step("open the readiness drawer from the dashboard teaser", async () => {
    await page.locator("#main-content").getByRole("button", { name: "ver informe →" }).click();
    const drawer = page.locator(".rc-modal");
    await expect(drawer).toBeVisible();
    await expect(drawer).toContainText("Preparación");
    await expect(drawer).toContainText("no listo");
  });

  await test.step("the blockers name their objects — CLAIMS/H chips", async () => {
    const drawer = page.locator(".rc-modal");
    await expect(drawer).toContainText("Elementos que bloquean");
    await expect(drawer).toContainText("CLAIMS-2");
    await expect(drawer).toContainText("CLAIMS-5");
    await expect(drawer).toContainText("CLAIMS-9");
    await expect(drawer).toContainText("H-31");
  });

  await test.step("the drawer also offers the tier-2 venue select", async () => {
    const drawer = page.locator(".rc-modal");
    await expect(drawer).toContainText("Listo para journal");
    await expect(drawer).toContainText("Selecciona una revista…");
  });
});

test("a clean live board reaches preprint-ready with the evidence trail", async ({ page }) => {
  await createMission(page, "Does the gain hold across seeds?");
  await openBoard(page, "Does the gain hold across seeds?");
  await addHypothesis(page, "The gain holds across seeds and datasets.", "H-1");
  await addClaim(page, "The measured gain is 12.3% with n=48.", "CLAIMS-1");

  await test.step("pin the claim to a citation", async () => {
    await page.locator("#main-content").getByRole("button", { name: "Anclar a cita" }).first().click();
    await expect(page.locator(".rc-modal")).toBeVisible();
    await pickRc(page, "pin-ref", "r1");
    await page.locator('.rc-modal [onclick="RC.submitPin()"]').click();
    await expect(page.locator(".rc-modal")).not.toBeVisible();
  });

  await test.step("resolve the hypothesis (proposed → testing → supported)", async () => {
    setPromptText(page, "Trial 1 ran; the pinned run held.");
    await pickRc(page, "hyp-status-1", "testing");
    await expect(page.locator("#main-content")).toContainText("En prueba");
    setPromptText(page, "The pinned evidence held across trials.");
    await pickRc(page, "hyp-status-1", "supported");
    await expect(page.locator("#main-content")).toContainText("Soportada");
  });

  await test.step("the board's readiness drawer folds to preprint-ready with the trail", async () => {
    await page
      .locator("#main-content")
      .getByRole("button", { name: "Informe de preparación" })
      .click();
    const drawer = page.locator(".rc-modal");
    await expect(drawer).toBeVisible();
    await expect(drawer).toContainText("listo");
    await expect(drawer).toContainText("Nada bloquea este tablero");
    await expect(drawer).toContainText("1/1 afirmaciones ancladas");
  });
});