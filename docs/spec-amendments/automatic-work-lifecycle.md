# Amendment — automatic WIP/abandoned lifecycle and completion review

**Status:** accepted 2026-11-24 (owner decision)
**Ledger:** `M45-76`
**Amends:** §8.8 (activity status), §56.4 (no automatic abandonment), §57.3
(completion badge), and the `completion` / `derived_activity` notes in §6.2.
**Plan:** `docs/plans/automatic-work-lifecycle.md`
**Roadmap:** `docs/ROADMAP-automatic-work-lifecycle.md`

## What was asked

A work that has not been marked complete and has not updated for more than
`X` months should be marked `abandoned` automatically. `X` is configurable and
defaults to four months. If the work updates after that, it returns to
`in_progress` (the UI's "WIP"). A work may be marked complete by its author or
contributors, by users at or above a configurable trust level, or by users the
instance has granted curation privileges.

## What exists today, measured 2026-11-24

The live `darkhold` PostgreSQL database on `thinkcentre`:

| Fact | Measurement |
|---|---|
| works | 58,042 |
| `completion = in_progress` | 42,531 |
| `completion = abandoned` | 15,220 (set by the pawchive import; no code path in this tree writes it) |
| of those, abandoned rows whose source date is newer than the import mark | 0 (no revival is triggered by the migration backfill) |
| `completion = complete` | 291 |
| `derived_activity` | 58,042 `active` — the column has **no production writer** in this tree |
| `library_items` | 90,662 |
| `in_progress` works with a source date | 11,586 |
| of those, stale > 120 days on `library_items.source_updated_at` | 9,722 |
| `in_progress` works with **no** clock at all | 30,570 |
| `accounts.is_curator = true` | 0 |
| `trust_levels` rows | 50 |
| `publication_events` | 58,041, all action `publish`; no `update` event has been written |

Two clocks disagree and only one is the author's:

- `works.updated_at` is the import/metadata clock. It is older than 120 days for
  **0 / 58,042** works.
- `library_items.source_updated_at` is the source's "last updated" date. It is
  older than 120 days for **86,541 / 90,662** library items and for **9,722**
  `in_progress` works.

The completion door is narrow. `PATCH /works/{id}` calls `author_work()` and
`can_edit()`, so only the owner and contributors can change `completion`.
`accounts.is_curator` exists but no completion route reads it. There is no
audit record of who changed completion or why, and `lifecycle_events` is
`UNIQUE (work_id, event_type)` — one completion and one resurrection per work,
ever — so it cannot be the history of a repeated stale/update cycle.

`docs/spec.md` §8.8 and §56.4 currently say the opposite of this amendment:
**"An author-set state always wins"** and **"No automatic abandonment."** The
owner chose automatic abandonment on 2026-11-24, so those two sentences are
superseded here rather than quietly contradicted.

## Decisions

1. **Automatic abandonment is on by default.** `[works].auto_abandon = true`.
   The threshold is `[works].auto_abandon_after_months = 4` (120 days). Setting
   the switch false leaves the review door working and writes no automatic
   state.
2. **Only `complete` is exempt.** The sweep considers `in_progress` and
   `hiatus`. A work the author put on hiatus is swept after the threshold too;
   this is the owner's explicit choice and it amends §57.3's "an author on
   hiatus has not abandoned anything" for the automatic path only. The badge
   says it was automatic, never that the author decided it.
3. **A source-complete work is exempt.** If any linked `library_items.status`
   is `complete`, the sweep sets `completion = 'complete'` (provenance
   `update`) instead of abandoning it. A finished work whose last chapter is old
   is finished, not abandoned.
4. **Revival is by story activity, not by metadata.** An `abandoned` work whose
   last author activity is newer than its completion mark returns to
   `in_progress` with provenance `update`. Editing a title, summary or tag does
   **not** revive a work. A source re-check that reports the same date does not
   revive it either.
5. **Every automatic mark carries provenance.** New columns
   `works.completion_source` (`import | owner | review | sweep | update`) and
   `works.completion_marked_at`, plus a `work_completion_marks` history row for
   every transition. `completion_source = 'sweep'` is what lets the UI say
   "marked automatically after 4 months without an update".
6. **Completion review is a real door.** `trust_level >= N` (default 3) **or**
   `accounts.is_curator = true` may set `complete` or `in_progress` through
   `POST /works/{id}/completion`. They may not set `abandoned` or `hiatus`;
   those remain the author's states. The action records the reviewer, the time
   and an optional note.
7. **`accounts.is_curator` is the grant**, not `curator_roles`. The latter is
   the media-resilience opt-in (§32.7.5, zero live rows) and reusing it for
   editorial completion would make one word mean two unrelated powers.
8. **Nothing about this is a ranking penalty.** `derived_activity` remains
   information. The `abandoned` state is filterable and visible; it changes no
   score, award, credit, badge or trust outcome. A machine-marked abandoned
   work keeps everything it earned.

## The state machine

```text
in_progress --(no story activity for X months)--> abandoned
hiatus      --(no story activity for X months)--> abandoned
abandoned   --(story activity newer than the mark)--> in_progress
abandoned   --(a linked source now says complete)--> complete
complete    ----------------------------------------> (never swept)
```

`auto_abandon = false` removes the three automatic arrows and leaves the manual
doors unchanged.

A work is eligible only when:

- `lifecycle = 'published'` and `deleted_at IS NULL`;
- `completion` is `in_progress` or `hiatus` (for abandonment) or `abandoned`
  (for revival);
- no linked `library_items.status = 'complete'`;
- a last-activity clock exists (see below).

## The clock

`last_author_activity` depends on whether the work has a library copy, because
the same event table means two different things:

- **A work with at least one `library_items` row is imported.** Its clock is
  `library_items.source_updated_at`, across every linked copy. Its
  `publication_events` rows are **import artifacts**: the 58,041 live `publish`
  events carry import dates between 2026-09-20 and 2026-10-27, not the author's
  publication dates. Counting them would make every imported work look weeks
  old and the rule would never fire. If the source date is null, fall back to
  `works.published_at`.
- **A work with no library item is hosted here.** Its clock is
  `publication_events.occurred_at` for `publish`, `update` and
  `restore_revision`, falling back to `works.published_at`.

So the maximum is taken over the clocks that belong to the work's kind, not over
all three indiscriminately. Measured with this rule: 9,722 `in_progress` works
are eligible (stale on the source clock) and 30,570 have no clock at all. The
first version of this amendment counted publication events for imported works
and reported **zero** eligible works — the trap is worth naming.

Explicitly excluded:

- `works.updated_at` — the import/metadata clock that made the old `updated:`
  search filter useless (0 / 58,042 stale);
- `library_items.last_synced_at` — when **our** crawler last looked, not when
  the author wrote;
- `publication_events.publish` for imported works — the import timestamp, not
  the author's;
- `works.derived_activity` and `works.completion_marked_at` — outputs of this
  rule, never inputs.

The three stored shapes are not the same and the parser must accept all of
them, or a work silently reads as "never updated":

- `published_at`: `2026-08-01T00:00:05` (naive, no offset — UTC);
- `source_updated_at`: `2001-08-24T00:00:00Z` (RFC 3339, second precision);
- `occurred_at`: `2026-09-20T08:42:00.771205161Z` (RFC 3339, nanoseconds).

`X` months is `X * 30` days, the same arithmetic the search grammar uses for
`updated:<=-120d`. A work exactly at the boundary is stale.

## Why not reuse the existing pieces

- **`derived_activity`** is a *reading* (`active | slow | dormant | concluded`)
  and has no writer. It is not a completion state, it has fixed 3/12-month
  bands, and §8.8 says a derived value is not a penalty. The owner asked for a
  real state that survives a page load, so this amendment writes
  `works.completion`.
- **`lifecycle_events`** is the incentive log (`completion`, `resurrection`)
  with one row per work and event type. A work can go stale, revive and go
  stale again; that table would refuse the second cycle. `work_completion_marks`
  is the history; `lifecycle_events` stays the one-time reward.
- **`PATCH /works/{id}`** is the author's door (`author_work`, `can_edit`). A
  reviewer who owns nothing would get a 404. Reviewing is a separate capability
  with a separate route and audit row.
- **`curator_roles`** is the media-resilience opt-in, not an editorial grant.

## Configuration

```toml
[works]
# Mark a non-complete work abandoned after this many months without author
# activity. Default 4. 0 is refused; turn auto_abandon off instead.
auto_abandon_after_months = 4

# The automatic state is on by default (owner decision 2026-11-24).
auto_abandon = true

# Trust level that may set complete/in_progress on a work it does not own.
# accounts.is_curator = true grants the same power regardless of level.
completion_review_min_trust = 3
```

`0` is refused at load time with a message naming `auto_abandon = false`,
because "0 months" would abandon every eligible work on the next sweep and is
far more likely a typo than an intention.

## Schema

Migration `0218_work_completion_marks.sql` (SQLite and PostgreSQL arms):

```sql
ALTER TABLE works ADD COLUMN completion_source TEXT NOT NULL DEFAULT 'import'
    CHECK (completion_source IN ('import', 'owner', 'review', 'sweep', 'update'));
ALTER TABLE works ADD COLUMN completion_marked_at TEXT;

CREATE TABLE work_completion_marks (
    id                    TEXT PRIMARY KEY,
    work_id               TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    from_completion       TEXT NOT NULL,
    to_completion         TEXT NOT NULL,
    source                TEXT NOT NULL
                          CHECK (source IN ('import', 'owner', 'review', 'sweep', 'update')),
    actor_account_id      TEXT REFERENCES accounts(id) ON DELETE SET NULL,
    note                  TEXT,
    observed_last_activity TEXT,
    marked_at             TEXT NOT NULL,
    CHECK (from_completion IN ('in_progress', 'complete', 'hiatus', 'abandoned')),
    CHECK (to_completion IN ('in_progress', 'complete', 'hiatus', 'abandoned'))
);

CREATE INDEX work_completion_marks_work_idx
    ON work_completion_marks (work_id, marked_at DESC);
```

On PostgreSQL, `id`, `work_id` and `actor_account_id` are `UUID`; on SQLite
they are `TEXT`, following `0217_user_lists.sql`. Existing rows get
`completion_marked_at = updated_at` where `completion = 'abandoned'`, so the
first sweep can tell whether a source update happened after the import mark.
**A writer that sets `completion` must set `completion_marked_at` in the same
statement**; an `abandoned` row with a NULL mark is left alone (no clock to
compare), not revived on a guess.

## Routes

### `POST /api/v1/works/{id}/completion`

Authenticated. Body:

```json
{ "completion": "complete", "note": "author confirmed in the thread" }
```

- `completion` is `complete` or `in_progress`; any other value is `422`
  naming the two accepted values.
- The caller must satisfy `trust_level >= completion_review_min_trust` or
  `accounts.is_curator = true`. A visible work with insufficient authority is
  `403` and the refusal names the bar and the caller's level. A work the caller
  cannot see is `404`, following ADR 0030.
- A draft (`lifecycle != 'published'`) is `422`.
- Marking the same value twice is `200` with `"changed": false` and writes no
  mark row.
- A successful change writes `completion_source = 'review'`,
  `completion_marked_at = now`, and one `work_completion_marks` row with the
  reviewer's account id and the optional note.
- It does not touch `lifecycle_events`; the one-time completion incentive is
  for the author's act and must not be farmed by a reviewer.

The author's existing `PATCH /works/{id}` keeps working. When it changes
`completion`, it writes `completion_source = 'owner'`, the mark timestamp, and
one history row with the acting pseud's account.

### `POST /api/v1/lifecycle/sweep`

Operator-only (`require_operator`, trust 5). Body:

```json
{ "dry_run": true, "limit": 500, "cursor": null }
```

`dry_run` defaults to **true**; a write requires `"dry_run": false`. The answer:

```json
{
  "scanned": 500,
  "abandoned": 0,
  "revived": 0,
  "concluded": 0,
  "unmeasured": 37,
  "next_cursor": "…",
  "done": false,
  "dry_run": true
}
```

`unmeasured` is a first-class count: a work with no clock is **not** abandoned
and must not be reported as "nothing to do". The operator can page with
`cursor` until `done` is true.

## The sweep

One page at a time, ordered by `works.id`, so a run is resumable and a crash
does not restart from the top:

1. Select published, non-deleted works whose completion is `in_progress`,
   `hiatus` or `abandoned`, plus the three clock candidates and the
   source-complete flag. For a work with a library item the event clock is
   nulled (its `publish` event is an import artifact); for a hosted work the
   source clock is null.
2. Parse the three clocks, take the maximum, and run
   `domain::work_lifecycle::decide`.
3. For `Abandon`: `UPDATE works SET completion = 'abandoned',
   completion_source = 'sweep', completion_marked_at = ? WHERE id = ? AND
   completion IN ('in_progress','hiatus')`. If zero rows, the author changed it
   between the read and the write; the mark row is not written.
4. For `Revive`: the same conditional update from `abandoned` to
   `in_progress`, provenance `update`.
5. For `Conclude`: conditional update to `complete`, provenance `update`.
6. Insert the `work_completion_marks` row in the same transaction as the update.
7. Count `unmeasured` and continue.

No batch is larger than the request's `limit`. The CLI `dh lifecycle sweep
--apply --all` loops until `done`; the systemd timer calls that form once a day
at 04:45, after `darkhold-archive-bodies` at 04:15.

## UI

The badge is still the real `works.completion` value. When
`completion_source = 'sweep'`, the badge carries the qualifier
**"no update in 4 months"** (the configured number, not a hard-coded four).
It never says "the author abandoned it". A reader can filter automatic
abandonment separately from author-declared abandonment once the API exposes
`completion_source`; the filter is off by default, like §8.8's existing
dormant/abandoned filters.

## Non-goals

- **No ownership transfer.** §56.1 stands unchanged.
- **No ranking penalty.** §8.8's "information and never a penalty" stands for
  `derived_activity`; the `abandoned` completion state may be filtered but does
  not change a score.
- **No drafts.** Only `lifecycle = 'published'` works are swept.
- **No reason text on an automatic mark.** The reason is the clock, and the UI
  says exactly that. Inventing "the author gave up" is the thing §57.3 forbids.
- **No per-source thresholds yet.** One `X` for the instance. A per-source
  override is a related idea, not this amendment.
- **No one-notice email in this pass.** §8.8's "one notice" remains the right
  eventual guardrail; this amendment ships the state, the provenance and the
  reversible door first, and records the notice as `M45-76.4`. That is a named
  debt, not a claim that the notice exists.

## Rejected alternatives

- **Derived-only `dormant` badge.** It would not change `works.completion`,
  would not survive a client that reads completion, and could not satisfy
  "marked abandoned" / "becomes WIP again". Rejected by the owner's choice.
- **Opt-in, default off.** Safer against the 9,722-work blast radius, but the
  owner chose on-by-default. The provenance, source-complete guard and
  conditional update are the safety measures instead.
- **Use `works.updated_at`.** It is the import clock: 0 / 58,042 stale, so the
  rule would never fire. It also changes on metadata edits, which would revive
  works the author only retagged.
- **Use `last_synced_at`.** That is when our crawler looked. A work would be
  marked abandoned because we stopped checking, which is our failure, not the
  author's.
- **Reuse `lifecycle_events`.** `UNIQUE (work_id, event_type)` makes a second
  stale/revive cycle impossible, and the table has no actor or note.
- **Reuse `curator_roles`.** It is the media-resilience opt-in, not an
  editorial grant; the live table has zero rows and §32.7.5 defines curators as
  infrastructure curators.
- **A new `wip` completion value.** The UI already labels `in_progress` as
  "In progress"/WIP; a fifth value would fork the vocabulary and break every
  existing `completion:` search and badge.

## Acceptance

The feature is done when all of these are observed, not asserted:

- Domain tests: 4 months exactly is stale; 3 months 29 days is not; an update
  after the mark revives; an update before it does not; a source-complete work
  concludes instead of abandoning; a work with no clock is `unmeasured`.
- Store tests on **both** SQLite and PostgreSQL: the candidate query returns the
  right last activity for a work whose only clock is `source_updated_at`, one
  whose only clock is a publication event, and one whose only clock is
  `published_at`; the conditional update refuses when the author changed
  completion between read and write; the mark row is written in the same
  transaction; `completion_source` and `completion_marked_at` move together.
- Route tests on both engines: TL3 succeeds, TL2 gets 403 naming the bar, an
  `is_curator` account at TL0 succeeds, a stranger gets 404, a draft gets 422,
  a second identical mark returns `changed: false` and writes no second row.
- Sweep tests: `dry_run` writes nothing; `auto_abandon = false` writes nothing;
  a `hiatus` work is swept; a source-complete work is concluded; a metadata
  edit between sweeps does not revive; a new `source_updated_at` does.
- Mutation check: removing the `source_says_complete` guard reddens the
  "concluded" test; removing the `completion IN (...)` condition reddens the
  race test; removing the `last_activity > completion_marked_at` comparison
  reddens the revival test.
- Live dry run against `darkhold` on `thinkcentre` reports 9,722 eligible
  `in_progress` works and 30,570 `unmeasured`; it writes zero rows. A live
  apply is a separate operator action, after the report is read.
