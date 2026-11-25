#!/usr/bin/env bash
# M45-75.7: prove the popularity ORDER BY arm is load-bearing.
# The source is restored by the EXIT trap whether the mutant passes or fails.
set -u
cd /personal/documents/code/rust/darkhold
F=crates/db/src/search/ast_search.rs
cp "$F" /tmp/m45-75-ast-orig.rs
trap 'cp /tmp/m45-75-ast-orig.rs "$F"' EXIT

python3 - "$F" <<'PY'
import sys
p = sys.argv[1]
s = open(p).read()
old = '''        Some("popularity" | "likes" | "kudos" | "aggregate") => {
            "s.likes DESC, s.score DESC, s.updated_at DESC"
        }
'''
assert s.count(old) == 1, "the popularity arm must exist exactly once"
new = '''        Some("popularity" | "likes" | "kudos" | "aggregate") => "s.score DESC, s.updated_at DESC",
'''
open(p, "w").write(s.replace(old, new, 1))
PY

CARGO_TARGET_DIR="$HOME/.cargo-target/darkhold" \
  cargo test -p darkhold-db --test stale_popular_unrated_search 2>&1 \
  | grep -E 'panicked|assertion|test result'
echo "MUT_EXIT=${PIPESTATUS[0]}"
