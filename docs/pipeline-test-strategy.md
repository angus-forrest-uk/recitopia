# Pipeline Test Strategy

The cookbook ingestion pipeline should not depend on a full 127-page cookbook import to prove every
part still works. Use the full `our-korean-kitchen` fixture as a broad regression check, but keep the
daily test surface smaller and sharper.

## Test Layers

1. Image packaging and dedupe
   - Input: a tiny archive with two or three images, including one duplicate hash.
   - Expected output: deterministic image paths, duplicate rejection, and stable page ordering.
   - This should not run OCR or LLM.

2. OCR adapter contract
   - Input: one known-good image or a mocked OCR server response.
   - Expected output: the JSON shape consumed by the importer: text, confidence, engine, and
     failures.
   - This verifies the GPU/Paddle process boundary without involving recipe extraction.

3. Source-map heuristics
   - Input: idealized `CookbookPage` OCR text.
   - Expected output: page kind classification, section ranges, connective text blocks, and page
     overlap rules.
   - Add smaller Rust
     tests beside the import pipeline when changing a single heuristic.

4. LLM mapper contract
   - Input: `tools/ml/testdata/mini_cookbook_payload.json`, a six-page ideal OCR payload.
   - Expected output: correct page batching, section bounds handling, deterministic merge order for
     parallel responses, duplicate recipe removal, and normalized content block positions.
   - Run with `bun run test:ml`. These tests fake LLM, so they are fast, deterministic, and do
     not spend API credits.

5. Persistence transaction contract
   - Input: already-normalized recipes, pages, sections, and content blocks.
   - Expected output: atomic DuckDB writes, stale import rows removed only for the target import,
     derived fields recomputed, and rollback on invalid rows.
   - This belongs in Rust/DuckDB tests and should not call OCR or LLM.

6. API smoke with miniature import
   - Input: a tiny archive or fixture payload through HTTP.
   - Expected output: import record, progress/job state, page count, mapped sections, reviewable
     recipes, and idempotent regenerate behavior.
   - This is the right replacement for using the whole cookbook as the normal end-to-end smoke.

7. Whole-book regression
   - Input: the full `our-korean-kitchen` OCR fixture or real import on the server.
   - Expected output: sensible section ranges, recipe counts, and no persistence failures.
   - Run this manually or nightly. It is a confidence check, not a unit test.

## Commands

```sh
bun run test:ml
bun run test:api
bun run test
bun run harness -- verify --suite quick
```

## Fixture Policy

- Keep tiny fixtures human-readable and checked in.
- Prefer idealized text at each boundary, then add one real-world fixture only when a bug needs it.
- Fake paid or slow services in unit tests. Networked LLM and GPU OCR runs belong in smoke or
  deployment checks.
- When a test fails, it should identify the broken layer: packaging, OCR contract, source map,
  LLM mapping, persistence, API orchestration, or frontend review.
