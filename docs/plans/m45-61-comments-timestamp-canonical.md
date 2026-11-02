# M45-61 — `comments.created_at` becomes a shape the database keeps

**Requirement row:** `M45-61` (`docs/requirements.csv`) · **Milestone:** M45 · **Status:** planned →
this plan.
**Spec:** §11.4 (canonical timestamps), §13 (community), §51 (erasure), and the precedent set by
`M63-31` / migration `0132` for `library_items`.

## The defect, stated as the failure it is

The welcome rota asks "did this reader comment inside the last 72 hours?" and the query is a text
comparison:

```sql
… AND created_at >= ?          -- crates/db/src/welcome.rs:426
```

Both sides are TEXT. PostgreSQL sorts a string, it does not parse a time, so a row written as
`2026-11-02 02:13:48+00` (a space, an offset) is **never** `>=` a bound written `2026-11-02T02:13:48Z`
(an `T`, a `Z`). The comparison matches nothing and returns no error:

> the reader commented nine minutes ago, the rota says they have not commented at all.

That is a refusal with no error, which is the worst shape a bug takes here — nothing logs, nothing
fails, and the reader is silently locked out of a welcome.

Live rows are canonical because `community::insert_comment` writes `now_rfc3339`. **The gap is that
nothing stops the next writer.** A test fixture already proved it: it wrote `$4::timestamptz`,
PostgreSQL stored `2026-11-02 02:13:48+00`, and the window matched nothing.

`M63-31` put exactly this constraint on `library_items` (`migrations/*/0132`). This applies the same
rule to the table whose mis-shape is silent rather than slow.

## The rule

**Canonical = `YYYY-MM-DDTHH:MM:SSZ`**, UTC, `T` separator, `Z` suffix, no fractional part, no
offset. That is `now_rfc3339`'s shape and it is what `0132` enforces. `comments.created_at` is
`NOT NULL`, so there is no NULL arm to allow.

## Steps

### 1. `migrations/postgres/0138_comments_timestamp_canonical.sql`

Backfill, then `CHECK (created_at ~ '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$')` added `NOT VALID`
and then `VALIDATE`d in the same migration — the pattern `0132` uses, so a failure to validate fails
the migration instead of leaving an unvalidated check behind.

The backfill follows `0132`'s CASE: a bare date becomes midnight, an offset-bearing value is cast to
`timestamptz` and formatted, a zone-less value is read as UTC.

### 2. `migrations/sqlite/0138_comments_timestamp_canonical.sql`

SQLite cannot add a `CHECK` to an existing table without rebuilding it, so the same rule is a pair of
triggers (`BEFORE INSERT`, `BEFORE UPDATE OF created_at`) using `GLOB`, plus the backfill through
`strftime('%Y-%m-%dT%H:%M:%SZ', …)`. Statement-for-statement with `0132`'s SQLite twin.

### 3. `crates/app/tests/comments_timestamp_canonical.rs`

Four tests, each naming the failure it exists to catch:

| test | what it proves |
|---|---|
| `a_non_canonical_created_at_is_refused_by_the_database` | the shape is enforced by the database, not by a convention — raw `INSERT` of `'2026-11-02 02:13:48+00'` errors |
| `the_canonical_writer_is_accepted` | the real writer's shape (`now_rfc3339`) still inserts, so the constraint is not a wall the app trips over |
| `the_window_sees_a_recent_canonical_comment` | the 72-hour comparison matches a comment written nine minutes ago — the bug, on the good path |
| `a_non_canonical_row_would_have_been_invisible_to_the_window` | the same bound against a hand-written non-canonical row: the constraint refuses it *before* the silent non-match can happen |

Both engines: the suite runs on SQLite and, when `LOREHAVEN_TEST_PG_URL` is set, on PostgreSQL —
because the CHECK and the triggers are different mechanisms reaching the same rule, and only
running both proves that.

### 4. Ledger, docs, verification

* `docs/requirements.csv` `M45-61` → `implemented-fully-tested`, with the evidence and the note that
  the constraint is per-table and the next table with a clock-window query needs its own line.
* `docs/verification.md` entry.
* `docs/known-gaps.md`: `comments.edited_at` and `deleted_at` are nullable and are **not** covered by
  this migration — the rota reads `created_at` only. Say so rather than implying the whole table.

## Verification

```bash
cargo test -p darkhold-app --test comments_timestamp_canonical        # SQLite: 4 passed
LOREHAVEN_TEST_PG_URL=… cargo test -p darkhold-app --test comments_timestamp_canonical   # PostgreSQL: 4 passed
python3 scripts/backlog.py --check                                     # ledger current
```

Expected on the unfixed tree: test 1 fails (the insert succeeds), test 4 fails (the row is accepted,
the window matches nothing). Expected after: all pass. **Both proven by reverting the migration file
and re-running** — a test that passes on unfixed code is not evidence.
