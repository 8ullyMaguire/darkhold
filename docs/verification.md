## 2026-11-02 — M47-09/10/11: thirteen red binaries, three root causes

A `cargo test -p darkhold-app -p darkhold-db --no-fail-fast` run reported **13 failing test
binaries**, all of them red before this pass. They were three causes, not thirteen.

**One: the module gate landed under suites written against the routers.** 46 tests across
`concierge_routes`, `milestone_26` (narration), `tasting_menu`, `taste_health` (quiz) and
`vanguard` read the `404` of an un-opted-in module as a missing router. Each suite now enables its
own module in the harness config — the way it already sets `storage.root` — because whether a fresh
instance ships a surface on is a product default, not something a test should settle by needing to
pass.

**Two: ordering by a one-second timestamp with a random UUID as tiebreak.** `trust_events`,
`adapter_reviews` and the `jobs` listing all do `ORDER BY created_at DESC, id DESC`. Two rows
written in the same second — which is what a scripted grant or a fast fixture produces — have no
defined order, and `id` is not a monotone tiebreak. Migrations `0140`/`0141` give the first two a
`seq` (a sequence on PostgreSQL, `MAX + 1` under SQLite's write lock); their reads order by it. The
append-only trigger on `trust_events` had to be dropped and recreated around the backfill, because
the backfill *is* an UPDATE and the trigger exists to refuse those. `jobs` keeps its second-
resolution ordering because its cursor is a public value; recorded as `M47-G01`.

**Three: fixtures and expectations that contradicted the schema.** `milestone_31`'s fixtures wrote
`+00:00` stamps that migration `0132`'s canonical trigger rejects (`created_at must be canonical
UTC (…Z)`), and `milestone_5`'s sweep test asked a strict `updated_at < cutoff` to delete a job
that finished at the cutoff — right behaviour, wrong assertion.

Also closed on the way: the pre-existing red `route_inventory` (34 routes never tabled, three with
no audience extractor) and `docs/known-gaps.md` **M10-D01** (`retire_encryption_key` re-stamped a
retired key; now `AND retired_at IS NULL`).

```
cargo test -p darkhold-app --test route_inventory          -> 5 passed  (was 3)
cargo test -p darkhold-app --test concierge_routes         -> 11 passed (was 0)
cargo test -p darkhold-app --test milestone_26             -> 11 passed (was 2)
cargo test -p darkhold-app --test tasting_menu             -> 14 passed (was 0)
cargo test -p darkhold-app --test taste_health             -> 5 passed  (was 3)
cargo test -p darkhold-app --test vanguard                 -> 5 passed  (was 0)
cargo test -p darkhold-app --test milestone_31_work_discussion -> 34 passed (was 27)
cargo test -p darkhold-app --test milestone_5              -> 25 passed (was 23)
cargo test -p darkhold-app --test trust_events             -> 4 passed, SQLite and PostgreSQL
cargo test -p darkhold-db  --test source_adapters          -> 11 passed, SQLite and PostgreSQL
cargo test -p darkhold-db  --test migration_catalogue      -> 6 passed
```

**Red by removal:** `ORDER BY seq ASC` on `trust_events` fails `every_trust_change_writes_a_row`
with the same `left: 3, right: 5` the suite was already failing; dropping `seq` from
`reviews_for` fails `reviews_are_returned_oldest_first`.

**Not still red, and one thing that is:** the suite is green apart from `M47-G01` (the `jobs`
cursor), which is written down rather than papered over.

