#!/usr/bin/env bash
set -euo pipefail

ITEM="${SAMPLE_ITEM:-famousoldreceipt00smit}"
BASE="${SAMPLE_BASE:-https://archive.org/download}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${SAMPLE_OUT:-$ROOT/data/sample/$ITEM}"
FIRST="${SAMPLE_FIRST_PAGE:-1}"
LAST="${SAMPLE_LAST_PAGE:-24}"
DPI="${SAMPLE_DPI:-200}"

die() { printf 'fetch-sample-cookbook: %s\n' "$*" >&2; exit 1; }
log() { printf '==> %s\n' "$*" >&2; }

command -v curl >/dev/null 2>&1 || die "need curl"
command -v pdftoppm >/dev/null 2>&1 || die "need pdftoppm (poppler-utils)"

mkdir -p "$OUT"
pdf="$OUT/$ITEM.pdf"

if [ ! -s "$pdf" ]; then
  log "downloading $ITEM.pdf"
  curl -fL --max-time 1800 --progress-bar -o "$pdf.part" "$BASE/$ITEM/$ITEM.pdf" \
    || die "download failed: $BASE/$ITEM/$ITEM.pdf"
  mv "$pdf.part" "$pdf"
else
  log "using cached $pdf"
fi

head -c 5 "$pdf" | grep -q '%PDF-' || die "$pdf is not a pdf"

log "rendering pages $FIRST-$LAST at ${DPI}dpi"
rm -f "$OUT"/page-*.png
pdftoppm -png -r "$DPI" -f "$FIRST" -l "$LAST" "$pdf" "$OUT/page"

count=$(find "$OUT" -maxdepth 1 -name 'page-*.png' | wc -l | tr -d ' ')
[ "$count" -gt 0 ] || die "pdftoppm produced no pages"

tar="$OUT.tar"
tar -cf "$tar" -C "$OUT" $(cd "$OUT" && ls page-*.png)

log "$count pages in $OUT"
log "archive $tar"
cat <<EOF

API="\${RECITOPIA_API_URL:-http://127.0.0.1:8077}"

curl -sS -X POST "\$API/api/cookbooks" \\
  -H 'content-type: application/json' \\
  -d '{"id":"$ITEM","title":"Famous Old Receipts","authors":["Jacqueline Harrison Smith"],"year":1908}'

curl -sS -X POST \\
  -H 'content-type: application/x-tar' \\
  --data-binary @"$tar" \\
  "\$API/api/cookbook-imports/archive?cookbookId=$ITEM&sourcePath=$OUT"
EOF
