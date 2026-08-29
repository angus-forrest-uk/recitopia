import { seedCatalogue, seedMealPlanEntries, seedPantryItems } from "@/data/seed";
import {
  type Catalogue,
  type Cookbook,
  type CookbookContentBlock,
  type CookbookCrossReference,
  type CookbookGlossaryEntry,
  type CookbookImageSetImportSummary,
  type CookbookImportProgress,
  type CookbookIndexEntry,
  type CookbookMenu,
  type CookbookPage,
  type CookbookPageKind,
  type CookbookPageReviewStatus,
  type CookbookPageText,
  type CookbookSection,
  type CookbookSupplier,
  type CookLogEntry,
  catalogueSchema,
  cookbookContentBlockSchema,
  cookbookImageSetImportSummarySchema,
  cookbookImportProgressSchema,
  cookbookPageSchema,
  cookbookPageTextSchema,
  cookbookSchema,
  cookLogEntrySchema,
  type MealPlanEntry,
  type MealType,
  mealPlanEntrySchema,
  type PantryCategory,
  type PantryItem,
  pantryItemSchema,
  type Recipe,
  type RecipeImport,
  recipeImportSchema,
  recipeSchema,
  recomputeCatalogue,
} from "@/lib/schema";

export type RecipeSaveResult = { ok: true; recipe: Recipe } | { ok: false; error: string };
export type CookbookSaveResult = { ok: true; cookbook: Cookbook } | { ok: false; error: string };

function parseJsonPayload(responseText: string): unknown {
  try {
    return JSON.parse(responseText);
  } catch {
    return null;
  }
}

function describeErrorText(status: number, responseText: string): string {
  const payload = parseJsonPayload(responseText);

  if (
    payload &&
    typeof payload === "object" &&
    "error" in payload &&
    typeof payload.error === "string"
  ) {
    return payload.error;
  }

  const fallback = responseText.trim().replace(/\s+/g, " ");
  if (fallback.length > 0) {
    return `Request failed with ${status}: ${fallback.slice(0, 240)}`;
  }

  return `Request failed with ${status}`;
}

async function describeError(response: Response): Promise<string> {
  const responseText = await response.text().catch(() => "");
  return describeErrorText(response.status, responseText);
}

export async function loadCatalogue(): Promise<Catalogue> {
  try {
    const response = await fetch("/api/catalogue");

    if (!response.ok) {
      throw new Error(`Catalogue request failed with ${response.status}`);
    }

    const payload = await response.json();
    return recomputeCatalogue(catalogueSchema.parse(payload));
  } catch {
    return seedCatalogue;
  }
}

export interface SubstitutionInput {
  ingredientId: string;
  originalItem: string;
  substituteText: string;
}

export interface MarkMadeInput {
  madeAt?: string;
  servingsMade?: number;
  servingsEaten?: number;
  leftoverServings?: number;
  notes?: string;
  substitutions?: SubstitutionInput[];
}

export async function markRecipeMade(
  recipeId: string,
  input: MarkMadeInput = {},
): Promise<Recipe | null> {
  try {
    const response = await fetch(`/api/recipes/${recipeId}/made`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        madeAt: new Date().toISOString(),
        ...input,
      }),
    });

    if (!response.ok) {
      throw new Error(`Cook event failed with ${response.status}`);
    }

    return (await response.json()) as Recipe;
  } catch {
    return null;
  }
}

export async function loadPantry(): Promise<PantryItem[]> {
  try {
    const response = await fetch("/api/pantry");

    if (!response.ok) {
      throw new Error(`Pantry request failed with ${response.status}`);
    }

    return pantryItemSchema.array().parse(await response.json());
  } catch {
    return seedPantryItems;
  }
}

export interface PantryItemInput {
  item: string;
  displayName: string;
  quantity?: number | null;
  unit?: string | null;
  category: PantryCategory;
  sourceRecipeId?: string | null;
  notes?: string | null;
  expiresAt?: string | null;
}

export async function addPantryItem(input: PantryItemInput): Promise<PantryItem | null> {
  try {
    const response = await fetch("/api/pantry", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(input),
    });

    if (!response.ok) {
      throw new Error(`Add pantry item failed with ${response.status}`);
    }

    return pantryItemSchema.parse(await response.json());
  } catch {
    return null;
  }
}

export interface PantryItemPatch {
  quantity?: number | null;
  unit?: string | null;
  category?: PantryCategory;
  notes?: string | null;
  expiresAt?: string | null;
}

export async function patchPantryItem(
  id: string,
  patch: PantryItemPatch,
): Promise<PantryItem | null> {
  try {
    const response = await fetch(`/api/pantry/${id}`, {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(patch),
    });

    if (!response.ok) {
      throw new Error(`Patch pantry item failed with ${response.status}`);
    }

    return pantryItemSchema.parse(await response.json());
  } catch {
    return null;
  }
}

export async function deletePantryItem(id: string): Promise<boolean> {
  try {
    const response = await fetch(`/api/pantry/${id}`, { method: "DELETE" });
    return response.ok;
  } catch {
    return false;
  }
}

export async function loadMealPlan(): Promise<MealPlanEntry[]> {
  try {
    const response = await fetch("/api/meal-plan");

    if (!response.ok) {
      throw new Error(`Meal plan request failed with ${response.status}`);
    }

    return mealPlanEntrySchema.array().parse(await response.json());
  } catch {
    return seedMealPlanEntries;
  }
}

export interface MealPlanEntryInput {
  date: string;
  mealType: MealType;
  recipeId: string;
  servings?: number | null;
  notes?: string | null;
}

export async function addMealPlanEntry(input: MealPlanEntryInput): Promise<MealPlanEntry | null> {
  try {
    const response = await fetch("/api/meal-plan", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(input),
    });

    if (!response.ok) {
      throw new Error(`Add meal plan entry failed with ${response.status}`);
    }

    return mealPlanEntrySchema.parse(await response.json());
  } catch {
    return null;
  }
}

export async function deleteMealPlanEntry(id: string): Promise<boolean> {
  try {
    const response = await fetch(`/api/meal-plan/${id}`, { method: "DELETE" });
    return response.ok;
  } catch {
    return false;
  }
}

export async function loadCookLog(): Promise<CookLogEntry[]> {
  try {
    const response = await fetch("/api/cook-log");

    if (!response.ok) {
      throw new Error(`Cook log request failed with ${response.status}`);
    }

    return cookLogEntrySchema.array().parse(await response.json());
  } catch {
    return [];
  }
}

export async function createRecipe(recipe: Recipe): Promise<RecipeSaveResult> {
  try {
    const response = await fetch("/api/recipes", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(recipe),
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, recipe: recipeSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not reach the API" };
  }
}

export async function updateRecipe(id: string, recipe: Recipe): Promise<RecipeSaveResult> {
  try {
    const response = await fetch(`/api/recipes/${id}`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(recipe),
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, recipe: recipeSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not reach the API" };
  }
}

export async function deleteRecipe(id: string): Promise<boolean> {
  try {
    const response = await fetch(`/api/recipes/${id}`, { method: "DELETE" });
    return response.ok;
  } catch {
    return false;
  }
}

export async function createCookbook(cookbook: Cookbook): Promise<CookbookSaveResult> {
  try {
    const response = await fetch("/api/cookbooks", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(cookbook),
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, cookbook: cookbookSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not create the cookbook" };
  }
}

export interface ImageRecipeImportInput {
  fileName: string;
  mimeType: string;
  imageBase64: string;
  cookbookId: string;
  authorIds?: string[];
  pageStart?: number | null;
  pageEnd?: number | null;
  sourceLabel?: string | null;
}

export type RecipeImportResult =
  | { ok: true; recipeImport: RecipeImport }
  | { ok: false; error: string };

export async function createImageRecipeImport(
  input: ImageRecipeImportInput,
): Promise<RecipeImportResult> {
  try {
    const response = await fetch("/api/imports/images", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(input),
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, recipeImport: recipeImportSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not import the image" };
  }
}

export interface CookbookImagePageImportInput {
  imageIndex: number;
  printedPageLabel?: string | null;
  printedPageNumber?: number | null;
  imagePath: string;
  imageHash?: string | null;
  ocrText?: string;
  ocrJson?: string;
  averageConfidence?: number | null;
  minimumConfidence?: number | null;
  pageKind?: CookbookPageKind;
  reviewStatus?: CookbookPageReviewStatus;
}

export interface CookbookImageSetImportInput {
  cookbookId: string;
  sourcePath: string;
  status?: "uploaded" | "ocr_ready" | "mapped" | "reviewed" | "committed";
  ocrEngine?: string | null;
  reviewNotes?: string | null;
  pages: CookbookImagePageImportInput[];
  sections?: CookbookSection[];
  contentBlocks?: CookbookContentBlock[];
  menus?: CookbookMenu[];
  glossaryEntries?: CookbookGlossaryEntry[];
  suppliers?: CookbookSupplier[];
  indexEntries?: CookbookIndexEntry[];
  crossReferences?: CookbookCrossReference[];
}

export interface CookbookArchiveImportInput {
  cookbookId: string;
  sourcePath: string;
  archive: Blob;
  onUploadProgress?: (progress: CookbookArchiveUploadProgress) => void;
}

export interface CookbookRecipeDraftInput {
  cookbookId: string;
  sourceBlockId?: string | null;
  pageId?: string | null;
  pageIds?: string[];
  sourceLabel?: string | null;
}

export type CookbookImageSetImportResult =
  | { ok: true; summary: CookbookImageSetImportSummary }
  | { ok: false; error: string };

export type CookbookOcrProcessResult =
  | { ok: true; progress: CookbookImportProgress }
  | { ok: false; error: string };

export type CookbookImportProgressResult =
  | { ok: true; progress: CookbookImportProgress }
  | { ok: false; error: string };

export type PipelineDiagnosticResult =
  | { ok: true; progress: CookbookImportProgress }
  | { ok: false; error: string };

export interface IntroductionPageDiagnosticArtifacts {
  workDir: string;
  ocrTextPath: string;
  ocrOutputPath: string;
  sourceMapInputPath: string;
  sourceMapOutputPath: string;
  deepseekInputPath: string;
  deepseekOutputPath: string;
  deepseekVerboseDir: string;
}

export interface IntroductionPageDiagnostic {
  jobId: string;
  cookbookId: string;
  pageId: string;
  selectedBy: string;
  imageIndex: number;
  storedPrintedPageNumber: number | null;
  detectedPrintedPageNumber: number | null;
  ocrEngine: string;
  ocrLayoutMode: string | null;
  ocrColumnDetection: string | null;
  extractionEngine: string;
  sourceMapSectionCount: number;
  sourceMapContentBlockCount: number;
  extractedRecipeCount: number;
  extractedContentBlockCount: number;
  checksPassed: boolean;
  issues: string[];
  expectedOcrOrder: string[];
  ocrTextPreview: string;
  extractedBlockPreview: string;
  artifacts: IntroductionPageDiagnosticArtifacts;
}

export type IntroductionPageDiagnosticResult =
  | { ok: true; diagnostic: IntroductionPageDiagnostic }
  | { ok: false; error: string };

export type IntroductionPageDiagnosticStartResult =
  | { ok: true; progress: CookbookImportProgress }
  | { ok: false; error: string };

export interface CookbookArchiveUploadProgress {
  loaded: number;
  total: number | null;
  percent: number | null;
}

export type CookbookRecipeDraftResult =
  | { ok: true; recipeImport: RecipeImport }
  | { ok: false; error: string };

export interface CookbookPageImageUploadInput {
  fileName: string;
  mimeType: string;
  imageBase64: string;
}

export interface CookbookPageImageUpload {
  imagePath: string;
  imageHash: string;
  sizeBytes: number;
}

export type CookbookPageImageUploadResult =
  | { ok: true; upload: CookbookPageImageUpload }
  | { ok: false; error: string };

export async function uploadCookbookPageImage(
  input: CookbookPageImageUploadInput,
): Promise<CookbookPageImageUploadResult> {
  try {
    const response = await fetch("/api/cookbook-page-images", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(input),
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, upload: (await response.json()) as CookbookPageImageUpload };
  } catch {
    return { ok: false, error: "Could not upload the cookbook page image" };
  }
}

export async function createCookbookArchiveImport(
  input: CookbookArchiveImportInput,
): Promise<CookbookImageSetImportResult> {
  const params = new URLSearchParams({
    cookbookId: input.cookbookId,
    sourcePath: input.sourcePath,
  });
  const url = `/api/cookbook-imports/archive?${params.toString()}`;

  if (input.onUploadProgress && typeof XMLHttpRequest !== "undefined") {
    return new Promise((resolve) => {
      const xhr = new XMLHttpRequest();
      xhr.open("POST", url);
      xhr.setRequestHeader("content-type", "application/x-tar");
      xhr.upload.onprogress = (event) => {
        const total = event.lengthComputable ? event.total : input.archive.size;
        input.onUploadProgress?.({
          loaded: event.loaded,
          total: total > 0 ? total : null,
          percent: total > 0 ? Math.min(100, Math.round((event.loaded / total) * 100)) : null,
        });
      };
      xhr.upload.onload = () => {
        input.onUploadProgress?.({
          loaded: input.archive.size,
          total: input.archive.size,
          percent: 100,
        });
      };
      xhr.onerror = () => resolve({ ok: false, error: "Could not upload the cookbook archive" });
      xhr.onload = () => {
        const payload = parseJsonPayload(xhr.responseText);

        if (xhr.status < 200 || xhr.status >= 300) {
          resolve({
            ok: false,
            error: describeErrorText(xhr.status, xhr.responseText),
          });
          return;
        }

        try {
          resolve({
            ok: true,
            summary: cookbookImageSetImportSummarySchema.parse(payload),
          });
        } catch {
          resolve({ ok: false, error: "Could not upload the cookbook archive" });
        }
      };
      xhr.send(input.archive);
    });
  }

  try {
    const response = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/x-tar" },
      body: input.archive,
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, summary: cookbookImageSetImportSummarySchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not upload the cookbook archive" };
  }
}

export async function processCookbookImportOcr(
  importId: string,
  options: { refreshOcr?: boolean } = {},
): Promise<CookbookOcrProcessResult> {
  try {
    const query = options.refreshOcr ? "?refreshOcr=true" : "";
    const response = await fetch(`/api/cookbook-imports/${importId}/ocr${query}`, {
      method: "POST",
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, progress: cookbookImportProgressSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not process cookbook OCR" };
  }
}

export async function getCookbookImportProgress(
  importId: string,
  signal?: AbortSignal,
): Promise<CookbookImportProgressResult> {
  try {
    const response = await fetch(`/api/cookbook-imports/${importId}/progress`, { signal });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, progress: cookbookImportProgressSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not load cookbook import progress" };
  }
}

export async function cancelCookbookImportProcessing(
  importId: string,
): Promise<CookbookImportProgressResult> {
  try {
    const response = await fetch(`/api/cookbook-imports/${importId}/cancel`, {
      method: "POST",
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, progress: cookbookImportProgressSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not cancel cookbook import processing" };
  }
}

export async function startCookbookPipelineDiagnostic(
  cookbookId: string,
): Promise<PipelineDiagnosticResult> {
  const params = new URLSearchParams({ cookbookId });

  try {
    const response = await fetch(`/api/pipeline-diagnostics/cookbook?${params.toString()}`, {
      method: "POST",
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, progress: cookbookImportProgressSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not start the pipeline diagnostic" };
  }
}

export async function runIntroductionPageDiagnostic(
  cookbookId: string,
): Promise<IntroductionPageDiagnosticStartResult> {
  const params = new URLSearchParams({
    cookbookId,
    imageIndex: "4",
    printedPage: "7",
  });

  try {
    const response = await fetch(
      `/api/pipeline-diagnostics/introduction-page?${params.toString()}`,
      {
        method: "POST",
      },
    );

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, progress: cookbookImportProgressSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not run the introduction page diagnostic" };
  }
}

export async function getIntroductionPageDiagnostic(
  diagnosticId: string,
): Promise<IntroductionPageDiagnosticResult> {
  try {
    const response = await fetch(`/api/pipeline-diagnostics/${diagnosticId}/introduction-page`);

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, diagnostic: (await response.json()) as IntroductionPageDiagnostic };
  } catch {
    return { ok: false, error: "Could not load the introduction page diagnostic" };
  }
}

export async function cancelPipelineDiagnostic(
  diagnosticId: string,
): Promise<PipelineDiagnosticResult> {
  try {
    const response = await fetch(`/api/pipeline-diagnostics/${diagnosticId}/cancel`, {
      method: "POST",
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, progress: cookbookImportProgressSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not cancel the pipeline diagnostic" };
  }
}

export async function getPipelineDiagnosticProgress(
  diagnosticId: string,
  signal?: AbortSignal,
): Promise<PipelineDiagnosticResult> {
  try {
    const response = await fetch(`/api/pipeline-diagnostics/${diagnosticId}/progress`, { signal });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, progress: cookbookImportProgressSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not load pipeline diagnostic progress" };
  }
}

export type CookbookContentBlocksResult =
  | { ok: true; blocks: CookbookContentBlock[] }
  | { ok: false; error: string };

/// Full-text content blocks for one cookbook; the catalogue payload only
/// carries 420-byte previews.
export async function loadCookbookContentBlocks(
  cookbookId: string,
): Promise<CookbookContentBlocksResult> {
  try {
    const response = await fetch(`/api/cookbooks/${cookbookId}/blocks`);

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, blocks: cookbookContentBlockSchema.array().parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not load the cookbook text" };
  }
}

export type AcceptPageContentResult =
  | { ok: true; block: CookbookContentBlock }
  | { ok: false; error: string };

export interface AcceptPageContentInput {
  kind?: CookbookContentBlock["kind"];
  title?: string;
}

/// Accept a page's OCR text as non-recipe cookbook content; the created
/// block appears in the single-page cookbook document.
export async function acceptCookbookPageContent(
  pageId: string,
  input: AcceptPageContentInput = {},
): Promise<AcceptPageContentResult> {
  try {
    const response = await fetch(`/api/cookbook-pages/${pageId}/accept-content`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(input),
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, block: cookbookContentBlockSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not accept the page content" };
  }
}

export type CookbookPageTextResult =
  | { ok: true; pageText: CookbookPageText }
  | { ok: false; error: string };

export type CookbookPagePatchResult =
  | { ok: true; page: CookbookPage }
  | { ok: false; error: string };

export interface CookbookPagePatchInput {
  pageKind?: CookbookPageKind;
  reviewStatus?: CookbookPageReviewStatus;
  /** Corrected OCR text; the stored OCR boxes (ocrJson) are preserved. */
  ocrText?: string;
}

export type CookbookContentBlockPatchResult =
  | { ok: true; block: CookbookContentBlock }
  | { ok: false; error: string };

export interface CookbookContentBlockPatchInput {
  text?: string;
  title?: string;
}

export async function patchCookbookContentBlock(
  blockId: string,
  patch: CookbookContentBlockPatchInput,
): Promise<CookbookContentBlockPatchResult> {
  try {
    const response = await fetch(`/api/cookbook-content-blocks/${blockId}`, {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(patch),
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, block: cookbookContentBlockSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not update the content block" };
  }
}

export function cookbookPageImageUrl(pageId: string): string {
  return `/api/cookbook-pages/${pageId}/image`;
}

export async function loadCookbookPageText(pageId: string): Promise<CookbookPageTextResult> {
  try {
    const response = await fetch(`/api/cookbook-pages/${pageId}/text`);

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, pageText: cookbookPageTextSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not load the page text" };
  }
}

export async function patchCookbookPage(
  pageId: string,
  patch: CookbookPagePatchInput,
): Promise<CookbookPagePatchResult> {
  try {
    const response = await fetch(`/api/cookbook-pages/${pageId}`, {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(patch),
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, page: cookbookPageSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not update the page" };
  }
}

export async function createCookbookRecipeDraft(
  input: CookbookRecipeDraftInput,
): Promise<CookbookRecipeDraftResult> {
  try {
    const response = await fetch("/api/cookbook-recipe-drafts", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(input),
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, recipeImport: recipeImportSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not create the cookbook recipe draft" };
  }
}

export async function createCookbookImageSetImport(
  input: CookbookImageSetImportInput,
): Promise<CookbookImageSetImportResult> {
  try {
    const response = await fetch("/api/cookbook-imports/images", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(input),
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, summary: cookbookImageSetImportSummarySchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not import the cookbook image set" };
  }
}

export async function updateRecipeImportDraft(
  importId: string,
  recipe: Recipe,
): Promise<RecipeImportResult> {
  try {
    const response = await fetch(`/api/imports/${importId}/draft`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(recipe),
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, recipeImport: recipeImportSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not update the import draft" };
  }
}

export async function commitRecipeImport(
  importId: string,
  recipe: Recipe,
): Promise<RecipeSaveResult> {
  try {
    const response = await fetch(`/api/imports/${importId}/commit`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(recipe),
    });

    if (!response.ok) {
      return { ok: false, error: await describeError(response) };
    }

    return { ok: true, recipe: recipeSchema.parse(await response.json()) };
  } catch {
    return { ok: false, error: "Could not commit the import draft" };
  }
}
