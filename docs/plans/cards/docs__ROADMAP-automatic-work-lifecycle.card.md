# docs/ROADMAP-automatic-work-lifecycle.md — plan card

Source: `docs/ROADMAP-automatic-work-lifecycle.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

<!-- uniform-docs: current state -> docs/STATUS.md; repo-specific detail below is retained unchanged -->
> Current state is the single pointer in `docs/STATUS.md`. The list below is retained as repo-specific detail, not as a second status authority.
**Spec:** `docs/spec-amendments/automatic-work-lifecycle.md`
**Plan:** `docs/plans/automatic-work-lifecycle.md`
  `active` (`grep -rn 'derived_activity' crates --include='*.rs'` finds no

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

Definition of done: the frontend build passes and the WorkCard test asserts
| `published_at` has no offset and is read as "never updated" | The domain parser accepts naive `YYYY-MM-DDTHH:MM:SS` as UTC; a unit test pins it |

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
