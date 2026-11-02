# 58. Supply: keystone authors, droughts, and the routing record

**Status:** reserved
**Milestone:** —
**Source:** §58 of `docs/spec.md`

M45-25. The platform's supply problem is not "too few works" — the catalogue holds 141,317
rows — but "too little conversation around the works a few people already love". This
section specifies the three pieces that make that visible and addressable: which authors
carry the catalogue, which of their works have readership and no feedback, and how a reader
who wants to give feedback is routed to one of them.

**Supply never touches ranking.** Nothing here feeds `TagWeights`, a sort order, a percentile
or a recommendation slot's score. It reports and it routes; §12's positivity rules govern
what any of it may say in public, and §16.16's gravity is a different mechanism with a
different dial.

## 58.1 A keystone author is computed, never chosen

An author is a **keystone** when both hold:

- they have at least `min_works` engaged works (default 2), where "engaged" means the
  catalogue row carries an endorsement count above zero; and
- the mean **within-source percentile** of those works is at least `min_pct` (default 0.75).

The percentile is §16's catalogue rule, not a new one: rank among the engaged rows *of the
work's own source*, `pct_raw = (rank - 0.5) / n`, then shrink toward 0.5 by `n / (n + 10)`.
A mean of these is comparable across sources; a mean of raw endorsement counts is not, and
would make every keystone author a SpaceBattles author.

An operator may **raise** a keystone and never lower one: the `admin_taste_profile` weights
whose `dimension_key` is `author:<key>` are summed, clamped to at most 0.25, and added to the
mean percentile. The floor is enforced twice, and deliberately: the table's own
`CHECK (weight >= 0.0)` is what stops a stored weight from lowering a keystone, and the
arithmetic floors at zero as well, so the invariant survives a schema change that relaxes
the check. When that table is empty the term is `0.0` and is *reported* as
`0.0` — an absent preference is not a silent default, because a reader of the output cannot
tell the two apart otherwise.

## 58.2 A drought is conversation debt, and an empty corpus refuses

A work is **in drought** when it carries endorsement and no conversation. Conversation is a
`comments` row on the work plus a `work_ratings` row for it.

**When the corpus holds no conversation at all, the report is `measured: false`, carries the
reason, and lists no authors.** It does not list every engaged work as "in drought": that is
arithmetically true and operationally meaningless, and it is the same refusal shape
`?sort=velocity` uses when a second snapshot does not exist yet. The first comment on a live
instance is what turns this report on.

There is no public per-work drought count and no author-facing verdict. §57.4's rule for DNF
counts applies unchanged: a public drought number is a quality judgement on the author
published by readers.

## 58.3 Routing is opt-in, and the record is the row

A reader asks for one work to read and comment on. The instance chooses: the keystone author
with the highest score that still has a work this reader has not been routed before, then the
least-routed of that author's works, then the most endorsed. Ties resolve by work id so two
calls in a row agree.

- **A routing is idempotent per (account, work).** The same reader is never routed the same
  work twice, and a repeated request moves to the next work rather than duplicating.
- **The row is never deleted.** Completion sets `completed_at`; an abandoned routing stays as
  evidence that the reader was asked.
- **Erasure rekeys it, and the rekey is per row.** §51 requires erasure to reach every column that
  identifies a person; §58.3 requires the row to survive. Both hold because `account_id` is `NOT
  NULL` and is replaced with `erased:<row id>` — unique, so two severed routings cannot be joined to
  each other, because a shared placeholder is a pseudonym and a pseudonym that links rows is still
  personal data. `UNIQUE (account_id, work_id)` is unchanged and still admits one live routing per
  reader per work. As built in `M45-58` (`Treatment::Rekey`).
- **Opt-in is per request, not per account.** Nothing is routed by the instance acting alone:
  a reader who never asks is never on a list.

## 58.4 Patronage pays through the existing ledger

Completion grants `PATRONAGE_BP` (500 basis points, i.e. 5.00 credits) to the reader, through
`economy::post_transaction` with type `Grant` and the idempotency key `supply-route:<id>`.

- **No second ledger.** A supply-specific balance would be a second definition of what a
  credit is, and §53 is the one place that is defined.
- **Completing twice pays once**, and the second call reports the first transaction rather
  than an error.
- Patronage is a reward for giving feedback, **not** a purchase and not a bid: it buys no
  placement, no priority and no visibility, and §12.6's separation of payment from reach
  applies.

## 58.6 The welcome rota: a first comment is a promise with a deadline

**Measured on the live instance, 2026-11-02:** `comments` holds **0** rows; **397** works carry
`status = 'complete'`, **1,247** carry likes, and the two sets have an overlap of **0**. A rule of
"finished *and* endorsed" would therefore route nothing for as long as the instance is starved of
conversation — which is exactly the condition the rota exists to end. The candidate rule is
`(likes > 0 OR status = 'complete') AND work_id IS NOT NULL`: endorsed **or** finished.

The rota is the supply surface's answer to drought (§58.2) that acts instead of reporting. It
gives one reader one work that has never been commented on, and asks for one comment within a
deadline.

- **The promise is made to a pseud, not an account.** A comment carries
  `comments.author_pseud`, so a welcome owed to an account could be closed by nobody. A session
  with no active pseud is refused on the field `pseud`.
- **One open welcome per reader, one per author, one per work.** Taking a welcome twice returns
  the first with `taken: "already-open"` rather than banking a second. A named author is never
  asked for twice; a work with no author text is exempt from that rule, so the clause that a work
  is only ever promised once cannot be masked by it.
- **Seventy-two hours, stored not computed.** `due_at` is written at assignment for the same
  reason §58.3 writes `completed_at`: a deadline recomputed from a constant at read time is a
  deadline that changes when the constant does.
- **The rota closes on the comment, not on the button.** Completion looks for a `comments` row on
  the work by that pseud with `created_at >= assigned_at`. Without one it answers `completed:
  false` with a reason naming the work, and nothing is written. A comment that predates the
  assignment is not evidence — the reader was asked after it existed.
- **A deleted comment is not a conversation.** Both rules read `deleted_at IS NULL`. Found on
  the live instance while proving the rota by hand: the probe's comment was removed through the
  real door (`delete_comment` soft-deletes), still counted, and the work stayed excluded from the
  rota for good — and the same reader could have closed a welcome with a comment they deleted.
- **The doors answer, they do not refuse.** An ineligible reader gets `200` with
  `assignment: null` and a reason, the shape `supply.rs` established for a result that is not an
  error. `404` is reserved for a welcome that belongs to somebody else.
- **The report is operator-only** and is the same refusal shape as §58.2: when the corpus cannot
  measure a drought it says so in words instead of listing every engaged work.
- **No second faucet.** §58.4 already pays `PATRONAGE_BP` for feedback; a welcome-specific bonus
  would pay the same act twice.

## 58.5 Acceptance

- A keystone list computed on a fixture with a 4-row source and a 400-row source ranks by the
  shrunk within-source percentile, and the test asserts the small source cannot win on the
  same raw percentile.
- `min_works = 2` excludes the author with one work, and the test names the author it excluded.
- An operator weight of `-0.4` on a keystone changes no score, and the test reads the bonus
  field to prove it was clamped rather than dropped.
- A corpus with zero comments and zero ratings reports `measured: false` with a reason and
  no authors.
- A reader asking twice receives two different works, and the second routing row names the
  first as the reason it moved on.
- Completing a routing twice leaves one `credit_entries` row for it.
- A work carrying a comment is never offered, and the test names the work it excluded.
- Two readers never hold the same work; one author is never asked for twice; a reader holds one
  open welcome at a time and a second claim returns the first rather than taking a second.
- Completion without a comment answers `completed: false` with a reason naming the work, and a
  comment older than the assignment does not close it.
- The due date is 72 hours after the assignment, asserted **literally** — comparing it against the
  constant that produces it is a tautology, and the mutation to 48 hours passed that form.
- The reader doors answer `401` anonymously, the report answers `404` to a non-operator, and the
  module carries the sidebar entry that makes the surface reachable.

## Guest access, trust levels and self-governance (adopted 2026-10-16)

The owner's 2026-10-16 request is specified per site in `docs/ROADMAP-guest-trust-governance.md`, against the estate standard
`secondbrain/10-Projects/2026-10-16-guest-access-trust-and-self-governance.md` (items GS-1…GS-5, each configurable). This repo's deltas, firewalls and order of work are
in that file; it is the place to start, not this pointer.


**Amendment — Ultimate Fanfiction Site (2026-10-17):** `docs/spec-amendments/ultimate-fanfiction-site.md` designates Darkhold as the base for the Ultimate Fanfiction Site (ficnexus = ingestion/search module, kindred = recommendation/taste module). See `~/secondbrain/10-Projects/ultimate-fanfiction-site-spec.md`.
