# docs/plans/100-ideas-remaining.md — plan card

Source: `docs/plans/100-ideas-remaining.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**THE CANONICAL INVENTORY IS `docs/requirements.csv`** — 703 rows, one per requirement, with
**This file supersedes `docs/plans/100-ideas-scope.md` §3** and **supersedes
`docs/plans/REMAINING-2026-10-03.md`** and `docs/plans/WHAT-IS-LEFT.md`, which already
file** — they live in `docs/plans/REMAINING-2026-10-05.md`, which owns that list and
defers every feature question back to this file and to `docs/requirements.csv`. The two
> `grep -rl <term> frontend/src` answers "does this string exist", which is not the same
evidence field I have written since cites **file paths in `frontend/src`**.
`frontend/src crates/app/src crates/db/src`, and every "store only" claim was then
**None of the 27 has a section in `docs/spec.md`** — checked, 0 hits for each id. So every one
wrong. What shipped was the *gate* (`scripts/check-library-visibility.py`) so the property stops
| 1 Continue Reading | `ContinueReadingBanner` mounted in `Home.svelte`; no page route needed, it is an enhancement to the homepage rather than a destination of its own | `7537035`, `f124a67` |
| 9 Why this? | `WhyRecommended` mounted in `Discover.svelte`, reading `slot_id` off every feed item | `1f5ce1a`, `5759616` |

## Copy-pasteable code / commands

GET /api/v1/discovery           -> {"items":[{"title":"Explaining The Feed",
                                    "slot_id":"f1f7a2c1-…"}], …}
GET /api/v1/discovery/slots/f1f7a2c1-…/explanation
                                -> {"reasons":["taste_tags","popular"], …}

## Verification command and expected output

| built by an earlier pass | 2 | items 8, 22 — the probe was wrong, not the code |
signature leaves a route nowhere else to put an account, and the injection-verified test predated
wrong. What shipped was the *gate* (`scripts/check-library-visibility.py`) so the property stops
### Item 8 (completion badges) is built, per the same misreading as last pass
`frontend/src/lib/router.test.ts:18` mentions `/surprise` as a `RouteId`. There is no
feature; this is the "implemented-but-unreachable" failure mode the pass-10 note
homepage, 2 wiring tests in `Home.test.ts`, and `e2e/continue-reading.spec.ts`.
test caught it rather than a reader: **progress is per-DEVICE, and there are two of this
Three more defects the mutation gate found, all of the same family — a green build is
  and scored empty output as a pass. It now refuses to score a run that tested nothing,
  and it asserts that a compile failure is distinguishable from a pass before it is
  so "no request was made" had nothing to assert about, and deleting the whole mount point
  left the test green. Passing `signedIn` as a prop keeps the node mounted and hidden,
  which is a state a test can pin. All four wiring mutations now go red.
**The two-engine run paid for itself twice.** Both mutations PASS on SQLite and FAIL only
test that only exercises another. 22 store tests green on both engines.
**Two of my own test fixtures were wrong before the code was.** 300 words / 250 is 1.2 and
"12k words" is right, not "12.0k". Both were failures in the test, not the formatter, and
each is now the thing the test documents.
at all, and I nearly recorded "the journey does not verify the field" from it.

## RED/GREEN evidence

of six reasons and a private note. All 8 E2E journeys green after the fix.
Three more defects the mutation gate found, all of the same family — a green build is
  SQLite ignored it, so the entire SQLite suite was green throughout. Swept the repo: 0
  left the test green. Passing `signedIn` as a prop keeps the node mounted and hidden,
  which is a state a test can pin. All four wiring mutations now go red.
`wordCount?: number` prop that no caller ever passed.** The component was written, the
All three PostgreSQL arms verified separately, since a mutant in one arm is invisible to a
test that only exercises another. 22 store tests green on both engines.
frontend tests stay GREEN, and the number is simply absent from every page. One E2E journey
goes red. Nothing else does.
`word_count_hidden` and the journey still passed. That was CORRECT — serde reads the struct
And then the sweep's BASELINE came back red with the source restored — which is a harness
