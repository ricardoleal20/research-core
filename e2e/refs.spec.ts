// (i) Library CRUD: add via arXiv + manual, the two-step remove archives
// with the pin-refusal guard (an archived ref leaves the pin picker), and
// restore brings it back.
import { test, expect, nav, createMission, openBoard, addHypothesis, addClaim } from "./helpers";

test("library CRUD: arXiv + manual adds, archive, pin-refusal, restore", async ({ page }) => {
  await nav(page, "refs");
  await expect(page.locator("#main-content")).toContainText("Attention Is All You Need");

  await test.step("add a reference by arXiv paste", async () => {
    await page.locator("#main-content").getByRole("button", { name: "Añadir referencia" }).click();
    await expect(page.locator(".rc-modal")).toBeVisible();
    await page.locator("#ref-arxiv-url").fill("https://arxiv.org/abs/2402.14871");
    await page.locator(".rc-modal").getByRole("button", { name: "Añadir", exact: true }).click();
    await expect(page.locator(".rc-modal")).not.toBeVisible();
    await expect(page.locator("#main-content")).toContainText("arXiv:2402.14871");
  });

  await test.step("add a reference by manual entry", async () => {
    await page.locator("#main-content").getByRole("button", { name: "Añadir referencia" }).click();
    await page.locator('[onclick="RC.setRefAddMode(\'manual\')"]').click();
    await page.locator("#ref-manual-title").fill("An E2E Manual Reference");
    await page.locator("#ref-manual-authors").fill("Leal et al.");
    await page.locator("#ref-manual-year").fill("2026");
    await page.locator("#ref-manual-venue").fill("TestCon");
    await page.locator("#ref-manual-doi").fill("10.0000/e2e.0001");
    await page.locator("#ref-manual-tags").fill("e2e");
    await page.locator(".rc-modal").getByRole("button", { name: "Añadir", exact: true }).click();
    await expect(page.locator(".rc-modal")).not.toBeVisible();
    await expect(page.locator("#main-content")).toContainText("An E2E Manual Reference");
  });

  await test.step("remove archives two-step and shows the archived badge", async () => {
    await page.locator("#main-content").getByText("Neural Machine Translation by Jointly").click();
    await expect(page.locator(".rc-modal").last()).toContainText("Detalle de referencia");
    await page.locator(".rc-modal").last().getByRole("button", { name: "Eliminar de la biblioteca" }).click();
    await page.locator(".rc-modal").last().getByRole("button", { name: "Confirmar eliminación" }).click();
    await expect(page.locator(".rc-modal").last()).toContainText("Archivada");
    // the ref drawer holds a full-screen backdrop — close it properly or it
    // swallows the next sidebar clicks
    await page.evaluate(() => window.RC.closeRefDetail());
    await expect(page.locator("#main-content")).toContainText("Archivadas (1)");
  });

  await test.step("pin-refusal: an archived ref is absent from the pin picker", async () => {
    await createMission(page, "Does the gain hold across seeds?");
    await openBoard(page, "Does the gain hold across seeds?");
    await addHypothesis(page, "The gain holds across seeds and datasets.", "H-1");
    await addClaim(page, "The measured gain is 12.3% with n=48.", "CLAIMS-1");
    await page.locator("#main-content").getByRole("button", { name: "Anclar a cita" }).first().click();
    await expect(page.locator(".rc-modal").last()).toBeVisible();
    const refs = page.locator('.rc-modal [data-select-id="pin-ref"]');
    await refs.locator("> button").click();
    await expect(refs.locator('.rc-select-option[data-value="r1"]')).toBeVisible();
    await expect(refs.locator(".rc-select-option", { hasText: "Neural Machine Translation" })).toHaveCount(0);
    await page.keyboard.press("Escape");
    // the modal's own close control
    await page.locator(".rc-modal [onclick='RC.closeRcModal()']").first().click();
  });

  await test.step("restore brings the archived ref back into the active library", async () => {
    await nav(page, "refs");
    await page.locator('#main-content [onclick="RC.setRefStatusFilter(\'removed\')"]').click();
    await page.locator("#main-content").getByText("Neural Machine Translation by Jointly").click();
    await expect(page.locator(".rc-modal").last()).toContainText("Archivada");
    await page.locator(".rc-modal").last().getByRole("button", { name: "Restaurar" }).click();
    await page.evaluate(() => window.RC.closeRefDetail());
    await expect(page.locator("#main-content")).not.toContainText("Archivadas (1)");
  });
});