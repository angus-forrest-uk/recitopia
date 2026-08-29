#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ITEM="${SAMPLE_ITEM:-famousoldreceipt00smit}"
PAGE="${1:-45}"
DPI="${SHOWCASE_DPI:-200}"
D="${SAMPLE_OUT:-$ROOT/data/sample/$ITEM}"
OUT="${SHOWCASE_OUT:-$ROOT/data/showcase}"
PDF="$D/$ITEM.pdf"

die() { printf 'showcase-page: %s\n' "$*" >&2; exit 1; }
log() { printf '==> %s\n' "$*" >&2; }

[ -n "${RECITOPIA_OCR_PYTHON:-}" ] \
  || die "set RECITOPIA_OCR_PYTHON (see scripts/setup-ocr-venv.sh)"
[ -x "$RECITOPIA_OCR_PYTHON" ] || die "not executable: $RECITOPIA_OCR_PYTHON"
[ -n "${RECITOPIA_LLM_PROVIDER:-}" ] || die "set RECITOPIA_LLM_PROVIDER"
command -v pdftoppm >/dev/null 2>&1 || die "need pdftoppm (poppler-utils)"
[ -s "$PDF" ] || die "no sample pdf at $PDF; run scripts/fetch-sample-cookbook.sh first"

mkdir -p "$OUT"
img="$OUT/page-$PAGE.png"
if [ ! -s "$img" ]; then
  log "rendering page $PAGE at ${DPI}dpi"
  pdftoppm -png -r "$DPI" -f "$PAGE" -l "$PAGE" -singlefile "$PDF" "${img%.png}" \
    || die "pdftoppm failed"
fi

log "stage 1: ocr"
"$RECITOPIA_OCR_PYTHON" "$ROOT/tools/ocr/paddle_ocr.py" "$img" \
  > "$OUT/ocr.json" 2>"$OUT/ocr.err" \
  || { tail -5 "$OUT/ocr.err" >&2; die "ocr failed"; }

"$RECITOPIA_OCR_PYTHON" - "$OUT/ocr.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
layout = d.get("layout") or {}
print(f"    engine  {d.get('engine')}")
print(f"    layout  {layout.get('mode')} columns={layout.get('columns')} blocks={len(layout.get('blocks') or [])}")
print(f"    text    {len(d.get('text') or '')} chars")
print(f"    raw     {len(json.dumps(d.get('raw'))):,} bytes")
PY

log "stage 2: mapper request"
"$RECITOPIA_OCR_PYTHON" - "$OUT/ocr.json" "$OUT/request.json" "$ITEM" "$PAGE" <<'PY'
import json, sys
ocr = json.load(open(sys.argv[1]))
page = int(sys.argv[4])
json.dump(
    {
        "cookbookId": sys.argv[3],
        "authorIds": [],
        "pageStart": page,
        "pageEnd": page,
        "sourceLabel": f"{sys.argv[3]} p.{page}",
        "sourceBlockId": None,
        "sourcePageSpans": [],
        "ocrText": ocr.get("text") or "",
    },
    open(sys.argv[2], "w"),
    ensure_ascii=False,
)
PY

log "stage 3: llm map via $RECITOPIA_LLM_PROVIDER"
"$RECITOPIA_OCR_PYTHON" "$ROOT/tools/ml/llm_mapper.py" "$OUT/request.json" \
  > "$OUT/recipe.json" 2>"$OUT/map.err" \
  || { tail -8 "$OUT/map.err" >&2; die "mapping failed"; }

"$RECITOPIA_OCR_PYTHON" - "$OUT/recipe.json" <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))
print(f"    title       {r.get('title')}")
print(f"    category    {r.get('category')}")
print(f"    status      {r.get('extractionStatus')}  tags={r.get('tags')}")
print(f"    ingredients {len(r.get('ingredients') or [])}")
print(f"    steps       {len(r.get('steps') or [])}")
PY

log "wrote $OUT/ocr.json, $OUT/request.json, $OUT/recipe.json"
