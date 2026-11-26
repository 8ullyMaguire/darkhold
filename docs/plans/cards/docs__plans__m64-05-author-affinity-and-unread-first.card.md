# docs/plans/m64-05-author-affinity-and-unread-first.md — plan card

Source: `docs/plans/m64-05-author-affinity-and-unread-first.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

ordered by that surface's field (`docs/requirements.csv` `M64-05`, priority P5).
## Step 1 — `crates/db/src/series_recs.rs`: viewer-aware ordering
Three tests, in `crates/db/tests/same_author_unread_first.rs`, on SQLite with `DARKHOLD_TEST_PG_URL`
## Step 2 — `crates/app/src/routes/works.rs`: pass the viewer
- `docs/spec/11-search-and-discovery.md` §11.5 "What the implementation settled": record that the
  `python3 scripts/split-spec.py && python3 scripts/check-split-spec.py`.)
- `docs/requirements.csv` `M64-05`: `implemented-locally-tested` only if step 2 lands; otherwise
- `docs/verification.md`: the fixture, the three tests, the mutation, and what the change does not
| 3 | spec §11.5 as-built + requirements + verification entry | `python3 scripts/check-split-spec.py` | `41 files, slots 00-40` |

## Copy-pasteable code / commands

pub async fn same_author_works(db: &Database, work_id: &str) -> Result<Vec<RelatedWork>>

pub async fn same_author_works(
    db: &Database,
    work_id: &str,
    viewer: Option<&str>,        // the account id, when a session exists
) -> Result<Vec<RelatedWork>>

ORDER BY CASE WHEN EXISTS (SELECT 1 FROM reading_status rs
                            WHERE rs.account_id = ?
                              AND rs.subject_type = 'work'
                              AND rs.subject_id = w.id
                              AND rs.status IN ('reading','finished')) THEN 1
              ELSE 0 END,
         w.published_at DESC, w.id

## Verification command and expected output

| `author_affinity_for(db, account)` — the graded ratio, 1–5 | **written down, not built**: an unconsumed helper is dead code, and a "feature" whose only test calls it directly is inert by construction |
`IS NULL` branch, and it was removed after a mutation exposed that no test could observe it: a branch
can test is a guard nobody should trust.
recommendation that looks slightly wrong rather than a failure — which is why the fourth test marks a
work through the library-item subject and the mutation that drops this arm fails only that test.
cargo test -p darkhold-db --test same_author_unread_first
   older]`. The only difference from test 1 is the viewer, so together they prove the viewer is what
**Mutation, run:** with `ELSE 0` → `ELSE 1` (every row demoted) test 1 fails and the other two pass;
restored, 3 pass. The opposite (`THEN 1` → `THEN 0`) also fails test 1. A mutation that leaves all
## Step 2 — `crates/app/src/routes/works.rs`: pass the viewer
cargo test -p darkhold-app --test more_like_this
plus one new test in that file: `a_signed_in_reader_is_offered_the_unread_work_first`, which finishes
no order to differ from, and the anonymous arm then decides nothing. The first run of this test
  be called only by its own test. Write it when rung 4 or an "authors you read" surface exists; the
| # | step | verification | expected |
| 1 | viewer-aware ORDER BY in `same_author_works` | `cargo test -p darkhold-db --test same_author_unread_first` | `4 passed` |
| 2 | route passes the session's account | `cargo test -p darkhold-app --test more_like_this` | `4 passed` |

## RED/GREEN evidence

# 3 passed
three green is not a mutation, and the first draft of this plan's `IS NULL` branch was exactly that.
# the same count as before the change, all green
passed alone and failed in the file — a fixture whose premise is a timestamp it did not control.
| 1 | viewer-aware ORDER BY in `same_author_works` | `cargo test -p darkhold-db --test same_author_unread_first` | `4 passed` |
| 2 | route passes the session's account | `cargo test -p darkhold-app --test more_like_this` | `4 passed` |
