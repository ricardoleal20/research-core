// (d) Quarantine: an agent proposal renders the diff card → Approve merges
// (the board changes), Reject discards → and the force case surfaces the
// basis-stale banner (the manuscript-diff seam, where staleness is derived
// live from the file's digest).
import { test, expect, createMission, openBoard, addHypothesis, pickRcByText, setPromptText } from "./helpers";

const MISSION = "Does the gain hold across seeds?";

test("an agent proposal quarantines; approve merges into the board, reject discards", async ({ page }) => {
  await createMission(page, MISSION);
  await openBoard(page, MISSION);
  await addHypothesis(page, "The gain holds across seeds and datasets.", "H-1");

  await test.step("the drafter step lands a quarantined transition proposal", async () => {
    setPromptText(page, "advance the mission with a testable claim");
    await page.locator("#main-content").getByRole("button", { name: "Redactor" }).click();
    await expect(page.locator("#main-content").getByText("Revisión pendiente")).toBeVisible();
    const quarantine = page.locator("#main-content").getByText("cambio de estado");
    await expect(quarantine.first()).toBeVisible();
    await expect(page.locator("#main-content")).toContainText("proposed → testing");
  });

  await test.step("approve merges — the hypothesis folds to testing", async () => {
    await page
      .locator("#main-content")
      .getByRole("button", { name: "Aprobar" })
      .first()
      .click();
    await expect(page.locator("#main-content")).toContainText("En prueba");
    await expect(page.locator("#main-content")).toContainText("decidida · merged");
  });

  await test.step("a second proposal rejects cleanly — the board does not change", async () => {
    setPromptText(page, "critic: the second run needs more evidence");
    await page.locator("#main-content").getByRole("button", { name: "Crítico" }).click();
    // the critic proposes nothing — the board stays folded at testing
    await expect(page.locator("#main-content")).toContainText("En prueba");

    // a fresh drafter proposal (testing → supported is next legal)
    setPromptText(page, "drafter: the supported direction is worth testing");
    await page.locator("#main-content").getByRole("button", { name: "Redactor" }).click();
    await expect(page.locator("#main-content")).toContainText("testing → supported");
    await page
      .locator("#main-content")
      .getByRole("button", { name: "Rechazar" })
      .first()
      .click();
    // reject discards: still testing, no pending merges left to act on
    await expect(page.locator("#main-content")).toContainText("En prueba");
    await expect(page.locator("#main-content").getByRole("button", { name: "Rechazar" })).toHaveCount(0);
  });
});

test("a basis-stale manuscript diff renders the amber banner and the force-approve affordance", async ({ page }) => {
  await createMission(page, MISSION);

  await test.step("register the manuscript for the mission (Ajustes → Manuscrito)", async () => {
    await page.locator("#sidebar nav button[onclick=\"RC.navigate('settings')\"]").click();
    await page.locator('[onclick="RC.setSettingsTab(\'manuscript\')"]').click();
    await expect(page.locator("#ms-reg-dir")).toBeVisible();
    await pickRcByText(page, "ms-reg-mission", "M-2");
    await page.locator("#ms-reg-dir").fill("/tmp/e2e-papers/gain");
    await page.locator("#ms-reg-main").fill("main.tex");
    await page.locator("#main-content").getByRole("button", { name: "Registrar manuscrito" }).click();
    await expect(
      page.locator("#main-content").getByRole("button", { name: "Abrir en el tablero" }),
    ).toBeVisible();
    await page.locator("#main-content").getByRole("button", { name: "Abrir en el tablero" }).click();
    await expect(page.locator("#new-hyp")).toBeVisible();
    await expect(page.locator("#main-content").getByText("Manuscrito").first()).toBeVisible();
    await expect(page.locator("#main-content")).toContainText("Fuentes .tex");
  });

  await test.step("the drafter proposes a quarantined LaTeX diff", async () => {
    await addHypothesis(page, "The gain holds across seeds and datasets.", "H-1");
    setPromptText(page, "advance the mission — propose the LaTeX gain sentence");
    await page.locator("#main-content").getByRole("button", { name: "Redactor" }).click();
    await expect(page.locator("#main-content").getByText("Diffs del agente")).toBeVisible();
    await expect(page.locator("#main-content").getByText("pendiente").first()).toBeVisible();
  });

  await test.step("editing the file stales the diff basis", async () => {
    await page.locator("#main-content").getByRole("button", { name: /main\.tex/ }).click();
    await expect(page.locator("#ms-editor")).toBeVisible();
    await page.locator("#ms-editor").fill("% rc-e2e overridden content\n\\documentclass{article}\n");
    await page.locator("#main-content").getByRole("button", { name: "Guardar" }).click();
    await expect(page.locator("#main-content")).toContainText("Guardado en el repo");
  });

  await test.step("the quarantined diff now carries the basis-stale banner + force merge", async () => {
    await expect(
      page.locator("#main-content").getByText("Base obsoleta — el archivo cambió"),
    ).toBeVisible();
    await expect(
      page.locator("#main-content").getByRole("button", { name: "Forzar fusión" }),
    ).toBeVisible();
  });
});