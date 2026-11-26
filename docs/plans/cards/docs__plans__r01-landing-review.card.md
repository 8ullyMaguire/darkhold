# docs/plans/r01-landing-review.md — plan card

Source: `docs/plans/r01-landing-review.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Fix only server-rendered Darkhold content and its inline style in `crates/app/src/routes/landing_page.rs`. The estate
- The row becomes Done in `docs/reviews/page-review-queue.md` in the same commit whose message names R-01 and whose

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

- After expected: `overflow=0`, `errors=0`, no small body links, grouped count `141,939`, one `Sign in`, no orphan
- Targeted Rust test: `cargo test -p darkhold-app --lib landing` (or its named landing test) must stay green.

## RED/GREEN evidence

- Targeted Rust test: `cargo test -p darkhold-app --lib landing` (or its named landing test) must stay green.
