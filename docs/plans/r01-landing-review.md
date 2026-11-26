# R-01 — landing-page review: plan and acceptance

## Problem
The live `/` at 1440×1000 and 390×844 was audited with Playwright (`r01_landing_audit.py`). Both widths had one `h1`,
zero page errors and no horizontal overflow, but the DOM and screenshots showed four concrete defects:
1. `141939 works catalogued from 11 sources` is an ungrouped digit blob.
2. Story metadata is one undifferentiated monospace run; at phone width `general` wraps alone onto its own line.
3. `Sign in · Help` is stranded between the rule and the estate footer and duplicates the estate's Sign in pill.
4. Body links (`Browse the catalogue`, `Open the editor`, `Browse the forum`) are 22px tall; the Search button is an
   unaccented outline.

## Decision
Fix only server-rendered Darkhold content and its inline style in `crates/app/src/routes/landing_page.rs`. The estate
chrome's white `Polaris` wordmark and unlabeled icon buttons belong to the injected bar, not this document; they are
recorded as SUGGESTION for the estate owner rather than patched through a cross-repo CSS override. "Open now" becomes
"Start reading" because it says what the list is for. Grouped digits use a local formatter; no new dependency.

## Changes
- Add `fn grouped(n: i64) -> String` beside the constants; use it for the catalogue count and each `words` value.
- Rename the shelf heading to `Start reading`.
- Split metadata into a `meta-main` span (`by X · N words`) and a `meta-state` span (`completion · rating`), joined by
  ` · ` in CSS. The state group is nowrap so the rating can never orphan alone.
- Change `p` link styling to a 44px inline-flex target.
- Fill the Search button with `--color-accent` / `--color-accent-contrast`.
- Remove the landing `<footer>` (duplicate Sign in); add a `Read the docs` link to the closing section so Help stays
  reachable in the document.

## Verification (before → after)
- Before: `small=7` desktop / `small=3` phone; `141939` present; `general` orphaned on phone; `Sign in` twice.
- After command: `python3 r01_landing_audit.py` against the deployed site at both widths.
- After expected: `overflow=0`, `errors=0`, no small body links, grouped count `141,939`, one `Sign in`, no orphan
  `general`, and the same seeded titles.
- Targeted Rust test: `cargo test -p darkhold-app --lib landing` (or its named landing test) must stay green.
- The row becomes Done in `docs/reviews/page-review-queue.md` in the same commit whose message names R-01 and whose
  body carries the before/after line.
