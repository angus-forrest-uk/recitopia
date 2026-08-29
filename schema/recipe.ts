export type ExtractionStatus = "draft" | "needs_review" | "verified";

export type AlternateNameKind = "native" | "romanized" | "translated" | "subtitle" | "alias";

export interface AlternateName {
  kind: AlternateNameKind;
  value: string;
}

export interface SourcePageSpan {
  pageId: string;
  printedPageNumber: number | null;
  lineStart: number | null;
  lineEnd: number | null;
  confidence: number | null;
}

export interface Ingredient {
  id: string;
  position: number;
  section: string | null;
  displayName: string;
  item: string | null;
  quantity: number | null;
  unit: string | null;
  preparation: string | null;
  optional: boolean;
  alternativeText: string | null;
  sourceLine: number | null;
  sourcePageId: string | null;
}

export interface InstructionStep {
  id: string;
  position: number;
  section: string | null;
  text: string;
  sourcePageId: string | null;
  sourceLineStart: number | null;
  sourceLineEnd: number | null;
}

export interface Recipe {
  id: string;
  cookbookId: string | null;
  title: string;
  subtitle: string | null;
  alternateNames: AlternateName[];
  headnote: string | null;
  notes: string | null;
  servingContext: string | null;
  yield: string | null;
  timeTotalMinutes: number | null;
  timeActiveMinutes: number | null;
  cuisine: string | null;
  category: string | null;
  tags: string[];
  ingredients: Ingredient[];
  steps: InstructionStep[];
  componentRecipeIds: string[];
  continuedFromRecipeId: string | null;
  continuedToRecipeId: string | null;
  pageStart: number | null;
  pageEnd: number | null;
  picturedPageNumber: number | null;
  sourceLabel: string | null;
  sourceBlockId: string | null;
  sourcePageSpans: SourcePageSpan[];
  extractionStatus: ExtractionStatus;
}
