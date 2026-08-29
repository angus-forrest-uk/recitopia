import type { Catalogue, Recipe } from "@/lib/schema";

export interface IngredientOption {
  item: string;
  displayName: string;
}

export function distinctIngredients(catalogue: Catalogue): IngredientOption[] {
  const seen = new Map<string, IngredientOption>();

  for (const recipe of catalogue.recipes) {
    for (const ingredient of recipe.ingredients) {
      const key = ingredient.item.trim().toLowerCase();
      if (!seen.has(key)) {
        seen.set(key, { item: ingredient.item, displayName: ingredient.item });
      }
    }
  }

  return Array.from(seen.values()).sort((a, b) => a.displayName.localeCompare(b.displayName));
}

export function searchIngredientOptions(
  options: IngredientOption[],
  query: string,
): IngredientOption[] {
  const normalized = query.trim().toLowerCase();
  if (normalized.length === 0) {
    return options;
  }
  return options.filter((option) => option.displayName.toLowerCase().includes(normalized));
}

export function recipeHasIngredient(recipe: Recipe, item: string): boolean {
  const normalized = item.trim().toLowerCase();
  return recipe.ingredients.some(
    (ingredient) => ingredient.item.trim().toLowerCase() === normalized,
  );
}

export function filterByIngredients(recipes: Recipe[], items: string[]): Recipe[] {
  if (items.length === 0) {
    return recipes;
  }
  return recipes.filter((recipe) => items.every((item) => recipeHasIngredient(recipe, item)));
}
