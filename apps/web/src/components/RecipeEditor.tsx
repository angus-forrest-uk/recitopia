import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import type {
  Author,
  Cookbook,
  Ingredient,
  InstructionStep,
  Recipe,
  RecipeImage,
  RecipeNote,
} from "@/lib/schema";

const selectClassName =
  "flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

function slugify(text: string): string {
  const slug = text
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return slug.length >= 2 ? slug : `${slug || "recipe"}-recipe`;
}

function toNumberOrNull(value: string): number | null {
  if (value.trim().length === 0) {
    return null;
  }
  const parsed = Number(value);
  return Number.isNaN(parsed) ? null : parsed;
}

function toIntOrNull(value: string): number | null {
  const parsed = toNumberOrNull(value);
  return parsed == null ? null : Math.round(parsed);
}

interface IngredientRow {
  rowId: string;
  id: string;
  displayName: string;
  item: string;
  quantity: string;
  unit: string;
  preparation: string;
  section: string;
  unitCostCents: string;
  estimatedCostCents: string;
}

interface StepRow {
  rowId: string;
  id: string;
  section: string;
  text: string;
}

interface ImageRow {
  rowId: string;
  id: string;
  url: string;
  alt: string;
  credit: string;
  isPrimary: boolean;
}

interface NoteRow {
  rowId: string;
  id: string;
  text: string;
  createdAt: string;
}

function ingredientToRow(ingredient: Ingredient): IngredientRow {
  return {
    rowId: crypto.randomUUID(),
    id: ingredient.id,
    displayName: ingredient.displayName,
    item: ingredient.item,
    quantity: ingredient.quantity?.toString() ?? "",
    unit: ingredient.unit ?? "",
    preparation: ingredient.preparation ?? "",
    section: ingredient.section ?? "",
    unitCostCents: ingredient.unitCostCents?.toString() ?? "",
    estimatedCostCents: ingredient.estimatedCostCents?.toString() ?? "",
  };
}

function stepToRow(step: InstructionStep): StepRow {
  return { rowId: crypto.randomUUID(), id: step.id, section: step.section ?? "", text: step.text };
}

function imageToRow(image: RecipeImage): ImageRow {
  return {
    rowId: crypto.randomUUID(),
    id: image.id,
    url: image.url,
    alt: image.alt,
    credit: image.credit ?? "",
    isPrimary: image.isPrimary,
  };
}

function noteToRow(note: RecipeNote): NoteRow {
  return { rowId: crypto.randomUUID(), id: note.id, text: note.text, createdAt: note.createdAt };
}

interface RecipeEditorProps {
  recipe: Recipe | null;
  mode?: "create" | "update";
  cookbooks: Cookbook[];
  authors: Author[];
  onSave: (recipe: Recipe) => Promise<{ ok: boolean; error?: string }>;
  onCancel: () => void;
}

export function RecipeEditor({
  recipe,
  mode,
  cookbooks,
  authors,
  onSave,
  onCancel,
}: RecipeEditorProps) {
  const isNew = mode ? mode === "create" : recipe == null;

  const [id, setId] = useState(recipe?.id ?? "");
  const [idTouched, setIdTouched] = useState(!isNew);
  const [title, setTitle] = useState(recipe?.title ?? "");
  const [cookbookId, setCookbookId] = useState(recipe?.cookbookId ?? cookbooks[0]?.id ?? "");
  const [authorIds, setAuthorIds] = useState<string[]>(recipe?.authorIds ?? []);
  const [sourceLabel, setSourceLabel] = useState(recipe?.sourceLabel ?? "");
  const [pageStart, setPageStart] = useState(recipe?.pageStart?.toString() ?? "");
  const [pageEnd, setPageEnd] = useState(recipe?.pageEnd?.toString() ?? "");
  const [yieldQuantity, setYieldQuantity] = useState(recipe?.yieldQuantity?.toString() ?? "");
  const [yieldUnit, setYieldUnit] = useState(recipe?.yieldUnit ?? "servings");
  const [prepMinutes, setPrepMinutes] = useState(recipe?.prepMinutes?.toString() ?? "");
  const [cookMinutes, setCookMinutes] = useState(recipe?.cookMinutes?.toString() ?? "");
  const [cuisine, setCuisine] = useState(recipe?.cuisine ?? "");
  const [category, setCategory] = useState(recipe?.category ?? "");
  const [tagsText, setTagsText] = useState(recipe?.tags.join(", ") ?? "");

  const [images, setImages] = useState<ImageRow[]>(recipe?.images.map(imageToRow) ?? []);
  const [ingredients, setIngredients] = useState<IngredientRow[]>(
    recipe?.ingredients.map(ingredientToRow) ?? [ingredientToRow(blankIngredient())],
  );
  const [steps, setSteps] = useState<StepRow[]>(
    recipe?.steps.map(stepToRow) ?? [stepToRow(blankStep())],
  );
  const [notes, setNotes] = useState<NoteRow[]>(recipe?.notes.map(noteToRow) ?? []);

  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  function blankIngredient(): Ingredient {
    return {
      id: crypto.randomUUID(),
      position: null,
      displayName: "",
      item: "",
      quantity: null,
      quantityText: null,
      quantityMin: null,
      quantityMax: null,
      quantityKind: "exact",
      quantityReviewStatus: "parsed",
      quantityReviewReason: null,
      unit: null,
      preparation: null,
      section: null,
      optional: false,
      alternativeText: null,
      sourceLine: null,
      sourcePageId: null,
      unitCostCents: null,
      estimatedCostCents: null,
    };
  }

  function blankStep(): InstructionStep {
    return {
      id: crypto.randomUUID(),
      position: 1,
      section: null,
      text: "",
      sourcePageId: null,
      sourceLineStart: null,
      sourceLineEnd: null,
    };
  }

  function handleTitleChange(value: string) {
    setTitle(value);
    if (isNew && !idTouched) {
      setId(slugify(value));
    }
  }

  function toggleAuthor(authorId: string) {
    setAuthorIds((current) =>
      current.includes(authorId)
        ? current.filter((value) => value !== authorId)
        : [...current, authorId],
    );
  }

  function updateIngredient(rowId: string, patch: Partial<IngredientRow>) {
    setIngredients((current) =>
      current.map((row) => (row.rowId === rowId ? { ...row, ...patch } : row)),
    );
  }

  function updateStep(rowId: string, patch: Partial<StepRow>) {
    setSteps((current) => current.map((row) => (row.rowId === rowId ? { ...row, ...patch } : row)));
  }

  function updateImage(rowId: string, patch: Partial<ImageRow>) {
    setImages((current) =>
      current.map((row) => (row.rowId === rowId ? { ...row, ...patch } : row)),
    );
  }

  function updateNote(rowId: string, patch: Partial<NoteRow>) {
    setNotes((current) => current.map((row) => (row.rowId === rowId ? { ...row, ...patch } : row)));
  }

  async function handleSubmit(event: React.SubmitEvent) {
    event.preventDefault();
    setError(null);

    const trimmedId = id.trim();
    const validIngredients = ingredients.filter((row) => row.displayName.trim() && row.item.trim());
    const validSteps = steps.filter((row) => row.text.trim());

    if (!trimmedId || !title.trim() || !cookbookId || !sourceLabel.trim()) {
      setError("Title, recipe ID, cookbook, and source are required.");
      return;
    }
    if (validIngredients.length === 0) {
      setError("At least one ingredient is required.");
      return;
    }
    if (validSteps.length === 0) {
      setError("At least one step is required.");
      return;
    }

    const originalIngredients = new Map(
      recipe?.ingredients.map((ingredient) => [ingredient.id, ingredient]),
    );
    const originalSteps = new Map(recipe?.steps.map((step) => [step.id, step]));

    const payload: Recipe = {
      id: trimmedId,
      title: title.trim(),
      subtitle: recipe?.subtitle ?? null,
      alternateNames: recipe?.alternateNames ?? [],
      cookbookId,
      authorIds,
      pageStart: toIntOrNull(pageStart),
      pageEnd: toIntOrNull(pageEnd),
      sourceLabel: sourceLabel.trim(),
      headnote: recipe?.headnote ?? null,
      servingContext: recipe?.servingContext ?? null,
      yieldQuantity: toNumberOrNull(yieldQuantity),
      yieldUnit: yieldUnit.trim() || null,
      prepMinutes: toIntOrNull(prepMinutes),
      cookMinutes: toIntOrNull(cookMinutes),
      totalMinutes: recipe?.totalMinutes ?? null,
      cuisine: cuisine.trim() || null,
      category: category.trim() || null,
      tags: tagsText
        .split(",")
        .map((tag) => tag.trim())
        .filter(Boolean),
      searchableText: recipe?.searchableText ?? "",
      sourceBlockId: recipe?.sourceBlockId ?? null,
      sourcePageSpans: recipe?.sourcePageSpans ?? [],
      componentRecipeIds: recipe?.componentRecipeIds ?? [],
      picturedPageNumber: recipe?.picturedPageNumber ?? null,
      extractionStatus: recipe?.extractionStatus ?? "verified",
      images: images
        .filter((row) => row.url.trim() && row.alt.trim())
        .map((row) => ({
          id: row.id,
          url: row.url.trim(),
          alt: row.alt.trim(),
          credit: row.credit.trim() || null,
          isPrimary: row.isPrimary,
        })),
      ingredients: validIngredients.map((row, index) => ({
        ...(originalIngredients.get(row.id) ?? {}),
        id: row.id,
        position: index + 1,
        displayName: row.displayName.trim(),
        item: row.item.trim(),
        quantity: toNumberOrNull(row.quantity),
        quantityText: originalIngredients.get(row.id)?.quantityText ?? null,
        quantityMin: originalIngredients.get(row.id)?.quantityMin ?? null,
        quantityMax: originalIngredients.get(row.id)?.quantityMax ?? null,
        quantityKind: originalIngredients.get(row.id)?.quantityKind ?? "exact",
        quantityReviewStatus: originalIngredients.get(row.id)?.quantityReviewStatus ?? "parsed",
        quantityReviewReason: originalIngredients.get(row.id)?.quantityReviewReason ?? null,
        unit: row.unit.trim() || null,
        preparation: row.preparation.trim() || null,
        section: row.section.trim() || null,
        optional: originalIngredients.get(row.id)?.optional ?? false,
        alternativeText: originalIngredients.get(row.id)?.alternativeText ?? null,
        sourceLine: originalIngredients.get(row.id)?.sourceLine ?? null,
        sourcePageId: originalIngredients.get(row.id)?.sourcePageId ?? null,
        unitCostCents: toIntOrNull(row.unitCostCents),
        estimatedCostCents: toIntOrNull(row.estimatedCostCents),
      })),
      steps: validSteps.map((row, index) => ({
        ...(originalSteps.get(row.id) ?? {}),
        id: row.id,
        position: index + 1,
        section: row.section.trim() || null,
        text: row.text.trim(),
        sourcePageId: originalSteps.get(row.id)?.sourcePageId ?? null,
        sourceLineStart: originalSteps.get(row.id)?.sourceLineStart ?? null,
        sourceLineEnd: originalSteps.get(row.id)?.sourceLineEnd ?? null,
      })),
      notes: notes
        .filter((row) => row.text.trim())
        .map((row) => ({ id: row.id, text: row.text.trim(), createdAt: row.createdAt })),
      lastMadeAt: recipe?.lastMadeAt ?? null,
      timesMade: recipe?.timesMade ?? 0,
      costCents: null,
      costPerServingCents: null,
      cacheKey: recipe?.cacheKey ?? "uncached",
      cacheUpdatedAt: recipe?.cacheUpdatedAt ?? null,
    };

    setIsSubmitting(true);
    const result = await onSave(payload);
    setIsSubmitting(false);
    if (!result.ok) {
      setError(result.error ?? "Something went wrong saving this recipe.");
    }
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle>{isNew ? "New recipe" : `Edit ${recipe?.title}`}</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-3 sm:grid-cols-2">
          <label className="space-y-1 text-sm" htmlFor="recipe-title">
            <span>Title</span>
            <Input
              id="recipe-title"
              value={title}
              onChange={(event) => handleTitleChange(event.target.value)}
              required
            />
          </label>
          <label className="space-y-1 text-sm" htmlFor="recipe-id">
            <span>Recipe ID</span>
            <Input
              id="recipe-id"
              value={id}
              disabled={!isNew}
              onChange={(event) => {
                setIdTouched(true);
                setId(event.target.value);
              }}
              required
            />
          </label>
          <label className="space-y-1 text-sm" htmlFor="recipe-cookbook">
            <span>Cookbook</span>
            <select
              id="recipe-cookbook"
              className={selectClassName}
              value={cookbookId}
              onChange={(event) => setCookbookId(event.target.value)}
              required
            >
              {cookbooks.map((cookbook) => (
                <option key={cookbook.id} value={cookbook.id}>
                  {cookbook.title}
                </option>
              ))}
            </select>
          </label>
          <label className="space-y-1 text-sm" htmlFor="recipe-source-label">
            <span>Source label</span>
            <Input
              id="recipe-source-label"
              value={sourceLabel}
              onChange={(event) => setSourceLabel(event.target.value)}
              required
            />
          </label>
          <div className="space-y-1 text-sm sm:col-span-2">
            <span>Authors</span>
            <div className="flex flex-wrap gap-2">
              {authors.map((author) => (
                <Button
                  key={author.id}
                  type="button"
                  size="sm"
                  variant={authorIds.includes(author.id) ? "default" : "outline"}
                  onClick={() => toggleAuthor(author.id)}
                >
                  {author.name}
                </Button>
              ))}
            </div>
          </div>
          <label className="space-y-1 text-sm" htmlFor="recipe-page-start">
            <span>Page start</span>
            <Input
              id="recipe-page-start"
              type="number"
              min="1"
              value={pageStart}
              onChange={(event) => setPageStart(event.target.value)}
            />
          </label>
          <label className="space-y-1 text-sm" htmlFor="recipe-page-end">
            <span>Page end</span>
            <Input
              id="recipe-page-end"
              type="number"
              min="1"
              value={pageEnd}
              onChange={(event) => setPageEnd(event.target.value)}
            />
          </label>
          <label className="space-y-1 text-sm" htmlFor="recipe-yield-quantity">
            <span>Yield quantity</span>
            <Input
              id="recipe-yield-quantity"
              type="number"
              step="any"
              min="0"
              value={yieldQuantity}
              onChange={(event) => setYieldQuantity(event.target.value)}
            />
          </label>
          <label className="space-y-1 text-sm" htmlFor="recipe-yield-unit">
            <span>Yield unit</span>
            <Input
              id="recipe-yield-unit"
              value={yieldUnit}
              onChange={(event) => setYieldUnit(event.target.value)}
            />
          </label>
          <label className="space-y-1 text-sm" htmlFor="recipe-prep-minutes">
            <span>Prep minutes</span>
            <Input
              id="recipe-prep-minutes"
              type="number"
              min="0"
              value={prepMinutes}
              onChange={(event) => setPrepMinutes(event.target.value)}
            />
          </label>
          <label className="space-y-1 text-sm" htmlFor="recipe-cook-minutes">
            <span>Cook minutes</span>
            <Input
              id="recipe-cook-minutes"
              type="number"
              min="0"
              value={cookMinutes}
              onChange={(event) => setCookMinutes(event.target.value)}
            />
          </label>
          <label className="space-y-1 text-sm" htmlFor="recipe-cuisine">
            <span>Cuisine</span>
            <Input
              id="recipe-cuisine"
              value={cuisine}
              onChange={(event) => setCuisine(event.target.value)}
            />
          </label>
          <label className="space-y-1 text-sm" htmlFor="recipe-category">
            <span>Category</span>
            <Input
              id="recipe-category"
              value={category}
              onChange={(event) => setCategory(event.target.value)}
            />
          </label>
          <label className="space-y-1 text-sm sm:col-span-2" htmlFor="recipe-tags">
            <span>Tags (comma separated)</span>
            <Input
              id="recipe-tags"
              value={tagsText}
              onChange={(event) => setTagsText(event.target.value)}
            />
          </label>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle>Ingredients</CardTitle>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() =>
                setIngredients((current) => [...current, ingredientToRow(blankIngredient())])
              }
            >
              Add ingredient
            </Button>
          </div>
        </CardHeader>
        <CardContent className="space-y-3">
          {ingredients.map((row) => (
            <div key={row.rowId} className="grid gap-2 rounded-md border p-2 sm:grid-cols-6">
              <Input
                className="sm:col-span-2"
                placeholder="Display name (e.g. 250 g red lentils)"
                value={row.displayName}
                onChange={(event) =>
                  updateIngredient(row.rowId, { displayName: event.target.value })
                }
              />
              <Input
                placeholder="Item (e.g. red lentils)"
                value={row.item}
                onChange={(event) => updateIngredient(row.rowId, { item: event.target.value })}
              />
              <Input
                type="number"
                step="any"
                placeholder="Quantity"
                value={row.quantity}
                onChange={(event) => updateIngredient(row.rowId, { quantity: event.target.value })}
              />
              <Input
                placeholder="Unit"
                value={row.unit}
                onChange={(event) => updateIngredient(row.rowId, { unit: event.target.value })}
              />
              <Input
                placeholder="Section"
                value={row.section}
                onChange={(event) => updateIngredient(row.rowId, { section: event.target.value })}
              />
              <Input
                placeholder="Preparation"
                value={row.preparation}
                onChange={(event) =>
                  updateIngredient(row.rowId, { preparation: event.target.value })
                }
              />
              <Input
                type="number"
                placeholder="Unit cost (cents)"
                value={row.unitCostCents}
                onChange={(event) =>
                  updateIngredient(row.rowId, { unitCostCents: event.target.value })
                }
              />
              <Input
                type="number"
                placeholder="Estimated cost (cents)"
                value={row.estimatedCostCents}
                onChange={(event) =>
                  updateIngredient(row.rowId, { estimatedCostCents: event.target.value })
                }
              />
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() =>
                  setIngredients((current) => current.filter((item) => item.rowId !== row.rowId))
                }
              >
                Remove
              </Button>
            </div>
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle>Steps</CardTitle>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => setSteps((current) => [...current, stepToRow(blankStep())])}
            >
              Add step
            </Button>
          </div>
        </CardHeader>
        <CardContent className="space-y-3">
          {steps.map((row, index) => (
            <div
              key={row.rowId}
              className="flex flex-wrap items-center gap-2 rounded-md border p-2"
            >
              <Badge variant="outline">{index + 1}</Badge>
              <Input
                className="w-32"
                placeholder="Section"
                value={row.section}
                onChange={(event) => updateStep(row.rowId, { section: event.target.value })}
              />
              <Input
                className="flex-1"
                placeholder="Step text"
                value={row.text}
                onChange={(event) => updateStep(row.rowId, { text: event.target.value })}
              />
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() =>
                  setSteps((current) => current.filter((item) => item.rowId !== row.rowId))
                }
              >
                Remove
              </Button>
            </div>
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle>Images</CardTitle>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() =>
                setImages((current) => [
                  ...current,
                  imageToRow({
                    id: crypto.randomUUID(),
                    url: "",
                    alt: "",
                    credit: null,
                    isPrimary: current.length === 0,
                  }),
                ])
              }
            >
              Add image
            </Button>
          </div>
        </CardHeader>
        <CardContent className="space-y-3">
          {images.map((row) => (
            <div key={row.rowId} className="grid gap-2 rounded-md border p-2 sm:grid-cols-5">
              <Input
                className="sm:col-span-2"
                placeholder="Image URL"
                value={row.url}
                onChange={(event) => updateImage(row.rowId, { url: event.target.value })}
              />
              <Input
                placeholder="Alt text"
                value={row.alt}
                onChange={(event) => updateImage(row.rowId, { alt: event.target.value })}
              />
              <Input
                placeholder="Credit"
                value={row.credit}
                onChange={(event) => updateImage(row.rowId, { credit: event.target.value })}
              />
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={row.isPrimary}
                  onChange={(event) => updateImage(row.rowId, { isPrimary: event.target.checked })}
                />
                Primary
              </label>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() =>
                  setImages((current) => current.filter((item) => item.rowId !== row.rowId))
                }
              >
                Remove
              </Button>
            </div>
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle>Notes</CardTitle>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() =>
                setNotes((current) => [
                  ...current,
                  noteToRow({
                    id: crypto.randomUUID(),
                    text: "",
                    createdAt: new Date().toISOString(),
                  }),
                ])
              }
            >
              Add note
            </Button>
          </div>
        </CardHeader>
        <CardContent className="space-y-3">
          {notes.map((row) => (
            <div key={row.rowId} className="flex items-center gap-2 rounded-md border p-2">
              <Input
                className="flex-1"
                placeholder="Note text"
                value={row.text}
                onChange={(event) => updateNote(row.rowId, { text: event.target.value })}
              />
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() =>
                  setNotes((current) => current.filter((item) => item.rowId !== row.rowId))
                }
              >
                Remove
              </Button>
            </div>
          ))}
        </CardContent>
      </Card>

      {error ? <p className="text-sm text-destructive">{error}</p> : null}

      <div className="flex gap-2">
        <Button type="submit" disabled={isSubmitting}>
          {isNew ? "Create recipe" : "Save changes"}
        </Button>
        <Button type="button" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
      </div>
    </form>
  );
}
