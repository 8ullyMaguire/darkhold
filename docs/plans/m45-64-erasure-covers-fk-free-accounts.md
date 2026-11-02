# M65-10 + M45-64 — erasure covers the account columns the foreign-key graph does not

**Rows:** `M65-10` (module_user_state rekeyed), `M45-64` (new: the other seventeen) · **Milestone:** M45/M65
**Spec:** §11.16 (erasure and de-identification), §51 (the ledger's erasure statement), §59.6 (module state)
**Related open row:** `M45-58` (supply routing keeps its row forever; the account link must go)

## What was measured, and what it overturns

`docs/plans/m45-51-subject-access-and-erasure.md` measured the erasure graph by parsing the migrations for
`CREATE TABLE` and `REFERENCES` and reported:

> **71 tables** whose foreign-key chain reaches `accounts`. Exactly one column into `accounts` does not
> cascade. So the erasure cascade is **already 99% present** as schema.

That measurement counted only tables **already in the FK graph**, so it could not see a table whose
`account_id` is a plain `TEXT`/`UUID` column with **no `REFERENCES` clause at all**. Measuring that set
directly — every column named `account_id` or `*_account`, then `PRAGMA foreign_key_list` for each table —
gives **18 columns across 16 tables** with no foreign key:

| table | column(s) | type | nullable | after erasure today |
|---|---|---|---|---|
| `directory_votes` | account_id | TEXT | no | **row survives** |
| `interaction_warmth` | account_id, author_account | UUID | no | **row survives** |
| `payment_events` | account_id, author_account | UUID | yes | **row survives** |
| `vanguard_roles` | account_id | UUID | yes | **row survives** |
| `curator_rewards` | account_id | UUID | no | **row survives** |
| `curator_roles` | account_id | TEXT | yes | **row survives** |
| `author_media_preferences` | account_id | TEXT | yes | **row survives** |
| `targeted_bounties` | account_id | TEXT | no | **row survives** |
| `category_votes` | account_id | TEXT | no | **row survives** |
| `entry_moderation_votes` | account_id | TEXT | no | **row survives** |
| `exchange_signals` | account_id | TEXT | no | **row survives** |
| `tasting_samples` | account_id | TEXT | no | **row survives** |
| `tasting_responses` | account_id | TEXT | no | **row survives** |
| `trust_events` | account_id, actor_account_id | UUID | no | **row survives** |
| `supply_routing` | account_id | TEXT | no | **row survives** |
| `module_user_state` | account_id | TEXT | no | **row survives** |

`DELETE FROM accounts` cascades through foreign keys and touches none of them. The account is gone; a
`tasting_responses` row still says which way that person voted on a taste pair, under an id that no longer
resolves to anybody — retained personal data the erasure reported as destroyed, because `plan_erasure`
counts only three tables (`PRIVATE_TABLES`).

## The rule this pass establishes

**Every account-keyed column is classified, in one place, and the classification is tested against the
schema.** "Classified" means one of three things:

* `Delete` — the row is about that person. It goes.
* `Sever` — the row must survive (a financial record, an audit trail) and its account link is cleared.
  Only possible where the column is nullable.
* `DeferredTo(row)` — must be severed and cannot yet be, because the column is `NOT NULL` and the
  rebuild is that row's work. **Named, with its owning requirement id, so it is a tracked gap rather
  than an omission.** `supply_routing` is the only one, and it belongs to `M45-58`.

The invariant test parses the migrations, collects every account-keyed column outside the FK graph, and
fails when one is not in this table — **a new table cannot be added without deciding what erasure does
to it** — and fails in the other direction too, when the table names a column the schema no longer has.

## Steps

### 1. `crates/db/src/erasure.rs`

* `Treatment` enum and `FK_FREE_ACCOUNT_COLUMNS: &[UnlinkedColumn]` — 18 columns, classified as above.
* `erase_account` applies each: `DELETE FROM t WHERE c = ?` for `Delete`, `UPDATE t SET c = NULL WHERE
  c = ?` for `Sever`. UUID columns compare with `::text` on PostgreSQL, TEXT columns without —
  binding a `&str` to a `uuid` column is `incorrect binary data format`, and this is the one place both
  kinds appear in a loop.
* `plan_erasure` counts them, split into `deleted` and `severed`, so the report says what actually
  happened instead of only counting the three cascaded tables.
* `ErasurePlan` gains `unlinked_rows: Vec<(String, i64)>` — named by table so the reader sees which
  surfaces held data about them.

### 2. `crates/db/tests/erasure_covers_fk_free_accounts.rs`

The schema-invariant test described above, plus one positive test per treatment against a real database:
a `tasting_responses` row is gone after erasure, a `payment_events` row survives with a NULL account.

### 3. Ledger

* `M65-10` → `implemented-fully-tested`, with the correction that the row's premise ("rekey with their
  account") described the **snapshot** treatment (spec §11.16.3b) and not live erasure, which deletes
  the account — so the row's own work is a delete, and the reason is written where the row says the
  opposite.
* `M45-64` (new) → the measurement, the table, and the invariant test.
* `M45-58` gains a note that its deferred classification is now enforced by the test.
* `docs/plans/m45-51-subject-access-and-erasure.md` gains the correction to its "99%" claim.

## Verification

```bash
cargo test -p darkhold-db --test erasure_covers_fk_free_accounts   # SQLite
DARKHOLD_TEST_PG_URL=… cargo test -p darkhold-db --test erasure_covers_fk_free_accounts   # PostgreSQL
```

Red first, by removal: with `FK_FREE_ACCOUNT_COLUMNS` empty and the apply loop gone, the invariant test
still passes (it reads the const) — **so the load-bearing test is the per-treatment one against a real
database**, and the invariant's own red is proven by adding a fake table to the migrations scan fixture,
not by deleting code. Both are stated because a test that cannot fail is not evidence.
