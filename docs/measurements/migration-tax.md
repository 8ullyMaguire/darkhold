# Darkhold — the dual-engine migration tax (measured 2026-11-23)

The 2026-11-07 review counted 351 `.sql` migration files serving SQLite and Postgres from one schema and called the
two-engine surface the largest recurring cost in the repository. This file re-measures it on the current tree and
makes a sequencing decision. It is a measurement and a decision, not a schema change.

## Measured

Command (read-only, from the project root):

```
python3 scratch/darkhold_migration_tax.py   # counts files under migrations/postgres and migrations/sqlite
```

Output:

```
postgres 214 sqlite 214 paired 214 only_pg 0 only_sqlite 0
identical_pairs 29 divergent_pairs 185
total_lines 21252
```

The repository has 431 `.sql` files in total, of which 428 are these 214 paired migrations; the other three are
outside `migrations/`. The count grew from the review's 351, and **185 of 214 pairs already differ** — the two engines
are not one schema with a thin translation layer, they are 428 files that must be kept in step by hand. The handoff's
"bug that only PostgreSQL shows, twice, in both directions" is the expected result of that surface.

## Decision

1. **Postgres is the primary engine.** The deployed instance uses it; new features are designed against it first.
2. **SQLite stays supported but frozen at the current schema.** It remains the library/reader mode; no new SQLite-only
   capability is added speculatively.
3. **Every new migration is paired from the day it is written**, with a test that exercises both engines. A
   Postgres-only migration does not merge; that is how the 185 divergent pairs arose.
4. **A single-engine migration needs a named consumer**, not a preference: if a real deployment requires dropping
   SQLite, the owner names it and the migration plan is written then. No deletion happens from this document.
5. **Freeze** new cross-engine schema work until there is a named consumer for the SQLite path or a measured reason
   to drop it. This is a cost decision, not a claim that either engine is wrong.

## Not claimed

No migration was edited or applied, no engine was removed, no build or test was run. The numbers above describe the
tree on 2026-11-23.
