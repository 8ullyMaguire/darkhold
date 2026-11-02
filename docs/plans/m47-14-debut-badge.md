# M47-14: §9.7.6 First Words, the debut badge (audit item 75)

**Ledger row:** `M47-14`. **Milestone:** M47. **Status:** implemented 2026-11-02.
**Source:** `docs/audit-100-reader-ideas-2026-10-04.md` item 75, "'New author' debut badge — one
boolean + a badge".

## What the probe found

There is no badges table in this tree: `grep -rln badge migrations/sqlite/` returns nothing, and
§9.7.6's fourteen milestone rows are specification only. What does exist is the ledger —
`credit_transactions` with a **unique idempotency key** and `economy::post_transaction`, which is
what makes a one-time bonus safe to attempt twice.

The audit's "one boolean" is the part this plan rejects. A stored `is_new_author` flag is a fact
about a moving target: it is true, then a second work is published and it is a lie until somebody
remembers to clear it. The debut signal is **derived** from the published-work count on every read
instead, so it cannot go stale.

## What ships

| surface | file |
|---|---|
| table | `migrations/{postgres,sqlite}/0144_badge_award.sql` |
| rules | `crates/domain/src/badges.rs` (the catalogue, the credit key) |
| store | `crates/db/src/badges.rs` (award, list, holds, published count, account lookup) |
| trigger | `content::publish_work` → `award_debut_badge`, **after** the commit |
| reader surface | `PublicPseudView.badges` + `.debut`, rendered on `PseudProfile.svelte` |

## The table

```sql
CREATE TABLE badge_award (
    id         TEXT NOT NULL PRIMARY KEY,
    account_id TEXT NOT NULL,
    badge      TEXT NOT NULL,
    context    TEXT,
    credits    BIGINT NOT NULL,
    awarded_at TEXT NOT NULL
);
CREATE UNIQUE INDEX badge_award_once ON badge_award (account_id, badge);
```

**On the account, not the pseud.** A reader who adds a second identity still has one first
publication. **`credits` is recorded, not recomputed**: the number the award actually paid is a fact
about what happened, and re-reading the constant would silently rewrite history when §9.7.6's table
changes.

## Why the award is outside the publication transaction

`publish_work` writes the state change, the publication event and two outbox rows in one
transaction, and the author is told it succeeded when that commits. The badge posts a *credit
transaction*, which is a second ledger write with its own idempotency key. Putting it inside would
mean a ledger failure rolls back a publication that has already been reported as done — the author
sees an error and no work, and nothing tells them which half failed. Outside, the failure is logged
and the next publication retries the award, which is idempotent anyway.

The cost is one read of the published-work count per publication, and two more reads while the badge
is still unheld.

## Why "debut" is not a column

`published == 1` is computed on every profile read. The three layers that make the award one-time are
the unique index, the early return when nothing was inserted, and the ledger's idempotency key.

**Measured, and worth recording:** removing the `if count != 1` guard from `award_debut_badge` leaves
every test green, because those three layers carry the property without it. The guard is an
optimisation — it skips two reads for every publication after the first — and not the invariant. A
future reader who deletes it will lose a fast path, not a guarantee, and this paragraph is here so
that they know which.

## Refusals and gaps

- §9.7.6's "appear on public profile **if user chooses**" is not built: there is no per-pseud
  preference to hold the choice, and a boolean nothing can set is a control with no reader. The
  profile's existing `discoverability = hidden` hides the page and the badges with it. Recorded as
  `M47-G03`.
- The other thirteen milestone badges, the eight recurring ones and the seasonal ones are not
  awarded: each needs a signal this build does not yet measure, and a badge whose condition cannot be
  measured is a table of promises.

## Gates

```bash
cargo test -p darkhold-domain --lib badges             # catalogue and the credit key
cargo test -p darkhold-app  --test m47_14_debut_badge  # publish → award → credits, both engines
cargo test -p darkhold-app  --test route_inventory     # the profile route is still tabled
cd frontend && npx vitest run src/routes/PseudProfile.test.ts
```
