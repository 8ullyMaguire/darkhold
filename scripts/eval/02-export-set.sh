#!/usr/bin/env bash
# Export the evaluation set for the baseline ladder (spec §12.7a, §24.4; requirement M64-16).
#
#   scripts/eval/02-export-set.sh sqlite data/darkhold.sqlite --out eval/eval-set.json
#   scripts/eval/02-export-set.sh postgres "postgres://darkhold@localhost/darkhold" --out eval/eval-set.json
#
# Read-only, temporal split, readers pseudonymised. `eval/` is gitignored: the file names who read
# what, and §0.3 keeps a reader-level artefact off any network.
#
# Exit codes: 0 wrote a set (which may hold zero readers — that is a measurement), 2 bad usage, and
# the engine's own non-zero on a broken query. A zero-reader set is the expected result on a corpus
# with no engagement, and the ladder prints its refusals rather than failing.
set -euo pipefail

usage() {
    echo "usage: $0 sqlite PATH|postgres DSN [--out FILE]" >&2
    exit 2
}

[ $# -ge 2 ] || usage
engine="$1"
source="$2"
out="eval/eval-set.json"
if [ $# -ge 3 ]; then
    [ "$3" = "--out" ] && [ $# -eq 4 ] || usage
    out="$4"
fi

dirname_of_source="$(dirname "$0")"
mkdir -p "$(dirname "$out")" "$(dirname "$out")/.." >/dev/null 2>&1 || true

# One salt, created once. A per-run salt would make two runs incomparable.
salt="$(dirname "$out")/salt"
if [ ! -f "$salt" ]; then
    head -c 32 /dev/urandom | base64 > "$salt"
    chmod 600 "$salt"
    echo "created $salt (kept: two runs must pseudonymise the same reader the same way)" >&2
fi

case "$engine" in
    sqlite)
        # The export reads post-0124 `canonical_entities` (the taxonomy, keyed by id) and post-0129
        # shapes. An older checkout answers "no such column: ce.id", which names neither the
        # migration nor the fix, so the probe does.
        sqlite3 -readonly "$source" "SELECT id FROM canonical_entities LIMIT 1" >/dev/null 2>&1 || {
            echo "the schema is behind: canonical_entities has no \`id\` column, which migration 0124" >&2
            echo "adds when it renames taxonomy_nodes. Run \`darkhold migrate\` on this database first." >&2
            exit 1
        }
        sqlite3 -readonly "$source" < "$dirname_of_source/02-export-set.sqlite.sql"
        ;;
    postgres)
        # ON_ERROR_STOP, because psql exits 0 after a failed statement by default — a broken query
        # would otherwise hand the pseudonymiser a partial document and the run would read as a
        # smaller population rather than as a failure.
        psql "$source" -tA -q -v ON_ERROR_STOP=1 -c "SELECT id FROM canonical_entities LIMIT 1" \
            >/dev/null 2>&1 || {
            echo "the schema is behind: canonical_entities has no \`id\` column, which migration 0124" >&2
            echo "adds when it renames taxonomy_nodes. Run \`darkhold migrate\` on this database first." >&2
            exit 1
        }
        psql "$source" -tA -q -v ON_ERROR_STOP=1 -f "$dirname_of_source/02-export-set.postgres.sql"
        ;;
    *)
        usage
        ;;
esac > "$out.raw"

# A failed query must not be piped into the pseudonymiser: empty input would produce an empty file
# that reads as "no readers qualified", which is a different claim from "the query did not run".
if [ ! -s "$out.raw" ]; then
    echo "the export produced nothing: the query failed above, and no file was written" >&2
    rm -f "$out.raw"
    exit 1
fi

python3 "$dirname_of_source/pseudonymise.py" "$salt" < "$out.raw" > "$out"
rm -f "$out.raw"

readers="$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1])).get("readers", [])))' "$out")"
echo "wrote $out ($readers reader(s) with >= 5 post-cutoff interactions)" >&2
if [ "$readers" = "0" ]; then
    echo "no reader cleared the split: the corpus has too little engagement to score. That is a" >&2
    echo "measurement, not a failure — run scripts/eval/01-engagement-census.sql to see which rung" >&2
    echo "is blocked, and read the ladder's REFUSED lines in the meantime." >&2
fi
