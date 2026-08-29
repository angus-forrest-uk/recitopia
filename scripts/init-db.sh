#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DB="${1:-${RECITOPIA_DB_PATH:-$ROOT/data/recitopia.duckdb}}"
FIXTURE="$ROOT/apps/api-rs/tests/fixtures/phase2_catalogue.sql"

die() { printf 'init-db: %s\n' "$*" >&2; exit 1; }
log() { printf '==> %s\n' "$*" >&2; }

command -v duckdb  >/dev/null 2>&1 || die "need duckdb"
command -v python3 >/dev/null 2>&1 || die "need python3"
[ -f "$FIXTURE" ] || die "no fixture at $FIXTURE"
[ ! -e "$DB" ] || die "$DB already exists"

mkdir -p "$(dirname "$DB")"
schema="$(mktemp "${TMPDIR:-/tmp}/recitopia-schema.XXXXXX.sql")"
trap 'rm -f "$schema"' EXIT

python3 - "$FIXTURE" "$schema" <<'PY'
import io, sys

fixture, out = sys.argv[1], sys.argv[2]
statements = [s.strip() for s in io.open(fixture, encoding="utf-8").read().split(";") if s.strip()]
creates = [s for s in statements if s.lower().lstrip().startswith("create")]
if not creates:
    raise SystemExit("no create statements in fixture")
io.open(out, "w", encoding="utf-8").write(";\n".join(creates) + ";\n")
print(f"{len(creates)} tables, {len(statements) - len(creates)} fixture rows skipped")
PY

log "creating $DB"
duckdb "$DB" -c ".read $schema" >/dev/null

count="$(duckdb "$DB" -noheader -list -c "select count(*) from information_schema.tables")"
log "$count tables"

cat <<EOF

The schema comes from the API's test fixture, which is the only schema in this
repository. It is not a migration system and can drift from a database created
by an older version.

A cookbook needs at least one author and there is no author endpoint, so seed
one before creating cookbooks:

  duckdb "$DB" -c "insert into authors values ('some-author','Some Author',null)"
EOF
