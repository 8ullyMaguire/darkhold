# FS-02 — role metadata from reviewed fic bodies

## Problem
`FS-02` was blocked because `work_character_roles` and `work_relationship_dynamics` were both `0`, and no importer or public source supplies role metadata. The owner has now authorised me to derive roles myself by reading a handful of fic bodies.

## Decision
Review three real works already present in the archive, using only roles that are explicit in their bodies. Store them through the existing schema: canonical role nodes in `canonical_entities` (kind `role`), `work_character_roles` joins, and `work_characters.prominence/is_pov` changes where the reviewed body makes the protagonist unambiguous. No role is inferred from tag order, and unobserved roles stay absent.

## Reviewed works and roles
- `Power Plays` (`3569bf92-…`): Harry Potter → protagonist/POV/narrator; Nymphadora Tonks → mentor and love interest (body calls her a former Auror-department mentor and an intimate partner).
- `Harry's Bucket List` (`84b7455c-…`): Harry Potter → protagonist/POV/narrator (added to the cast; he was missing); Hermione Granger → friend.
- `The Attendants Chapters` (`8f841724-…`): Harry Potter → protagonist/POV/narrator; Hermione Granger → friend; James Potter and Lily Potter → parents (added to the cast); Daphne Greengrass and Tracey Davis → attendants (body names them as Harry's assigned attendants).

Role vocabulary added as curated canonical entities: `narrator`, `mentor`, `love_interest`, `friend`, `parent`, `attendant`. Explicit snippets are recorded in `docs/fs02-reviewed-roles.md`; no explicit body text is shipped in the crate or migration.

## Changes
- `migrations/{sqlite,postgres}/0221_fs02_role_metadata.sql`: insert the six curated role nodes, promote Harry on the three works, add missing character rows, and insert role joins idempotently.
- `crates/db/tests/fs02_role_metadata.rs`: dual-engine proof against SQLite and PostgreSQL that a work keeps protagonist and supporting roles together, that all reviewed roles are visible, and that unobserved characters remain role-less.
- Docs: roadmap row, handoff, decisions, requirements evidence.

## Verification
- `cargo test -p darkhold-db --test fs02_role_metadata` on SQLite and with `DARKHOLD_TEST_PG_URL` on PostgreSQL: `3 passed` each.
- Mutant: change one role join to an unreviewed role; the test must fail, then restore.
- Apply migration 0221 on the live instance; query `work_character_roles` count and the three works' roles.
- Gates once at end: `ok: repo`, `ok: ledger`.
