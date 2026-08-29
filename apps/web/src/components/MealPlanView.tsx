import { useMemo, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { canMakeRecipe } from "@/lib/pantry";
import type { MealPlanEntry, MealType, PantryItem, Recipe } from "@/lib/schema";

const selectClassName =
  "flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

const MEAL_TYPES: MealType[] = ["breakfast", "lunch", "dinner"];

const MEAL_TYPE_LABELS: Record<MealType, string> = {
  breakfast: "Breakfast",
  lunch: "Lunch",
  dinner: "Dinner",
};

function toDateKey(date: Date): string {
  // Local date components rather than toISOString(): the days array is built
  // from local midnights, and toISOString() converts to UTC first, which
  // shifts the key by a day whenever the local timezone isn't UTC.
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function startOfWeek(date: Date): Date {
  const result = new Date(date);
  result.setHours(0, 0, 0, 0);
  const weekday = result.getDay(); // 0 = Sunday .. 6 = Saturday
  const diffToMonday = weekday === 0 ? -6 : 1 - weekday;
  result.setDate(result.getDate() + diffToMonday);
  return result;
}

function formatDayLabel(date: Date): string {
  return new Intl.DateTimeFormat("en-GB", {
    weekday: "short",
    day: "numeric",
    month: "short",
  }).format(date);
}

interface MealPlanViewProps {
  mealPlan: MealPlanEntry[];
  recipes: Recipe[];
  pantryItems: PantryItem[];
  familyName: string | null;
  isShared: boolean;
  onAssign: (date: string, mealType: MealType, recipeId: string) => Promise<void>;
  onRemove: (id: string) => Promise<void>;
}

export function MealPlanView({
  mealPlan,
  recipes,
  pantryItems,
  familyName,
  isShared,
  onAssign,
  onRemove,
}: MealPlanViewProps) {
  const [weekOffset, setWeekOffset] = useState(0);

  const days = useMemo(() => {
    const monday = startOfWeek(new Date());
    monday.setDate(monday.getDate() + weekOffset * 7);

    return Array.from({ length: 7 }, (_, index) => {
      const date = new Date(monday);
      date.setDate(date.getDate() + index);
      return date;
    });
  }, [weekOffset]);

  const entriesBySlot = useMemo(() => {
    const map = new Map<string, MealPlanEntry[]>();
    for (const entry of mealPlan) {
      const key = `${entry.date}:${entry.mealType}`;
      const list = map.get(key) ?? [];
      list.push(entry);
      map.set(key, list);
    }
    return map;
  }, [mealPlan]);

  const recipesById = useMemo(
    () => new Map(recipes.map((recipe) => [recipe.id, recipe] as const)),
    [recipes],
  );

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="text-lg font-semibold tracking-normal">
            {familyName ? `${familyName} plan` : "Meal plan"}
          </h2>
          {isShared ? <Badge variant="outline">Family shared</Badge> : null}
        </div>
        <div className="flex gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => setWeekOffset((value) => value - 1)}
          >
            Previous week
          </Button>
          <Button type="button" variant="outline" size="sm" onClick={() => setWeekOffset(0)}>
            This week
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => setWeekOffset((value) => value + 1)}
          >
            Next week
          </Button>
        </div>
      </div>

      <div className="overflow-x-auto pb-2">
        <div className="grid min-w-[1120px] grid-cols-7 gap-3">
          {days.map((date) => {
            const dateKey = toDateKey(date);

            return (
              <Card key={dateKey} className="min-h-full">
                <CardHeader>
                  <CardTitle className="text-base">{formatDayLabel(date)}</CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">
                  {MEAL_TYPES.map((mealType) => {
                    const entries = entriesBySlot.get(`${dateKey}:${mealType}`) ?? [];

                    return (
                      <div key={mealType} className="space-y-2">
                        <p className="text-xs font-medium uppercase text-muted-foreground">
                          {MEAL_TYPE_LABELS[mealType]}
                        </p>
                        {entries.map((entry) => {
                          const recipe = recipesById.get(entry.recipeId) ?? null;
                          const makeable = recipe ? canMakeRecipe(recipe, pantryItems) : null;

                          return (
                            <div
                              key={entry.id}
                              className="flex items-center justify-between gap-2 rounded-md border bg-background p-2"
                            >
                              <div>
                                <p className="text-sm font-medium">
                                  {recipe?.title ?? entry.recipeId}
                                </p>
                                {makeable != null ? (
                                  <Badge
                                    variant={makeable ? "default" : "outline"}
                                    className="mt-1"
                                  >
                                    {makeable ? "Can make now" : "Missing ingredients"}
                                  </Badge>
                                ) : null}
                              </div>
                              <Button
                                type="button"
                                variant="ghost"
                                size="sm"
                                onClick={() => void onRemove(entry.id)}
                              >
                                Remove
                              </Button>
                            </div>
                          );
                        })}
                        <select
                          className={selectClassName}
                          value=""
                          onChange={(event) => {
                            const recipeId = event.target.value;
                            if (recipeId) {
                              void onAssign(dateKey, mealType, recipeId);
                              event.target.value = "";
                            }
                          }}
                        >
                          <option value="" disabled>
                            Add recipe
                          </option>
                          {recipes.map((recipeOption) => (
                            <option key={recipeOption.id} value={recipeOption.id}>
                              {recipeOption.title}
                            </option>
                          ))}
                        </select>
                      </div>
                    );
                  })}
                </CardContent>
              </Card>
            );
          })}
        </div>
      </div>
    </div>
  );
}
