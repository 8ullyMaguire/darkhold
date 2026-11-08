# 0032 — A migration corrected after the only instance had applied it

**Date:** 2026-11-08
**Status:** Accepted
**Context:** `migrations/*/0183_manifest_versions.sql`, the live instance on thinkcentre, `darkhold migrate`

## Problem

Migration 0183 changes `extension_manifests`' key from `id` to `(id, version)` so a published 1.1.0 can sit beside 1.0.0.
The first version of the file did it the PostgreSQL way — `ALTER TABLE … DROP CONSTRAINT …` then `ADD PRIMARY KEY` — which
SQLite cannot run at all. A later pass rewrote it as a table rebuild so both dialects reach the same key by the same route (the
parity test in `migrate.rs` compares *declared* schema, and the two files had been declaring different things).

By then the live instance had already applied the first version. `_migrations` records a SHA-256 of the SQL text and refuses to
run when a compiled migration's checksum does not match the recorded one:

```
error: migration 0183_manifest_versions has been modified after it was applied
   (recorded 316ab9da…, compiled 85e0997e…); migrations are append-only
```

That refusal is correct in general. Here it blocked every future migration on the only instance that exists, because the
correction was made after the file had run.

## Decision

The live row's checksum is repaired to the compiled one, **after verifying that the live schema already is what the corrected
file declares**, and the deploy records the verification. Specifically:

* live `extension_manifests` has `PRIMARY KEY (id, version)` — the corrected file's target key. The rebuild the corrected file
  performs produces the same columns and the same key from the same rows.
* the one thing the corrected file creates that the live schema lacks is `idx_extension_manifests_id_version`, and on PostgreSQL
  the primary key's own index already covers `(id, version)` — the explicit index is redundant there and necessary on SQLite, where
  the rebuild creates it. So the live schema is not behind the corrected file in any way a query can observe.

The repair is one `UPDATE _migrations SET checksum = … WHERE version = '0183'`, run once, by hand, with the service stopped, and
the commands are in this ADR so the next person can check them rather than trust them:

```sql
select version, checksum from _migrations where version = '0183';
select pg_get_constraintdef(oid) from pg_constraint where conrelid = 'extension_manifests'::regclass;
update _migrations set checksum = '<sha256 of migrations/postgres/0183_manifest_versions.sql>'
 where version = '0183';
```

## Alternatives rejected

**Rewrite 0183 back to the PostgreSQL-only form.** It would restore the checksum and keep the live instance deploying — at the
cost of a migration SQLite cannot run, which is the defect the rewrite fixed. Every fresh SQLite instance would fail at 0183.

**Add 0200 that applies the corrected 0183 to live instances.** A migration that runs only on instances which applied an older
0183 cannot be written conditionally without reading `_migrations` from inside SQL, and the two engines would need different
guards. It also leaves 0183 permanently wrong for SQLite, which is the engine the correction was for.

**Never correct an applied migration; ship only 0200.** Correct in general, and it is the rule that follows from this ADR — but
here the defect was dialect portability, and leaving 0183 unportable means the repository does not build a database on SQLite at
all. The correction had to happen somewhere.

**Delete the row and let 0183 re-apply.** It would re-run `DROP CONSTRAINT` on a table whose primary key is already
`(id, version)`, and the rebuild would copy every row into a new table — a destructive re-application to fix a checksum.

## Consequences

* **The rule for the future:** a migration may be corrected only before it has been applied anywhere that matters. After that, the
  correction is a new migration. This instance is the exception, and it is recorded here rather than assumed.
* Any instance that applied the *corrected* 0183 (a fresh install after this date) needs nothing.
* The parity test now passes for both dialects and both files declare one rebuild technique — the reason the correction existed.
* `darkhold migrate` on a production instance verifies checksums before touching anything, so this class of drift fails loudly
  rather than applying half a batch. That behaviour is unchanged and is what caught it.
