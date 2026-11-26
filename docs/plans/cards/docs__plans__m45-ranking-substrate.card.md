# docs/plans/m45-ranking-substrate.md — plan card

Source: `docs/plans/m45-ranking-substrate.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Implements `docs/spec.md` §47 (added 2026-09-30) and closes five M45 rows:
exists: `personalized_recommendations` (`crates/db/src/discovery.rs:209`) orders
Read `docs/spec.md` §47 first. It is the contract; this file is the sequence.
>    (`crates/db/tests/`), and it compares migration *declarations*, not columns —
>    workspace `time` crate (`crates/db/Cargo.toml:26`); see
>    `crates/db/src/economy.rs:227`. And `sql_owned` takes `String`, not `&str`.
> `crates/db/src/ranking.rs`.
| `personalized_recommendations` | `crates/db/src/discovery.rs:209` |
| `public_recommendations` | `crates/db/src/discovery.rs:148` |
| `content_filter_sql::for_pseud` / `::exclusion_for` | `crates/db/src/content_filter_sql.rs` |
| `sql_owned(db, sqlite, postgres)` | `crates/db/src/lib.rs:395` |
| `crate::Backend::{Sqlite,Postgres}` | `crates/db/src/lib.rs` |

## Copy-pasteable code / commands

cargo test -p darkhold-db --test migration_catalogue   # 6 passed

let sqlite  = "UPDATE recommendation_slots SET slot_kind = ?, propensity = ? WHERE id = ?";
let postgres = "UPDATE recommendation_slots SET slot_kind = $1, propensity = $2 WHERE id = $3";
// ...
let sql = sql_owned(db, sqlite.to_owned(), postgres.to_owned());

#[must_use]
pub fn for_source(source: &str) -> Self {
    match source {
        "reading_club" | "topic_subscription" | "bounty" | "taste_notification" => {
            Self::Incentivized
        }
        _ => Self::Earned,
    }
}

## Verification command and expected output

> 1. **`migration_parity` does not exist.** The real test is `migration_catalogue`
> 4. **A unit test caught a real defect in the spec's own formula.**
> was not a wiring exercise: it exposed three defects in `rank_works` that no test
> in its own module could see, and the workspace gate then exposed a conflict
| `TestDb::connect_with_dir(tag, dir)` | `crates/test-support/src/lib.rs:283` |
| `scratch_dir(tag)` | `crates/test-support/src/lib.rs:143` |
cargo test -p darkhold-db --test migration_catalogue   # 6 passed
Plus a behavioural probe against both engines, because the catalogue test does not
the one place where the spec's formula was wrong and the test proved it.
**Verified:** `cargo test -p darkhold-db --lib` → **82 passed**. The NaN fix is
   which is §47.6's own worked example written as an assertion. The test failed
   on first run and the fix went into the blend, not into the test.
**Verified:** `cargo test -p darkhold-db --lib` → **97 passed**, no clippy
**Verify:** a table-driven unit test over the corners (0.0, 1.0, negatives,
Every test runs on **both** engines via `TestDb::connect_with_dir`. One file, one
cargo test -p darkhold-app --test m45_ranking
  cargo test -p darkhold-app --test m45_ranking
**Injection gate.** Prove cases 1, 3 and 5 by breaking them: delete the
and confirm case 5 fails. A test that has never been seen red is not evidence.
`implemented-fully-tested`, with the test names in `evidence`, in the same commit

## RED/GREEN evidence

cargo test -p darkhold-db --test migration_catalogue   # 6 passed
**Verified:** `cargo test -p darkhold-db --lib` → **82 passed**. The NaN fix is
shown red by removing the filter: `scout_value_is_clamped_at_both_ends` fails
   which is §47.6's own worked example written as an assertion. The test failed
**Verified:** `cargo test -p darkhold-db --lib` → **97 passed**, no clippy
and confirm case 5 fails. A test that has never been seen red is not evidence.
**A test that must go red first.** The route's ordering is currently
| SQLite | **3426 passed, 0 failed**, exit 0, `--test-threads=4` |
| PostgreSQL | **3426 passed, 0 failed**, exit 0, `--test-threads=2` |
`the_readers_weights_decide_the_order` passed alone and failed in the workspace
Both present in the tree while an earlier gate reported green. A gate result is
- [x] `cargo test -p darkhold-app --test m45_ranking` green on **both** engines
