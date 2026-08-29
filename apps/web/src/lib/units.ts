export type UnitDimension = "mass" | "volume" | "count";

interface UnitDefinition {
  dimension: UnitDimension;
  toBase: number;
}

// Base units: gram (mass), millilitre (volume), each (count). Freeform units
// like "tin", "clove", "bunch", or "pinch" are intentionally left out since
// they can't be reliably converted; matching falls back to presence for those.
const UNIT_TABLE: Record<string, UnitDefinition> = {
  g: { dimension: "mass", toBase: 1 },
  gram: { dimension: "mass", toBase: 1 },
  grams: { dimension: "mass", toBase: 1 },
  kg: { dimension: "mass", toBase: 1000 },
  kilogram: { dimension: "mass", toBase: 1000 },
  kilograms: { dimension: "mass", toBase: 1000 },
  oz: { dimension: "mass", toBase: 28.3495 },
  ounce: { dimension: "mass", toBase: 28.3495 },
  ounces: { dimension: "mass", toBase: 28.3495 },
  lb: { dimension: "mass", toBase: 453.592 },
  pound: { dimension: "mass", toBase: 453.592 },
  pounds: { dimension: "mass", toBase: 453.592 },
  ml: { dimension: "volume", toBase: 1 },
  millilitre: { dimension: "volume", toBase: 1 },
  milliliter: { dimension: "volume", toBase: 1 },
  l: { dimension: "volume", toBase: 1000 },
  litre: { dimension: "volume", toBase: 1000 },
  liter: { dimension: "volume", toBase: 1000 },
  tsp: { dimension: "volume", toBase: 4.92892 },
  teaspoon: { dimension: "volume", toBase: 4.92892 },
  tbsp: { dimension: "volume", toBase: 14.7868 },
  tablespoon: { dimension: "volume", toBase: 14.7868 },
  cup: { dimension: "volume", toBase: 236.588 },
  "fl oz": { dimension: "volume", toBase: 29.5735 },
  each: { dimension: "count", toBase: 1 },
  dozen: { dimension: "count", toBase: 12 },
};

export function normalizeUnit(unit: string | null | undefined): string | null {
  if (!unit) {
    return null;
  }
  return unit.trim().toLowerCase();
}

export function unitDefinition(unit: string | null | undefined): UnitDefinition | null {
  const normalized = normalizeUnit(unit);
  if (!normalized) {
    return null;
  }
  return UNIT_TABLE[normalized] ?? null;
}

export interface BaseQuantity {
  dimension: UnitDimension;
  amount: number;
}

export function toBaseQuantity(
  quantity: number,
  unit: string | null | undefined,
): BaseQuantity | null {
  const definition = unitDefinition(unit);
  if (!definition) {
    return null;
  }
  return { dimension: definition.dimension, amount: quantity * definition.toBase };
}
