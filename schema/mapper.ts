import type { BlockKind, PageKind } from "./document";
import type { Recipe } from "./recipe";

export interface MapperRequest {
  importId: string;
  sourceKind: "page" | "block";
  ocrText: string;
  pageKind: PageKind | null;
  blockKind: BlockKind | null;
  pageIds: string[];
  printedPageNumbers: (number | null)[];
  cookbook: {
    title: string | null;
    authors: string[];
    isbn: string | null;
    year: number | null;
  };
  knownRecipeTitles: string[];
  knownGlossaryTerms: string[];
}

export type IssueSeverity = "info" | "warning" | "error";

export type IssueCode =
  | "missing_yield"
  | "missing_page_number"
  | "missing_timing"
  | "ingredient_without_quantity"
  | "step_looks_like_ingredient"
  | "duplicate_recipe_title"
  | "duplicate_recipe_id"
  | "unresolved_cross_reference"
  | "low_ocr_confidence"
  | "truncated_model_output"
  | "invalid_json";

export interface ValidationIssue {
  code: IssueCode;
  severity: IssueSeverity;
  field: string | null;
  message: string;
}

export interface FieldConfidence {
  field: string;
  confidence: number;
}

export type MapperRecipeDraft = Omit<Recipe, "id" | "extractionStatus"> & {
  id: string | null;
};

export interface MapperResponse {
  recipes: MapperRecipeDraft[];
  fieldConfidence: FieldConfidence[];
  unmappedText: string[];
  notes: string[];
}

export interface ImportRecord {
  id: string;
  status: "uploaded" | "ocr_ready" | "mapped" | "reviewed" | "committed";
  ocrText: string | null;
  draft: MapperResponse | null;
  issues: ValidationIssue[];
  createdAt: string;
  updatedAt: string;
}
