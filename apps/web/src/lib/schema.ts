import { z } from "zod";

const isoDateTimeSchema = z.string().datetime({ offset: true });
const idSchema = z
  .string()
  .min(2)
  .max(80)
  .regex(/^[a-z0-9][a-z0-9-]*$/);
const moneyCentsSchema = z.number().int().min(0);
const positiveNumberOrNullSchema = z.preprocess((value) => {
  if (typeof value === "number" && value <= 0) {
    return null;
  }
  return value;
}, z.number().positive().nullable().default(null));

export const authorSchema = z.object({
  id: idSchema,
  name: z.string().min(1),
  website: z.string().url().nullable().default(null),
});

export const familySchema = z.object({
  id: idSchema,
  name: z.string().min(1),
  pantryShared: z.boolean().default(true),
  mealPlanShared: z.boolean().default(true),
});

export const userSchema = z.object({
  id: idSchema,
  displayName: z.string().min(1),
  email: z.string().email().nullable().default(null),
  familyId: idSchema.nullable().default(null),
});

export const shareScopeSchema = z.enum(["personal", "family", "users"]);

export const cookbookSchema = z.object({
  id: idSchema,
  title: z.string().min(1),
  authorIds: z.array(idSchema).min(1),
  isbn: z.string().nullable().default(null),
  publisher: z.string().nullable().default(null),
  publishedYear: z.number().int().min(1400).max(2600).nullable().default(null),
  coverImageUrl: z.string().url().nullable().default(null),
  ownerUserId: idSchema.nullable().default(null),
  familyId: idSchema.nullable().default(null),
  shareScope: shareScopeSchema.default("personal"),
  sharedWithUserIds: z.array(idSchema).default([]),
});

export const recipeImageSchema = z.object({
  id: idSchema,
  url: z.string().url(),
  alt: z.string().min(1),
  credit: z.string().nullable().default(null),
  isPrimary: z.boolean().default(false),
});

export const recipeAlternateNameSchema = z.object({
  kind: z.string().min(1),
  value: z.string().min(1),
});

export const recipeSourcePageSpanSchema = z.object({
  pageId: idSchema.nullable().default(null),
  printedPageNumber: z.number().int().positive().nullable().default(null),
  lineStart: z.number().int().positive().nullable().default(null),
  lineEnd: z.number().int().positive().nullable().default(null),
  confidence: z.number().min(0).max(1).nullable().default(null),
});

export const recipeExtractionStatusSchema = z.enum(["draft", "needs_review", "verified"]);
const ingredientQuantityKindValueSchema = z.enum(["exact", "range", "as_needed", "unknown"]);
const ingredientQuantityReviewStatusValueSchema = z.enum(["parsed", "needs_review"]);
export const ingredientQuantityKindSchema = z.preprocess(
  (value) => value ?? undefined,
  ingredientQuantityKindValueSchema.default("exact"),
);
export const ingredientQuantityReviewStatusSchema = z.preprocess(
  (value) => value ?? undefined,
  ingredientQuantityReviewStatusValueSchema.default("parsed"),
);

export const ingredientSchema = z.object({
  id: idSchema,
  position: z.number().int().positive().nullable().default(null),
  displayName: z.string().min(1),
  item: z.string().min(1),
  quantity: positiveNumberOrNullSchema,
  quantityText: z.string().nullable().default(null),
  quantityMin: positiveNumberOrNullSchema,
  quantityMax: positiveNumberOrNullSchema,
  quantityKind: ingredientQuantityKindSchema.default("exact"),
  quantityReviewStatus: ingredientQuantityReviewStatusSchema.default("parsed"),
  quantityReviewReason: z.string().nullable().default(null),
  unit: z.string().nullable().default(null),
  preparation: z.string().nullable().default(null),
  section: z.string().nullable().default(null),
  optional: z.boolean().default(false),
  alternativeText: z.string().nullable().default(null),
  sourceLine: z.number().int().positive().nullable().default(null),
  sourcePageId: idSchema.nullable().default(null),
  unitCostCents: moneyCentsSchema.nullable().default(null),
  estimatedCostCents: moneyCentsSchema.nullable().default(null),
});

export const instructionStepSchema = z.object({
  id: idSchema,
  position: z.number().int().positive(),
  section: z.string().nullable().default(null),
  text: z.string().min(1),
  sourcePageId: idSchema.nullable().default(null),
  sourceLineStart: z.number().int().positive().nullable().default(null),
  sourceLineEnd: z.number().int().positive().nullable().default(null),
});

export const recipeNoteSchema = z.object({
  id: idSchema,
  text: z.string().min(1),
  createdAt: isoDateTimeSchema,
});

export const recipeSchema = z.object({
  id: idSchema,
  title: z.string().min(1),
  subtitle: z.string().nullable().default(null),
  alternateNames: z.array(recipeAlternateNameSchema).default([]),
  cookbookId: idSchema,
  authorIds: z.array(idSchema).default([]),
  pageStart: z.number().int().positive().nullable().default(null),
  pageEnd: z.number().int().positive().nullable().default(null),
  sourceLabel: z.string().min(1),
  headnote: z.string().nullable().default(null),
  servingContext: z.string().nullable().default(null),
  yieldQuantity: positiveNumberOrNullSchema,
  yieldUnit: z.string().nullable().default(null),
  prepMinutes: z.number().int().min(0).nullable().default(null),
  cookMinutes: z.number().int().min(0).nullable().default(null),
  totalMinutes: z.number().int().min(0).nullable().default(null),
  cuisine: z.string().nullable().default(null),
  category: z.string().nullable().default(null),
  tags: z.array(z.string().min(1)).default([]),
  searchableText: z.string().default(""),
  sourceBlockId: idSchema.nullable().default(null),
  sourcePageSpans: z.array(recipeSourcePageSpanSchema).default([]),
  componentRecipeIds: z.array(idSchema).default([]),
  picturedPageNumber: z.number().int().positive().nullable().default(null),
  extractionStatus: recipeExtractionStatusSchema.default("verified"),
  images: z.array(recipeImageSchema).default([]),
  ingredients: z.array(ingredientSchema).min(1),
  steps: z.array(instructionStepSchema).min(1),
  notes: z.array(recipeNoteSchema).default([]),
  lastMadeAt: isoDateTimeSchema.nullable().default(null),
  timesMade: z.number().int().min(0).default(0),
  costCents: moneyCentsSchema.nullable().default(null),
  costPerServingCents: moneyCentsSchema.nullable().default(null),
  cacheKey: z.string().min(1).default("uncached"),
  cacheUpdatedAt: isoDateTimeSchema.nullable().default(null),
});

export const cookbookSourceKindSchema = z.enum(["image_set", "pdf", "manual", "web"]);
export const cookbookImportStatusSchema = z.enum([
  "uploaded",
  "ocr_ready",
  "mapped",
  "reviewed",
  "committed",
]);

export const cookbookImportSchema = z.object({
  id: idSchema,
  cookbookId: idSchema,
  sourceKind: cookbookSourceKindSchema,
  sourcePath: z.string().min(1),
  status: cookbookImportStatusSchema,
  ocrEngine: z.string().nullable().default(null),
  createdAt: isoDateTimeSchema,
  updatedAt: isoDateTimeSchema,
  reviewNotes: z.string().nullable().default(null),
});

export const cookbookPageKindSchema = z.enum([
  "cover",
  "title",
  "contents",
  "chapter_opener",
  "essay",
  "reference",
  "recipe",
  "supplier",
  "index",
  "acknowledgements",
  "blank",
  "unknown",
]);

export const cookbookPageReviewStatusSchema = z.enum([
  "pending",
  "accepted",
  "needs_crop",
  "needs_ocr_fix",
  "ignored",
]);

export const cookbookPageSchema = z.object({
  id: idSchema,
  cookbookId: idSchema,
  importId: idSchema,
  imageIndex: z.number().int().positive(),
  printedPageLabel: z.string().nullable().default(null),
  printedPageNumber: z.number().int().positive().nullable().default(null),
  imagePath: z.string().min(1),
  imageHash: z.string().length(64).nullable().default(null),
  // Catalogue responses carry only a preview in ocrText and elide ocrJson;
  // hasOcrText is the reliable emptiness signal.
  ocrText: z.string().default(""),
  ocrJson: z.string().default("{}"),
  hasOcrText: z.boolean().default(false),
  averageConfidence: z.number().min(0).max(1).nullable().default(null),
  minimumConfidence: z.number().min(0).max(1).nullable().default(null),
  pageKind: cookbookPageKindSchema.default("unknown"),
  reviewStatus: cookbookPageReviewStatusSchema.default("pending"),
});

// Full page text fetched on demand from GET /api/cookbook-pages/:id/text;
// the catalogue payload only carries previews.
export const cookbookPageTextSchema = z.object({
  id: idSchema,
  ocrText: z.string().default(""),
  ocrJson: z.string().default("{}"),
});

export const cookbookSectionKindSchema = z.enum([
  "front_matter",
  "chapter",
  "essay",
  "reference",
  "recipes",
  "back_matter",
]);

export const cookbookSectionSchema = z.object({
  id: idSchema,
  cookbookId: idSchema,
  parentSectionId: idSchema.nullable().default(null),
  title: z.string().min(1),
  kind: cookbookSectionKindSchema,
  position: z.number().int().positive(),
  pageStart: z.number().int().positive().nullable().default(null),
  pageEnd: z.number().int().positive().nullable().default(null),
});

export const cookbookContentBlockKindSchema = z.enum([
  "paragraph",
  "recipe",
  "recipe_headnote",
  "ingredient_glossary_entry",
  "menu",
  "supplier",
  "index_entry",
  "caption",
  "callout",
]);

export const cookbookContentBlockSchema = z.object({
  id: idSchema,
  cookbookId: idSchema,
  sectionId: idSchema.nullable().default(null),
  pageStart: z.number().int().positive().nullable().default(null),
  pageEnd: z.number().int().positive().nullable().default(null),
  position: z.number().int().positive(),
  kind: cookbookContentBlockKindSchema,
  title: z.string().nullable().default(null),
  // Catalogue responses carry only a preview in text and elide sourceJson;
  // hasText is the reliable emptiness signal.
  text: z.string().default(""),
  hasText: z.boolean().default(false),
  confidence: z.number().min(0).max(1).nullable().default(null),
  sourceJson: z.string().default("{}"),
});

export const cookbookMenuRecipeSchema = z.object({
  recipeId: idSchema,
  position: z.number().int().positive(),
  role: z.string().nullable().default(null),
  servingNotes: z.string().nullable().default(null),
});

export const cookbookMenuSchema = z.object({
  id: idSchema,
  cookbookId: idSchema,
  sourceBlockId: idSchema.nullable().default(null),
  title: z.string().min(1),
  theme: z.string().nullable().default(null),
  notes: z.string().nullable().default(null),
  recipes: z.array(cookbookMenuRecipeSchema).default([]),
});

export const cookbookGlossaryEntrySchema = z.object({
  id: idSchema,
  cookbookId: idSchema,
  sourceBlockId: idSchema.nullable().default(null),
  title: z.string().min(1),
  aliases: z.array(z.string().min(1)).default([]),
  nativeNames: z.array(z.string().min(1)).default([]),
  description: z.string().default(""),
  storageNotes: z.string().nullable().default(null),
  substitutionNotes: z.string().nullable().default(null),
  pageStart: z.number().int().positive().nullable().default(null),
  pageEnd: z.number().int().positive().nullable().default(null),
});

export const cookbookSupplierSchema = z.object({
  id: idSchema,
  cookbookId: idSchema,
  sourceBlockId: idSchema.nullable().default(null),
  name: z.string().min(1),
  url: z.string().url().nullable().default(null),
  region: z.string().nullable().default(null),
  notes: z.string().nullable().default(null),
  sourcePage: z.number().int().positive().nullable().default(null),
  reviewStatus: cookbookPageReviewStatusSchema.default("pending"),
});

export const cookbookIndexEntrySchema = z.object({
  id: idSchema,
  cookbookId: idSchema,
  term: z.string().min(1),
  subterm: z.string().nullable().default(null),
  targetPageLabel: z.string().nullable().default(null),
  targetPageNumber: z.number().int().positive().nullable().default(null),
  targetRecipeId: idSchema.nullable().default(null),
  illustration: z.boolean().default(false),
});

export const cookbookCrossReferenceSchema = z.object({
  id: idSchema,
  cookbookId: idSchema,
  fromKind: z.string().min(1),
  fromId: idSchema,
  toKind: z.string().min(1),
  toId: idSchema.nullable().default(null),
  label: z.string().nullable().default(null),
  relationKind: z.string().min(1),
});

export const catalogueSchema = z.object({
  currentUserId: idSchema.nullable().default(null),
  families: z.array(familySchema).default([]),
  users: z.array(userSchema).default([]),
  authors: z.array(authorSchema),
  cookbooks: z.array(cookbookSchema),
  recipes: z.array(recipeSchema),
  cookbookImports: z.array(cookbookImportSchema).default([]),
  cookbookPages: z.array(cookbookPageSchema).default([]),
  cookbookSections: z.array(cookbookSectionSchema).default([]),
  cookbookContentBlocks: z.array(cookbookContentBlockSchema).default([]),
  cookbookMenus: z.array(cookbookMenuSchema).default([]),
  cookbookGlossaryEntries: z.array(cookbookGlossaryEntrySchema).default([]),
  cookbookSuppliers: z.array(cookbookSupplierSchema).default([]),
  cookbookIndexEntries: z.array(cookbookIndexEntrySchema).default([]),
  cookbookCrossReferences: z.array(cookbookCrossReferenceSchema).default([]),
});

export const pantryCategorySchema = z.enum(["raw", "prepared", "leftover"]);

export const pantryItemSchema = z.object({
  id: idSchema,
  item: z.string().min(1),
  displayName: z.string().min(1),
  quantity: z.number().positive().nullable().default(null),
  unit: z.string().nullable().default(null),
  category: pantryCategorySchema,
  sourceRecipeId: idSchema.nullable().default(null),
  notes: z.string().nullable().default(null),
  expiresAt: z.string().nullable().default(null),
  addedAt: isoDateTimeSchema,
  ownerUserId: idSchema.nullable().default(null),
  familyId: idSchema.nullable().default(null),
});

export const mealTypeSchema = z.enum(["breakfast", "lunch", "dinner"]);

export const mealPlanEntrySchema = z.object({
  id: idSchema,
  date: z.string().regex(/^\d{4}-\d{2}-\d{2}$/),
  mealType: mealTypeSchema,
  recipeId: idSchema,
  servings: z.number().positive().nullable().default(null),
  notes: z.string().nullable().default(null),
  ownerUserId: idSchema.nullable().default(null),
  familyId: idSchema.nullable().default(null),
});

export const substitutionSchema = z.object({
  id: z.string(),
  ingredientId: idSchema,
  originalItem: z.string(),
  substituteText: z.string().min(1),
});

export const cookLogEntrySchema = z.object({
  id: idSchema,
  recipeId: idSchema,
  madeAt: isoDateTimeSchema,
  servingsMade: z.number().positive().nullable().default(null),
  servingsEaten: z.number().positive().nullable().default(null),
  leftoverServings: z.number().positive().nullable().default(null),
  notes: z.string().nullable().default(null),
  substitutions: z.array(substitutionSchema).default([]),
});

export const recipeImportStatusSchema = z.enum([
  "processing",
  "draft_ready",
  "failed",
  "committed",
]);

export const importIssueSeveritySchema = z.enum(["info", "warning", "error"]);

export const importIssueSchema = z.object({
  field: z.string().min(1),
  message: z.string().min(1),
  severity: importIssueSeveritySchema.default("warning"),
});

export const recipeImportSchema = z.object({
  id: idSchema,
  status: recipeImportStatusSchema,
  fileName: z.string().min(1),
  mimeType: z.string().min(1),
  imagePath: z.string().min(1),
  ocrEngine: z.string().min(1),
  ocrText: z.string().default(""),
  ocrJson: z.string().default("{}"),
  draft: recipeSchema.nullable().default(null),
  validationIssues: z.array(importIssueSchema).default([]),
  createdAt: isoDateTimeSchema,
  updatedAt: isoDateTimeSchema,
});

export const cookbookImageSetImportSummarySchema = z.object({
  importRecord: cookbookImportSchema,
  pageCount: z.number().int().nonnegative(),
  sectionCount: z.number().int().nonnegative(),
  contentBlockCount: z.number().int().nonnegative(),
  recipeCount: z.number().int().nonnegative().default(0),
  menuCount: z.number().int().nonnegative(),
  glossaryEntryCount: z.number().int().nonnegative(),
  supplierCount: z.number().int().nonnegative(),
  indexEntryCount: z.number().int().nonnegative(),
  crossReferenceCount: z.number().int().nonnegative(),
});

export const cookbookOcrProcessSummarySchema = z.object({
  importRecord: cookbookImportSchema,
  pageCount: z.number().int().nonnegative(),
  processedCount: z.number().int().nonnegative(),
  skippedCount: z.number().int().nonnegative(),
  failedCount: z.number().int().nonnegative(),
  sectionCount: z.number().int().nonnegative().default(0),
  contentBlockCount: z.number().int().nonnegative().default(0),
  recipeCount: z.number().int().nonnegative().default(0),
  ocrEngine: z.string().nullable().default(null),
  extractionEngine: z.string().nullable().default(null),
});

export const cookbookImportProgressStateSchema = z.enum([
  "running",
  "complete",
  "failed",
  "canceled",
]);
export const cookbookImportProgressStageSchema = z.enum([
  "queued",
  "loading_pages",
  "ocr_pages",
  "source_map",
  "llm_plan",
  "llm_section",
  "normalizing",
  "persisting",
  "complete",
  "failed",
  "canceled",
]);

export const cookbookImportProgressSchema = z.object({
  importId: idSchema,
  state: cookbookImportProgressStateSchema,
  stage: cookbookImportProgressStageSchema,
  message: z.string().min(1),
  current: z.number().int().nonnegative().nullable().default(null),
  total: z.number().int().nonnegative().nullable().default(null),
  processedCount: z.number().int().nonnegative().default(0),
  skippedCount: z.number().int().nonnegative().default(0),
  failedCount: z.number().int().nonnegative().default(0),
  sectionCount: z.number().int().nonnegative().default(0),
  contentBlockCount: z.number().int().nonnegative().default(0),
  recipeCount: z.number().int().nonnegative().default(0),
  currentSectionIndex: z.number().int().nonnegative().nullable().default(null),
  sectionTotal: z.number().int().nonnegative().nullable().default(null),
  currentSectionTitle: z.string().nullable().default(null),
  extractionEngine: z.string().nullable().default(null),
  error: z.string().nullable().default(null),
});

export type Author = z.infer<typeof authorSchema>;
export type Family = z.infer<typeof familySchema>;
export type User = z.infer<typeof userSchema>;
export type ShareScope = z.infer<typeof shareScopeSchema>;
export type Cookbook = z.infer<typeof cookbookSchema>;
export type RecipeImage = z.infer<typeof recipeImageSchema>;
export type RecipeAlternateName = z.infer<typeof recipeAlternateNameSchema>;
export type RecipeSourcePageSpan = z.infer<typeof recipeSourcePageSpanSchema>;
export type RecipeExtractionStatus = z.infer<typeof recipeExtractionStatusSchema>;
export type Ingredient = z.infer<typeof ingredientSchema>;
export type InstructionStep = z.infer<typeof instructionStepSchema>;
export type RecipeNote = z.infer<typeof recipeNoteSchema>;
export type Recipe = z.infer<typeof recipeSchema>;
export type Catalogue = z.infer<typeof catalogueSchema>;
export type CookbookSourceKind = z.infer<typeof cookbookSourceKindSchema>;
export type CookbookImportStatus = z.infer<typeof cookbookImportStatusSchema>;
export type CookbookImport = z.infer<typeof cookbookImportSchema>;
export type CookbookPageKind = z.infer<typeof cookbookPageKindSchema>;
export type CookbookPageReviewStatus = z.infer<typeof cookbookPageReviewStatusSchema>;
export type CookbookPage = z.infer<typeof cookbookPageSchema>;
export type CookbookPageText = z.infer<typeof cookbookPageTextSchema>;
export type CookbookSectionKind = z.infer<typeof cookbookSectionKindSchema>;
export type CookbookSection = z.infer<typeof cookbookSectionSchema>;
export type CookbookContentBlockKind = z.infer<typeof cookbookContentBlockKindSchema>;
export type CookbookContentBlock = z.infer<typeof cookbookContentBlockSchema>;
export type CookbookMenuRecipe = z.infer<typeof cookbookMenuRecipeSchema>;
export type CookbookMenu = z.infer<typeof cookbookMenuSchema>;
export type CookbookGlossaryEntry = z.infer<typeof cookbookGlossaryEntrySchema>;
export type CookbookSupplier = z.infer<typeof cookbookSupplierSchema>;
export type CookbookIndexEntry = z.infer<typeof cookbookIndexEntrySchema>;
export type CookbookCrossReference = z.infer<typeof cookbookCrossReferenceSchema>;
export type PantryCategory = z.infer<typeof pantryCategorySchema>;
export type PantryItem = z.infer<typeof pantryItemSchema>;
export type MealType = z.infer<typeof mealTypeSchema>;
export type MealPlanEntry = z.infer<typeof mealPlanEntrySchema>;
export type Substitution = z.infer<typeof substitutionSchema>;
export type CookLogEntry = z.infer<typeof cookLogEntrySchema>;
export type RecipeImportStatus = z.infer<typeof recipeImportStatusSchema>;
export type ImportIssueSeverity = z.infer<typeof importIssueSeveritySchema>;
export type ImportIssue = z.infer<typeof importIssueSchema>;
export type RecipeImport = z.infer<typeof recipeImportSchema>;
export type CookbookImageSetImportSummary = z.infer<typeof cookbookImageSetImportSummarySchema>;
export type CookbookOcrProcessSummary = z.infer<typeof cookbookOcrProcessSummarySchema>;
export type CookbookImportProgress = z.infer<typeof cookbookImportProgressSchema>;

export function ingredientCostCents(ingredient: Ingredient) {
  if (ingredient.estimatedCostCents != null) {
    return ingredient.estimatedCostCents;
  }

  if (ingredient.unitCostCents == null || ingredient.quantity == null) {
    return 0;
  }

  return Math.round(ingredient.unitCostCents * ingredient.quantity);
}

export function recomputeRecipe(input: Recipe): Recipe {
  const recipe = recipeSchema.parse(input);
  const costCents = recipe.ingredients.reduce(
    (total, ingredient) => total + ingredientCostCents(ingredient),
    0,
  );
  const totalMinutes =
    recipe.totalMinutes ?? ((recipe.prepMinutes ?? 0) + (recipe.cookMinutes ?? 0) || null);
  const costPerServingCents =
    recipe.yieldQuantity && recipe.yieldQuantity > 0
      ? Math.round(costCents / recipe.yieldQuantity)
      : null;
  const searchableText = [
    recipe.title,
    recipe.subtitle,
    recipe.sourceLabel,
    recipe.headnote,
    recipe.servingContext,
    recipe.cuisine,
    recipe.category,
    ...recipe.alternateNames.map((name) => name.value),
    ...recipe.tags,
    ...recipe.ingredients.map((ingredient) => ingredient.displayName),
    ...recipe.steps.map((step) => step.text),
    ...recipe.notes.map((note) => note.text),
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();

  const cacheKey = hashText(
    JSON.stringify({
      title: recipe.title,
      subtitle: recipe.subtitle,
      alternateNames: recipe.alternateNames,
      headnote: recipe.headnote,
      ingredients: recipe.ingredients,
      steps: recipe.steps,
      notes: recipe.notes,
      componentRecipeIds: recipe.componentRecipeIds,
      lastMadeAt: recipe.lastMadeAt,
      timesMade: recipe.timesMade,
    }),
  );

  return {
    ...recipe,
    costCents,
    costPerServingCents,
    totalMinutes,
    searchableText,
    cacheKey,
    cacheUpdatedAt: new Date().toISOString(),
  };
}

export function recomputeCatalogue(catalogue: Catalogue): Catalogue {
  const parsed = catalogueSchema.parse(catalogue);

  return {
    ...parsed,
    recipes: parsed.recipes.map(recomputeRecipe),
  };
}

export function filterRecipes(recipes: Recipe[], query: string, tag: string | null) {
  const normalized = query.trim().toLowerCase();

  return recipes.filter((recipe) => {
    const matchesQuery =
      normalized.length === 0 ||
      recipe.searchableText.includes(normalized) ||
      recipe.title.toLowerCase().includes(normalized);
    const matchesTag = tag == null || recipe.tags.includes(tag);

    return matchesQuery && matchesTag;
  });
}

function hashText(value: string) {
  let hash = 5381;

  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 33) ^ value.charCodeAt(index);
  }

  return `cache-${(hash >>> 0).toString(16)}`;
}
