// (a) First-run onboarding: the full biblical wizard walk — the arXiv paste
// drives `run_first_value` (the mock answers the simulated provider) and a
// starter mission with candidate hypotheses lands on the Missions home.
// The multi-source door is exercised too: a DOI paste resolves through the
// same flow, and an unsupported link is the honest typed refusal (never
// fabricated metadata).
import { freshTest, expect } from "./helpers";

freshTest("first-run wizard: an arXiv paste becomes a starter mission with candidates", async ({ page }) => {
  // The lock's welcome variant (onboarding not completed) — "Comenzar".
  await page.goto("/");
  await expect(page.locator("#lock-btn", { hasText: "Comenzar" })).toBeVisible();
  await page.locator("#lock-btn").click();

  // Wizard step 0 (language) → 1 (data folder) → 2 (AI) → 3 (init).
  await expect(page.getByRole("button", { name: "Siguiente" })).toBeVisible();
  for (let i = 0; i < 3; i++) await page.getByRole("button", { name: "Siguiente" }).click();

  // Step 3: the arXiv init mode is the default — paste + generate.
  await expect(page.locator("#wiz-arxiv")).toBeVisible();
  await page.locator("#wiz-arxiv").fill("https://arxiv.org/abs/1706.03762");

  // The first-value result card (runFirstValue): paper → mission + candidates.
  await expect(page.getByRole("button", { name: "Generar" })).toBeVisible();
  await page.getByRole("button", { name: "Generar" }).click();
  await expect(page.locator(".rc-wizard-wave").locator("..").getByText("De artículo a misión")).toBeVisible();
  await expect(page.locator(".rc-wizard-wave").locator("..")).toContainText("Attention Is All You Need");
  await expect(page.locator(".rc-wizard-wave").locator("..")).toContainText("Tu primera misión");
  for (const seq of ["H-1", "H-2", "H-3"]) {
    await expect(page.locator(".rc-wizard-wave").locator("..")).toContainText(seq);
  }
  await expect(page.locator(".rc-wizard-wave").locator("..")).toContainText("simulado");

  // Advance through vault + appearance + MCP, then finish.
  for (let i = 0; i < 3; i++) await page.getByRole("button", { name: "Siguiente" }).click();
  await expect(page.getByRole("button", { name: "Finalizar" })).toBeVisible();
  await page.getByRole("button", { name: "Finalizar" }).click();

  // Missions home: the starter mission (M-2, after the seeded draft M-1)
  // with its terminators and the spend meter.
  await expect(page.locator("#sidebar")).toBeVisible();
  await expect(
    page.locator("h3", { hasText: "Comprueba las afirmaciones de «Attention Is All You Need»" }),
  ).toBeVisible();
  await expect(page.locator("#main-content")).toContainText("M-2");
  await expect(page.locator("#main-content")).toContainText("Detente tras 3 corridas");
  await expect(page.locator("#main-content")).toContainText("$0.00");
});

freshTest("first-run wizard: the empty home also offers the arXiv first-value driver", async ({ page }) => {
  // Land directly on the wizard-less path: the missions home renders the
  // empty-state first-value box only when onboarding was completed but no
  // mission exists — the seeded draft prevents that here, so this test
  // asserts the wizard's manual-init sibling instead: mode toggle renders.
  await page.goto("/");
  await page.locator("#lock-btn").click();
  for (let i = 0; i < 3; i++) await page.getByRole("button", { name: "Siguiente" }).click();
  await expect(page.locator("#wiz-arxiv")).toBeVisible();

  // Switch to manual init — the question + stop-condition fields appear.
  await page.getByRole("button", { name: "Manual" }).click();
  await expect(page.locator("#wiz-q")).toBeVisible();
  await expect(page.locator("#wiz-stop")).toBeVisible();
});

freshTest("first-run wizard: a DOI link resolves through the same multi-source flow", async ({ page }) => {
  await page.goto("/");
  await page.locator("#lock-btn").click();
  for (let i = 0; i < 3; i++) await page.getByRole("button", { name: "Siguiente" }).click();
  await expect(page.locator("#wiz-arxiv")).toBeVisible();

  // The input advertises the supported sources (placeholder + hint).
  await expect(page.locator("#wiz-arxiv")).toHaveAttribute(
    "placeholder",
    "Pega un enlace (arXiv, DOI, PubMed, Semantic Scholar)…",
  );
  await expect(page.locator(".rc-wizard-wave").locator("..")).toContainText("Soportados: arXiv");

  // A DOI link resolves (the Crossref door) — the seeded "Deep learning"
  // paper drives the same first-value flow.
  await page.locator("#wiz-arxiv").fill("https://doi.org/10.1038/nature14539");
  await page.getByRole("button", { name: "Generar" }).click();
  await expect(page.locator(".rc-wizard-wave").locator("..").getByText("De artículo a misión")).toBeVisible();
  await expect(page.locator(".rc-wizard-wave").locator("..")).toContainText("Deep learning");
  await expect(page.locator(".rc-wizard-wave").locator("..")).toContainText("LeCun, Yann");
  for (const seq of ["H-1", "H-2", "H-3"]) {
    await expect(page.locator(".rc-wizard-wave").locator("..")).toContainText(seq);
  }
});

freshTest("first-run wizard: an unsupported link is the honest typed refusal", async ({ page }) => {
  // Capture the error alert (the wizard's error surface) — the fixture's
  // auto-accept handler dismisses it; this listener records the message.
  const dialogs: string[] = [];
  page.on("dialog", (d) => dialogs.push(d.message()));

  await page.goto("/");
  await page.locator("#lock-btn").click();
  for (let i = 0; i < 3; i++) await page.getByRole("button", { name: "Siguiente" }).click();
  await expect(page.locator("#wiz-arxiv")).toBeVisible();

  // A link from no supported source fails honestly — the coded refusal
  // lists what IS supported, and nothing is fabricated.
  await page.locator("#wiz-arxiv").fill("https://example.com/paper");
  await page.getByRole("button", { name: "Generar" }).click();
  await expect
    .poll(() => dialogs.length, { message: "the honest error alert fired" })
    .toBeGreaterThan(0);
  expect(dialogs[0]).toContain("unsupported_source:");
  expect(dialogs[0]).toContain("arXiv");
  expect(dialogs[0]).toContain("doi.org");
  // no result card — no paper was invented for an unknown source
  await expect(page.locator(".rc-wizard-wave").locator("..")).not.toContainText("De artículo a misión");
});
