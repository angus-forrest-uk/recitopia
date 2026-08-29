import {
  AlertTriangle,
  BookOpen,
  CalendarDays,
  ChefHat,
  Clock3,
  Coins,
  Image as ImageIcon,
  LogOut,
  RotateCw,
  Search,
  Settings,
  User,
  Utensils,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { CookbookOverview } from "@/components/CookbookOverview";
import { HistoryView } from "@/components/HistoryView";
import { MealPlanView } from "@/components/MealPlanView";
import { type PantryItemFormInput, PantryView } from "@/components/PantryView";
import { RecipeEditor } from "@/components/RecipeEditor";
import { RecipeImportPanel } from "@/components/RecipeImportPanel";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Tooltip } from "@/components/ui/tooltip";
import {
  addMealPlanEntry,
  addPantryItem,
  commitRecipeImport,
  createCookbook,
  createRecipe,
  deleteMealPlanEntry,
  deletePantryItem,
  deleteRecipe,
  loadCatalogue,
  loadCookLog,
  loadMealPlan,
  loadPantry,
  type MarkMadeInput,
  markRecipeMade,
  patchPantryItem,
  updateRecipe,
} from "@/lib/api";
import {
  distinctIngredients,
  filterByIngredients,
  searchIngredientOptions,
} from "@/lib/ingredients";
import { canMakeRecipe, describePantryAvailability, missingIngredients } from "@/lib/pantry";
import {
  type Catalogue,
  type Cookbook,
  type CookLogEntry,
  filterRecipes,
  type MealPlanEntry,
  type MealType,
  type PantryItem,
  type Recipe,
  type RecipeImport,
  recomputeRecipe,
} from "@/lib/schema";
import { formatDate, formatMinutes, formatMoney } from "@/lib/utils";

const selectClassName =
  "flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

const pageGridClassName =
  "mx-auto grid w-full max-w-[1440px] grid-cols-12 gap-6 px-4 sm:px-6 lg:px-[80px]";

function formatIngredientQuantity(ingredient: Recipe["ingredients"][number]) {
  if (
    ingredient.quantityKind === "range" &&
    ingredient.quantityMin != null &&
    ingredient.quantityMax != null
  ) {
    const range = `${ingredient.quantityMin}-${ingredient.quantityMax}`;
    return ingredient.unit ? `${range} ${ingredient.unit}` : range;
  }
  if (ingredient.quantity != null && ingredient.unit) {
    return `${ingredient.quantity} ${ingredient.unit}`;
  }
  if (ingredient.quantity != null) {
    return `${ingredient.quantity}`;
  }
  return ingredient.quantityKind === "unknown" ? "Needs review" : "As needed";
}

function App() {
  const [catalogue, setCatalogue] = useState<Catalogue | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [activeTag, setActiveTag] = useState<string | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [ingredientQuery, setIngredientQuery] = useState("");
  const [ingredientFilters, setIngredientFilters] = useState<string[]>([]);
  const [onlyMakeable, setOnlyMakeable] = useState(false);
  const [editingRecipe, setEditingRecipe] = useState<Recipe | "new" | null>(null);
  const [editorMode, setEditorMode] = useState<"create" | "update">("create");
  const [editingImportId, setEditingImportId] = useState<string | null>(null);

  const [pantryItems, setPantryItems] = useState<PantryItem[]>([]);
  const [mealPlan, setMealPlan] = useState<MealPlanEntry[]>([]);
  const [cookLog, setCookLog] = useState<CookLogEntry[]>([]);

  const refreshCatalogue = useCallback(async () => {
    setIsRefreshing(true);
    const nextCatalogue = await loadCatalogue();
    setCatalogue(nextCatalogue);
    setSelectedId((current) => current ?? nextCatalogue.recipes[0]?.id ?? null);
    setIsRefreshing(false);
  }, []);

  const refreshPantry = useCallback(async () => {
    setPantryItems(await loadPantry());
  }, []);

  const refreshMealPlan = useCallback(async () => {
    setMealPlan(await loadMealPlan());
  }, []);

  const refreshCookLog = useCallback(async () => {
    setCookLog(await loadCookLog());
  }, []);

  useEffect(() => {
    void refreshCatalogue();
    void refreshPantry();
    void refreshMealPlan();
    void refreshCookLog();
  }, [refreshCatalogue, refreshPantry, refreshMealPlan, refreshCookLog]);

  const recipes = catalogue?.recipes ?? [];
  const ingredientOptions = useMemo(
    () => (catalogue ? distinctIngredients(catalogue) : []),
    [catalogue],
  );
  const ingredientSuggestions = useMemo(
    () => searchIngredientOptions(ingredientOptions, ingredientQuery).slice(0, 8),
    [ingredientOptions, ingredientQuery],
  );
  const visibleRecipes = useMemo(() => {
    const byText = filterRecipes(recipes, query, activeTag);
    const byIngredients = filterByIngredients(byText, ingredientFilters);
    return onlyMakeable
      ? byIngredients.filter((recipe) => canMakeRecipe(recipe, pantryItems))
      : byIngredients;
  }, [recipes, query, activeTag, ingredientFilters, onlyMakeable, pantryItems]);
  const selectedRecipe =
    recipes.find((recipe) => recipe.id === selectedId) ?? visibleRecipes[0] ?? recipes[0] ?? null;
  const tags = useMemo(
    () => Array.from(new Set(recipes.flatMap((recipe) => recipe.tags))).sort(),
    [recipes],
  );
  const cookbooksById = useMemo(() => {
    const entries = catalogue?.cookbooks.map((book) => [book.id, book] as const) ?? [];
    return new Map(entries);
  }, [catalogue]);
  const recipesById = useMemo(
    () => new Map(recipes.map((recipe) => [recipe.id, recipe] as const)),
    [recipes],
  );
  const currentUser = useMemo(
    () => catalogue?.users.find((user) => user.id === catalogue.currentUserId) ?? null,
    [catalogue],
  );
  const currentFamily = useMemo(
    () =>
      catalogue?.families.find((family) => family.id === currentUser?.familyId) ??
      catalogue?.families[0] ??
      null,
    [catalogue, currentUser],
  );
  const familyMembers = useMemo(
    () =>
      currentFamily && catalogue
        ? catalogue.users.filter((user) => user.familyId === currentFamily.id)
        : [],
    [catalogue, currentFamily],
  );
  const sharedCookbookCount = useMemo(
    () =>
      (catalogue?.cookbooks ?? []).filter(
        (cookbook) =>
          cookbook.shareScope !== "personal" ||
          (currentUser ? cookbook.sharedWithUserIds.includes(currentUser.id) : false),
      ).length,
    [catalogue, currentUser],
  );

  function addIngredientFilter(rawValue: string) {
    const trimmed = rawValue.trim().toLowerCase();
    if (trimmed.length === 0) {
      return;
    }
    const match = ingredientOptions.find(
      (option) =>
        option.item.toLowerCase() === trimmed || option.displayName.toLowerCase() === trimmed,
    );
    const value = match?.item ?? rawValue.trim();
    setIngredientFilters((current) => (current.includes(value) ? current : [...current, value]));
    setIngredientQuery("");
  }

  function removeIngredientFilter(value: string) {
    setIngredientFilters((current) => current.filter((item) => item !== value));
  }

  async function handleMarkMade(recipe: Recipe, details: MarkMadeInput = {}) {
    const apiRecipe = await markRecipeMade(recipe.id, details);
    const fallbackRecipe = recomputeRecipe({
      ...recipe,
      lastMadeAt: details.madeAt ?? new Date().toISOString(),
      timesMade: recipe.timesMade + 1,
    });
    const updatedRecipe = apiRecipe ?? fallbackRecipe;

    setCatalogue((current) => {
      if (!current) {
        return current;
      }

      return {
        ...current,
        recipes: current.recipes.map((item) =>
          item.id === updatedRecipe.id ? updatedRecipe : item,
        ),
      };
    });

    await refreshCookLog();
    await refreshPantry();
  }

  async function handleAddPantryItem(input: PantryItemFormInput) {
    const added = await addPantryItem(input);
    if (added) {
      setPantryItems((current) => [...current, added]);
    }
  }

  async function handleAdjustPantryQuantity(id: string, quantity: number) {
    const updated = await patchPantryItem(id, { quantity });
    if (updated) {
      setPantryItems((current) => current.map((item) => (item.id === id ? updated : item)));
    }
  }

  async function handleDeletePantryItem(id: string) {
    const ok = await deletePantryItem(id);
    if (ok) {
      setPantryItems((current) => current.filter((item) => item.id !== id));
    }
  }

  async function handleAssignMealPlan(date: string, mealType: MealType, recipeId: string) {
    const added = await addMealPlanEntry({ date, mealType, recipeId });
    if (added) {
      setMealPlan((current) => [...current, added]);
    }
  }

  async function handleRemoveMealPlan(id: string) {
    const ok = await deleteMealPlanEntry(id);
    if (ok) {
      setMealPlan((current) => current.filter((item) => item.id !== id));
    }
  }

  async function handleSaveRecipe(recipe: Recipe): Promise<{ ok: boolean; error?: string }> {
    const result = editingImportId
      ? await commitRecipeImport(editingImportId, recipe)
      : editorMode === "create"
        ? await createRecipe(recipe)
        : await updateRecipe(recipe.id, recipe);

    if (!result.ok) {
      return { ok: false, error: result.error };
    }

    setCatalogue((current) => {
      if (!current) {
        return current;
      }

      const exists = current.recipes.some((item) => item.id === result.recipe.id);
      return {
        ...current,
        recipes: exists
          ? current.recipes.map((item) => (item.id === result.recipe.id ? result.recipe : item))
          : [...current.recipes, result.recipe],
      };
    });
    setSelectedId(result.recipe.id);
    setEditingRecipe(null);
    setEditingImportId(null);
    setEditorMode("create");
    return { ok: true };
  }

  async function handleCreateCookbook(
    cookbook: Cookbook,
  ): Promise<{ ok: boolean; error?: string }> {
    const result = await createCookbook(cookbook);

    if (!result.ok) {
      return { ok: false, error: result.error };
    }

    setCatalogue((current) => {
      if (!current) {
        return current;
      }

      return {
        ...current,
        cookbooks: [...current.cookbooks, result.cookbook].sort((left, right) =>
          left.title.localeCompare(right.title),
        ),
      };
    });

    return { ok: true };
  }

  function handleNewRecipe() {
    setEditingRecipe("new");
    setEditingImportId(null);
    setEditorMode("create");
  }

  function handleEditRecipe(recipe: Recipe) {
    setEditingRecipe(recipe);
    setEditingImportId(null);
    setEditorMode("update");
  }

  function handleUseImportDraft(recipeImport: RecipeImport) {
    if (!recipeImport.draft) {
      return;
    }
    setEditingRecipe(recipeImport.draft);
    setEditingImportId(recipeImport.id);
    setEditorMode("create");
  }

  async function handleDeleteRecipe(id: string) {
    if (!window.confirm("Delete this recipe? This also removes it from the meal planner.")) {
      return;
    }

    const ok = await deleteRecipe(id);
    if (ok) {
      setCatalogue((current) => {
        if (!current) {
          return current;
        }
        return { ...current, recipes: current.recipes.filter((item) => item.id !== id) };
      });
      setMealPlan((current) => current.filter((entry) => entry.recipeId !== id));
      setSelectedId(null);
      setEditingRecipe(null);
      setEditingImportId(null);
    }
  }

  return (
    <Tabs defaultValue="recipes" className="min-h-screen">
      <header className="border-b bg-background/95">
        <div className={`${pageGridClassName} items-center py-4`}>
          <div className="col-span-12 lg:col-span-2">
            <h1 className="text-xl font-semibold tracking-normal text-foreground">recitopia</h1>
          </div>
          <nav className="col-span-12 lg:col-span-7" aria-label="Primary navigation">
            <TabsList className="h-auto flex-wrap justify-start gap-1 bg-transparent p-0">
              <TabsTrigger className="h-9 px-3" value="recipes">
                Compendium
              </TabsTrigger>
              <TabsTrigger className="h-9 px-3" value="cookbooks">
                Library
              </TabsTrigger>
              <TabsTrigger className="h-9 px-3" value="pantry">
                Pantry
              </TabsTrigger>
              <TabsTrigger className="h-9 px-3" value="planner">
                Plan
              </TabsTrigger>
              <TabsTrigger className="h-9 px-3" value="history">
                Logs
              </TabsTrigger>
            </TabsList>
          </nav>
          <div className="col-span-12 flex items-center justify-start gap-2 lg:col-span-3 lg:justify-end">
            {currentUser ? (
              <div className="min-w-0 text-right">
                <p className="truncate text-sm font-medium">{currentUser.displayName}</p>
                {currentFamily ? (
                  <p className="truncate text-xs text-muted-foreground">{currentFamily.name}</p>
                ) : null}
              </div>
            ) : null}
            <Tooltip label="Settings">
              <Button type="button" variant="ghost" size="icon" aria-label="Settings">
                <Settings className="h-4 w-4" aria-hidden="true" />
              </Button>
            </Tooltip>
            <Tooltip label="User">
              <Button type="button" variant="ghost" size="icon" aria-label="User">
                <User className="h-4 w-4" aria-hidden="true" />
              </Button>
            </Tooltip>
            <Tooltip label="Logout">
              <Button type="button" variant="ghost" size="icon" aria-label="Logout">
                <LogOut className="h-4 w-4" aria-hidden="true" />
              </Button>
            </Tooltip>
          </div>
        </div>
      </header>

      <main className="py-6">
        <div className={pageGridClassName}>
          <section className="col-span-12 grid gap-3 md:grid-cols-4">
            <Metric label="Recipes" value={recipes.length.toString()} />
            <Metric label="Cookbooks" value={(catalogue?.cookbooks.length ?? 0).toString()} />
            <Metric
              label="Made"
              value={recipes.reduce((total, recipe) => total + recipe.timesMade, 0).toString()}
            />
            <Metric
              label="Shared"
              value={`${familyMembers.length || 1} users · ${sharedCookbookCount} books`}
            />
          </section>

          <TabsContent value="recipes" className="col-span-12 mt-0">
            <div className="grid grid-cols-12 gap-6">
              <aside className="col-span-12 space-y-4 lg:col-span-4 xl:col-span-3">
                <div className="flex gap-2">
                  <div className="relative flex-1">
                    <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-muted-foreground" />
                    <Input
                      aria-label="Search recipes"
                      className="pl-9"
                      value={query}
                      placeholder="Search recipes, ingredients, notes"
                      onChange={(event) => setQuery(event.target.value)}
                    />
                  </div>
                  <Button
                    type="button"
                    variant="outline"
                    size="icon"
                    title="Refresh catalogue"
                    aria-label="Refresh catalogue"
                    onClick={() => void refreshCatalogue()}
                  >
                    <RotateCw className={isRefreshing ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
                  </Button>
                </div>

                <Button type="button" variant="outline" size="sm" onClick={handleNewRecipe}>
                  New recipe
                </Button>

                <RecipeImportPanel
                  cookbooks={catalogue?.cookbooks ?? []}
                  authors={catalogue?.authors ?? []}
                  onUseDraft={handleUseImportDraft}
                />

                <div className="space-y-2">
                  <div className="relative">
                    <Input
                      aria-label="Search by ingredient"
                      list="ingredient-filter-options"
                      placeholder="Filter by ingredient"
                      value={ingredientQuery}
                      onChange={(event) => setIngredientQuery(event.target.value)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          event.preventDefault();
                          addIngredientFilter(ingredientQuery);
                        }
                      }}
                    />
                    <datalist id="ingredient-filter-options">
                      {ingredientSuggestions.map((option) => (
                        <option key={option.item} value={option.displayName} />
                      ))}
                    </datalist>
                  </div>
                  {ingredientFilters.length > 0 ? (
                    <div className="flex flex-wrap gap-2">
                      {ingredientFilters.map((filter) => (
                        <Badge key={filter} variant="secondary" className="gap-1">
                          {filter}
                          <button
                            type="button"
                            aria-label={`Remove ${filter} filter`}
                            onClick={() => removeIngredientFilter(filter)}
                          >
                            <X className="h-3 w-3" />
                          </button>
                        </Badge>
                      ))}
                    </div>
                  ) : null}
                  <Button
                    type="button"
                    variant={onlyMakeable ? "default" : "outline"}
                    size="sm"
                    onClick={() => setOnlyMakeable((value) => !value)}
                  >
                    Only what I can make
                  </Button>
                </div>

                <div className="flex flex-wrap gap-2">
                  <Button
                    type="button"
                    variant={activeTag == null ? "default" : "outline"}
                    size="sm"
                    onClick={() => setActiveTag(null)}
                  >
                    All
                  </Button>
                  {tags.map((tag) => (
                    <Button
                      key={tag}
                      type="button"
                      variant={activeTag === tag ? "default" : "outline"}
                      size="sm"
                      onClick={() => setActiveTag(tag)}
                    >
                      {tag}
                    </Button>
                  ))}
                </div>

                <section aria-label="Recipe results" className="space-y-2">
                  {visibleRecipes.map((recipe) => (
                    <button
                      type="button"
                      key={recipe.id}
                      className={
                        recipe.id === selectedRecipe?.id
                          ? "w-full rounded-lg border bg-card p-3 text-left shadow-sm ring-2 ring-primary"
                          : "w-full rounded-lg border bg-card p-3 text-left shadow-sm transition-colors hover:bg-accent/50"
                      }
                      onClick={() => setSelectedId(recipe.id)}
                    >
                      <div className="flex items-start gap-3">
                        {recipe.images[0] ? (
                          <img
                            src={recipe.images[0].url}
                            alt={recipe.images[0].alt}
                            className="recipe-image h-16 w-16 rounded-md object-cover"
                          />
                        ) : (
                          <div className="flex h-16 w-16 items-center justify-center rounded-md bg-muted">
                            <ImageIcon
                              className="h-5 w-5 text-muted-foreground"
                              aria-hidden="true"
                            />
                          </div>
                        )}
                        <div className="min-w-0 flex-1">
                          <p className="truncate font-medium">{recipe.title}</p>
                          <p className="mt-1 text-sm text-muted-foreground">{recipe.sourceLabel}</p>
                          <div className="mt-2 flex flex-wrap gap-1">
                            <Badge variant="outline">
                              {formatMoney(recipe.costPerServingCents)} / serving
                            </Badge>
                            <Badge variant="outline">{formatMinutes(recipe.totalMinutes)}</Badge>
                            {canMakeRecipe(recipe, pantryItems) ? (
                              <Badge>Can make now</Badge>
                            ) : null}
                          </div>
                        </div>
                      </div>
                    </button>
                  ))}
                </section>
              </aside>

              <div className="col-span-12 lg:col-span-8 xl:col-span-9">
                {editingRecipe ? (
                  <RecipeEditor
                    recipe={editingRecipe === "new" ? null : editingRecipe}
                    mode={editorMode}
                    cookbooks={catalogue?.cookbooks ?? []}
                    authors={catalogue?.authors ?? []}
                    onSave={handleSaveRecipe}
                    onCancel={() => {
                      setEditingRecipe(null);
                      setEditingImportId(null);
                    }}
                  />
                ) : selectedRecipe ? (
                  <RecipeDetail
                    recipe={selectedRecipe}
                    cookbook={cookbooksById.get(selectedRecipe.cookbookId) ?? null}
                    pantryItems={pantryItems}
                    onMarkMade={(details) => void handleMarkMade(selectedRecipe, details)}
                    onEdit={() => handleEditRecipe(selectedRecipe)}
                    onDelete={() => void handleDeleteRecipe(selectedRecipe.id)}
                  />
                ) : (
                  <Card>
                    <CardHeader>
                      <CardTitle>No recipes yet</CardTitle>
                    </CardHeader>
                    <CardContent className="text-sm text-muted-foreground">
                      Add a recipe through the API seed flow or import path.
                    </CardContent>
                  </Card>
                )}
              </div>
            </div>
          </TabsContent>

          <TabsContent value="cookbooks" className="col-span-12 mt-0">
            {catalogue ? (
              <CookbookOverview
                catalogue={catalogue}
                onCreateCookbook={handleCreateCookbook}
                onImportComplete={refreshCatalogue}
                onUseImportDraft={handleUseImportDraft}
              />
            ) : null}
          </TabsContent>

          <TabsContent value="pantry" className="col-span-12 mt-0">
            <PantryView
              pantryItems={pantryItems}
              ingredientOptions={ingredientOptions}
              familyName={currentFamily?.name ?? null}
              isShared={currentFamily?.pantryShared ?? false}
              onAdd={handleAddPantryItem}
              onAdjustQuantity={handleAdjustPantryQuantity}
              onDelete={handleDeletePantryItem}
            />
          </TabsContent>

          <TabsContent value="planner" className="col-span-12 mt-0">
            <MealPlanView
              mealPlan={mealPlan}
              recipes={recipes}
              pantryItems={pantryItems}
              familyName={currentFamily?.name ?? null}
              isShared={currentFamily?.mealPlanShared ?? false}
              onAssign={handleAssignMealPlan}
              onRemove={handleRemoveMealPlan}
            />
          </TabsContent>

          <TabsContent value="history" className="col-span-12 mt-0">
            <HistoryView cookLog={cookLog} recipesById={recipesById} />
          </TabsContent>
        </div>
      </main>
    </Tabs>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border bg-card px-3 py-2">
      <div className="text-xs uppercase text-muted-foreground">{label}</div>
      <div className="text-xl font-semibold">{value}</div>
    </div>
  );
}

function RecipeDetail({
  recipe,
  cookbook,
  pantryItems,
  onMarkMade,
  onEdit,
  onDelete,
}: {
  recipe: Recipe;
  cookbook: Cookbook | null;
  pantryItems: PantryItem[];
  onMarkMade: (details?: MarkMadeInput) => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const [showLogForm, setShowLogForm] = useState(false);
  const primaryImage = recipe.images.find((image) => image.isPrimary) ?? recipe.images[0] ?? null;
  const missing = missingIngredients(recipe, pantryItems);

  return (
    <article className="space-y-4">
      <section className="overflow-hidden rounded-lg border bg-card shadow-sm">
        <div className="grid lg:grid-cols-[minmax(0,1fr)_340px]">
          <div className="p-5">
            <div className="flex flex-wrap gap-2">
              {recipe.tags.map((tag) => (
                <Badge key={tag}>{tag}</Badge>
              ))}
              {missing.length === 0 ? (
                <Badge variant="outline">Can make now</Badge>
              ) : (
                <Badge variant="outline">{missing.length} ingredients missing</Badge>
              )}
            </div>
            <h2 className="mt-4 text-3xl font-semibold tracking-normal">{recipe.title}</h2>
            {recipe.subtitle ? (
              <p className="mt-1 text-base text-muted-foreground">{recipe.subtitle}</p>
            ) : null}
            <p className="mt-2 max-w-2xl text-sm text-muted-foreground">{recipe.sourceLabel}</p>
            {recipe.headnote ? <p className="mt-4 max-w-3xl text-sm">{recipe.headnote}</p> : null}

            <div className="mt-5 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
              <Stat icon={Clock3} label="Time" value={formatMinutes(recipe.totalMinutes)} />
              <Stat
                icon={Utensils}
                label="Yield"
                value={
                  recipe.yieldQuantity && recipe.yieldUnit
                    ? `${recipe.yieldQuantity} ${recipe.yieldUnit}`
                    : "Unknown"
                }
              />
              <Stat icon={Coins} label="Recipe cost" value={formatMoney(recipe.costCents)} />
              <Stat icon={CalendarDays} label="Last made" value={formatDate(recipe.lastMadeAt)} />
            </div>

            <div className="mt-5 flex flex-wrap items-center gap-3">
              <Button type="button" onClick={() => onMarkMade()}>
                <ChefHat className="h-4 w-4" aria-hidden="true" />
                Mark made
              </Button>
              <Button
                type="button"
                variant="outline"
                onClick={() => setShowLogForm((value) => !value)}
              >
                {showLogForm ? "Cancel details" : "Log details…"}
              </Button>
              <span className="text-sm text-muted-foreground">
                Made {recipe.timesMade} {recipe.timesMade === 1 ? "time" : "times"}
              </span>
              <Button type="button" variant="ghost" size="sm" onClick={onEdit}>
                Edit
              </Button>
              <Button type="button" variant="ghost" size="sm" onClick={onDelete}>
                Delete
              </Button>
              <span className="text-sm text-muted-foreground">
                Cache {recipe.cacheKey} updated {formatDate(recipe.cacheUpdatedAt)}
              </span>
            </div>

            {showLogForm ? (
              <CookLogForm
                recipe={recipe}
                onSubmit={(details) => {
                  onMarkMade(details);
                  setShowLogForm(false);
                }}
                onCancel={() => setShowLogForm(false)}
              />
            ) : null}
          </div>

          {primaryImage ? (
            <img
              src={primaryImage.url}
              alt={primaryImage.alt}
              className="recipe-image h-72 w-full object-cover lg:h-full"
            />
          ) : (
            <div className="flex min-h-72 items-center justify-center bg-muted">
              <ImageIcon className="h-8 w-8 text-muted-foreground" aria-hidden="true" />
            </div>
          )}
        </div>
      </section>

      <Tabs defaultValue="ingredients">
        <TabsList>
          <TabsTrigger value="ingredients">Ingredients</TabsTrigger>
          <TabsTrigger value="steps">Steps</TabsTrigger>
          <TabsTrigger value="metadata">Metadata</TabsTrigger>
        </TabsList>

        <TabsContent value="ingredients">
          <Card>
            <CardHeader>
              <CardTitle>Ingredients and costs</CardTitle>
            </CardHeader>
            <CardContent>
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Ingredient</TableHead>
                    <TableHead>Section</TableHead>
                    <TableHead>Quantity</TableHead>
                    <TableHead>Pantry</TableHead>
                    <TableHead className="text-right">Cost</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {recipe.ingredients.map((ingredient) => (
                    <TableRow key={ingredient.id}>
                      <TableCell>
                        <div className="font-medium">{ingredient.displayName}</div>
                        {ingredient.preparation ? (
                          <div className="text-xs text-muted-foreground">
                            {ingredient.preparation}
                          </div>
                        ) : null}
                        {ingredient.quantityReviewStatus === "needs_review" ? (
                          <div className="mt-2">
                            <Tooltip
                              label={ingredient.quantityReviewReason ?? "Quantity needs review"}
                            >
                              <Badge variant="outline">
                                <AlertTriangle className="mr-1 h-3 w-3" aria-hidden="true" />
                                Quantity review
                              </Badge>
                            </Tooltip>
                          </div>
                        ) : null}
                      </TableCell>
                      <TableCell>{ingredient.section ?? "General"}</TableCell>
                      <TableCell>{formatIngredientQuantity(ingredient)}</TableCell>
                      <TableCell>
                        <Tooltip label={describePantryAvailability(ingredient, pantryItems)}>
                          {missing.some((item) => item.id === ingredient.id) ? (
                            <Badge variant="outline">Missing</Badge>
                          ) : (
                            <Badge>Have it</Badge>
                          )}
                        </Tooltip>
                      </TableCell>
                      <TableCell className="text-right">
                        {formatMoney(
                          ingredient.estimatedCostCents ??
                            (ingredient.unitCostCents && ingredient.quantity
                              ? Math.round(ingredient.unitCostCents * ingredient.quantity)
                              : null),
                        )}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="steps">
          <Card>
            <CardHeader>
              <CardTitle>Instructions and notes</CardTitle>
            </CardHeader>
            <CardContent className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_280px]">
              <ol className="space-y-3">
                {recipe.steps.map((step) => (
                  <li key={step.id} className="grid grid-cols-[2rem_1fr] gap-3">
                    <span className="flex h-8 w-8 items-center justify-center rounded-md bg-primary text-sm font-semibold text-primary-foreground">
                      {step.position}
                    </span>
                    <div>
                      {step.section ? (
                        <div className="text-xs font-medium uppercase text-muted-foreground">
                          {step.section}
                        </div>
                      ) : null}
                      <p className="text-sm leading-6">{step.text}</p>
                    </div>
                  </li>
                ))}
              </ol>

              <div className="rounded-lg border bg-muted/40 p-3">
                <h3 className="text-sm font-semibold">Notes</h3>
                <div className="mt-3 space-y-3">
                  {recipe.notes.map((note) => (
                    <p key={note.id} className="text-sm leading-6 text-muted-foreground">
                      {note.text}
                    </p>
                  ))}
                </div>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="metadata">
          <Card>
            <CardHeader>
              <CardTitle>Source and processing</CardTitle>
            </CardHeader>
            <CardContent className="grid gap-4 sm:grid-cols-2">
              <MetadataItem
                icon={BookOpen}
                label="Cookbook"
                value={cookbook?.title ?? recipe.cookbookId}
              />
              <MetadataItem icon={BookOpen} label="Pages" value={formatPages(recipe)} />
              <MetadataItem icon={Utensils} label="Cuisine" value={recipe.cuisine ?? "Unknown"} />
              <MetadataItem
                icon={ChefHat}
                label="Category"
                value={recipe.category ?? "Uncategorised"}
              />
              <MetadataItem
                icon={Coins}
                label="Cost per serving"
                value={formatMoney(recipe.costPerServingCents)}
              />
              <MetadataItem icon={RotateCw} label="Cache key" value={recipe.cacheKey} />
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </article>
  );
}

interface SubstitutionRow {
  id: string;
  ingredientId: string;
  substituteText: string;
}

function CookLogForm({
  recipe,
  onSubmit,
  onCancel,
}: {
  recipe: Recipe;
  onSubmit: (details: MarkMadeInput) => void;
  onCancel: () => void;
}) {
  const [servingsEaten, setServingsEaten] = useState("");
  const [leftoverServings, setLeftoverServings] = useState("");
  const [notes, setNotes] = useState("");
  const [substitutions, setSubstitutions] = useState<SubstitutionRow[]>([]);

  function addSubstitutionRow() {
    setSubstitutions((current) => [
      ...current,
      {
        id: crypto.randomUUID(),
        ingredientId: recipe.ingredients[0]?.id ?? "",
        substituteText: "",
      },
    ]);
  }

  function updateSubstitution(id: string, patch: Partial<SubstitutionRow>) {
    setSubstitutions((current) =>
      current.map((row) => (row.id === id ? { ...row, ...patch } : row)),
    );
  }

  function removeSubstitution(id: string) {
    setSubstitutions((current) => current.filter((row) => row.id !== id));
  }

  function handleSubmit(event: React.SubmitEvent) {
    event.preventDefault();

    const validSubstitutions = substitutions
      .filter((row) => row.substituteText.trim().length > 0)
      .map((row) => {
        const ingredient = recipe.ingredients.find(
          (candidate) => candidate.id === row.ingredientId,
        );
        return {
          ingredientId: row.ingredientId,
          originalItem: ingredient?.item ?? row.ingredientId,
          substituteText: row.substituteText.trim(),
        };
      });

    onSubmit({
      servingsEaten: servingsEaten.trim().length > 0 ? Number(servingsEaten) : undefined,
      leftoverServings: leftoverServings.trim().length > 0 ? Number(leftoverServings) : undefined,
      notes: notes.trim().length > 0 ? notes.trim() : undefined,
      substitutions: validSubstitutions.length > 0 ? validSubstitutions : undefined,
    });
  }

  return (
    <form className="mt-4 space-y-3 rounded-lg border bg-muted/30 p-4" onSubmit={handleSubmit}>
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="space-y-1 text-sm" htmlFor="cook-log-servings-eaten">
          <span>Servings eaten</span>
          <Input
            id="cook-log-servings-eaten"
            type="number"
            step="any"
            min="0"
            value={servingsEaten}
            onChange={(event) => setServingsEaten(event.target.value)}
          />
        </label>
        <label className="space-y-1 text-sm" htmlFor="cook-log-leftover-servings">
          <span>Leftover servings</span>
          <Input
            id="cook-log-leftover-servings"
            type="number"
            step="any"
            min="0"
            value={leftoverServings}
            onChange={(event) => setLeftoverServings(event.target.value)}
          />
        </label>
      </div>

      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <p className="text-sm font-medium">Substitutions</p>
          <Button type="button" variant="outline" size="sm" onClick={addSubstitutionRow}>
            Add substitution
          </Button>
        </div>
        {substitutions.map((row) => (
          <div key={row.id} className="flex flex-wrap items-center gap-2">
            <select
              className={selectClassName}
              value={row.ingredientId}
              onChange={(event) => updateSubstitution(row.id, { ingredientId: event.target.value })}
            >
              {recipe.ingredients.map((ingredient) => (
                <option key={ingredient.id} value={ingredient.id}>
                  {ingredient.displayName}
                </option>
              ))}
            </select>
            <span className="text-sm text-muted-foreground">instead used</span>
            <Input
              className="flex-1"
              placeholder="Substitute"
              value={row.substituteText}
              onChange={(event) =>
                updateSubstitution(row.id, { substituteText: event.target.value })
              }
            />
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => removeSubstitution(row.id)}
            >
              Remove
            </Button>
          </div>
        ))}
      </div>

      <Input
        placeholder="Notes (optional)"
        value={notes}
        onChange={(event) => setNotes(event.target.value)}
      />

      <div className="flex gap-2">
        <Button type="submit">Save cook log</Button>
        <Button type="button" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
      </div>
    </form>
  );
}

function Stat({ icon: Icon, label, value }: { icon: typeof Clock3; label: string; value: string }) {
  return (
    <div className="rounded-lg border bg-background p-3">
      <div className="flex items-center gap-2 text-xs uppercase text-muted-foreground">
        <Icon className="h-4 w-4" aria-hidden="true" />
        {label}
      </div>
      <div className="mt-2 text-lg font-semibold">{value}</div>
    </div>
  );
}

function MetadataItem({
  icon: Icon,
  label,
  value,
}: {
  icon: typeof BookOpen;
  label: string;
  value: string;
}) {
  return (
    <div className="rounded-lg border bg-background p-3">
      <div className="flex items-center gap-2 text-xs uppercase text-muted-foreground">
        <Icon className="h-4 w-4" aria-hidden="true" />
        {label}
      </div>
      <div className="mt-2 break-words text-sm font-medium">{value}</div>
    </div>
  );
}

function formatPages(recipe: Recipe) {
  if (recipe.pageStart == null) {
    return "Not captured";
  }

  if (recipe.pageEnd == null || recipe.pageEnd === recipe.pageStart) {
    return `p. ${recipe.pageStart}`;
  }

  return `pp. ${recipe.pageStart}-${recipe.pageEnd}`;
}

export default App;
