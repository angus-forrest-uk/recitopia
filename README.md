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
  -> llm map            tools/ml/deepseek_mapper.py -> RecipeDraft
  -> validate                                       -> issues[]
  -> review                                         -> extractionStatus
  -> commit                                         -> Recipe
```

## Structure

```
schema/document.ts    CookbookImport, CookbookPage, CookbookSection, CookbookContentBlock
schema/recipe.ts      Recipe, Ingredient, InstructionStep, AlternateName, SourcePageSpan
schema/mapper.ts      MapperRequest, MapperResponse
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
  environmentFile = "/etc/recitopia/deepseek";
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

## LLM mapper

```sh
DEEPSEEK_BASE_URL=https://api.deepseek.com
DEEPSEEK_MODEL=deepseek-v4-flash
DEEPSEEK_API_KEY=
```

```json
{
  "model": "deepseek-v4-flash",
  "response_format": { "type": "json_object" },
  "messages": [
    { "role": "system", "content": "..." },
    { "role": "user", "content": "..." }
  ]
}
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

RECITOPIA_DEEPSEEK_SCRIPT=tools/ml/deepseek_mapper.py
DEEPSEEK_API_KEY=
```

```sh
install -d -m 0700 /etc/recitopia
printf 'DEEPSEEK_API_KEY=%s\n' "$KEY" > /etc/recitopia/deepseek
chmod 0600 /etc/recitopia/deepseek
```

## API

```http
GET  /api/health
GET  /api/catalogue

POST /api/imports/images
POST /api/imports/cookbook-archive
GET  /api/imports/:id
PUT  /api/imports/:id/draft
POST /api/imports/:id/commit

POST /api/cookbook-recipe-drafts

GET  /api/recipes
PUT  /api/recipes/:id
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

## Checks

```sh
curl -s http://127.0.0.1:8077/api/health
curl -s http://127.0.0.1:8078/health
curl -s http://127.0.0.1:8077/api/catalogue
```
