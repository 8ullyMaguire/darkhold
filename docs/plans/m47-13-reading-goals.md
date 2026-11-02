# M47-13: §9.7.9 reading goals — the progress ring (audit item 41)

**Ledger row:** `M47-13`. **Milestone:** M47. **Status:** implemented 2026-11-02.
**Source:** `docs/audit-100-reader-ideas-2026-10-04.md` item 41, "reading-goal progress ring —
§9.6 has the goal; nothing renders it".

## What the probe found

The audit pointed at §9.6, which is reading *history and analytics*; goals are §9.7.9. Neither has
a `reading_goal` table — `grep -rn "reading_goal" crates/ migrations/` returns nothing — so the
ring has nothing to render because the goal has nowhere to be stored. Item 41 is therefore a
feature, not a component, and the plan below is the table first.

What the schema does already hold, which decides the metrics:

| source | gives |
|---|---|
| `reading_progress.chapter_id`, `updated_at` | which chapters were read, and when |
| `chapter_revisions.word_count` | how long each of those chapters is |
| `reading_status.finished_at` | which works were finished, and when |

Three metrics, all computed from data the reader can already delete (§9.6's deletion rule applies:
clear your history and the goal's progress goes with it, because it is derived, never copied).

## What ships

| surface | file |
|---|---|
| table | `migrations/{postgres,sqlite}/0143_reading_goal.sql` |
| rules | `crates/domain/src/reading_goal.rs` (periods, metrics, target bounds, window maths) |
| store | `crates/db/src/reading_goal.rs` |
| routes | `GET`/`PUT`/`DELETE /api/v1/reading-goal`, on `reading`'s authenticated router |
| reader UI | `frontend/src/lib/components/ReadingGoalRing.svelte` on the library page |

## The table

```sql
CREATE TABLE reading_goal (
    pseud_id   TEXT NOT NULL PRIMARY KEY,
    period     TEXT NOT NULL CHECK (period IN ('daily', 'weekly')),
    metric     TEXT NOT NULL CHECK (metric IN ('chapters', 'words', 'works')),
    target     INTEGER NOT NULL CHECK (target > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

**Keyed on the pseud, one row, no `enabled` column.** "Opt-in" is the absence of a row: a reader who
has not set a goal has no goal, and there is no state in which a goal exists but is switched off.
That is §9.7.9's "private, opt-in" with one less flag to disagree with itself.

**`target > 0` is a CHECK, not a Rust validation alone.** A zero-word daily goal is a goal no
reader can meet and no surface can render a ring for; the database refusing it means a migration or
a repair script cannot leave one behind.

## Windows

- `daily` — the reader's UTC day: `[floor(now, day), floor(now, day) + 1 day)`.
- `weekly` — the ISO week (Monday 00:00 UTC).

Computed per read, not stored. A stored period row would need a sweep to roll over, and the sweep
would be wrong for every reader not on UTC anyway; computing it means the answer is always the
current window and the boundaries come back with the payload so a client does not have to guess.

UTC rather than the reader's timezone is a deliberate limit, stated rather than hidden: the schema
has no per-pseud timezone, and inventing one here would put a timezone in the reader's settings that
only this feature reads. The window boundaries are in the response, so a client can label them.

## Progress, by metric

| metric | query | scope |
|---|---|---|
| `chapters` | `COUNT(DISTINCT chapter_id)` from `reading_progress` for the pseud | pseud |
| `words` | `SUM(r.word_count)` over those distinct chapters' current revisions | pseud |
| `works` | `COUNT(*)` from `reading_status` where `status='finished'` and `finished_at` in window | **account** |

`works` is account-scoped because `reading_status` is (§9.6 keeps one shelf per account). The
payload says `scope: "account"` for that metric and `"pseud"` for the others — a per-pseud goal
whose third metric silently read another pseud's shelf would be a privacy bug with a tidy
interface.

## Refusals

- Unknown `period` or `metric` → `422` naming the accepted values (the same shape as the quick
  reactions vocabulary refusal).
- `target` outside `1..=100_000` → `422` on `target`.
- No session → `401`; no active pseud → `422` on `pseud`, because the goal is a pseud's.

## The ring

`ReadingGoalRing.svelte` renders an SVG arc, the count over the target, the metric's own noun, and
the window ("today", "this week"). It renders **nothing** when there is no goal, and the empty state
is a link to set one rather than a prompt. Nothing in it is red, crossed out, or worded as loss.

## Gates

```bash
cargo test -p darkhold-domain --lib reading_goal        # window maths and bounds
cargo test -p darkhold-db  --test reading_goal          # both engines
cargo test -p darkhold-app --test m47_13_reading_goals  # routes
cd frontend && npx vitest run src/lib/components/ReadingGoalRing.test.ts
```
