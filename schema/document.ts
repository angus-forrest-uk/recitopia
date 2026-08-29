export type SourceKind = "image_set" | "pdf" | "manual" | "web";

export type ImportStatus = "uploaded" | "ocr_ready" | "mapped" | "reviewed" | "committed";

export type PageKind =
  | "cover"
  | "title"
  | "contents"
  | "chapter_opener"
  | "essay"
  | "reference"
  | "recipe"
  | "supplier"
  | "index"
  | "acknowledgements"
  | "blank"
  | "unknown";

export type PageReviewStatus = "pending" | "accepted" | "needs_crop" | "needs_ocr_fix" | "ignored";

export type SectionKind =
  | "front_matter"
  | "chapter"
  | "essay"
  | "reference"
  | "recipes"
  | "back_matter";

export type BlockKind =
  | "paragraph"
  | "recipe"
  | "recipe_headnote"
  | "ingredient_glossary_entry"
  | "menu"
  | "supplier"
  | "index_entry"
  | "caption"
  | "callout";

export type RelationKind =
  | "uses_component"
  | "see_page"
  | "pictured_on"
  | "served_with"
  | "menu_item"
  | "index_reference";

export interface CookbookImport {
  id: string;
  cookbookId: string;
  sourceKind: SourceKind;
  sourcePath: string;
  status: ImportStatus;
  ocrEngine: string | null;
  createdAt: string;
  updatedAt: string;
  reviewNotes: string | null;
}

export interface OcrLine {
  text: string;
  confidence: number;
  box: [number, number, number, number];
  order: number;
}

export interface CookbookPage {
  id: string;
  cookbookId: string;
  importId: string;
  imageIndex: number;
  printedPageLabel: string | null;
  printedPageNumber: number | null;
  imagePath: string;
  ocrText: string | null;
  ocrJson: { lines: OcrLine[] } | null;
  averageConfidence: number | null;
  minimumConfidence: number | null;
  pageKind: PageKind;
  reviewStatus: PageReviewStatus;
}

export interface CookbookSection {
  id: string;
  cookbookId: string;
  parentSectionId: string | null;
  title: string;
  kind: SectionKind;
  position: number;
  pageStart: number | null;
  pageEnd: number | null;
}

export interface CookbookContentBlock {
  id: string;
  cookbookId: string;
  sectionId: string | null;
  pageStart: number;
  pageEnd: number;
  position: number;
  kind: BlockKind;
  title: string | null;
  text: string;
  confidence: number | null;
  sourceJson: unknown | null;
}

export interface CookbookMenu {
  id: string;
  cookbookId: string;
  title: string;
  occasion: string | null;
  items: { recipeId: string | null; label: string; order: number; note: string | null }[];
  pageStart: number | null;
  pageEnd: number | null;
}

export interface CookbookGlossaryEntry {
  id: string;
  cookbookId: string;
  title: string;
  aliases: string[];
  nativeNames: string[];
  description: string | null;
  storageNotes: string | null;
  substitutionNotes: string | null;
  pageStart: number | null;
  pageEnd: number | null;
}

export interface CookbookSupplier {
  id: string;
  cookbookId: string;
  name: string;
  url: string | null;
  region: string | null;
  notes: string | null;
  sourcePageId: string | null;
  reviewStatus: PageReviewStatus;
}

export interface CookbookIndexEntry {
  id: string;
  cookbookId: string;
  term: string;
  subterm: string | null;
  targetPageLabel: string | null;
  targetRecipeId: string | null;
  illustration: boolean;
}

export interface CookbookCrossReference {
  id: string;
  cookbookId: string;
  fromBlockId: string | null;
  fromRecipeId: string | null;
  toPageId: string | null;
  toRecipeId: string | null;
  toGlossaryEntryId: string | null;
  label: string | null;
  relationKind: RelationKind;
}
