import type { Ingredient, PantryItem, Recipe } from "@/lib/schema";
import { toBaseQuantity } from "@/lib/units";

function normalizeItem(item: string): string {
  return item.trim().toLowerCase();
}

interface ItemAvailability {
  base: { dimension: string; amount: number } | null;
  anyEntry: boolean;
}

function summarizeItem(item: string, pantryItems: PantryItem[]): ItemAvailability {
  const normalized = normalizeItem(item);
  let baseAmount = 0;
  let baseDimension: string | null = null;
  let anyEntry = false;

  for (const pantryItem of pantryItems) {
    // Leftovers are whole meals to reheat, not ingredients other recipes can draw on.
    if (pantryItem.category === "leftover") {
      continue;
    }
    if (normalizeItem(pantryItem.item) !== normalized) {
      continue;
    }
    anyEntry = true;

    if (pantryItem.quantity == null) {
      continue;
    }

    const base = toBaseQuantity(pantryItem.quantity, pantryItem.unit);
    if (base && (baseDimension == null || baseDimension === base.dimension)) {
      baseDimension = base.dimension;
      baseAmount += base.amount;
    }
  }

  return {
    base: baseDimension ? { dimension: baseDimension, amount: baseAmount } : null,
    anyEntry,
  };
}

export function canMakeIngredient(ingredient: Ingredient, pantryItems: PantryItem[]): boolean {
  const availability = summarizeItem(ingredient.item, pantryItems);

  if (!availability.anyEntry) {
    return false;
  }

  if (ingredient.quantity == null) {
    // Freeform "as needed" ingredients: presence in the pantry is enough.
    return true;
  }

  const required = toBaseQuantity(ingredient.quantity, ingredient.unit);
  if (required && availability.base && required.dimension === availability.base.dimension) {
    return availability.base.amount >= required.amount;
  }

  // Unconvertible or mismatched units (tins, cloves, bunches, ...): fall back
  // to presence rather than risk a false negative from a system that can't
  // reason about the units involved.
  return true;
}

export function canMakeRecipe(recipe: Recipe, pantryItems: PantryItem[]): boolean {
  return recipe.ingredients.every((ingredient) => canMakeIngredient(ingredient, pantryItems));
}

export function missingIngredients(recipe: Recipe, pantryItems: PantryItem[]): Ingredient[] {
  return recipe.ingredients.filter((ingredient) => !canMakeIngredient(ingredient, pantryItems));
}

function baseUnitLabel(dimension: string): string {
  switch (dimension) {
    case "mass":
      return "g";
    case "volume":
      return "ml";
    case "count":
      return "each";
    default:
      return "";
  }
}

function roundForDisplay(amount: number): number {
  return Math.round(amount * 100) / 100;
}

export function describePantryAvailability(
  ingredient: Ingredient,
  pantryItems: PantryItem[],
): string {
  const normalized = normalizeItem(ingredient.item);
  const matches = pantryItems.filter(
    (item) => item.category !== "leftover" && normalizeItem(item.item) === normalized,
  );

  if (matches.length === 0) {
    return "Not in pantry";
  }

  let baseAmount = 0;
  let baseDimension: string | null = null;
  const rawParts: string[] = [];

  for (const item of matches) {
    if (item.quantity == null) {
      continue;
    }
    const base = toBaseQuantity(item.quantity, item.unit);
    if (base && (baseDimension == null || baseDimension === base.dimension)) {
      baseDimension = base.dimension;
      baseAmount += base.amount;
    } else {
      rawParts.push(`${item.quantity} ${item.unit ?? ""}`.trim());
    }
  }

  const parts: string[] = [];
  if (baseDimension) {
    parts.push(`${roundForDisplay(baseAmount)} ${baseUnitLabel(baseDimension)}`);
  }
  parts.push(...rawParts);

  return parts.length > 0 ? `${parts.join(" + ")} available` : "In pantry (no quantity recorded)";
}
