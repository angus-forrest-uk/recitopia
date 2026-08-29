# Cookbook ingestion structure

This note uses the `our-korean-kitchen` image set as the representative cookbook format. The
important lesson is that a cookbook is not just a list of recipes. It is a structured source with
front matter, essays, ingredient references, recipe components, menus, suppliers, indexes, page
images, and OCR uncertainty. Recitopia should preserve that source structure first, then materialize
recipes from it.

## Extraction artifacts

The full OCR pass for `our-korean-kitchen` produced:

- `our-korean-kitchen/ocr-text-full.jsonl`: one JSON record per page image, including source path,
  dimensions, confidence metrics, and uncorrected OCR text.
- `our-korean-kitchen/ocr-text-full.md`: a readable transcript grouped by image number.
- Existing quality reports in `our-korean-kitchen/ocr-quality-full-summary.md` and
  `our-korean-kitchen/ocr-quality-full-ranked.csv`.

There are 127 page images. The existing quality summary shows a strong first-pass OCR set, with most
pages above 0.99 average confidence and a small review set caused mostly by page curvature, gutter
text, or dense index layout. The transcript is useful as an ingestion artifact, but it should remain a
private source artifact rather than being treated as user-facing app copy.

## Implementation status

The source-first foundation described below is now represented in the Recitopia model, frontend Zod
contracts, DuckDB schema, catalogue hydration, Parquet export list, and seed data. Recipes now carry
headnotes, alternate names, source spans, component links, ingredient positions/source hints, and an
extraction status. The catalogue now has first-class collections for cookbook imports, pages,
sections, content blocks, menus, glossary entries, suppliers, index entries, and cross-references.
Photo imports also run an optional local Python page-edge crop before OCR, preserving the original
image while writing `page-crop.png` and `page-crop.json` as preprocessing artifacts. The cropper is
Linux/GPU-friendly for the server OCR pipeline and can use OpenCV when available.

Still remaining for a full ingestion product: a batch import endpoint for whole books, layout-aware
OCR bounding boxes, automated page/block classification, and a review UI that shows page images next
to extracted source blocks.

## Observed book structure

`Our Korean Kitchen` follows a common cookbook shape:

- Cover and title pages.
- Contents.
- Introductory narrative.
- Cultural or technique essays, such as the Korean meal, pantry, rice, tofu, kimchi, seasons,
  feast days, and ingredient notes.
- Reference recipes and components, such as stocks and wrappers.
- Menu ideas that are curated collections of recipes by occasion.
- Chapter opener pages with chapter recipe lists.
- Recipe pages, sometimes one recipe per page, sometimes one recipe over multiple pages, and
  sometimes multiple short recipes on one page.
- Recipe sub-sections such as marinade, broth, toppings, dipping sauce, paste, dough, optional stock,
  poaching ingredients, and filling.
- Recipe headnotes with cultural context, serving guidance, storage guidance, substitutions, or
  author commentary.
- Cross-references to pantry pages, related recipes, component recipes, and pictured pages.
- Supplier lists grouped by region.
- Index entries with terms, subterms, page references, and illustration references.

The OCR image number and the printed page number are separate concepts. For example, the contents
page is an image but not the same as printed recipe page numbering. The model needs both.

## Current model fit

The current Recitopia model handles a useful recipe core:

- `Cookbook`: title, authors, ISBN, publisher, year, cover, ownership, sharing.
- `Recipe`: title, cookbook, page range, source label, yield, time, cuisine, category, tags, images,
  ingredients, steps, user notes, cook history, costs, and search text.
- `Ingredient`: display text, normalized item, quantity, unit, preparation, section, and costs.
- `InstructionStep`: position, section, text.

Before this source layer was implemented, that was enough for manually entered recipe cards but not
enough to preserve a cookbook as a source. The main gaps were:

- No durable page-level source text or OCR confidence.
- No chapter or section hierarchy.
- No way to store non-recipe text without misusing recipe notes.
- No recipe headnote field distinct from private/user cooking notes.
- No recipe alternate names, such as native script, romanization, subtitle, or translated name.
- No ingredient position, which means recipe ingredient ordering can be lost even though section
  names exist.
- No component relationship between recipes, such as dumpling wrappers, stocks, sauces, fillings,
  or recipes that explicitly use another recipe.
- No structured menu ideas, supplier resources, glossary/pantry entries, or index entries.
- No source spans connecting an extracted field back to page images and OCR lines.
- No layout data, which makes multi-recipe pages and two-column pages harder to review.

## Recommended source-first model

Add a source layer that stores the cookbook as a document before trying to normalize it into recipe
cards.

### Cookbook import

`CookbookImport`

- `id`
- `cookbookId`
- `sourceKind`: `image_set`, `pdf`, `manual`, `web`
- `sourcePath`
- `status`: `uploaded`, `ocr_ready`, `mapped`, `reviewed`, `committed`
- `ocrEngine`
- `createdAt`
- `updatedAt`
- `reviewNotes`

### Cookbook page

`CookbookPage`

- `id`
- `cookbookId`
- `importId`
- `imageIndex`
- `printedPageLabel`
- `printedPageNumber`
- `imagePath`
- `ocrText`
- `ocrJson`
- `averageConfidence`
- `minimumConfidence`
- `pageKind`: `cover`, `title`, `contents`, `chapter_opener`, `essay`, `reference`, `recipe`,
  `supplier`, `index`, `acknowledgements`, `blank`, `unknown`
- `reviewStatus`: `pending`, `accepted`, `needs_crop`, `needs_ocr_fix`, `ignored`

Future OCR should keep bounding boxes and line order in `ocrJson`; cookbook layout matters.

### Cookbook section

`CookbookSection`

- `id`
- `cookbookId`
- `parentSectionId`
- `title`
- `kind`: `front_matter`, `chapter`, `essay`, `reference`, `recipes`, `back_matter`
- `position`
- `pageStart`
- `pageEnd`

This represents the table of contents and chapter hierarchy independently from recipes.

### Source block

`CookbookContentBlock`

- `id`
- `cookbookId`
- `sectionId`
- `pageStart`
- `pageEnd`
- `position`
- `kind`: `paragraph`, `recipe`, `recipe_headnote`, `ingredient_glossary_entry`, `menu`,
  `supplier`, `index_entry`, `caption`, `callout`
- `title`
- `text`
- `confidence`
- `sourceJson`

This is the bridge between raw pages and normalized entities. It lets Recitopia preserve essays,
pantry explanations, captions, and back matter without forcing everything into `Recipe`.

## Recommended recipe extensions

Keep the current recipe core, but add fields that cookbook extraction needs:

- `subtitle`
- `alternateNames`: array of `{ kind, value }` for romanized names, native names, aliases, and
  translated names.
- `headnote`: the cookbook's recipe introduction, separate from user `notes`.
- `servingContext`: text such as "as a side dish", "starter", "main course", or "to share".
- `sourceBlockId`
- `sourcePageSpans`: array of `{ pageId, printedPageNumber, lineStart, lineEnd, confidence }`.
- `continuedFromRecipeId` and `continuedToRecipeId`, or a simpler `sourcePageSpans` relationship for
  multi-page recipes.
- `componentRecipeIds`: recipes used as stock, wrappers, sauces, marinades, or bases.
- `picturedPageNumber` and/or image crop references.
- `extractionStatus`: `draft`, `needs_review`, `verified`.

Ingredient improvements:

- Add `position`.
- Keep `section`.
- Add `optional`, `alternativeText`, `sourceLine`, and `sourcePageId`.
- Preserve exact `displayName`; use `item`, `quantity`, `unit`, and `preparation` only as normalized
  derivatives.

Step improvements:

- Add `sourcePageId` and optional `sourceLineStart/sourceLineEnd`.
- Consider optional structured hints later: temperature, duration, equipment, storage, make-ahead.

## Non-recipe entities to support

`CookbookMenu`

- Stores menu ideas from the book, grouped by occasion or theme.
- Links to recipes with role/order and optional serving notes.
- This is separate from the user's `MealPlanEntry`.

`CookbookGlossaryEntry`

- Stores pantry or ingredient reference entries.
- Fields: title, aliases, native names, description, storage notes, substitution notes, page range.
- Links recipe ingredients to canonical glossary entries when possible.

`CookbookSupplier`

- Stores supplier lists by region.
- Fields: name, URL, region, notes, source page, review status.
- Supplier data can become stale, so it should be clearly source-derived and reviewable.

`CookbookIndexEntry`

- Stores index terms and subterms.
- Fields: term, subterm, target page label, target recipe ID if resolved, illustration flag.
- This helps search and also helps validate recipe/page mapping.

`CookbookCrossReference`

- Generic link table for references discovered in text.
- Fields: from block/entity, to page/recipe/glossary entry, label, relation kind.
- Relation kinds include `uses_component`, `see_page`, `pictured_on`, `served_with`, `menu_item`,
  and `index_reference`.

## Ingestion workflow

1. Import source images or PDF into `CookbookImport`.
2. Create `CookbookPage` records for every page image.
3. OCR each page and store text, confidence, and layout JSON.
4. Classify pages into front matter, essays, chapter openers, recipes, suppliers, index, and unknown.
5. Build `CookbookSection` from contents and chapter opener pages.
6. Split pages into `CookbookContentBlock` records.
7. Extract recipe candidates from recipe blocks, preserving source spans and confidence.
8. Resolve cross-references: page numbers, related recipes, pantry references, picture references,
   menu links, and index links.
9. Present review UI that shows the page image beside extracted fields.
10. Commit verified recipes while keeping the source layer intact.

## Practical next steps

- Add a whole-book import API that creates `CookbookImport` and `CookbookPage` records from an image
  set or PDF.
- Add automated page and source-block classification before mapping the whole cookbook into recipes.
- Build a review UI that shows page images beside OCR text, source blocks, and extracted recipe
  fields.
- Store OCR bounding boxes in future runs; the current full transcript is useful, but layout-aware
  review will need line geometry.
- Use the index and contents as validation data. They are not just back matter; they help detect
  missing recipes, page-range mistakes, unresolved aliases, and bad OCR.
