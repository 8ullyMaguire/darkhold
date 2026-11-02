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

