import { expect, test } from "@playwright/test";

test("searches recipes, opens details, and records a cook event", async ({ page }) => {
  const apiCatalogue = await page.request.get("/api/catalogue");
  expect(apiCatalogue.ok()).toBe(true);
  const payload = (await apiCatalogue.json()) as {
    recipes: unknown[];
    users: unknown[];
    families: unknown[];
  };
  expect(payload.recipes).toHaveLength(2);
  expect(payload.users).toHaveLength(2);
  expect(payload.families).toHaveLength(1);

  await page.goto("/");

  await expect(page.getByRole("heading", { name: "recitopia" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Compendium" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Library" })).toBeVisible();
  await expect(page.getByRole("button", { name: /Tomato Coconut Dal/ })).toBeVisible();

  await page.getByLabel("Search recipes").fill("lentils");
  await expect(page.getByRole("button", { name: /Tomato Coconut Dal/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Lemon Tahini Noodles/ })).toHaveCount(0);

  await page.getByRole("button", { name: /Tomato Coconut Dal/ }).click();
  await expect(page.getByRole("heading", { name: "Tomato Coconut Dal" })).toBeVisible();
  await expect(page.getByText("£0.89")).toBeVisible();

  await page.getByRole("button", { name: "Mark made" }).click();
  await expect(page.getByText("Made 9 times")).toBeVisible();

  await page.getByRole("button", { exact: true, name: "Pantry" }).click();
  await expect(page.getByText("River House pantry")).toBeVisible();
  await expect(page.getByText("Family shared")).toBeVisible();

  await page.getByRole("button", { exact: true, name: "Plan" }).click();
  await expect(page.getByRole("heading", { name: "River House plan" })).toBeVisible();
  await expect(page.getByText("Family shared")).toBeVisible();
});

test("reviews cookbook source pages side by side and patches review status", async ({ page }) => {
  // Preflight the live API so seed fallback cannot mask backend failures.
  const pageText = await page.request.get("/api/cookbook-pages/east-page-086/text");
  expect(pageText.ok()).toBe(true);
  const textPayload = (await pageText.json()) as { ocrText: string };
  expect(textPayload.ocrText).toContain("Tomato Coconut Dal");

  await page.goto("/");
  await page.getByRole("button", { name: "Library" }).click();
  await page.getByText("East", { exact: true }).click();

  await expect(page.getByText("Source review")).toBeVisible();
  await expect(page.getByRole("option", { name: /Page 86/ })).toBeVisible();

  // Full OCR text loads on demand into the correction editor.
  await expect(page.getByLabel("OCR text")).toHaveValue(/Tomato Coconut Dal/);

  // Corrections persist through the live API and update the stored text.
  await page.getByLabel("OCR text").fill("Tomato Coconut Dal\nCorrected via e2e");
  await page.getByRole("button", { name: "Save correction" }).click();
  await expect(page.getByText(/OCR text saved/)).toBeVisible();
  const correctedText = await page.request.get("/api/cookbook-pages/east-page-086/text");
  expect(((await correctedText.json()) as { ocrText: string }).ocrText).toContain(
    "Corrected via e2e",
  );

  // Source entities render beyond count badges.
  await expect(page.getByText("Menus (1)")).toBeVisible();
  await expect(page.getByText("Glossary (1)")).toBeVisible();

  // Review-status patch persists through the live API.
  await page.getByLabel("Review status").selectOption("needs_ocr_fix");
  await expect(
    page.getByRole("option", { name: /Page 86/ }).getByText("needs ocr fix"),
  ).toBeVisible();

  const patched = await page.request.get("/api/cookbook-pages/east-page-086/text");
  expect(patched.ok()).toBe(true);

  // Non-recipe page content can be accepted straight from the OCR pass: it
  // becomes a content block and joins the cookbook document.
  await page.getByRole("button", { name: "Accept as content" }).click();
  await expect(page.getByText(/Page accepted as paragraph content/)).toBeVisible();
  const acceptedBlocks = await page.request.get("/api/cookbooks/east/blocks");
  expect(acceptedBlocks.ok()).toBe(true);
  const blockPayload = (await acceptedBlocks.json()) as Array<{ id: string; text: string }>;
  expect(blockPayload.some((block) => block.id === "east-page-086-content")).toBe(true);

  // The single-page cookbook document embeds the extracted recipe inside its
  // section, using full block text from the live blocks endpoint.
  const blocksResponse = await page.request.get("/api/cookbooks/east/blocks");
  expect(blocksResponse.ok()).toBe(true);
  await expect(page.getByText("East — cookbook document")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Mains" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Tomato Coconut Dal" })).toBeVisible();
  await expect(
    page.getByText("Simmer lentils with tomatoes, coconut milk, water, and salt until soft."),
  ).toBeVisible();
});
