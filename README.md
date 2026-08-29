# recitopia

TODO

## Pipeline

```
image set | pdf
  -> page crop          tools/ocr/page_crop.py
  -> ocr                tools/ocr/paddle_ocr.py     -> CookbookPage.ocrText, ocrJson
  -> page classify                                  -> CookbookPage.pageKind
  -> section build                                  -> CookbookSection
  -> block split                                    -> CookbookContentBlock
  -> llm map            tools/ml/llm_mapper.py -> RecipeDraft
  -> validate                                       -> issues[]
  -> review                                         -> extractionStatus
  -> commit                                         -> Recipe
```

## Structure

```
schema/document.ts    CookbookImport, CookbookPage, CookbookSection, CookbookContentBlock
schema/recipe.ts      Recipe, Ingredient, InstructionStep, AlternateName, SourcePageSpan
schema/mapper.ts      MapperRequest, MapperResponse
docs/cookbook-ingestion-structure.md
```

## Layout

```
apps/web         React, Vite, TypeScript, Zod
apps/api-rs      Rust, Axum, DuckDB
tools/ocr        PaddleOCR adapter, FastAPI server, page-edge crop
tools/ml         LLM provider layer and schema mappers
nix              package and NixOS module
```

## Client

```sh
bun install
bun run dev:web
```

```sh
bun run check
bun run test:web
bun run test:e2e
```

```sh
bun run build
```

## Server

```sh
cd apps/api-rs
RECITOPIA_API_PORT=8077 \
RECITOPIA_RUST_STORE_MODE=read-write \
cargo run --features bundled-duckdb
```

```sh
cd apps/api-rs
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

```sh
nix --extra-experimental-features 'nix-command flakes' build .#recitopia-api-rust
```

```nix
services.recitopia-api = {
  enable = true;
  host = "0.0.0.0";
  port = 8077;
  dataDir = "/var/lib/recitopia";
  importDir = "/var/lib/recitopia/imports";
  environmentFile = "/etc/recitopia/llm";
  ocrPython = "/var/lib/recitopia/ocr-venv/bin/python";
  ocrServerEnable = true;
  ocrServerHost = "127.0.0.1";
};
```

## OCR service

```sh
python -m venv /var/lib/recitopia/ocr-venv
/var/lib/recitopia/ocr-venv/bin/python -m pip install \
  paddlepaddle-gpu==3.2.0 -i https://www.paddlepaddle.org.cn/packages/stable/cu118/
/var/lib/recitopia/ocr-venv/bin/python -m pip install "paddleocr[all]"
/var/lib/recitopia/ocr-venv/bin/python -m pip install \
  numpy pillow opencv-python-headless fastapi uvicorn
```

```sh
systemctl enable --now recitopia-ocr.service
curl -s http://127.0.0.1:8078/health
```

```http
GET  /health
POST /ocr
POST /ocr/batch
```

## LLM provider

```
anthropic   google   openai   openrouter   llm
```

No default. `RECITOPIA_LLM_PROVIDER` must be set.

```sh
RECITOPIA_LLM_PROVIDER=anthropic
RECITOPIA_LLM_API_KEY=
```

```sh
RECITOPIA_LLM_PROVIDER=anthropic     ANTHROPIC_API_KEY=
RECITOPIA_LLM_PROVIDER=google        GOOGLE_API_KEY=      # or GEMINI_API_KEY
RECITOPIA_LLM_PROVIDER=openai        OPENAI_API_KEY=
RECITOPIA_LLM_PROVIDER=openrouter    OPENROUTER_API_KEY=
RECITOPIA_LLM_PROVIDER=llm      RECITOPIA_LLM_API_KEY=
```

```sh
RECITOPIA_LLM_MODEL=
RECITOPIA_LLM_BASE_URL=
RECITOPIA_LLM_MAX_TOKENS=5000
RECITOPIA_LLM_TIMEOUT=90
RECITOPIA_LLM_ATTEMPTS=3
RECITOPIA_LLM_HTTP_REFERER=
RECITOPIA_LLM_APP_TITLE=
```

```sh
RECITOPIA_LLM_COOKBOOK_MAX_TOKENS=16000
RECITOPIA_LLM_COOKBOOK_PAGES_PER_REQUEST=4
RECITOPIA_LLM_COOKBOOK_PAGE_OVERLAP=1
RECITOPIA_LLM_COOKBOOK_PARALLELISM=4
RECITOPIA_LLM_COOKBOOK_SECTION_CHAR_BUDGET=28000
RECITOPIA_VERBOSE_LLM_LOGS=true
```

```
anthropic    claude-sonnet-5              https://api.anthropic.com
google       gemini-2.5-flash             https://generativelanguage.googleapis.com
openai       gpt-5                        https://api.openai.com/v1
openrouter   anthropic/claude-sonnet-5    https://openrouter.ai/api/v1
llm     llm-v4-flash            https://api.llm.com
```

```sh
tools/ml/llm_provider.py            provider registry, request build, response parse
tools/ml/llm_mapper.py         single recipe -> Recipe json
tools/ml/llm_cookbook_mapper.py  whole cookbook -> recipes + content blocks
```

## Environment

```sh
RECITOPIA_API_PORT=8077
RECITOPIA_DB_PATH=data/recitopia.duckdb
RECITOPIA_IMPORT_DIR=data/imports
RECITOPIA_RUST_STORE_MODE=read-write
RECITOPIA_TAR_BIN=/usr/bin/tar

RECITOPIA_OCR_PYTHON=/var/lib/recitopia/ocr-venv/bin/python
RECITOPIA_OCR_SCRIPT=tools/ocr/paddle_ocr.py
RECITOPIA_OCR_SERVER_URL=http://127.0.0.1:8078
RECITOPIA_PAGE_CROP_PYTHON=/var/lib/recitopia/ocr-venv/bin/python
RECITOPIA_PAGE_CROP_SCRIPT=tools/ocr/page_crop.py
RECITOPIA_PAGE_CROP_DISABLED=0

RECITOPIA_LLM_SCRIPT=tools/ml/llm_mapper.py
RECITOPIA_LLM_COOKBOOK_SCRIPT=tools/ml/llm_cookbook_mapper.py
RECITOPIA_LLM_PYTHON=python3
RECITOPIA_LLM_PROVIDER=
RECITOPIA_LLM_API_KEY=
```

```sh
install -d -m 0700 /etc/recitopia
printf 'RECITOPIA_LLM_PROVIDER=%s\nRECITOPIA_LLM_API_KEY=%s\n' "$PROVIDER" "$KEY" \
  > /etc/recitopia/llm
chmod 0600 /etc/recitopia/llm
```

## API

```http
GET  /api/health
GET  /api/catalogue
GET  /api/cookbooks

POST /api/cookbook-imports/archive?cookbookId=&sourcePath=
POST /api/cookbook-imports/images
POST /api/cookbook-imports/:id/ocr
GET  /api/cookbook-imports/:id/progress
POST /api/cookbook-imports/:id/cancel

GET  /api/cookbook-pages/:id
GET  /api/cookbook-pages/:id/image
GET  /api/cookbook-pages/:id/text
POST /api/cookbook-pages/:id/accept-content
GET  /api/cookbooks/:id/blocks
GET  /api/cookbook-content-blocks/:id

POST /api/cookbook-recipe-drafts
POST /api/imports/images
GET  /api/imports/:id
PUT  /api/imports/:id/draft
POST /api/imports/:id/commit

GET  /api/recipes
GET  /api/recipes/:id
PUT  /api/recipes/:id
POST /api/recipes/:id/made

POST /api/pipeline-diagnostics/cookbook
GET  /api/pipeline-diagnostics/:job/progress
```

## Artifacts

```
data/imports/<import-id>/original.<ext>
data/imports/<import-id>/page-crop.png
data/imports/<import-id>/page-crop.json
data/imports/<import-id>/mapper-request.json
data/imports/cookbook-images/<sha256>.<ext>
data/imports/cookbook-archives/<import-id>.tar
```

## Sample

Public domain source for trying the pipeline end to end: *Famous Old Receipts*
(Smith, 1908), archive.org item `famousoldreceipt00smit`, 392 pages,
`NOT_IN_COPYRIGHT`.

```sh
SAMPLE_FIRST_PAGE=1 SAMPLE_LAST_PAGE=24 ./scripts/fetch-sample-cookbook.sh
```

```sh
curl -sS -X POST "$API/api/cookbooks" \
  -H 'content-type: application/json' \
  -d '{"id":"famousoldreceipt00smit","title":"Famous Old Receipts","authors":["Jacqueline Harrison Smith"],"year":1908}'

curl -sS -X POST \
  -H 'content-type: application/x-tar' \
  --data-binary @data/sample/famousoldreceipt00smit.tar \
  "$API/api/cookbook-imports/archive?cookbookId=famousoldreceipt00smit&sourcePath=data/sample/famousoldreceipt00smit"

curl -sS -X POST "$API/api/cookbook-imports/$IMPORT_ID/ocr"
curl -sS "$API/api/cookbook-imports/$IMPORT_ID/progress"
curl -sS "$API/api/cookbooks/famousoldreceipt00smit/blocks"
```

## Tests

```sh
bun run check
bun run test:web
bun run test:api
bun run test:ml
bun run test:e2e
```

## Checks

```sh
curl -s http://127.0.0.1:8077/api/health
curl -s http://127.0.0.1:8078/health
curl -s http://127.0.0.1:8077/api/catalogue
```
