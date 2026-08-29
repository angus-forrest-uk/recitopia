import { describe, expect, it } from "vitest";
import { seedCatalogue } from "@/data/seed";
import {
  catalogueSchema,
  cookbookSchema,
  filterRecipes,
  ingredientSchema,
  recipeImportSchema,
  recomputeRecipe,
} from "@/lib/schema";

describe("recipe model derivations", () => {
  it("recomputes recipe and per-serving costs from ingredients", () => {
    const recipe = seedCatalogue.recipes.find((item) => item.id === "tomato-coconut-dal");
    expect(recipe).toBeDefined();
    if (!recipe) {
      throw new Error("Expected tomato-coconut-dal seed recipe");
    }

    const recomputed = recomputeRecipe(recipe);

    expect(recomputed.costCents).toBe(355);
    expect(recomputed.costPerServingCents).toBe(89);
    expect(recomputed.totalMinutes).toBe(45);
    expect(recomputed.searchableText).toContain("red lentils");
    expect(recomputed.cacheKey).toMatch(/^cache-/);
  });

  it("searches across generated searchable text", () => {
    const recipes = filterRecipes(seedCatalogue.recipes, "chilli crisp", null);

    expect(recipes.map((recipe) => recipe.id)).toEqual(["lemon-tahini-noodles"]);
  });

  it("filters by tag after recomputation", () => {
    const recipes = filterRecipes(seedCatalogue.recipes, "", "freezer");

    expect(recipes).toHaveLength(1);
    expect(recipes[0].title).toBe("Tomato Coconut Dal");
  });
});

describe("failsafe validation", () => {
  it("rejects negative ingredient costs", () => {
    const parsed = ingredientSchema.safeParse({
      id: "bad-cost",
      displayName: "Impossible ingredient",
      item: "impossible ingredient",
      quantity: 1,
      unit: "each",
      preparation: null,
      section: null,
      unitCostCents: -1,
      estimatedCostCents: null,
    });

    expect(parsed.success).toBe(false);
  });

  it("coerces non-positive recipe quantities to null", () => {
    const parsed = ingredientSchema.parse({
      id: "to-taste",
      displayName: "wasabi paste, to taste",
      item: "wasabi paste",
      quantity: 0,
      unit: null,
      preparation: "to taste",
      section: null,
      unitCostCents: null,
      estimatedCostCents: null,
    });

    expect(parsed.quantity).toBeNull();
  });

  it("parses refined quantity review metadata", () => {
    const parsed = ingredientSchema.parse({
      id: "halibut",
      displayName: "00g halibut, cut into chunks",
      item: "halibut",
      quantity: null,
      quantityText: null,
      quantityMin: null,
      quantityMax: null,
      quantityKind: "unknown",
      quantityReviewStatus: "needs_review",
      quantityReviewReason: "possible_ocr_leading_digit_loss",
      unit: "g",
      preparation: "cut into chunks",
      section: null,
      unitCostCents: null,
      estimatedCostCents: null,
    });

    expect(parsed.quantityKind).toBe("unknown");
    expect(parsed.quantityReviewStatus).toBe("needs_review");
    expect(parsed.quantityReviewReason).toBe("possible_ocr_leading_digit_loss");
  });

  it("defaults refined quantity metadata for older ingredients", () => {
    const parsed = ingredientSchema.parse({
      id: "rice",
      displayName: "200g rice",
      item: "rice",
      quantity: 200,
      unit: "g",
      preparation: null,
      section: null,
      unitCostCents: null,
      estimatedCostCents: null,
    });

    expect(parsed.quantityKind).toBe("exact");
    expect(parsed.quantityReviewStatus).toBe("parsed");
    expect(parsed.quantityText).toBeNull();
  });
});

describe("image import schema", () => {
  it("parses an import draft with OCR issues", () => {
    const recipe = seedCatalogue.recipes[0];
    const parsed = recipeImportSchema.parse({
      id: "import-test",
      status: "draft_ready",
      fileName: "page.jpg",
      mimeType: "image/jpeg",
      imagePath: "data/imports/import-test/original.jpg",
      ocrEngine: "paddleocr:paddle",
      ocrText: "Recipe OCR text",
      ocrJson: "{}",
      draft: recipe,
      validationIssues: [
        {
          field: "yieldQuantity",
          message: "Yield was not confidently detected.",
          severity: "warning",
        },
      ],
      createdAt: "2026-07-07T10:00:00.000Z",
      updatedAt: "2026-07-07T10:00:00.000Z",
    });

    expect(parsed.draft?.id).toBe(recipe.id);
    expect(parsed.validationIssues[0].severity).toBe("warning");
  });
});

describe("cookbook schema", () => {
  it("accepts a newly-created cookbook with optional metadata", () => {
    const parsed = cookbookSchema.parse({
      id: "simple",
      title: "Simple",
      authorIds: ["anna-jones"],
      isbn: null,
      publisher: "Kitchen Press",
      publishedYear: 2026,
      coverImageUrl: null,
    });

    expect(parsed.authorIds).toEqual(["anna-jones"]);
  });

  it("hydrates user and family sharing metadata from the catalogue", () => {
    const parsed = catalogueSchema.parse(seedCatalogue);
    const east = parsed.cookbooks.find((cookbook) => cookbook.id === "east");

    expect(parsed.currentUserId).toBe("avery-river");
    expect(parsed.families[0].pantryShared).toBe(true);
    expect(east?.shareScope).toBe("users");
    expect(east?.sharedWithUserIds).toEqual(["avery-river"]);
  });

  it("hydrates source-first cookbook ingestion data", () => {
    const parsed = catalogueSchema.parse(seedCatalogue);
    const dal = parsed.recipes.find((recipe) => recipe.id === "tomato-coconut-dal");

    expect(parsed.cookbookImports[0].sourceKind).toBe("image_set");
    expect(parsed.cookbookPages[0].pageKind).toBe("recipe");
    expect(parsed.cookbookContentBlocks[0].kind).toBe("recipe");
    expect(parsed.cookbookMenus[0].recipes[0].recipeId).toBe("tomato-coconut-dal");
    expect(parsed.cookbookGlossaryEntries[0].aliases).toEqual(["dal"]);
    expect(parsed.cookbookIndexEntries[0].targetRecipeId).toBe("tomato-coconut-dal");
    expect(parsed.cookbookCrossReferences[0].relationKind).toBe("index_reference");
    expect(dal?.headnote).toContain("preserved cookbook page block");
    expect(dal?.sourcePageSpans[0].pageId).toBe("east-page-086");
  });
});
