# docs/plans/m45-29-finisher-signals.md — plan card

Source: `docs/plans/m45-29-finisher-signals.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

## Step 1 — domain (`crates/domain/src/finishers.rs`, pure)
## Step 2 — store (`crates/db/src/finishers.rs`)
## Step 3 — routes (`crates/app/src/routes/finishers.rs`), module `finishers`
`crates/db/tests/finishers.rs` (both engines), 8 cases: history counts only `complete` as finished
`crates/app/tests/m45_29_finishers.rs`, 6 cases: both doors are public; a missing pseud is `404`;
`crates/domain/src/finishers.rs` unit tests: median of an odd and an even count; a zero gap is 0
| 6 | `python3 scripts/ideas_catalogue.py` | 176 ideas, 0 homeless |

## Copy-pasteable code / commands

pub enum Cadence { Measurable { median_days: i64, samples: usize }, Unmeasured { reason: String } }
pub struct History { pub finished: i64, pub concluded: i64 }
pub fn cadence(days: &[i64]) -> Cadence;          // median of gaps in whole days
pub fn history(finished: i64, abandoned: i64) -> History;
pub fn summary(h: &History) -> String;            // "finished 9 of 10 concluded works"
pub fn is_positive(summary: &str) -> bool;        // the framing guard, tested

## Verification command and expected output

| # | command | expected |
| 1 | `cargo test -p darkhold-domain --lib finishers` | 6 passed |
| 2 | `cargo test -p darkhold-db --test finishers` | 8 passed |
| 4 | `cargo test -p darkhold-app --test m45_29_finishers` | 6 passed |

## RED/GREEN evidence

"unfinished", "incomplete", "failed", "stalled".
| 1 | `cargo test -p darkhold-domain --lib finishers` | 6 passed |
| 2 | `cargo test -p darkhold-db --test finishers` | 8 passed |
| 3 | same with `DARKHOLD_TEST_PG_URL` | 8 passed |
| 4 | `cargo test -p darkhold-app --test m45_29_finishers` | 6 passed |
