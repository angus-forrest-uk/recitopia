#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:-"$ROOT/dist/recitopia-api.tar.gz"}"
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/recitopia-api.XXXXXX")"
PAYLOAD="$STAGE/recitopia-api"
trap 'rm -rf "$STAGE"' EXIT

mkdir -p "$PAYLOAD/apps" "$PAYLOAD/tools" "$PAYLOAD/docs" "$(dirname "$OUT")"

rsync -a --exclude target "$ROOT/apps/api-rs" "$PAYLOAD/apps/"
rsync -a "$ROOT/nix" "$PAYLOAD/"
rsync -a --exclude __pycache__ --exclude '*.py[co]' \
  "$ROOT/tools/ocr" "$ROOT/tools/ml" "$PAYLOAD/tools/"
rsync -a "$ROOT/docs/" "$PAYLOAD/docs/"
rsync -a "$ROOT/README.md" "$ROOT/LICENSE" "$ROOT/flake.nix" "$ROOT/flake.lock" "$PAYLOAD/"

COPYFILE_DISABLE=1 tar --no-xattrs -czf "$OUT" -C "$STAGE" recitopia-api

echo "wrote $OUT"
if command -v shasum >/dev/null 2>&1; then
  shasum -a 256 "$OUT"
elif command -v sha256sum >/dev/null 2>&1; then
  sha256sum "$OUT"
fi
