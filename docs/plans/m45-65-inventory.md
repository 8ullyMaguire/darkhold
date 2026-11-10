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
