# docs/plans/m45-64-erasure-covers-fk-free-accounts.md — plan card

Source: `docs/plans/m45-64-erasure-covers-fk-free-accounts.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`docs/plans/m45-51-subject-access-and-erasure.md` measured the erasure graph by parsing the migrations for
### 1. `crates/db/src/erasure.rs`
### 2. `crates/db/tests/erasure_covers_fk_free_accounts.rs`
* `docs/plans/m45-51-subject-access-and-erasure.md` gains the correction to its "99%" claim.

## Copy-pasteable code / commands

cargo test -p darkhold-db --test erasure_covers_fk_free_accounts   # SQLite
DARKHOLD_TEST_PG_URL=… cargo test -p darkhold-db --test erasure_covers_fk_free_accounts   # PostgreSQL

## Verification command and expected output

## The rule this pass establishes
The invariant test parses the migrations, collects every account-keyed column outside the FK graph, and
The schema-invariant test described above, plus one positive test per treatment against a real database:
* `M45-64` (new) → the measurement, the table, and the invariant test.
* `M45-58` gains a note that its deferred classification is now enforced by the test.
cargo test -p darkhold-db --test erasure_covers_fk_free_accounts   # SQLite
DARKHOLD_TEST_PG_URL=… cargo test -p darkhold-db --test erasure_covers_fk_free_accounts   # PostgreSQL
Red first, by removal: with `FK_FREE_ACCOUNT_COLUMNS` empty and the apply loop gone, the invariant test
still passes (it reads the const) — **so the load-bearing test is the per-treatment one against a real
not by deleting code. Both are stated because a test that cannot fail is not evidence.

## RED/GREEN evidence

Red first, by removal: with `FK_FREE_ACCOUNT_COLUMNS` empty and the apply loop gone, the invariant test
database**, and the invariant's own red is proven by adding a fake table to the migrations scan fixture,
