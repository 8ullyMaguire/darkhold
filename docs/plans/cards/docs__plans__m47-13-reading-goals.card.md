# docs/plans/m47-13-reading-goals.md — plan card

Source: `docs/plans/m47-13-reading-goals.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Source:** `docs/audit-100-reader-ideas-2026-10-04.md` item 41, "reading-goal progress ring —
a `reading_goal` table — `grep -rn "reading_goal" crates/ migrations/` returns nothing — so the
| table | `migrations/{postgres,sqlite}/0143_reading_goal.sql` |
| rules | `crates/domain/src/reading_goal.rs` (periods, metrics, target bounds, window maths) |
| store | `crates/db/src/reading_goal.rs` |
| reader UI | `frontend/src/lib/components/ReadingGoalRing.svelte` on the library page |
`ReadingGoalRing.svelte` renders an SVG arc, the count over the target, the metric's own noun, and
cd frontend && npx vitest run src/lib/components/ReadingGoalRing.test.ts

## Copy-pasteable code / commands

CREATE TABLE reading_goal (
    pseud_id   TEXT NOT NULL PRIMARY KEY,
    period     TEXT NOT NULL CHECK (period IN ('daily', 'weekly')),
    metric     TEXT NOT NULL CHECK (metric IN ('chapters', 'words', 'works')),
    target     INTEGER NOT NULL CHECK (target > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

cargo test -p darkhold-domain --lib reading_goal        # window maths and bounds
cargo test -p darkhold-db  --test reading_goal          # both engines
cargo test -p darkhold-app --test m47_13_reading_goals  # routes
cd frontend && npx vitest run src/lib/components/ReadingGoalRing.test.ts

## Verification command and expected output

cargo test -p darkhold-domain --lib reading_goal        # window maths and bounds
cargo test -p darkhold-db  --test reading_goal          # both engines
cargo test -p darkhold-app --test m47_13_reading_goals  # routes
cd frontend && npx vitest run src/lib/components/ReadingGoalRing.test.ts

## RED/GREEN evidence

is a link to set one rather than a prompt. Nothing in it is red, crossed out, or worded as loss.
