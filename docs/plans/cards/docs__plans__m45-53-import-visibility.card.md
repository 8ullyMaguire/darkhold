# docs/plans/m45-53-import-visibility.md — plan card

Source: `docs/plans/m45-53-import-visibility.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Row: `docs/requirements.csv` `M45-53`, status `planned`, notes "Gaps review G3. Biggest
`crates/app/tests/reader_body_copies.rs` — with the comment **"VERIFIED BY INJECTION before
| `routes/library.rs:260,284,299,333,…` | `user.account_id.to_string()` (`RequireSession`) |
| `routes/imports.rs:1451` (`list_library`, `#[allow(dead_code)]`) | `user.account_id.to_string()` |
| `routes/imports.rs:703` → `find_duplicate` | `user.account_id.to_string()` |
`grep -rln MaybeSession crates/app/src/routes/library.rs` → **nothing**. There is no
session-scoped routes (`imports.rs:708,1328,1479`, `library.rs:672`). No visitor can see an
### 2a. `scripts/check-library-visibility.py` — the static gate
Reads `crates/app/src/routes/*.rs` and fails if any handler that reaches `library_items` or
### 2b. `crates/app/tests/import_visibility.rs` — the behavioural proof
| `scripts/check-library-visibility.py` | the static gate + its 6-case self-test |
| `crates/app/tests/import_visibility.rs` | behavioural proof, both engines |

## Copy-pasteable code / commands

WHERE work_id = ?1 AND account_id = ?2 AND state = 'ready' AND plain_text IS NOT NULL

## Verification command and expected output

reputational and copyright risk; verify with counsel."
And the test already exists — `a_reader_cannot_read_another_readers_copy` in
this work was called done — the plan requires it, and a privacy test that has never failed is
That test is the strongest evidence in this repo that the requirement is met, and it predates
2. The one thing genuinely missing is a **standing test that no visitor-reachable route can
   project has shipped four times: an invariant held by convention, not by a gate.
3. `verify with counsel` is **not something code can discharge.** It stays open in the notes
## 2. The work: a static gate, plus one behavioural test
### 2a. `scripts/check-library-visibility.py` — the static gate
This is the same class of gate as the existing `check-page-headings.py`, and it has the same
So the gate's job is to find every candidate and assert the *shape* — and the script prints
**Its self-test is 6 cases and must run in CI**: three that must be caught (account from a path
- **Both engines** on the behavioural test.
- **The static gate's self-test must go red when a planted violation is added**, then the
  planted violation is removed and it goes green again. A gate that has never failed is a gate
| `scripts/check-library-visibility.py` | the static gate + its 6-case self-test |
## 5. Steps, with the command and expected output for each
**Step 1 — write the gate and fail it first.**
`python3 scripts/check-library-visibility.py --self-test` → 6 cases, **6 passed**.
**Step 2 — run the gate on the tree.**

## RED/GREEN evidence

this work was called done — the plan requires it, and a privacy test that has never failed is
- **The static gate's self-test must go red when a planted violation is added**, then the
  planted violation is removed and it goes green again. A gate that has never failed is a gate
`python3 scripts/check-library-visibility.py --self-test` → 6 cases, **6 passed**.
**exactly 1 red**, naming the file and line. Remove the plant, re-run → 6 passed.
identifier) and confirm it goes red on the planted case. Restore byte-identical by `diff`.
`cargo test --workspace -- --test-threads=2` → 0 failed (SQLite).
`cargo test --workspace --no-fail-fast -- --test-threads=1` → 0 failed (PostgreSQL).
