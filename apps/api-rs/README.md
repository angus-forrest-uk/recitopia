# Recitopia Rust API

This directory contains the Rust Recitopia API. It is the default production
implementation for the main `recitopia-api` Nix package and systemd service.


## Current milestone: production cutover wiring

- Axum/Tokio server. The standalone Rust default remains port `8079`; the
  production NixOS module sets `RECITOPIA_API_PORT=8077`.
- Exact Zig-compatible `GET /api/health`, catch-all `OPTIONS`, CORS, and 404
  response contracts.
- Read-only implementations of `GET /api/catalogue`, `GET /api/pantry`,
  `GET /api/meal-plan`, `GET /api/cook-log`,
  `GET /api/cookbook-pages/:id/text`, `GET /api/cookbook-pages/:id/image`,
  and `GET /api/cookbooks/:id/blocks`.
- Opt-in pantry, meal-plan, cookbook, and recipe mutations, including
  `POST /api/recipes/:id/made`. The latter atomically updates cook history,
  writes substitutions, and creates leftover pantry rows.
- Transactional replacement of all recipe child tables, recipe deletion with
  meal-plan cleanup, and preservation of historical cook-log/pantry references.
- Zig-compatible recipe validation, derived cost/duration/search fields, and
  cache buckets using the exact Zig 0.16 WyHash revision.
- Typed catalogue hydration for cookbook source entities and complete nested
  recipes, including ingredient quantity-review fields.
- Zig-compatible 420-byte UTF-8-safe page/block previews, elided source JSON,
  full-text detail reads, page-image ETags, and conditional `304` responses.
- Structured JSON logs with stable `event` fields and `FAULT` panic severity.
- Read-only-by-default DuckDB connection and readiness checks.
- Native DuckDB behind an explicit Cargo feature so HTTP/config tests stay
  lightweight; system `libduckdb` for Nix/the server builds.
- Graceful SIGINT/SIGTERM shutdown.
- Cookbook image/archive ingestion, content-addressed source assets, cached AVIF
  derivatives, editable OCR/content blocks, and accepted page content.
- Detached cookbook OCR/regeneration and diagnostic jobs with persisted
  progress, duplicate-start suppression, cooperative cancel files, and
  cancellation-safe terminal state handling.
- Persistent PaddleOCR batch calls with subprocess fallback, deterministic
  source mapping, conservative printed-page interpolation, LLM cookbook
  extraction, and atomic replacement of generated sections, blocks, and
  recipes. Recipe heuristics are intentionally not used as a fallback.
- Editable photo/text recipe drafts with validation and transactional commit.
- Bounded cookbook and introduction-page diagnostics that retain every
  OCR/source-map/LLM input and output artifact.
- Production Nix packaging and a NixOS service path through
  `services.recitopia-api`.
- A legacy NixOS shadow service and Go normalized-response comparator for
  comparison work.

The implementation still does not own schema migrations, so deployment uses an
already migrated DuckDB. Interrupted jobs are persisted but not resumed after
process restart. The Playwright suite starts this service with
`RECITOPIA_DB_PATH=:memory:`, which is only useful once a schema/seed
initializer exists.

## Run locally

```sh
cd apps/api-rs
RECITOPIA_RUST_DB_PATH=/tmp/recitopia-shadow.duckdb \
RECITOPIA_RUST_STORE_MODE=read-write \
cargo run --features bundled-duckdb
```

The Rust service does not own schema migrations yet. Use a copied, already
migrated Recitopia database or initialize a disposable fixture from
`tests/fixtures/phase2_catalogue.sql`; an empty `:memory:` database only supports
the health probe.

The Rust-specific environment variables take precedence over their existing
Zig equivalents:

- `RECITOPIA_RUST_API_HOST` (default `127.0.0.1`)
- `RECITOPIA_RUST_API_PORT` (default `8079`)
- `RECITOPIA_RUST_DB_PATH` (default `../../data/recitopia.duckdb`)
- `RECITOPIA_RUST_STORE_MODE` (`read-only` by default; `read-write` is explicit)
- `RECITOPIA_RUST_IMPORT_DIR`
- `RECITOPIA_RUST_OCR_SERVER_URL`
- `RECITOPIA_RUST_OCR_PYTHON` and `RECITOPIA_RUST_OCR_SCRIPT`
- `RECITOPIA_RUST_LLM_PYTHON`
- `RECITOPIA_RUST_LLM_COOKBOOK_SCRIPT`
- `RECITOPIA_RUST_LLM_RECIPE_SCRIPT`
- `RECITOPIA_RUST_OCR_BATCH_PAGE_LIMIT`
- `RECITOPIA_RUST_PIPELINE_CONCURRENCY`

Use a copied the server database for shadow tests. Do not point the Rust service at
the live database in read-write mode while the Zig service is running.

## Helix CLI frontend

`recitopia-hx` turns the local Helix command line into a temporary file-based
Recitopia frontend for the remote API:

```sh
cd apps/api-rs
cargo run --bin recitopia-hx -- open
```

The default API URL is `http://127.0.0.1:8077`. Override it with
`RECITOPIA_API_URL` or `--api-url http://127.0.0.1:8077` for local API work. It
writes a temporary POSIX-style recipe shelf: canonical recipe pages live as
Recitopia-profile `.cook` files in `recipes/`, browse indexes live under
`cookbooks/`, `tags/`, `categories/`, `ingredients/`, and `time/`, and
pantry/meal-plan/history pages sit at the workspace root as markdown. Source OCR
pages are written under `source/`, and interpreted non-recipe cookbook content is
written under each cookbook's `content/` directory. The generated
`.helix/languages.toml` starts `recitopia-hx lsp` as a Rust language server for
those files. The LSP exposes markdown and Cooklang headings as document symbols,
supports hover/goto-definition for known Recitopia IDs, validates the
Recitopia-profile `.cook` files, and sends derived recipe JSON back to the API on
save with `PUT /api/recipes/:id`.

Materialize without launching Helix:

```sh
cargo run --bin recitopia-hx -- materialize --workspace /tmp/recitopia-hx
```

## Verify

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Validate a captured Zig catalogue without storing it in the repository:

```sh
cargo run --example check_catalogue_contract -- /tmp/zig-catalogue.json
```

The checker deserializes the complete response into the Rust model and requires
lossless JSON round-trip parity. Optional floating-point values use Zig's JSON
formatting, so whole values remain `3` rather than becoming `3.0`; it also
recomputes and compares every recipe cache bucket.

From the repository root, the equivalent first-party commands are:

```sh
bun run test:api:rust
bun run check:api:rust
bun run build:api:rust
```

The bundled DuckDB feature needs several gigabytes of temporary build space.
Production and the server compatibility checks use:

```sh
cargo test --no-default-features --features system-duckdb
```

The flake exposes this service as `recitopia-api` and `recitopia-api-rust`.
The package pins stable
Rust `1.88.0` through `rust-overlay`: the server's current nixpkgs Rust 1.94/LLVM
21 compiler crashes in `LoopSimplifyCFG` while optimizing ordinary DuckDB
dependencies.

The pinned package and all 30 native tests have been verified on the server
against DuckDB 1.5.2. The native suite includes a 30-table miniature cookbook
fixture, nested hydration, malformed-JSON failsafes, read-only enforcement,
complete Phase 3 HTTP CRUD, and real constraint failures proving transaction
rollback. A live Zig catalogue containing 3 cookbooks, 63 recipes, 128 pages,
and 52 blocks round-trips exactly through the Rust model. A temporary
read-write listener on `127.0.0.1:8079` persisted a pantry mutation and was
stopped afterward; no Rust systemd service has been installed.

Those figures describe the Phase 3 checkpoint. The crate now defines 47 tests;
23 native-light tests pass locally, strict Clippy passes both target sets, and
all 47 pass in the pinned Nix build against the server's system DuckDB 1.5.2. The
NixOS module flake check also builds on the server. Installing a consistent copied
database and running the live shadow comparator remain Phase 6 gates.

## Shadow on the server

`./module.nix` installs the Rust API on Tailscale port
`8079` with a copied database, separate import directory, shared local OCR
service, and read-only storage by default. The module rejects read-write access
to the live Zig database path.

