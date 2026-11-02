## 2026-11-02 — M47-13: §9.7.9 reading goals (audit item 41)

**Requirement:** `M47-13` · **Tests:** 7 domain + 5 db + 4 route (both engines) + 5 frontend

The audit line was "§9.6 has the goal; nothing renders it". Two things were missing, not one: there
was no `reading_goal` table, and **no per-chapter read record either**.

`reading_progress` is unique on `(account_id, pseud_id, subject_type, subject_id)` — one row per
work per device — so it holds the chapter the reader is *on*. Three chapters of one work are one
row. A `chapters` target, which §9.7.9 offers by name, could not be counted from it. So `0143` ships
two tables:

```
reading_goal      (pseud_id PK, period, metric, target)          — opt-in by existing
reading_unit_read (pseud_id, unit_id UNIQUE, last_read_at)       — written by the progress route
```

and `reading::save_progress` writes the chapter record **inside the same transaction** as the
position, because a progress row that landed while its chapter record did not is a goal that
silently under-counts. `reading_a_chapter_through_the_reader_route_moves_the_goal` is the test that
proves the hook: it drives the real `PUT /reading/progress` door and asserts the ring's `current`.

Decisions worth the ink:

- **The window is computed, never stored.** A stored period row needs a sweep to roll it over and
  still resets at the wrong hour for most readers; the boundaries go back in the payload.
- **`works` is account-scoped and says so.** `reading_status` is keyed on the account (§9.6 keeps one
  shelf), so the payload carries `scope: "account"` for that metric and `"pseud"` for the others. A
  per-pseud goal silently reading another pseud's shelf would be a privacy bug with a tidy interface.
- **No verdict, no streak, no red.** No `failed`, `missed`, `streak` or `behind` key exists in the
  payload, and the route test asserts their absence on the serialised JSON. §9.7.1's "never create
  anxiety" is cheaper to keep when there is no word for the alternative.

**Schema names, measured rather than assumed.** The tables are `units` and `revisions` (keyed by
`unit_id`), not `chapters`/`chapter_revisions`: the 0004 migration alone gives the pre-rename names,
and the first version of this store compiled, then failed at runtime with `no such table: chapters`.
`reading_unit_read.unit_id` is TEXT while `units.id` is UUID, so the PostgreSQL arm casts the
*column* and never the join to `revisions`.

**PostgreSQL found three engine-specific faults** that SQLite could not: `target` had to be BIGINT
(ADR 0004 — INT4 will not decode into `i64`), `reading_status.account_id::text = ?` for the TEXT
bind, and the hook's statement needs `$n` placeholders — a `?` on PostgreSQL surfaces as `INTERNAL`
rather than as a bind error, which is why this one had to be found by running the second engine
rather than by reading the code.

```
cargo test -p darkhold-domain --lib reading_goal          -> 7 passed
cargo test -p darkhold-db  --test reading_goal            -> 5 passed, SQLite and PostgreSQL
cargo test -p darkhold-app --test m47_13_reading_goals    -> 4 passed, SQLite and PostgreSQL
cargo test -p darkhold-db  --test reader_surface_t1       -> 22 passed (the hook touched progress)
cargo test -p darkhold-app --test route_inventory         -> 5 passed
cargo test -p darkhold-db  --test migration_catalogue     -> 6 passed
cd frontend && npx vitest run                             -> 602 passed (5 new)
npx svelte-check --threshold error                        -> 0 errors
```

**Red by removal:** accepting `target <= 0` fails the domain test *and* the route's 422 test;
un-flooring `remaining()` fails the "no -3 to go" test; disabling the chapter-record hook fails the
end-to-end goal test with `current: 0`.

## 2026-11-02 — M47-12: §9.4 quick reactions (audit item 43)

**Requirement:** `M47-12` · **Tests:** 6 db + 5 route (both engines) + 6 frontend

The audit's line for item 43 was "reaction rows exist; the label is free text", which reads as a
validation rule on a column that is already there. The probe found the wrong table: `work_reactions`
(0038) is §35.1's **typed vote** — one per reader, retractable, with a costed negative option — keyed
`(work, pseud)`. §9.4's labelled bar had never been built, and it is a different surface: six labels,
no typing, and **several at once per reader**. A reader may vote "insightful" and react "made me
cry" to the same work; a table keyed `(work, pseud)` cannot hold both.

```
table:   quick_reaction (0142), UNIQUE (work_id, pseud_id, label)   ← the "count once" rule
routes:  GET    /api/v1/works/{id}/quick-reactions            public, ships the vocabulary
         PUT    /api/v1/works/{id}/quick-reactions/{label}    pseudonymous, idempotent
         DELETE /api/v1/works/{id}/quick-reactions/{label}    pseudonymous, idempotent
gate:    work_feedback_preferences.comments_enabled → owner's account default → enabled
ui:      QuickReactions.svelte, mounted under ReactionBar on the work page
```

**Why the unique index is the feature.** §9.4 says "count once per reader per target"; making that
the key means every read is a plain `GROUP BY` and no query can forget a `COUNT(DISTINCT …)`.

**Why the refusal names the vocabulary.** An unknown slug is a `422` carrying all six, because
"unknown label" leaves the reader guessing at slugs they cannot see.

**Why `GET` answers `enabled: false` instead of `403`.** The work page stays readable when an author
turns feedback off; only the *write* is refused. A `403` on the read would tell the reader they had
done something wrong.

**PostgreSQL needed three things SQLite did not**, all found by the second engine rather than by
reading: `works.id = ?::uuid` (TEXT bind against a UUID column is "operator does not exist: uuid =
text"), `COALESCE(...)::bigint` (INT4 into `i64`), and dialect-aware boolean literals in the fixtures
(PG rejects `0` for a `BOOLEAN` column).

```
cargo test -p darkhold-db  --test quick_reactions        -> 6 passed, SQLite and PostgreSQL
cargo test -p darkhold-app --test m47_12_quick_reactions -> 5 passed, SQLite and PostgreSQL
cargo test -p darkhold-app --test route_inventory        -> 5 passed (the three new routes are tabled)
cargo test -p darkhold-app --test milestone_31_work_discussion -> 34 passed
cd frontend && npx vitest run                            -> 597 passed (6 new)
npx svelte-check --threshold error                       -> 0 errors
```

**Red by removal:** making `is_known` return `true` fails the domain vocabulary test *and* the
route's 422 test; dropping `{#if view?.enabled}` fails the "renders nothing when feedback is off"
test.

