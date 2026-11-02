## 2026-11-02 — M45-58: erasure reaches the supply routing record (§58.3 × §51)

**Requirement:** `M45-58` · **Tests:** 3 store (both engines) + the erasure-coverage gate

Two clauses that pull opposite ways: §58.3 says a routing row is **never deleted** (an abandoned
routing is the evidence the reader was asked), and §51 says erasure reaches every column that
identifies a person. The compatible answer is the third one — the row stays, the link becomes
**`erased:<row id>`**.

**No migration.** The first attempt rebuilt `supply_routing.account_id` to be nullable (0146, SQLite
table rebuild + `DROP NOT NULL` on PostgreSQL). It was withdrawn: SQLite's rebuild leaves the helper
table's name in the migration text, which `erasure_covers_fk_free_accounts` reads as a brand-new
unclassified account column, and NULL is the weaker answer anyway. `Treatment::Rekey` updates a `NOT
NULL` column in place, so the schema did not have to move.

**Why per row and not a shared placeholder.** `'erased'` would be unique-per-nothing: every severed
routing of one person would share it, and a pseudonym that links rows is still personal data. The
mutation test proves the difference — switching to a shared placeholder turns all three tests red,
the collision one first.

**The gate found a real leak while this was being fixed.** `badge_award.account_id` — the table
M47-14 added three commits ago — was unclassified, so erasing an account left its badges behind. It
is now `Delete` (a badge is personal recognition, not an audit record), and removing that entry again
turns the gate red with the column named.

```
cargo test -p darkhold-db  --test supply_routing_erasure         -> 3 passed, SQLite and PostgreSQL
cargo test -p darkhold-db  --test erasure_covers_fk_free_accounts -> 4 passed
cargo test -p darkhold-app --test erasure                         -> 10 passed
cargo test -p darkhold-db  --test migration_catalogue             -> 6 passed
```

**Red by removal:** a shared `'erased'` placeholder fails all three; removing the `badge_award` entry
fails the coverage gate naming the column.

## 2026-11-02 — M45-39b: the account archive (§13, M45-39 phase 2)

**Requirement:** `M45-39b` · **Tests:** 3 store + 3 route (both engines)

Measured before building, as M45-53 taught: a per-work exporter already exists (EPUB, PDF, AZW3, as a
queued job). This is the other artefact — the **recovery copy**. One document per account: profile,
pseuds, works, shelf, reading state, ratings, reviews, credit balances, each section with its own
SHA-256 and row count.

Four decisions:

- **Not a job.** A rendered novel is expensive; this is a handful of indexed reads. A reader on a
  dying instance should not be queued behind a worker that may never run.
- **No work bodies.** The words travel through the exporter, where the licence, the CTA and the
  sanitising live. A second path to the same bytes would be a second, unaudited path.
- **Erased rows excluded.** An archive of erased data would be an erasure that did not happen, and
  the test soft-deletes a pseud, a rating and a review to check exactly that.
- **The checksums are real.** The tests recompute each section's SHA-256 from the returned rows
  instead of comparing the field to itself.

**PostgreSQL found five separate faults**, all of them the ADR 0004 family and none of them visible on
SQLite: `id`/`account_id`/`subject_id`/`work_id` are UUID there and need `::text` on both the cast and
the predicate; `is_public` and `contains_spoilers` are BOOLEAN there and INTEGER here, so the `::int`
cast differs by engine rather than by width; and `stars` is BIGINT there while SQLite's INTEGER decodes
as `i64` anyway.

```
cargo test -p darkhold-db  --test archive          -> 3 passed, SQLite and PostgreSQL
cargo test -p darkhold-app --test m45_39_archive   -> 3 passed, both engines
cargo test -p darkhold-app --test route_inventory  -> 5 passed
```

**Red by removal:** a constant section hash fails the checksum test; dropping the account predicate
from `bookmarks` fails both the store and the route isolation tests; ignoring `deleted_at` on works
fails the erasure test.

## 2026-11-02 — M45-39a: the succession document and the redirect registry (§11.11a)

**Requirement:** `M45-39a` · **Tests:** 8 domain + 6 route (both engines)

`M45-39` sat at the front of the queue for a month because it was filed as "needs external
dependencies", which is true of one of its three parts: ActivityPub `Move`. The document and the
registry need a table and two routes. They ship as `M45-39a`; the row keeps the halves that really do
need a peer protocol.

The refusal this section exists for is **an export URL is mandatory**: a destination without one is a
forwarding address where the destination holds the only copy, and a reader who follows it is trusting
it with their archive.

Two properties worth naming:

- **Staleness is computed at read time** (`confirmed_at + review_days`), because a stored `is_stale`
  flag needs something to flip it and the case it exists for is the case where nothing is running. An
  unreadable `confirmed_at` reads as **stale**, not as fresh — a parse failure is not evidence of
  liveness.
- **A stale plan is served, marked** rather than withheld. `stale: true` plus `confirmed_at`
  distinguishes "had a plan, stopped confirming" from "never had one", and the first is the fact
  worth knowing.

Resolution is by **longest matching path prefix**, applied in Rust rather than SQL because the two
engines sort prefixes differently and a tie in the database would be decided by row order — which is
not a rule. A prefix matches at a segment boundary, so `/works` does not claim `/workshop`.

**PostgreSQL rejected the first schema**, and for the reason ADR 0004 already names: `review_days`
must be `BIGINT` because the value travels as `i64`, and PostgreSQL will not store an INT8 bind in an
INTEGER column. SQLite was green throughout; the route returned 500 on the second engine only.

```
cargo test -p darkhold-domain --lib succession          -> 11 passed (8 new)
cargo test -p darkhold-app --test m45_39_succession     -> 6 passed, SQLite and PostgreSQL
cargo test -p darkhold-app --test route_inventory       -> 5 passed
cargo test -p darkhold-db  --test migration_catalogue   -> 6 passed
cargo test -p darkhold-domain --lib modules             -> 16 passed
```

**Red by removal:** shortest-prefix resolution fails the registry test; widening the review window by
100,000 days fails the staleness test. **Green when removed, and recorded:** the empty-`export_url`
branch, because `!is_https("")` already refuses it — that branch buys a better message, not the
guarantee.

## 2026-11-02 — M47-14: First Words, the debut badge (audit item 75)

**Requirement:** `M47-14` · **Tests:** 3 domain + 3 route (both engines) + 6 frontend

No badges table existed. §9.7.6's fourteen milestone rows were specification only, so the audit's
"one boolean + a badge" was a feature and the boolean was the part to reject: a stored
`is_new_author` flag goes stale the instant a second work is published.

```
table:   badge_award (0144), UNIQUE (account_id, badge)
rule:    First Words — publish your first work — 20 credits (§9.7.6's own row)
trigger: content::publish_work → award_debut_badge, AFTER the commit
surface: PublicPseudView.badges / .debut → PseudProfile.svelte
```

**Why after the commit.** `publish_work` is one transaction covering the state change, the
publication event and two outbox rows, and the author is told it succeeded when that commits. The
bonus is a second ledger write with its own idempotency key; inside the transaction, a ledger failure
would roll back a publication already reported as done.

**One-time-ness has three layers**, and it matters which one carries it: `UNIQUE (account_id, badge)`,
the early return when the insert conflicted, and `post_transaction`'s key
`badge:<slug>:<account>`. Mutating the publish hook's `if count != 1` guard away leaves every test
green — so that guard is a fast path (it skips two reads per later publication), not the invariant.
Recorded in `docs/plans/m47-14-debut-badge.md` so the next reader does not delete it expecting a
failure and conclude the tests are asleep.

**PostgreSQL, again, found what SQLite could not:** `owner_pseud_id::text = ?` (UUID vs TEXT bind),
and `account_id::text` on the *select* as well (PostgreSQL refuses to decode a UUID column into a
`String`, which surfaced as a 500 on the profile route rather than as a type error).

```
cargo test -p darkhold-domain --lib badges              -> 3 passed
cargo test -p darkhold-app --test m47_14_debut_badge    -> 3 passed, SQLite and PostgreSQL
cargo test -p darkhold-app --test route_inventory       -> 5 passed
cargo test -p darkhold-db  --test migration_catalogue   -> 6 passed
cd frontend && npx vitest run src/routes/PseudProfile.test.ts -> 6 passed
```

**Red by removal:** skipping the bonus (`if false`) fails the credit assertion; widening
`published == 1` to `>= 1` fails the second-publication test.

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

