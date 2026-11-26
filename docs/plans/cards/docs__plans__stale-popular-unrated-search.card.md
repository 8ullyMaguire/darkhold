# docs/plans/stale-popular-unrated-search.md — plan card

Source: `docs/plans/stale-popular-unrated-search.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`docs/spec-amendments/stale-popular-unrated-search.md` (row `M45-75`).
- Every store statement is written twice (`db.sql(sqlite, postgres)` or a
- Targeted tests only. No `gate.sh all`, no full-workspace sweep mid-turn.
File: `crates/domain/src/tropes.rs`. Add a group **before** `MAX_MEMBERS`:
so a `tag:` leaf costs **one** term, not two (see `query.rs::QueryBudget::walk`).
Files: `crates/domain/src/query.rs`, `crates/domain/src/query_sql.rs`.
**2.3** SQL. In `query_sql.rs` add the viewer-bind sentinel near the top:
File: `crates/db/src/search/ast_search.rs`.
**3.4** Contract test `crates/db/tests/stale_popular_unrated_search.rs`
`DARKHOLD_TEST_PG_URL` exactly as `docs/plans/.../gate-run-hygiene` prescribes).
File: `crates/cli/src/q.rs`.
Tests in `crates/cli/tests/q_source_sort.rs`: URL contains five `source:` terms

## Copy-pasteable code / commands

git -C ~/code/rust/darkhold status --short          # expect empty
git -C ~/code-local/rust/darkhold fetch cmp
git -C ~/code-local/rust/darkhold merge --ff-only cmp/master
git -C ~/code-local/rust/darkhold worktree add -b m45-75-stale-search \
  ~/code-local/rust/darkhold-m45-75 master
export CARGO_TARGET_DIR=~/.cargo-target-darkhold-dev

ssh thinkcentre python3 - <<'PY'
import json, urllib.parse, urllib.request
base = 'http://127.0.0.1:8081/api/v1/search'
for q in ['source:qq', 'source:qq OR source:sv', 'trope:system-grind',
          'tag:celestial', 'tag:cyoa', 'updated:<=-120d']:
    u = base + '?' + urllib.parse.urlencode({'q': q, 'limit': 5})
    with urllib.request.urlopen(u, timeout=180) as r:
        b = json.loads(r.read().decode())
    print(q, len(b['items']), [i['title'][:28] for i in b['items'][:2]])
PY

ssh thinkcentre "sudo -u postgres psql -d darkhold -P pager=off -c \"
select count(*) filter (where updated_at::timestamptz < now() - interval '120 days') as works_stale, count(*) as works_total from works;
select count(*) filter (where source_updated_at < to_char(now() - interval '120 days','YYYY-MM-DD\\\"T\\\"HH24:MI:SS\\\"Z\\\"')) as items_stale, count(*) as items_total from library_items where source_updated_at is not null;\""

## Verification command and expected output

`~/code-local/rust/darkhold` on a worktree, build/test on **thinkcentre**
- Every new test runs on SQLite **and** on PostgreSQL
  (`DARKHOLD_TEST_PG_URL`, container `lh-pg-test`, 55433, reached from the
- Targeted tests only. No `gate.sh all`, no full-workspace sweep mid-turn.
  this plan adds **no route**, so that gate should stay green unchanged.
Expected: `Preparing worktree (new branch 'm45-75-stale-search')`, both
Expected (measured): `source:qq` → `0`, `source:qq OR source:sv` → `0`,
Expected: `0 | 58042` and `86541 | 90662` (counts may drift with ingestion; the
Test in the same file:
    #[test]
            assert!(members.contains(&word), "{word} missing from {members:?}");
        assert!(members.len() <= MAX_MEMBERS);
Verify:
cargo test -p darkhold-domain system_progression_names_the_five_words_a_reader_types
Expected: `test result: ok. 1 passed; 0 failed` on the worktree. Commit:
Verify:
cargo test -p darkhold-domain query --lib
cargo test -p darkhold-domain --lib query_sql
Expected: both `test result: ok`, no failures. Commit:
**3.4** Contract test `crates/db/tests/stale_popular_unrated_search.rs`

## RED/GREEN evidence

  this plan adds **no route**, so that gate should stay green unchanged.
Expected: `test result: ok. 1 passed; 0 failed` on the worktree. Commit:
Expected: `test result: ok. N passed; 0 failed` **twice**, same N both times.
test — it must go red; restore before committing. Commit:
Expected: all three green; `route_inventory` stays 5/5 (no new route).
