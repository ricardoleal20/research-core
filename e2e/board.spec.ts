// (c) Hypothesis board: create → claim → lifecycle transitions (illegal
// ones hidden from the picker) → relation chips → citation + numerical
// pins (with confidence and the mono digest) → unpinned amber flag.
import { test, expect, createMission, openBoard, addHypothesis, addClaim, pickRc, pickRcByText, setPromptText } from "./helpers";

const MISSION = "Does sparse attention hold at 32k context?";

test("board walk: hypothesis, unpinned claim, citation pin with confidence, legal transition", async ({ page }) => {
  await test.step("mission + first hypothesis", async () => {
    await createMission(page, MISSION);
    await openBoard(page, MISSION);
    await addHypothesis(page, "Sparse attention matches full attention at 32k.", "H-1");
  });

  await test.step("an AI claim registers unpinned (amber flag, no pin yet)", async () => {
    await addClaim(page, "Sparse attention runs within 0.3 BLEU of full attention.", "CLAIMS-1");
    await expect(page.locator("#main-content")).toContainText("sin ancla");
    await expect(page.locator("#main-content")).toContainText("CLAIMS-1");
  });

  await test.step("the lifecycle picker only offers legal next states", async () => {
    const wrap = page.locator('.rc-select[data-select-id="hyp-status-1"]');
    await wrap.locator("> button").click();
    const options = wrap.locator(".rc-select-option");
    const values = await options.evaluateAll((els) => els.map((e) => e.getAttribute("data-value")));
    // proposed → only testing is legal; supported/refuted are impossible
    expect(values.sort()).toEqual(["proposed", "testing"]);
    await page.keyboard.press("Escape");
  });

  await test.step("citation pin: ref picker, excerpt prefill, confidence, model stamp", async () => {
    await page.locator("#main-content").getByRole("button", { name: "Anclar a cita" }).first().click();
    await expect(page.locator(".rc-modal")).toBeVisible();
    // the excerpt is prefilled with the claim text
    await expect(page.locator("#pin-excerpt")).toHaveValue(/0\.3 BLEU/);
    await pickRc(page, "pin-ref", "r1"); // "2017 — Attention Is All You Need"
    await pickRc(page, "pin-confidence", "0.9");
    await page.locator('.rc-modal [onclick="RC.submitPin()"]').click();
    await expect(page.locator(".rc-modal")).not.toBeVisible();
  });

  await test.step("the pinned row renders its citation chip + confidence attribution", async () => {
    await expect(page.locator("#main-content")).toContainText("Vaswani et al. 2017");
    await expect(page.locator("#main-content")).toContainText("90% · GLM-5.3");
    await expect(page.locator("#main-content")).not.toContainText("sin ancla");
  });

  await test.step("transition proposed → testing asks for a basis and re-folds", async () => {
    setPromptText(page, "Trial 1 ran — the measures held at 32k.");
    await pickRc(page, "hyp-status-1", "testing");
    await expect(page.locator("#main-content")).toContainText("En prueba");
    await expect(page.locator("#main-content")).toContainText("Trial 1 ran — the measures held at 32k.");
  });
});

test("numerical pin anchors an artifact with a mono digest and confidence", async ({ page }) => {
  await createMission(page, MISSION);
  await openBoard(page, MISSION);
  await addHypothesis(page, "Sparse attention matches full attention at 32k.", "H-1");
  await addClaim(page, "Sparse attention runs within 0.3 BLEU of full attention.", "CLAIMS-1");

  await page.locator("#main-content").getByRole("button", { name: "Anclar a cita" }).first().click();
  await expect(page.locator(".rc-modal")).toBeVisible();

  await test.step("flip the kind toggle to the numerical pin", async () => {
    await page.locator('.rc-modal [onclick="RC.setPinKind(\'numerical\')"]').click();
    await expect(page.locator("#pin-artifact")).toBeVisible();
    await page.locator("#pin-artifact").fill("runs/007/table-3.csv");
    await page.locator("#pin-content").fill("accuracy: 0.912, ±0.006, n=5 seeds");
    await page.locator('.rc-modal [onclick="RC.submitPin()"]').click();
    await expect(page.locator(".rc-modal")).not.toBeVisible();
  });

  await test.step("the numerical chip shows the digest mono prefix", async () => {
    await expect(page.locator("#main-content")).toContainText("50% · GLM-5.3");
    // digest chip: first 10 hex chars + ellipsis
    await expect(page.locator("#main-content .font-mono", { hasText: "…" })).toBeVisible();
    await expect(page.locator("#main-content")).not.toContainText("sin ancla");
  });
});

test("typed relations render as chips on both endpoints", async ({ page }) => {
  await createMission(page, MISSION);
  await openBoard(page, MISSION);
  await addHypothesis(page, "Sparse attention matches full attention at 32k.", "H-1");
  await addHypothesis(page, "Grounded generation still hallucinates under distribution shift.", "H-2");

  await test.step("relate H-1 → H-2 as contradicts", async () => {
    // the first card's Relate button
    await page
      .locator("#main-content p", { hasText: "Sparse attention matches" })
      .first()
      .locator("..")
      .getByRole("button", { name: "Relacionar" })
      .click();
    await expect(page.locator(".rc-modal")).toBeVisible();
    await pickRcByText(page, "relate-to", "H-2");
    await pickRc(page, "relate-kind", "contradicts");
    await page.locator(".rc-modal").getByRole("button", { name: "Relacionar" }).click();
    await expect(page.locator(".rc-modal")).not.toBeVisible();
  });

  await test.step("both cards carry the directed relation chips", async () => {
    await expect(page.locator("#main-content")).toContainText("contradice a H-2");
    await expect(page.locator("#main-content")).toContainText("contradicha por H-1");
  });
});

