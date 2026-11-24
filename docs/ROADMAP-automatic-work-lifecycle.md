# ROADMAP — automatic WIP/abandoned lifecycle (`M45-76`)

**Status:** planned, not implemented
**Spec:** `docs/spec-amendments/automatic-work-lifecycle.md`
**Plan:** `docs/plans/automatic-work-lifecycle.md`
**Amends:** §8.8, §56.4, §57.3

## Why this exists

The archive can already store `completion = abandoned`, and it already knows the
author's last update date on imported works. It has no process that turns one
into the other. Today:

- `derived_activity` is declared and never written — all 58,042 live works are
  `active` (`grep -rn 'derived_activity' crates --include='*.rs'` finds no
  production UPDATE).
- 9,722 `in_progress` works are older than 120 days on
  `library_items.source_updated_at`; 30,570 `in_progress` works have no source
  date at all and cannot be judged yet.
- `completion` is owner/contributor-only. `accounts.is_curator` exists but no
  completion route reads it; `trust_levels` has 50 rows and
  `accounts.is_curator = true` has zero.
- `lifecycle_events` is `UNIQUE (work_id, event_type)`, so it cannot record a
  second stale/revive cycle.

The owner's decision on 2026-11-24: automatic abandonment is **on by default**
at **4 months**, `hiatus` is included, and completion review is granted by
`trust_level >= 3` **or** `accounts.is_curator`.

## Phases

### Phase 0 — the blocking decision (done)

The product decision is recorded in the spec amendment and accepted by the
owner. No code before this: the rest of the roadmap is downstream of "on by
default" and "hiatus is swept".

### Phase 1 — state and provenance (the minimum honest feature)

Deliverables:

- migration `0218` with `completion_source`, `completion_marked_at`, and
  `work_completion_marks`;
- `domain::work_lifecycle` with the pure decision and the three-shape timestamp
  parser;
- store `candidates`, `apply_transition`, `may_review_completion`;
- the daily sweep, operator route and CLI.

Definition of done: a stale `in_progress` work with a source date becomes
`abandoned` with `completion_source='sweep'`; a new source date revives it; a
work with no clock is reported `unmeasured` and left alone.

### Phase 2 — the review door

Deliverables:

- `POST /api/v1/works/{id}/completion` for TL3 / `is_curator`;
- owner provenance in `PATCH /works/{id}`;
- tests for TL3, TL2, `is_curator`, draft, idempotence, and the audit row.

Definition of done: a TL3 account that owns nothing can mark a work complete;
a TL2 account gets a 403 that names the bar; the author's own edit writes
`completion_source='owner'`.

### Phase 3 — scheduling and rollout

Deliverables:

- `deploy/darkhold-lifecycle.service` and `.timer` at 04:45, after
  `darkhold-archive-bodies`;
- dry-run first; the timer starts **disabled**;
- one bounded `--apply` batch read by the operator before the timer is enabled.

Definition of done: the live dry run reports 9,722 eligible and 30,570
unmeasured with zero writes; a single 500-work apply batch reports
`abandoned + revived + concluded + raced + unmeasured = scanned`.

### Phase 4 — the badge tells the truth

Deliverables:

- API exposes `completion_source` and `completion_marked_at`;
- `WorkCard` and `WorkPage` show `Abandoned · no update in 4 months` when the
  mark is automatic, plain `Abandoned` when a human set it.

Definition of done: the frontend build passes and the WorkCard test asserts
both labels.

## Dependencies

- `M4-01` — the `works.derived_activity` column and `domain::content`'s bands.
  This feature does not write `derived_activity`; it writes `completion`.
- `M45-28` — `lifecycle_events` and the adoption offer. The auto-abandoned
  works become eligible for the existing adoption path without a new table.
- `M45-75` — the search clock. The two features agree that
  `library_items.source_updated_at` is the author clock; neither depends on the
  other shipping first.
- `M45-76` is blocked on no other feature. It is blocked on the operator's live
  dry run before the timer is enabled.

## Risks and the answer to each

| Risk | Answer |
|---|---|
| First sweep marks 9,722 works abandoned at once | Dry run first; bounded `--apply`; the timer stays disabled until the report is read |
| A finished work with an old last chapter is labeled abandoned | `library_items.status = 'complete'` guard; `completion = 'complete'` guard |
| A metadata edit revives a work the author abandoned | Only story activity counts; `works.updated_at` and `last_synced_at` are excluded |
| A reviewer marks a work complete to farm the completion incentive | The review route does not write `lifecycle_events`; the incentive stays the author's |
| A human mark is mistaken for a machine mark | `completion_source` is written on every transition; the badge uses it |
| A second stale/revive cycle is lost | `work_completion_marks` has no unique `(work, event)` rule; `lifecycle_events` stays a one-time reward |
| `published_at` has no offset and is read as "never updated" | The domain parser accepts naive `YYYY-MM-DDTHH:MM:SS` as UTC; a unit test pins it |

## Deliberately not in this roadmap

- Per-source thresholds (`QQ` 2 months, `AO3` 6 months, etc.). One `X` first;
  the per-source override is a follow-up with its own config shape.
- The §8.8 one-notice email. Recorded as `M45-76.4`; the automatic mark is
  labeled in the UI, but the notice is a named debt, not an implemented claim.
- Automatic `derived_activity` writes. The field stays the 3/12-month reading;
  this feature writes `completion`.
- Automatic adoption offers. §56 already lets an author offer an abandoned work;
  this roadmap does not auto-create an offer.
- A "keep abandoned" lock for authors. The current rule revives on any story
  update, which is the owner's request; a lock is a related idea.

## Related ideas worth considering next

1. **A resurrection watch.** §57 and `wip_watch` already notify readers when a
   work completes. The same watch should fire when an auto-abandoned work
   revives — "a work you follow is back" — with the same one-per-watch rule.
2. **Source-status reconciliation.** Map `library_items.status` (`ongoing`,
   `complete`, `cancelled`, `hiatus`) into `completion` at import time, instead
   of inferring from the date. The 30,570 works with `status='unknown'` are the
   real gap this feature cannot fill.
3. **A curator queue for the unmeasured.** The 30,570 works with no source date
   are not abandoned; they are unknown. A filtered queue for TL3/curators with
   the source link and the last import report would let a human resolve them
   without inventing a clock.
4. **Per-source thresholds.** A source that publishes in bursts (QQ) and one
   that publishes slowly (AO3) do not mean the same thing by four months. A
   `[sources.<key>] abandon_after_months` override is a natural second step.
5. **A "still with me?" author nudge.** §8.8 already asks for one notice; send
   it at `X - 1 month` with two buttons: "still going" (resets the clock) and
   "mark complete". Declining is silent and permanent.
6. **Appeal and undo.** A one-click "this is not abandoned" from the work page
   for the owner, which writes `completion='hiatus'` or `in_progress` and a
   reason. The audit row already exists; only the button is missing.
7. **Automatic adoption offers.** An auto-abandoned work with no open offer
   could be offered to the adoption pool after a grace period; §56.1's
   ownership-never-moves rule makes this safe to try.
8. **A "completed at source" backfill.** The 291 `complete` works and the
   source-complete rows are a tiny training set for a classifier that says
   "likely complete" — §8.8 already permits the note as a suggestion, never a
   reader-facing claim.
