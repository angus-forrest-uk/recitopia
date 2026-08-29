import { BookOpenCheck, ChevronLeft, ChevronRight, FileWarning } from "lucide-react";
import { useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  acceptCookbookPageContent,
  cookbookPageImageUrl,
  createCookbookRecipeDraft,
  loadCookbookPageText,
  patchCookbookPage,
} from "@/lib/api";
import type {
  Cookbook,
  CookbookCrossReference,
  CookbookGlossaryEntry,
  CookbookIndexEntry,
  CookbookMenu,
  CookbookPage,
  CookbookPageKind,
  CookbookPageReviewStatus,
  CookbookSupplier,
  Recipe,
  RecipeImport,
} from "@/lib/schema";

const selectClassName =
  "flex h-9 rounded-md border border-input bg-background px-3 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

const pageKinds: CookbookPageKind[] = [
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
];

const reviewStatuses: CookbookPageReviewStatus[] = [
  "pending",
  "accepted",
  "needs_crop",
  "needs_ocr_fix",
  "ignored",
];

function kindLabel(value: string) {
  return value.replaceAll("_", " ");
}

function pageNumber(page: CookbookPage) {
  return page.printedPageNumber ?? page.imageIndex;
}

function pageTitle(page: CookbookPage) {
  return `Page ${page.printedPageLabel ?? pageNumber(page)}`;
}

interface CookbookSourceReviewProps {
  cookbook: Cookbook;
  pages: CookbookPage[];
  recipesById: Map<string, Recipe>;
  menus: CookbookMenu[];
  glossaryEntries: CookbookGlossaryEntry[];
  suppliers: CookbookSupplier[];
  indexEntries: CookbookIndexEntry[];
  crossReferences: CookbookCrossReference[];
  onPageUpdated?: () => Promise<void>;
  onUseDraft?: (recipeImport: RecipeImport) => void;
}

export function CookbookSourceReview({
  cookbook,
  pages,
  recipesById,
  menus,
  glossaryEntries,
  suppliers,
  indexEntries,
  crossReferences,
  onPageUpdated,
  onUseDraft,
}: CookbookSourceReviewProps) {
  const [selectedPageId, setSelectedPageId] = useState<string | null>(pages[0]?.id ?? null);
  const [pageText, setPageText] = useState<string | null>(null);
  const [textError, setTextError] = useState<string | null>(null);
  const [imageFailed, setImageFailed] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [isDrafting, setIsDrafting] = useState(false);
  const [isAccepting, setIsAccepting] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [draftText, setDraftText] = useState<string>("");
  const [isSavingText, setIsSavingText] = useState(false);

  const selectedPage = pages.find((page) => page.id === selectedPageId) ?? null;
  const selectedIndex = selectedPage ? pages.indexOf(selectedPage) : -1;
  const textIsDirty = pageText !== null && draftText !== pageText;

  useEffect(() => {
    if (!selectedPageId) {
      return;
    }
    let cancelled = false;
    setPageText(null);
    setDraftText("");
    setTextError(null);
    setImageFailed(false);

    void (async () => {
      const result = await loadCookbookPageText(selectedPageId);
      if (cancelled) {
        return;
      }
      if (result.ok) {
        setPageText(result.pageText.ocrText);
        setDraftText(result.pageText.ocrText);
      } else {
        setTextError(result.error);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [selectedPageId]);

  if (pages.length === 0) {
    return null;
  }

  function selectPage(pageId: string) {
    setSelectedPageId(pageId);
    setNotice(null);
    setError(null);
  }

  function stepPage(offset: number) {
    const next = pages[selectedIndex + offset];
    if (next) {
      selectPage(next.id);
    }
  }

  async function handlePatch(patch: {
    pageKind?: CookbookPageKind;
    reviewStatus?: CookbookPageReviewStatus;
  }) {
    if (!selectedPage) {
      return;
    }
    setIsSaving(true);
    setError(null);
    const result = await patchCookbookPage(selectedPage.id, patch);
    setIsSaving(false);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    await onPageUpdated?.();
  }

  async function handleSaveText() {
    if (!selectedPage || pageText === null) {
      return;
    }
    setIsSavingText(true);
    setNotice(null);
    setError(null);
    const result = await patchCookbookPage(selectedPage.id, { ocrText: draftText });
    setIsSavingText(false);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    setPageText(draftText);
    setNotice("OCR text saved. Drafts and accepted content will use the corrected text.");
    await onPageUpdated?.();
  }

  async function handleCreateDraft() {
    if (!selectedPage) {
      return;
    }
    setIsDrafting(true);
    setNotice(null);
    setError(null);
    const result = await createCookbookRecipeDraft({
      cookbookId: cookbook.id,
      pageId: selectedPage.id,
    });
    setIsDrafting(false);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    onUseDraft?.(result.recipeImport);
    setNotice(
      "Recipe draft created. Open the Compendium tab to review, edit, and commit the recipe.",
    );
  }

  async function handleAcceptContent() {
    if (!selectedPage) {
      return;
    }
    setIsAccepting(true);
    setNotice(null);
    setError(null);
    const result = await acceptCookbookPageContent(selectedPage.id);
    setIsAccepting(false);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    setNotice(
      `Page accepted as ${kindLabel(result.block.kind)} content. It now appears in the cookbook document.`,
    );
    await onPageUpdated?.();
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <BookOpenCheck className="h-4 w-4" aria-hidden="true" />
          Source review
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="grid gap-4 lg:grid-cols-[260px_1fr]">
          <div
            className="max-h-[560px] space-y-1 overflow-y-auto pr-1"
            role="listbox"
            aria-label="Cookbook pages"
          >
            {pages.map((page) => (
              <button
                key={page.id}
                type="button"
                role="option"
                aria-selected={page.id === selectedPageId}
                className={`w-full rounded-md border px-3 py-2 text-left text-sm transition-colors ${
                  page.id === selectedPageId ? "border-primary bg-accent/60" : "hover:bg-accent/40"
                }`}
                onClick={() => selectPage(page.id)}
              >
                <span className="font-medium">{pageTitle(page)}</span>
                <span className="mt-1 flex flex-wrap gap-1">
                  <Badge variant="outline">{kindLabel(page.pageKind)}</Badge>
                  <Badge
                    variant="outline"
                    className={
                      page.reviewStatus === "accepted"
                        ? "border-primary"
                        : page.reviewStatus === "pending"
                          ? ""
                          : "border-destructive text-destructive"
                    }
                  >
                    {kindLabel(page.reviewStatus)}
                  </Badge>
                  {!page.hasOcrText ? <Badge variant="outline">no text</Badge> : null}
                </span>
              </button>
            ))}
          </div>

          {selectedPage ? (
            <div className="space-y-3">
              <div className="flex flex-wrap items-center gap-2">
                <p className="font-medium">{pageTitle(selectedPage)}</p>
                <div className="ml-auto flex flex-wrap items-center gap-2">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={selectedIndex <= 0}
                    onClick={() => stepPage(-1)}
                  >
                    <ChevronLeft className="h-4 w-4" aria-hidden="true" />
                    Previous
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={selectedIndex < 0 || selectedIndex >= pages.length - 1}
                    onClick={() => stepPage(1)}
                  >
                    Next
                    <ChevronRight className="h-4 w-4" aria-hidden="true" />
                  </Button>
                </div>
              </div>

              <div className="flex flex-wrap items-end gap-3">
                <label className="space-y-1 text-sm" htmlFor="source-review-page-kind">
                  <span>Page kind</span>
                  <select
                    id="source-review-page-kind"
                    className={selectClassName}
                    value={selectedPage.pageKind}
                    disabled={isSaving}
                    onChange={(event) =>
                      void handlePatch({ pageKind: event.target.value as CookbookPageKind })
                    }
                  >
                    {pageKinds.map((kind) => (
                      <option key={kind} value={kind}>
                        {kindLabel(kind)}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="space-y-1 text-sm" htmlFor="source-review-page-status">
                  <span>Review status</span>
                  <select
                    id="source-review-page-status"
                    className={selectClassName}
                    value={selectedPage.reviewStatus}
                    disabled={isSaving}
                    onChange={(event) =>
                      void handlePatch({
                        reviewStatus: event.target.value as CookbookPageReviewStatus,
                      })
                    }
                  >
                    {reviewStatuses.map((status) => (
                      <option key={status} value={status}>
                        {kindLabel(status)}
                      </option>
                    ))}
                  </select>
                </label>
                <Button
                  type="button"
                  size="sm"
                  disabled={isDrafting || !selectedPage.hasOcrText}
                  onClick={() => void handleCreateDraft()}
                >
                  {isDrafting ? "Creating draft" : "Create recipe draft"}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={isAccepting || !selectedPage.hasOcrText}
                  onClick={() => void handleAcceptContent()}
                >
                  {isAccepting ? "Accepting" : "Accept as content"}
                </Button>
              </div>

              {notice ? <p className="text-sm text-muted-foreground">{notice}</p> : null}
              {error ? <p className="text-sm text-destructive">{error}</p> : null}

              <div className="grid gap-3 xl:grid-cols-2">
                <div className="rounded-md border bg-muted/30 p-2">
                  {imageFailed ? (
                    <div className="flex h-64 flex-col items-center justify-center gap-2 text-sm text-muted-foreground">
                      <FileWarning className="h-6 w-6" aria-hidden="true" />
                      Page image is not available on this host.
                    </div>
                  ) : (
                    <img
                      key={selectedPage.id}
                      src={cookbookPageImageUrl(selectedPage.id)}
                      alt={`Scanned cookbook ${pageTitle(selectedPage).toLowerCase()}`}
                      className="max-h-[560px] w-full rounded object-contain"
                      onError={() => setImageFailed(true)}
                    />
                  )}
                </div>
                <div className="rounded-md border bg-background p-3">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <label className="text-sm font-medium" htmlFor="source-review-ocr-text">
                      OCR text
                    </label>
                    {textIsDirty ? (
                      <div className="flex gap-2">
                        <Button
                          type="button"
                          size="sm"
                          disabled={isSavingText}
                          onClick={() => void handleSaveText()}
                        >
                          {isSavingText ? "Saving" : "Save correction"}
                        </Button>
                        <Button
                          type="button"
                          size="sm"
                          variant="ghost"
                          disabled={isSavingText}
                          onClick={() => setDraftText(pageText ?? "")}
                        >
                          Discard
                        </Button>
                      </div>
                    ) : null}
                  </div>
                  {textError ? <p className="mt-2 text-sm text-destructive">{textError}</p> : null}
                  {pageText === null && !textError ? (
                    <p className="mt-2 text-sm text-muted-foreground">Loading page text…</p>
                  ) : null}
                  {pageText !== null ? (
                    <textarea
                      id="source-review-ocr-text"
                      className="mt-2 h-[480px] w-full resize-y rounded-md border border-input bg-background p-2 font-sans text-sm text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                      value={draftText}
                      placeholder="No OCR text for this page yet. Type the page text to correct it."
                      onChange={(event) => setDraftText(event.target.value)}
                    />
                  ) : null}
                </div>
              </div>
            </div>
          ) : null}
        </div>

        <SourceEntitySections
          recipesById={recipesById}
          menus={menus}
          glossaryEntries={glossaryEntries}
          suppliers={suppliers}
          indexEntries={indexEntries}
          crossReferences={crossReferences}
        />
      </CardContent>
    </Card>
  );
}

function SourceEntitySections({
  recipesById,
  menus,
  glossaryEntries,
  suppliers,
  indexEntries,
  crossReferences,
}: {
  recipesById: Map<string, Recipe>;
  menus: CookbookMenu[];
  glossaryEntries: CookbookGlossaryEntry[];
  suppliers: CookbookSupplier[];
  indexEntries: CookbookIndexEntry[];
  crossReferences: CookbookCrossReference[];
}) {
  const sections = [
    menus.length > 0 ? (
      <details key="menus" className="rounded-md border p-3">
        <summary className="cursor-pointer text-sm font-medium">Menus ({menus.length})</summary>
        <div className="mt-2 space-y-2">
          {menus.map((menu) => (
            <div key={menu.id} className="rounded border bg-background p-2 text-sm">
              <p className="font-medium">{menu.title}</p>
              {menu.theme ? <p className="text-muted-foreground">Theme: {menu.theme}</p> : null}
              {menu.notes ? <p className="text-muted-foreground">{menu.notes}</p> : null}
              {menu.recipes.length > 0 ? (
                <ul className="mt-1 list-inside list-disc text-muted-foreground">
                  {menu.recipes.map((entry) => (
                    <li key={`${menu.id}-${entry.recipeId}`}>
                      {recipesById.get(entry.recipeId)?.title ?? entry.recipeId}
                      {entry.role ? ` — ${entry.role}` : ""}
                      {entry.servingNotes ? ` (${entry.servingNotes})` : ""}
                    </li>
                  ))}
                </ul>
              ) : null}
            </div>
          ))}
        </div>
      </details>
    ) : null,
    glossaryEntries.length > 0 ? (
      <details key="glossary" className="rounded-md border p-3">
        <summary className="cursor-pointer text-sm font-medium">
          Glossary ({glossaryEntries.length})
        </summary>
        <div className="mt-2 space-y-2">
          {glossaryEntries.map((entry) => (
            <div key={entry.id} className="rounded border bg-background p-2 text-sm">
              <p className="font-medium">
                {entry.title}
                {entry.aliases.length > 0 ? (
                  <span className="text-muted-foreground"> · {entry.aliases.join(", ")}</span>
                ) : null}
              </p>
              {entry.nativeNames.length > 0 ? (
                <p className="text-muted-foreground">{entry.nativeNames.join(", ")}</p>
              ) : null}
              {entry.description ? (
                <p className="text-muted-foreground">{entry.description}</p>
              ) : null}
              {entry.storageNotes ? (
                <p className="text-muted-foreground">Storage: {entry.storageNotes}</p>
              ) : null}
              {entry.substitutionNotes ? (
                <p className="text-muted-foreground">Substitute: {entry.substitutionNotes}</p>
              ) : null}
            </div>
          ))}
        </div>
      </details>
    ) : null,
    suppliers.length > 0 ? (
      <details key="suppliers" className="rounded-md border p-3">
        <summary className="cursor-pointer text-sm font-medium">
          Suppliers ({suppliers.length})
        </summary>
        <div className="mt-2 space-y-2">
          {suppliers.map((supplier) => (
            <div key={supplier.id} className="rounded border bg-background p-2 text-sm">
              <p className="font-medium">
                {supplier.url ? (
                  <a
                    href={supplier.url}
                    target="_blank"
                    rel="noreferrer"
                    className="underline underline-offset-2"
                  >
                    {supplier.name}
                  </a>
                ) : (
                  supplier.name
                )}
                {supplier.region ? (
                  <span className="text-muted-foreground"> · {supplier.region}</span>
                ) : null}
              </p>
              {supplier.notes ? <p className="text-muted-foreground">{supplier.notes}</p> : null}
            </div>
          ))}
        </div>
      </details>
    ) : null,
    indexEntries.length > 0 ? (
      <details key="index" className="rounded-md border p-3">
        <summary className="cursor-pointer text-sm font-medium">
          Index ({indexEntries.length})
        </summary>
        <ul className="mt-2 columns-2 text-sm text-muted-foreground md:columns-3">
          {indexEntries.map((entry) => (
            <li key={entry.id}>
              {entry.term}
              {entry.subterm ? `, ${entry.subterm}` : ""}
              {entry.targetPageLabel ? ` — p. ${entry.targetPageLabel}` : ""}
            </li>
          ))}
        </ul>
      </details>
    ) : null,
    crossReferences.length > 0 ? (
      <details key="references" className="rounded-md border p-3">
        <summary className="cursor-pointer text-sm font-medium">
          Cross references ({crossReferences.length})
        </summary>
        <ul className="mt-2 space-y-1 text-sm text-muted-foreground">
          {crossReferences.map((reference) => (
            <li key={reference.id}>
              {kindLabel(reference.fromKind)} {reference.fromId} → {kindLabel(reference.toKind)}{" "}
              {reference.toId ?? "?"}
              {reference.label ? ` (${reference.label})` : ""}
            </li>
          ))}
        </ul>
      </details>
    ) : null,
  ].filter(Boolean);

  if (sections.length === 0) {
    return null;
  }

  return <div className="space-y-2">{sections}</div>;
}
