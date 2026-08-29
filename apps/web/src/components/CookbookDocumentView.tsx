import { BookOpen } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { loadCookbookContentBlocks, patchCookbookContentBlock } from "@/lib/api";
import type { Cookbook, CookbookContentBlock, CookbookSection, Recipe } from "@/lib/schema";
import { formatMinutes, formatMoney } from "@/lib/utils";

type DocumentItem =
  | { kind: "block"; block: CookbookContentBlock }
  | { kind: "recipe"; recipe: Recipe };

interface DocumentSection {
  key: string;
  title: string;
  pageStart: number | null;
  pageEnd: number | null;
  items: DocumentItem[];
}

function normalizedTitle(value: string | null): string {
  return (value ?? "").trim().toLowerCase();
}

function itemPage(item: DocumentItem): number {
  const page = item.kind === "block" ? item.block.pageStart : item.recipe.pageStart;
  return page ?? Number.MAX_SAFE_INTEGER;
}

function itemPosition(item: DocumentItem): number {
  return item.kind === "block" ? item.block.position : Number.MAX_SAFE_INTEGER;
}

function sortItems(items: DocumentItem[]): DocumentItem[] {
  return items.sort((left, right) => {
    const byPage = itemPage(left) - itemPage(right);
    if (byPage !== 0) {
      return byPage;
    }
    return itemPosition(left) - itemPosition(right);
  });
}

/// Assemble the single-page reading document: sections in book order, prose
/// blocks inline, and extracted recipes embedded at their source position.
/// A recipe replaces its source block (matched by sourceBlockId, or by a
/// recipe-kind block with the same title) so the text is not duplicated;
/// recipes without a block anchor are placed by page within their section.
export function assembleCookbookDocument(
  sections: CookbookSection[],
  blocks: CookbookContentBlock[],
  recipes: Recipe[],
): DocumentSection[] {
  const orderedSections = [...sections].sort(
    (left, right) => left.position - right.position || left.title.localeCompare(right.title),
  );
  const sectionIds = new Set(orderedSections.map((section) => section.id));

  const recipeByBlockId = new Map<string, Recipe>();
  const unanchored: Recipe[] = [];
  for (const recipe of recipes) {
    const anchor =
      (recipe.sourceBlockId != null && blocks.find((block) => block.id === recipe.sourceBlockId)) ||
      blocks.find(
        (block) =>
          block.kind === "recipe" &&
          !recipeByBlockId.has(block.id) &&
          normalizedTitle(block.title) === normalizedTitle(recipe.title),
      );
    if (anchor) {
      recipeByBlockId.set(anchor.id, recipe);
    } else {
      unanchored.push(recipe);
    }
  }

  function blockToItem(block: CookbookContentBlock): DocumentItem {
    const embedded = recipeByBlockId.get(block.id);
    return embedded ? { kind: "recipe", recipe: embedded } : { kind: "block", block };
  }

  const remaining = [...unanchored];
  function takeRecipesForSection(section: CookbookSection): DocumentItem[] {
    if (section.pageStart == null && section.pageEnd == null) {
      return [];
    }
    const taken: DocumentItem[] = [];
    for (let index = remaining.length - 1; index >= 0; index -= 1) {
      const page = remaining[index].pageStart;
      if (page == null) {
        continue;
      }
      if (section.pageStart != null && page < section.pageStart) {
        continue;
      }
      if (section.pageEnd != null && page > section.pageEnd) {
        continue;
      }
      taken.push({ kind: "recipe", recipe: remaining[index] });
      remaining.splice(index, 1);
    }
    return taken;
  }

  const document: DocumentSection[] = orderedSections.map((section) => ({
    key: section.id,
    title: section.title,
    pageStart: section.pageStart,
    pageEnd: section.pageEnd,
    items: sortItems([
      ...blocks.filter((block) => block.sectionId === section.id).map(blockToItem),
      ...takeRecipesForSection(section),
    ]),
  }));

  const orphanItems = sortItems([
    ...blocks
      .filter((block) => block.sectionId == null || !sectionIds.has(block.sectionId))
      .map(blockToItem),
    ...remaining.map((recipe): DocumentItem => ({ kind: "recipe", recipe })),
  ]);
  if (orphanItems.length > 0) {
    document.push({
      key: "unsectioned",
      title: document.length > 0 ? "More from this book" : "Recipes",
      pageStart: null,
      pageEnd: null,
      items: orphanItems,
    });
  }

  return document.filter((section) => section.items.length > 0 || sectionIds.has(section.key));
}

function pageRange(pageStart: number | null, pageEnd: number | null): string | null {
  if (pageStart && pageEnd && pageStart !== pageEnd) {
    return `pp. ${pageStart}-${pageEnd}`;
  }
  const page = pageStart ?? pageEnd;
  return page ? `p. ${page}` : null;
}

export function CookbookDocumentView({
  cookbook,
  sections,
  previewBlocks,
  recipes,
}: {
  cookbook: Cookbook;
  sections: CookbookSection[];
  previewBlocks: CookbookContentBlock[];
  recipes: Recipe[];
}) {
  // Render immediately from catalogue previews, then swap in the full block
  // text once it arrives from /api/cookbooks/:id/blocks.
  const [fullBlocks, setFullBlocks] = useState<CookbookContentBlock[] | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // previewBlocks is a dependency so a catalogue refresh (e.g. after a page
  // is accepted as content) refetches the full text instead of going stale.
  // biome-ignore lint/correctness/useExhaustiveDependencies: previewBlocks is the deliberate refresh signal for the full-text fetch
  useEffect(() => {
    let cancelled = false;
    setNotice(null);

    void (async () => {
      const result = await loadCookbookContentBlocks(cookbook.id);
      if (cancelled) {
        return;
      }
      if (result.ok) {
        setFullBlocks(result.blocks);
      } else {
        setFullBlocks(null);
        setNotice("Showing text previews; the full cookbook text could not be loaded.");
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [cookbook.id, previewBlocks]);

  const blocks = fullBlocks ?? previewBlocks;
  const document = useMemo(
    () => assembleCookbookDocument(sections, blocks, recipes),
    [sections, blocks, recipes],
  );

  if (document.length === 0) {
    return null;
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <BookOpen className="h-4 w-4" aria-hidden="true" />
          {cookbook.title} — cookbook document
        </CardTitle>
      </CardHeader>
      <CardContent>
        {notice ? <p className="mb-3 text-sm text-muted-foreground">{notice}</p> : null}
        <article className="mx-auto max-w-3xl space-y-8">
          {document.map((section) => (
            <section key={section.key} className="space-y-4">
              <div className="flex flex-wrap items-baseline gap-2 border-b pb-2">
                <h3 className="text-lg font-semibold">{section.title}</h3>
                {pageRange(section.pageStart, section.pageEnd) ? (
                  <span className="text-sm text-muted-foreground">
                    {pageRange(section.pageStart, section.pageEnd)}
                  </span>
                ) : null}
              </div>
              {section.items.map((item) =>
                item.kind === "recipe" ? (
                  <EmbeddedRecipe key={`recipe-${item.recipe.id}`} recipe={item.recipe} />
                ) : (
                  <DocumentBlock
                    key={`block-${item.block.id}`}
                    block={item.block}
                    // Editing needs full text: previews would truncate on save.
                    editable={fullBlocks !== null}
                    onSaved={(updated) =>
                      setFullBlocks(
                        (current) =>
                          current?.map((candidate) =>
                            candidate.id === updated.id ? updated : candidate,
                          ) ?? current,
                      )
                    }
                  />
                ),
              )}
            </section>
          ))}
        </article>
      </CardContent>
    </Card>
  );
}

function DocumentBlock({
  block,
  editable,
  onSaved,
}: {
  block: CookbookContentBlock;
  editable: boolean;
  onSaved: (block: CookbookContentBlock) => void;
}) {
  const [isEditing, setIsEditing] = useState(false);
  const [draft, setDraft] = useState(block.text);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!block.hasText && !block.title && !isEditing) {
    return null;
  }

  async function handleSave() {
    setIsSaving(true);
    setError(null);
    const result = await patchCookbookContentBlock(block.id, { text: draft });
    setIsSaving(false);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    setIsEditing(false);
    onSaved(result.block);
  }

  if (isEditing) {
    return (
      <div className="space-y-2">
        {block.title ? <p className="font-medium">{block.title}</p> : null}
        <textarea
          className="h-56 w-full resize-y rounded-md border border-input bg-background p-2 text-sm text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          value={draft}
          aria-label={`Edit text of ${block.title ?? block.id}`}
          onChange={(event) => setDraft(event.target.value)}
        />
        {error ? <p className="text-sm text-destructive">{error}</p> : null}
        <div className="flex gap-2">
          <Button type="button" size="sm" disabled={isSaving} onClick={() => void handleSave()}>
            {isSaving ? "Saving" : "Save text"}
          </Button>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            disabled={isSaving}
            onClick={() => {
              setDraft(block.text);
              setIsEditing(false);
              setError(null);
            }}
          >
            Cancel
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="group space-y-1">
      <div className="flex items-baseline justify-between gap-2">
        {block.title ? <p className="font-medium">{block.title}</p> : <span />}
        {editable ? (
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="opacity-0 transition-opacity focus-visible:opacity-100 group-hover:opacity-100"
            onClick={() => {
              setDraft(block.text);
              setIsEditing(true);
            }}
          >
            Edit text
          </Button>
        ) : null}
      </div>
      {block.hasText ? (
        <p className="whitespace-pre-wrap text-sm leading-relaxed text-muted-foreground">
          {block.text}
        </p>
      ) : null}
    </div>
  );
}

function EmbeddedRecipe({ recipe }: { recipe: Recipe }) {
  const ingredientSections = new Map<string, typeof recipe.ingredients>();
  for (const ingredient of recipe.ingredients) {
    const key = ingredient.section ?? "";
    const list = ingredientSections.get(key) ?? [];
    list.push(ingredient);
    ingredientSections.set(key, list);
  }
  const steps = [...recipe.steps].sort((left, right) => left.position - right.position);

  return (
    <div className="rounded-lg border bg-card p-4 shadow-sm">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h4 className="text-base font-semibold">{recipe.title}</h4>
        <span className="text-xs text-muted-foreground">{recipe.sourceLabel}</span>
      </div>
      <div className="mt-2 flex flex-wrap gap-1">
        {recipe.yieldQuantity ? (
          <Badge variant="outline">
            {recipe.yieldQuantity} {recipe.yieldUnit ?? "servings"}
          </Badge>
        ) : null}
        <Badge variant="outline">{formatMinutes(recipe.totalMinutes)}</Badge>
        <Badge variant="outline">{formatMoney(recipe.costPerServingCents)} / serving</Badge>
        {recipe.tags.slice(0, 3).map((tag) => (
          <Badge key={tag}>{tag}</Badge>
        ))}
      </div>
      {recipe.headnote ? (
        <p className="mt-3 whitespace-pre-wrap text-sm italic text-muted-foreground">
          {recipe.headnote}
        </p>
      ) : null}
      <div className="mt-3 grid gap-4 sm:grid-cols-[1fr_2fr]">
        <div>
          <p className="text-sm font-medium">Ingredients</p>
          {[...ingredientSections.entries()].map(([sectionName, ingredients]) => (
            <div key={sectionName || "main"} className="mt-1">
              {sectionName ? (
                <p className="text-xs font-medium uppercase text-muted-foreground">{sectionName}</p>
              ) : null}
              <ul className="mt-1 space-y-1 text-sm text-muted-foreground">
                {ingredients.map((ingredient) => (
                  <li key={ingredient.id}>{ingredient.displayName}</li>
                ))}
              </ul>
            </div>
          ))}
        </div>
        <div>
          <p className="text-sm font-medium">Method</p>
          <ol className="mt-1 list-inside list-decimal space-y-1 text-sm text-muted-foreground">
            {steps.map((step) => (
              <li key={step.id}>{step.text}</li>
            ))}
          </ol>
          {recipe.notes.length > 0 ? (
            <div className="mt-3 space-y-1">
              {recipe.notes.map((note) => (
                <p key={note.id} className="text-xs text-muted-foreground">
                  Note: {note.text}
                </p>
              ))}
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}
