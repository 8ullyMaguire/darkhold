# docs/plans/m63-01-dh-rec.md — plan card

Source: `docs/plans/m63-01-dh-rec.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Decision recorded elsewhere, honoured here:** `docs/plans/cli-placement.md` puts the CLI in this
workspace as `crates/cli`, binary `dh`, an **HTTP client of `/api/v1`** that may not depend on
`crates/app` or `crates/db` at runtime. This plan builds the first verb, and therefore the crate.
The anchor ask, from `docs/ideas/2026-11-01-idea-sweep-power-users.md`:
### 3. `crates/cli` — binary `dh`
`crates/db/tests/catalogue_fandom.rs` — both engines:
  route's, tested in `crates/app/tests/m63_01_rec.rs`.
`crates/app/tests/m63_01_rec.rs`:
`crates/cli/tests/rec.rs` — the CLI's contract, against a router built in-process:
4. **`crates/cli`.**
   numbers; `docs/requirements.csv` M63-01 → `implemented-verified-e2e`.
- **Schema drift on the live instance.** `migrations/postgres` creates 11 tables the live database

## Copy-pasteable code / commands

dh rec --fandom naruto --updated 7d --engine rrf --sort wilson

$ sudo -u postgres psql -tA -F'|' -d darkhold -c "SELECT ce.canonical, count(DISTINCT li.id) \
  FROM library_items li JOIN work_tags wt ON wt.work_id = li.work_id \
  JOIN canonical_entities ce ON ce.id = wt.node_id WHERE ce.kind='fandom' GROUP BY 1 ORDER BY 2 DESC LIMIT 3"
Harry Potter|2311
Marvel|349
A Song of Ice and Fire - George R. R. Martin|338
Naruto|265

AND EXISTS (
    SELECT 1 FROM work_tags wt
      JOIN canonical_entities ce ON ce.id = wt.node_id
     WHERE wt.work_id = library_items.work_id
       AND ce.kind = 'fandom'
       AND ce.norm = ?
)

## Verification command and expected output

  only in older test comments; it is the pre-rename name.
| condition | what `dh` prints | exit |
   `cargo test -p darkhold-db --test catalogue_fandom` → `4 passed`.
   `cargo test -p darkhold-app --test m63_01_rec` → the four catalogue cases pass;
   `cargo test -p darkhold-app --test m63_01_rec` → `6 passed`.
   `cargo test -p darkhold-cli` → parse + render + contract tests pass.
- **`engine` on a public route is impossible.** If a token-less `dh rec --engine rrf` is expected to
| the filter works on both engines | `cargo test -p darkhold-db --test catalogue_fandom` | 5 passed on SQLite, 5 passed on PostgreSQL |
| the routes work on both engines | `cargo test -p darkhold-app --test m63_01_rec` | 9 passed on SQLite, 9 passed on PostgreSQL |
| the CLI's parsing and rendering | `cargo test -p darkhold-cli` | 10 passed |
| nothing else moved | `cargo test -p darkhold-db -p darkhold-app --lib` | 236 + 157 + 784 passed |
- never **suggest** a near name (`suggest("zzzznomatchzzzz", …)`) → exactly one test fails, with the
`dead_code` (warnings are denied), which is not a verdict about the test. The mutation has to keep

## RED/GREEN evidence

   `cargo test -p darkhold-db --test catalogue_fandom` → `4 passed`.
   `cargo test -p darkhold-app --test m63_01_rec` → `6 passed`.
  axum answer `400 Failed to deserialize query string: explain: provided string was not true or
| the filter works on both engines | `cargo test -p darkhold-db --test catalogue_fandom` | 5 passed on SQLite, 5 passed on PostgreSQL |
| the routes work on both engines | `cargo test -p darkhold-app --test m63_01_rec` | 9 passed on SQLite, 9 passed on PostgreSQL |
| the CLI's parsing and rendering | `cargo test -p darkhold-cli` | 10 passed |
| nothing else moved | `cargo test -p darkhold-db -p darkhold-app --lib` | 236 + 157 + 784 passed |
"engines verified live" on the strength of a green unit suite would be the exact claim this project
