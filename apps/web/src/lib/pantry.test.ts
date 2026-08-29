import { describe, expect, it } from "vitest";
import { canMakeRecipe, missingIngredients } from "@/lib/pantry";
import type { PantryItem, Recipe } from "@/lib/schema";

const defaultIngredientSource = {
  position: null,
  quantityText: null,
  quantityMin: null,
  quantityMax: null,
  quantityKind: "exact" as const,
  quantityReviewStatus: "parsed" as const,
  quantityReviewReason: null,
  optional: false,
  alternativeText: null,
  sourceLine: null,
  sourcePageId: null,
};

const defaultStepSource = {
  sourcePageId: null,
  sourceLineStart: null,
  sourceLineEnd: null,
};

const recipe: Recipe = {
  id: "test-soup",
  title: "Test Soup",
  subtitle: null,
  alternateNames: [],
  cookbookId: "east",
  authorIds: [],
  pageStart: null,
  pageEnd: null,
  sourceLabel: "Test source",
  headnote: null,
  servingContext: null,
  yieldQuantity: 4,
  yieldUnit: "servings",
  prepMinutes: 10,
  cookMinutes: 10,
  totalMinutes: 20,
  cuisine: null,
  category: null,
  tags: [],
  searchableText: "",
  sourceBlockId: null,
  sourcePageSpans: [],
  componentRecipeIds: [],
  picturedPageNumber: null,
  extractionStatus: "verified",
  images: [],
  ingredients: [
    {
      id: "lentils",
      ...defaultIngredientSource,
      displayName: "300 g red lentils",
      item: "red lentils",
      quantity: 300,
      unit: "g",
      preparation: null,
      section: null,
      unitCostCents: null,
      estimatedCostCents: null,
    },
    {
      id: "spice-base",
      ...defaultIngredientSource,
      displayName: "Garlic and ginger",
      item: "spice base",
      quantity: null,
      unit: null,
      preparation: null,
      section: null,
      unitCostCents: null,
      estimatedCostCents: null,
    },
    {
      id: "stock-tin",
      ...defaultIngredientSource,
      displayName: "1 tin stock",
      item: "stock",
      quantity: 1,
      unit: "tin",
      preparation: null,
      section: null,
      unitCostCents: null,
      estimatedCostCents: null,
    },
  ],
  steps: [{ id: "cook", ...defaultStepSource, position: 1, section: null, text: "Cook it." }],
  notes: [],
  lastMadeAt: null,
  timesMade: 0,
  costCents: null,
  costPerServingCents: null,
  cacheKey: "uncached",
  cacheUpdatedAt: null,
};

function pantryItem(overrides: Partial<PantryItem>): PantryItem {
  return {
    id: "pantry-1",
    item: "red lentils",
    displayName: "Red lentils",
    quantity: null,
    unit: null,
    category: "raw",
    sourceRecipeId: null,
    notes: null,
    expiresAt: null,
    addedAt: "2026-07-07T09:00:00.000Z",
    ownerUserId: null,
    familyId: null,
    ...overrides,
  };
}

describe("canMakeRecipe", () => {
  it("is false when a quantified ingredient is entirely missing", () => {
    expect(canMakeRecipe(recipe, [])).toBe(false);
    expect(missingIngredients(recipe, []).map((ingredient) => ingredient.item)).toEqual([
      "red lentils",
      "spice base",
      "stock",
    ]);
  });

  it("converts compatible units before comparing quantities", () => {
    const pantry: PantryItem[] = [
      pantryItem({ id: "p1", item: "red lentils", quantity: 0.35, unit: "kg" }),
      pantryItem({ id: "p2", item: "spice base", quantity: null, unit: null }),
      pantryItem({ id: "p3", item: "stock", quantity: 1, unit: "tin" }),
    ];

    expect(canMakeRecipe(recipe, pantry)).toBe(true);
  });

  it("fails quantity matching when pantry has less than required after conversion", () => {
    const pantry: PantryItem[] = [
      pantryItem({ id: "p1", item: "red lentils", quantity: 100, unit: "g" }),
      pantryItem({ id: "p2", item: "spice base" }),
      pantryItem({ id: "p3", item: "stock", quantity: 1, unit: "tin" }),
    ];

    expect(canMakeRecipe(recipe, pantry)).toBe(false);
    expect(missingIngredients(recipe, pantry).map((ingredient) => ingredient.item)).toEqual([
      "red lentils",
    ]);
  });

  it("ignores leftover pantry entries when matching ingredients", () => {
    const pantry: PantryItem[] = [
      pantryItem({ id: "p1", item: "red lentils", quantity: 1, unit: "kg", category: "leftover" }),
      pantryItem({ id: "p2", item: "spice base" }),
      pantryItem({ id: "p3", item: "stock", quantity: 1, unit: "tin" }),
    ];

    expect(canMakeRecipe(recipe, pantry)).toBe(false);
  });
});
