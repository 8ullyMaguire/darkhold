# docs/plans/m45-65-inventory.md — plan card

Source: `docs/plans/m45-65-inventory.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`scripts/check-snapshot-pii.py` and running it: **87 undecided columns across the three spellings**, not the 37 the
Read from `migrations/postgres/*.sql` (CREATE TABLE bodies plus `ALTER TABLE … ADD COLUMN`), scoped per table:
across 395 tables`, and `build-snapshot-sql.py --out /tmp/snap.sql` emits 2493 lines with **119 `snapshot_account`
**Still open on this row, and therefore still `planned`:** the *second* gate. `crates/db/tests/erasure_covers_fk_free_accounts.rs`
**Found on the way, unrelated and pre-existing:** `scripts/build-snapshot-sql.py` crashed on *every* invocation without
Widening `account_keyed` in `crates/db/tests/erasure_covers_fk_free_accounts.rs` to `created_by` and the `*pseud_id`
`taxonomy_edges.created_by` — **do not exist in the migrated schema**: `0013_community.sql` begins

## Copy-pasteable code / commands

accounts.folded_from_account_id            announcements.created_by            bounties.account
bounties.created_by                        categories.created_by               category_proposals.created_by
challenges.created_by                      chapter_revisions.created_by_pseud_id
collaboration_invites.invited_by_pseud_id  collaboration_invites.invited_pseud_id
directory_lists.created_by                 entity_edges.created_by             entry_moderation_proposals.created_by
events.created_by                          public_pseud_links.source_pseud_id  public_pseud_links.target_pseud_id
publication_events.actor_pseud_id          reader_sort_preferences.pseud_id    series.owner_pseud_id
source_credentials.pseud_id                specialist_profiles.pseud_id        specialist_requests.matched_pseud_id
specialist_requests.requester_pseud_id     specialist_skills.pseud_id          translation_jobs.created_by
work_contributors.pseud_id                 work_tag_votes.voter_pseud_id       works.owner_pseud_id

## Verification command and expected output

until this list is classified — the gate's own rule is that it fails, in both directions, until every column carries a
treatment, and a green gate with half-read treatments would be worse than the planned row.
projections, 29 of them on the bare `account` columns** — i.e. the decision reaches the masking SQL, not just the gate.
**Still open on this row, and therefore still `planned`:** the *second* gate. `crates/db/tests/erasure_covers_fk_free_accounts.rs`
`ArgumentError: conflicting option string` and exits with a traceback instead of usage (exit 2). Fixed; the builder now
## The second gate — measured 2026-11-10, and the defect that stopped it
family makes the gate name **52 columns** with no live-erasure treatment. Classified against the migrations (nullability
unions every `CREATE TABLE` block it can find with no ordering, so it keeps the dropped table's shape. The gate cannot go
**What the next pass should do instead of a better parser:** build the test's schema by running the migrations into a temp
one. Small change, large consequence: the gate stops being a text heuristic about migrations and becomes a statement

## RED/GREEN evidence

treatment, and a green gate with half-read treatments would be worse than the planned row.
The 87 columns now carry policy rows, so the widened detector is **green**: `check-snapshot-pii.py` → `OK: 2878 columns
green while it measures columns the database does not have, and adding treatments for them would teach `erase_account` to
relationship). The treatments were written; the suite went red in **both** directions at once, and that is the finding:
Both Rust files are reverted to HEAD in this commit; the suite is 4/4 green and the row stays `planned`.
