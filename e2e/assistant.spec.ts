// (j) Assistant: the unconfigured provider gate, then a real chat over the
// configured provider with the mission-scoping chip, the skill chip, the
// model picker, and the attachment chips.
import { test, expect, nav, pickRc, pickRcByText } from "./helpers";

test("assistant shows the configure-provider state when no provider is configured", async ({ page }) => {
  await nav(page, "assistant");
  await expect(
    page.locator("#main-content").getByText("Configura un proveedor para empezar"),
  ).toBeVisible();
  await expect(
    page.locator("#main-content").getByRole("button", { name: "Configurar en Ajustes → IA" }),
  ).toBeVisible();
  // no composer until a real provider exists
  await expect(page.locator("#chat-input")).toHaveCount(0);
});

test("a configured provider unlocks the chat: skill, mission scope, model picker, attachments", async ({ page }) => {
  await test.step("configure an API provider in Ajustes → IA", async () => {
    await nav(page, "settings");
    await page.locator('[onclick="RC.setSettingsTab(\'ai\')"]').click();
    await expect(page.locator("#ai-key-input")).toBeVisible();
    await page.locator("#ai-key-input").fill("sk-e2e-test");
    await page.locator('[oninput="RC.aiDraftField(\'model\', this.value)"]').fill("gpt-4o-mini");
    await page.locator("#main-content").getByRole("button", { name: "Guardar proveedor" }).click();
    await expect(page.locator("#main-content")).toContainText("activo");
  });

  await test.step("the assistant now renders the composer and the model picker", async () => {
    await nav(page, "assistant");
    await expect(page.locator("#chat-input")).toBeVisible();
    await expect(page.locator('.rc-select[data-select-id="chat-model-select"]')).toBeVisible();
  });

  await test.step("mission scope -> context chip; skill -> skill chip", async () => {
    await pickRcByText(page, "chat-mission-select", "M-1");
    await expect(page.locator("#main-content")).toContainText("contexto: M-1 · board sincronizado");
    await pickRc(page, "chat-skill-select", "librarian");
    await expect(page.locator("#main-content")).toContainText("skill: Bibliotecario");
  });

  await test.step("an attachment lands as a composer chip", async () => {
    await page.locator("#chat-file-input").setInputFiles({
      name: "notes.md",
      mimeType: "text/markdown",
      buffer: Buffer.from("# e2e notes\nthe gain holds"),
    });
    await expect(page.locator("#chat-composer")).toContainText("notes.md");
    await expect(page.locator("#chat-composer")).toContainText("…"); // pending classification chip
  });

  await test.step("the send lands a user bubble + the honest attributed reply", async () => {
    await page.locator("#chat-input").fill("What does the board say?");
    await page.locator("#chat-send").click();
    await expect(page.locator(".rc-bubble-user", { hasText: "What does the board say?" })).toBeVisible();
    await expect(page.locator(".rc-bubble-ai").first()).toBeVisible();
    await expect(page.locator("#main-content")).toContainText("Entendido");
    // the scoped context echo: the mission board, the skill, the attachment
    await expect(page.locator("#main-content")).toContainText("contexto: tablero M-1 sincronizado");
    await expect(page.locator("#main-content")).toContainText("skill: librarian");
    await expect(page.locator("#main-content")).toContainText("adjuntos: 1");
  });
});