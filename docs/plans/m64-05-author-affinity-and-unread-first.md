# M64-05 — the "same author" rung: unread first, and the affinity number that has no consumer yet

**Requirement.** `story.more_by_author` and `author.works` are served from the author's own rows,
ordered by that surface's field (`docs/requirements.csv` `M64-05`, priority P5).

**What the study changes.** §12.7a's last paragraph records the one measured win that needs no model
and no new table: turning implicit feedback into *graded* feedback — a reader's affinity for an author
is `works of that author read ÷ works by that author`, scaled 1–5 — lifted the reference model's
target from `0.0039` to `0.1358` f1@20. §11.5 rung 2 was then written to order by that affinity, and
the implementation ordered by `published_at DESC`. This plan closes the part of that gap that has a
consumer today, and records why the rest does not.

## What is actually possible now

Rung 2 is *one author's* other works. Within one author the affinity ratio is **constant** — the
numerator and denominator share their author — so affinity cannot order rung 2's rows. What it can
order is the choice *between* authors, and no surface in this build compares two authors' works:
rung 3+ (theme neighbours, co-bookmark, taste-matched) are named-unavailable refusals (§11.5) and
`author.works` is a single author's page.

So this plan does two things and refuses a third:

| piece | status |
|---|---|
| `same_author_works` orders **unread first**, then `published_at DESC`, then id | **built here** — the half of §11.5 rung 2 that has a definition and a consumer |
| `author_affinity_for(db, account)` — the graded ratio, 1–5 | **written down, not built**: an unconsumed helper is dead code, and a "feature" whose only test calls it directly is inert by construction |
| reordering the byline block by taste | **refused**: §12.7a and M64-05 both say the byline is a promise, `g = 0` |

## Definitions this plan fixes

- **"Read"** is a `reading_status` row for that work whose `status` is `reading` or `finished`.
  `want-to-read` is *intent*, not evidence, and counting it would let a reader inflate an author's
  affinity without opening anything. `dropped` is evidence *against* and is deliberately not counted
  as read — the study had no negative signal, and this build should not invent one inside a
  denominator.
- **The denominator** is the author's *published, visible, undeleted* works — the same set rung 2
  serves. An unpublished draft must not dilute a reader's affinity for an author.
- **The number is never displayed** (§0.3, §12.7a). It is an ordering key. A surface that prints it
  turns a reader's behaviour into a public score about an author.

## Step 1 — `crates/db/src/series_recs.rs`: viewer-aware ordering

Today:

```rust
pub async fn same_author_works(db: &Database, work_id: &str) -> Result<Vec<RelatedWork>>
```

and the order is `w.published_at DESC, w.id`.

Change the signature to take the viewer:

```rust
pub async fn same_author_works(
    db: &Database,
    work_id: &str,
    viewer: Option<&str>,        // the account id, when a session exists
) -> Result<Vec<RelatedWork>>
```

and the ORDER BY, in **both** dialect bodies:

```sql
ORDER BY CASE WHEN EXISTS (SELECT 1 FROM reading_status rs
                            WHERE rs.account_id = ?
                              AND rs.subject_type = 'work'
                              AND rs.subject_id = w.id
                              AND rs.status IN ('reading','finished')) THEN 1
              ELSE 0 END,
         w.published_at DESC, w.id
```

The `CASE` puts **unread (`0`) first** and read (`1`) last. **There is deliberately no
`viewer IS NULL` branch:** a NULL account compares false against every `reading_status` row, so an
anonymous viewer gets `0` for every work, and the stable sort leaves `published_at DESC` in charge —
exactly what an anonymous visitor saw before. The first version of this plan *did* carry an
`IS NULL` branch, and it was removed after a mutation exposed that no test could observe it: a branch
short-circuited by SQL's own NULL semantics is a branch that cannot be verified, and a guard nobody
can test is a guard nobody should trust.

One bind per arm, and it must be added to the *existing* bind sequence **after** the two `work_id`
binds, because positional binding is unchecked: `work_id` appears in the `WHERE`, the viewer in the
`ORDER BY`, and the text is read in that order.

**Both status subjects count.** `reading_status.subject_type` is either `'work'` (set from the work
page) or `'library_item'` (set from the reader's library) and both routes write the same table, so
the predicate has to accept both — the second arm resolves the item back to its work:

```sql
AND ((rs.subject_type = 'work' AND rs.subject_id = w.id)
  OR (rs.subject_type = 'library_item'
      AND rs.subject_id IN (SELECT li.id FROM library_items li
                             WHERE li.account_id = rs.account_id AND li.work_id = w.id)))
```

A query that read only `'work'` would demote some readers and not others, and the symptom would be a
recommendation that looks slightly wrong rather than a failure — which is why the fourth test marks a
work through the library-item subject and the mutation that drops this arm fails only that test.

PostgreSQL needs `rs.subject_id = w.id` to be a `uuid` comparison on both sides or an explicit cast
(`reading_status.subject_id` is `uuid` in that dialect; `w.id` is too).

**Verification.**

```bash
cargo test -p darkhold-db --test same_author_unread_first
# 3 passed
```

Three tests, in `crates/db/tests/same_author_unread_first.rs`, on SQLite with `DARKHOLD_TEST_PG_URL`
making each run on PostgreSQL too. The fixture is **three** works by one author — the seed, an older
and a newer — because the rung excludes its own subject, and a two-work fixture would leave exactly
one candidate whose order no assertion could distinguish.

1. `an_unread_work_comes_before_a_read_one_even_when_older` — the reader **finished the newer work**,
   so recency and unread-first disagree: `[older, newer]`. This is the assertion the old code fails.
2. `an_anonymous_viewer_keeps_published_at_order` — the same marking, `viewer: None`: `[newer,
   older]`. The only difference from test 1 is the viewer, so together they prove the viewer is what
   changes the answer.
3. `want_to_read_is_not_read` — marking the newer work `want-to-read` must not demote it: `[newer,
   older]`.

**Mutation, run:** with `ELSE 0` → `ELSE 1` (every row demoted) test 1 fails and the other two pass;
restored, 3 pass. The opposite (`THEN 1` → `THEN 0`) also fails test 1. A mutation that leaves all
three green is not a mutation, and the first draft of this plan's `IS NULL` branch was exactly that.

## Step 2 — `crates/app/src/routes/works.rs`: pass the viewer

```rust
let viewer = session.as_ref().map(|s| s.account_id.to_string());
...
for related in darkhold_db::series_recs::same_author_works(state.db(), &work_id.to_string(), viewer.as_deref())
```

The handler currently takes `MaybeSession(_session)`; it becomes `MaybeSession(session)`. **The reason
string does not change** — `more by {author}` — because the affinity that ordered the rows is not
something the row says.

**Verification.**

```bash
cargo test -p darkhold-app --test more_like_this
# the same count as before the change, all green
```

plus one new test in that file: `a_signed_in_reader_is_offered_the_unread_work_first`, which finishes
the newer work through `PUT /api/v1/works/{id}/reading-status` and asserts the ladder serves
`[older, newer]` to that reader while a visitor — no session, no history — gets the recency order.
`published_at` is pinned with an `UPDATE` first: works published inside the same second give recency
no order to differ from, and the anonymous arm then decides nothing. The first run of this test
passed alone and failed in the file — a fixture whose premise is a timestamp it did not control.

## Step 3 — docs

- `docs/spec/11-search-and-discovery.md` §11.5 "What the implementation settled": record that the
  unread-first half is built and that the affinity half waits on a surface that compares authors.
  (Edit the master at `~/secondbrain/10-Projects/ultimate-fanfiction-site-spec.md`, then
  `python3 scripts/split-spec.py && python3 scripts/check-split-spec.py`.)
- `docs/requirements.csv` `M64-05`: `implemented-locally-tested` only if step 2 lands; otherwise
  `planned` with the notes pointing here.
- `docs/verification.md`: the fixture, the three tests, the mutation, and what the change does not
  measure.

## What this plan refuses

- **Building `author_affinity_for` now.** No surface asks two authors to compete, so the helper would
  be called only by its own test. Write it when rung 4 or an "authors you read" surface exists; the
  definition above is the part that must not be re-invented then.
- **Counting `want-to-read` as read.** Convenient and wrong.
- **Showing the ratio.** An ordering key is not a fact about an author for public display.
- **Ordering by recency first.** That is the defect §12.7a rule 1 convicted in
  `personalized_recommendations`, fixed on 2026-11-02; this rung must not reintroduce it in miniature.

## Build order

| # | step | verification | expected |
|---|---|---|---|
| 1 | viewer-aware ORDER BY in `same_author_works` | `cargo test -p darkhold-db --test same_author_unread_first` | `4 passed` |
| 2 | route passes the session's account | `cargo test -p darkhold-app --test more_like_this` | `4 passed` |
| 3 | spec §11.5 as-built + requirements + verification entry | `python3 scripts/check-split-spec.py` | `41 files, slots 00-40` |
