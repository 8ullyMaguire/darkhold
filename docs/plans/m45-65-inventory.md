# M45-65 — the 87 columns the widened detectors can now see

Measured 2026-11-10 by adding `\baccount\b|created_by|pseud_id` to `IDENTITY_NAME_RE` in
`scripts/check-snapshot-pii.py` and running it: **87 undecided columns across the three spellings**, not the 37 the
requirement names (that count was the foreign-key analysis only; `pseud_id` adds the rest). The detector is reverted
until this list is classified — the gate's own rule is that it fails, in both directions, until every column carries a
treatment, and a green gate with half-read treatments would be worse than the planned row.

## `account` — 29 columns (a row about a person's standing, balance or membership)

- ai_requests.account
- bounties.account
- conversation_participants.account
- credit_entries.account
- credit_holds.account
- dashboard_layouts.account
- event_participation.account
- extension_documents.account
- extension_grants.account
- forum_post_first_vote_notified.account
- forum_sanctions.account
- forum_throttle_log.account
- forum_topic_subscriptions.account
- group_members.account
- operator_role.account
- payments.account
- post_drafts.account
- presence.account
- privacy_requests.account
- push_subscriptions.account
- reader_skins.account
- reader_warning_prefs.account
- reader_work_progress.account
- sanctions.account
- subscriptions.account
- taste_profiles.account
- trust_levels.account
- usage_counters.account
- wishlists.account

## `created_by` — 13 columns (attribution on something that outlives the author)

- announcements.created_by
- bounties.created_by
- categories.created_by
- category_proposals.created_by
- challenges.created_by
- content_policy_blocks.created_by
- directory_lists.created_by
- entity_edges.created_by
- entry_moderation_proposals.created_by
- events.created_by
- exclusion_registry.created_by
- story_identity_members.created_by
- translation_jobs.created_by

## `pseud_id` — 45 columns (attribution or preference attached to a pen name)

- api_tokens.acting_pseud_id
- author_ai_training.pseud_id
- collaboration_invites.invited_by_pseud_id
- collaboration_invites.invited_pseud_id
- content_filters.pseud_id
- content_subscriptions.subscriber_pseud_id
- creators.pseud_id
- credential_audit_events.pseud_id
- did_not_finish.pseud_id
- extension_documents.pseud_id
- feedback_allowlist.trusted_pseud_id
- feedback_denylist.refused_pseud_id
- import_jobs.pseud_id
- link_challenges.pseud_id
- privacy_settings.pseud_id
- publication_events.actor_pseud_id
- public_pseud_links.source_pseud_id
- public_pseud_links.target_pseud_id
- quick_reaction.pseud_id
- rating.pseud_id
- reader_attention_settings.pseud_id
- reader_note.pseud_id
- reader_sort_preferences.pseud_id
- reading_goal.pseud_id
- reading_history_entry.pseud_id
- reading_progress.pseud_id
- reading_unit_read.pseud_id
- recommendation_slots.pseud_id
- review.pseud_id
- revisions.created_by_pseud_id
- search_alerts.owner_pseud_id
- search_history.pseud_id
- search_settings.pseud_id
- series.owner_pseud_id
- source_credentials.pseud_id
- specialist_profiles.pseud_id
- specialist_requests.matched_pseud_id
- specialist_requests.requester_pseud_id
- specialist_skills.pseud_id
- taste_leakage_payouts.recipient_pseud_id
- taste_leakage_resonance_labels.owner_pseud_id
- work_contributors.pseud_id
- work_gifts.recipient_pseud_id
- work_orphans.former_owner_pseud_id
- work_tag_votes.voter_pseud_id

## What the types say — measured 2026-11-10, and why the treatments are NOT uniform

Read from `migrations/postgres/*.sql` (CREATE TABLE bodies plus `ALTER TABLE … ADD COLUMN`), scoped per table:

| spelling | column type | columns |
|---|---|---|
| pseud | `uuid` | 36 |
| account | `text` | 21 |
| created_by | `text` | 9 |
| account | `uuid` | 8 |
| pseud | `text` | 8 |
| created_by | `uuid` | 3 |
| created_by | `None` | 1 |
| pseud | `None` | 1 |

This matters because the snapshot builder's treatments are typed:

- `rekey_account` emits `snapshot_account(voter_pseud_id::uuid)` and `rekey_pseud` emits `snapshot_pseud(voter_pseud_id::uuid)`, so **both
  require a `uuid` column**. The `account` columns that are `text` are handles, not keys — re-keying them with the account
  derivation would either fail the cast or, worse, hash a handle into something that is not the key foreign keys join on.
  Those want `rekey_text` (which the policy already warns is the wrong answer for a *key* and the right one for a name).
- **8 `pseud_id` columns are `text`**, so `rekey_pseud` cannot take them as written. Either they hold uuid strings and the
  cast is safe (check the writer), or the builder needs a text variant of `snapshot_pseud`. Deciding this per column, from
  the writer, is the rest of this row.
- Two columns the parser did not resolve — `entity_edges.created_by` and `revisions.created_by_pseud_id` — were added in a
  form the CREATE TABLE scan misses; read them out of the migration that adds them before classifying.

So the judgement is not "pick a treatment for a name"; it is "read the type, then the writer". The widening stays reverted
until that is done column by column.

## Resolution, 2026-11-10 — all 87 classified, and the detector is widened

The 87 columns now carry policy rows, so the widened detector is **green**: `check-snapshot-pii.py` → `OK: 2878 columns
across 395 tables`, and `build-snapshot-sql.py --out /tmp/snap.sql` emits 2493 lines with **119 `snapshot_account`
projections, 29 of them on the bare `account` columns** — i.e. the decision reaches the masking SQL, not just the gate.

The treatment rule, and the measurement behind it:

- **45 `pseud_id` → `rekey_pseud`**, **29 `account` → `rekey_account`**, **9 `created_by` → `rekey_account`**, re-keyed
  rather than dropped so rows that reference the same person keep joining to each other while nothing walks back to the
  account. The choice was only safe because of a measurement: every populated text `*pseud_id`/`account` column in the
  live database holds a **uuid string** (credit_entries 1125 rows, trust_levels 27, taste_profiles 2, operator_role 1,
  api_tokens 1 — 0 rows outside `^[0-9a-f-]{36}$`), so the builder's `::uuid` cast is valid on a `text` column.
- **`categories.created_by` → `keep`**, because it is not a person: all 9 live rows hold the literal `system`. This is
  the one column of the 87 where `rekey_account` would have broken the build rather than de-identified anything.

**Still open on this row, and therefore still `planned`:** the *second* gate. `crates/db/tests/erasure_covers_fk_free_accounts.rs`
matches `account_id`/`*_account`/`*_account_id` and a bare `account`, but not `created_by` or `pseud_id`, and its entries
live in `darkhold_db::erasure::FK_FREE_ACCOUNT_COLUMNS` — a Rust inventory with a different treatment vocabulary (live
erasure: Delete / Pseudonymise, not snapshot re-keying). Widening it is the remaining half; it was not attempted here
because each entry needs the live-erasure treatment read off the handler, which is a second inventory, not a copy.

**Found on the way, unrelated and pre-existing:** `scripts/build-snapshot-sql.py` crashed on *every* invocation without
`--out` — it called `ap.add_argument("--out", required=True)` **after** `parse_args()`, which raises
`ArgumentError: conflicting option string` and exits with a traceback instead of usage (exit 2). Fixed; the builder now
refuses with a sentence. The snapshot pipeline could not be run at all before this.

## The second gate — measured 2026-11-10, and the defect that stopped it

Widening `account_keyed` in `crates/db/tests/erasure_covers_fk_free_accounts.rs` to `created_by` and the `*pseud_id`
family makes the gate name **52 columns** with no live-erasure treatment. Classified against the migrations (nullability
and type) they split cleanly:

- **42 `pseud_id` columns and 10 `created_by` columns.** Every one of the 10 attribution columns is `NOT NULL`, so they
  need `Treatment::Rekey("id")` (per-row, not one shared placeholder — a shared one re-links every row of the same
  person). Everything that is the person's own act, preference, subscription, invitation or block is `Delete`: a
  relationship with an erased account does not exist.

**And there the attempt stops, because of a defect in the scanner that both gates share.** Five of the 52 —
`blocks.source_pseud_id`, `blocks.target_pseud_id`, `mutes.pseud_id`, `chapter_revisions.created_by_pseud_id`,
`taxonomy_edges.created_by` — **do not exist in the migrated schema**: `0013_community.sql` begins
`DROP TABLE IF EXISTS blocks; DROP TABLE IF EXISTS mutes;` and recreates both with different columns, and the scanner
unions every `CREATE TABLE` block it can find with no ordering, so it keeps the dropped table's shape. The gate cannot go
green while it measures columns the database does not have, and adding treatments for them would teach `erase_account` to
`DELETE` from a table that isn't there (measured: `erase: no such column: source_pseud_id`).

**So the fix this row needs is in the scanner, and it is the next slice:** walk the migrations in order, applying
`CREATE TABLE` (later definition replaces earlier), `DROP TABLE`, `ALTER TABLE … ADD COLUMN` and `DROP COLUMN`, and take
the unlinked set from that final shape. Then the 52-column classification above can be written against a schema that
exists. The two Rust files are untouched in this commit and the row stays `planned`.

## The ordered walk — attempted, and why it needs a database, not a better parser

The next slice above was written and run: `unlinked_account_columns()` rebuilt as an ordered walk (`CREATE TABLE`
replaces a shape, `DROP TABLE` removes it, `ALTER … ADD/DROP COLUMN/RENAME TO` applies), statements applied in file
order. It fixes the phantom columns — `blocks.source_pseud_id`, `blocks.target_pseud_id`, `mutes.pseud_id` disappear —
and it changes the measured set from 52 to **28**:

```
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
```

All 28 were classified (10 `Rekey("id")` where the column is NOT NULL attribution on a row that outlives its author,
1 `Sever` for the nullable taxonomy edge, the rest `Delete` as the person's own act, request, vote, claim or
relationship). The treatments were written; the suite went red in **both** directions at once, and that is the finding:

- **13 existing treatments became "stale"** — `payment_events.account_id`, `module_user_state.account_id`,
  `curator_lens.account_id`, `push_subscriptions.account` and nine more. The ordered walk counts them as *linked* (their
  `REFERENCES accounts … ON DELETE SET NULL/CASCADE` is now seen), so they are no longer "FK-free"; but two behaviour
  tests (`a_row_about_the_person_is_deleted`, `a_row_that_must_survive_loses_its_account_link`) still depend on their
  treatments for the `erase_account` path. Removing them to satisfy the `stale` assertion broke those tests.
- So the two directions are not symmetric: `measured` is a claim about the SCHEMA, `classified` is a claim about the
  ERASURE CODE, and they are only comparable when both come from the same source. A text parser can be made to agree with
  itself; it cannot be made to agree with a migrated database.

**What the next pass should do instead of a better parser:** build the test's schema by running the migrations into a temp
SQLite file (this file already has `TestDb` for exactly that) and read `pragma table_info` plus
`pragma foreign_key_list`. Then `measured` is the schema's own answer and any disagreement with `classified` is a real
one. Small change, large consequence: the gate stops being a text heuristic about migrations and becomes a statement
about the database the migrations produce.

Both Rust files are reverted to HEAD in this commit; the suite is 4/4 green and the row stays `planned`.
