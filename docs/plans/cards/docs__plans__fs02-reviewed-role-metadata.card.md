# docs/plans/fs02-reviewed-role-metadata.md — plan card

Source: `docs/plans/fs02-reviewed-role-metadata.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Role vocabulary added as curated canonical entities: `narrator`, `mentor`, `love_interest`, `friend`, `parent`, `attendant`. Explicit snippets are recorded in `docs/fs02-reviewed-roles.md`; no explicit body text is shipped in the crate or migration.
- `migrations/{sqlite,postgres}/0221_fs02_role_metadata.sql`: insert the six curated role nodes, promote Harry on the three works, add missing character rows, and insert role joins idempotently.
- `crates/db/tests/fs02_role_metadata.rs`: dual-engine proof against SQLite and PostgreSQL that a work keeps protagonist and supporting roles together, that all reviewed roles are visible, and that unobserved characters remain role-less.
Execution note: `TA3-B3` was later closed with a throwaway account; see `docs/handoff.md` and `docs/DECISIONS.md`. No credential values were retained.

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

- `cargo test -p darkhold-db --test fs02_role_metadata` on SQLite and with `DARKHOLD_TEST_PG_URL` on PostgreSQL: `3 passed` each.
- Mutant: change one role join to an unreviewed role; the test must fail, then restore.

## RED/GREEN evidence

- `cargo test -p darkhold-db --test fs02_role_metadata` on SQLite and with `DARKHOLD_TEST_PG_URL` on PostgreSQL: `3 passed` each.
- Mutant: change one role join to an unreviewed role; the test must fail, then restore.
