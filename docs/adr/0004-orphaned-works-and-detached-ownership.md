# 0004 — Orphaned works are rekeyed to a detached pseud, owned by a holdings account

Status: accepted, 2026-11-05. Supersedes nothing; implements M70-01's schema half.

## Problem

A creator must be able to leave without deleting their work. Today they cannot: `accounts → pseuds → works` is
`NOT NULL … ON DELETE CASCADE` at every link, so removing the account removes the works, their chapters, comments,
bookmarks and collection entries with it.

Two facts constrain the answer before any design is considered:

* **Ownership is per pseud, not per account** (ADR 0003). The author line a reader recognises is a pseud, so the
  frozen label must come from the pseud — orphaning an account with five pseudonyms must not relabel all five works
  to one login.
* **`db-migration-integrity` forbids widening a column to NULL when a value change will do.** Widening
  `works.owner_pseud_id` would cost a Postgres `DROP NOT NULL` plus a full SQLite rebuild of the largest content
  table — twelve schema objects to reproduce exactly, each one failing silently if missed.

## Decision

Orphaning **rekeys the work to a detached pseud owned by the instance's existing system account**, in one
transaction. `darkhold_db::SYSTEM_ACCOUNT` is already the instance-not-a-person (created in `0094` for retention
proposals, `status = 'system'`, un-routable address, no credentials), so holdings adds no account row and **no
migration at all**:

1. create a pseud owned by `SYSTEM_ACCOUNT`, with
   `display_name = '<last pseud> (orphaned)'`, `handle = 'orphaned-<work>'`, `bio = NULL`;
2. `UPDATE works SET owner_pseud_id = <detached pseud> WHERE id = ?`;
3. nothing else. Chapters, comments, kudos, collections and bookmarks stay attached to the work, because they
   reference the work, not the pseud.

`works` and `pseuds` keep their shape. The real pseud is then unreferenced and can be deleted with its account,
which is the whole point: the cascade finds no works to take.

## Rejected alternatives

**Nullable `works.owner_pseud_id`, with a CHECK that a work is either owned or detached.** The honest model — "this
work belongs to nobody" is representable — and rejected on cost, not on correctness: it rebuilds `works` on SQLite
and requires recreating its indexes and triggers exactly. The rekey reaches the same observable state (author line
frozen, no live account behind it) without touching the table every read path queries.

**A nullable `pseuds.account_id`.** Pushes the same rebuild one table up, onto the table authentication reads on
every request.

**A shared `'orphan'` pseud for all orphaned works.** One row for every orphaned work re-links them into a single
anonymous author, which is the opposite of the promise: two readers who bookmarked two different works would see
the same author and could join the two. Each orphaning gets its own pseud, so uniqueness per work holds by
construction.

**Soft-deleting the account instead of deleting it.** Leaves the account row and its pseuds as a person who is
merely hidden, and every query has to remember to exclude them. Rekeying removes the reference instead, so nothing
downstream needs to know orphaning exists.

## Consequences

* **The schema does not change.** The first draft of this ADR reserved a new `orphan-holdings` account; the
  repository already has `darkhold_db::SYSTEM_ACCOUNT` doing exactly that job for retention proposals. Reusing it
  means M70-01 is a store function, a call site and tests — no migration, no rebuild, no new person-shaped row.
* **Pseud handles are globally unique** (`pseuds_handle_normalized`), so each detached pseud is named
  `orphaned-<work id>` rather than all of them sharing one handle.
* **A CHECK cannot enforce the rule in SQLite** without the rebuild this design avoids, so the rule is enforced by
  those three facts plus the store function. Tests assert them directly rather than trusting the comment.
* **Erasure changes category.** `works` moves from "the cascade deletes it" to "erasure keeps it, rekeyed to a
  detached owner". The scaffolding test asserts survival, searchability and the frozen label — survival without
  discovery is the failure this feature exists to prevent.
* **Orphaning is irreversible by construction**: the original pseud id appears in no row afterwards, so there is
  nothing to reattach. `darkhold_domain::orphan::may_reclaim` answers `false` rather than leaving the question to a
  missing route.
