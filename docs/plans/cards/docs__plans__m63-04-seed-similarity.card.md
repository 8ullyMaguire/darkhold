# docs/plans/m63-04-seed-similarity.md — plan card

Source: `docs/plans/m63-04-seed-similarity.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

| `crates/domain/src/similar.rs` | **new** — tokenizer, idf, similarity, ranking. No database, no clock. |
| `crates/domain/src/lib.rs` | `pub mod similar;` |
| `crates/db/src/catalogue.rs` | `CatalogueQuery` + `author`, `tokens`, `exclude_account`; `filters_sql` emits them; `order_by` unchanged |
| `crates/db/src/metrics.rs` | `CandidateRow` + `title`, `summary`, `author_text`; new `similar()` |
| `crates/app/src/routes/catalogue.rs` | `CatalogueParams` + `author`, `seed`, `exclude_read`; `search()` gains a seed branch; `MaybeSession` |
| `crates/cli/src/rec.rs` | `--seed`, `--author`, `--exclude-read` |
| `crates/db/tests/catalogue_similar.rs` | **new** — SQLite + PostgreSQL |
| `crates/app/tests/m63_04_seed.rs` | **new** — route contract |
Three pre-existing warnings in `crates/db/src/work_ratings.rs` are expected and untouched.
Seven unit tests in `crates/domain/src/similar.rs`, six store tests in
`crates/db/tests/catalogue_similar.rs` (6 SQLite, 6 PostgreSQL), six route tests in
`crates/app/tests/m63_04_seed.rs`. Five mutations, each caught by the test that names it:

## Copy-pasteable code / commands

pub fn tokens(text: &str) -> Vec<String>      // lower, split on non-alnum, len >= 3, stopwords out, dedup
pub struct Seed { id, title_tokens, body_tokens, author }   // title weight 3.0, body weight 1.0
pub struct Idf { df: HashMap<String, u32>, n: u32 }          // built from the candidate population
pub fn similarity(seed: &Seed, other: &Seed, idf: &Idf) -> (f64, Vec<String>)

cargo test -p darkhold-db --test catalogue_similar                       # sqlite, 5 passed
DARKHOLD_TEST_PG_URL=postgres://... cargo test -p darkhold-db --test catalogue_similar   # pg, 5 passed

curl -s '127.0.0.1:8081/api/v1/catalogue?seed=<id>&limit=5'          # 200, sort=similarity
curl -s '127.0.0.1:8081/api/v1/catalogue?seed=nope'                  # 404, "no catalogue row matches"
curl -s '127.0.0.1:8081/api/v1/catalogue?seed=worm&limit=3'          # 422 listing the candidate titles when ambiguous
curl -s '127.0.0.1:8081/api/v1/catalogue?exclude_read=1'             # 422, names the field, says it needs a session
curl -s '127.0.0.1:8081/api/v1/catalogue?author=Oshha&limit=50'      # every item's author_text == "Oshha"

## Verification command and expected output

Every response carries the gate: `ranked N of M` (`N` = candidates that share at least one token
**Step 1 — domain.** `cargo test -p darkhold-domain similar` → `6 passed`.
cargo test -p darkhold-db --test catalogue_similar                       # sqlite, 5 passed
DARKHOLD_TEST_PG_URL=postgres://... cargo test -p darkhold-db --test catalogue_similar   # pg, 5 passed
dh rec --exclude-read --limit 5        # exit 2: "--exclude-read needs --token"
cargo test -p darkhold-domain -p darkhold-db -p darkhold-app --lib
cargo test -p darkhold-app --test m63_04_seed
Three pre-existing warnings in `crates/db/src/work_ratings.rs` are expected and untouched.
| mutation | test that must fail |
`crates/app/tests/m63_04_seed.rs`. Five mutations, each caught by the test that names it:
runner-up — is documented on `resolve_seed` and has its own test
prove the test notices.

## RED/GREEN evidence

**Step 1 — domain.** `cargo test -p darkhold-domain similar` → `6 passed`.
cargo test -p darkhold-db --test catalogue_similar                       # sqlite, 5 passed
DARKHOLD_TEST_PG_URL=postgres://... cargo test -p darkhold-db --test catalogue_similar   # pg, 5 passed
   `400 Failed to deserialize query string` for `exclude_read=1`, which is exactly
