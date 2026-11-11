-- 0207 — why a taste version changed, not only who pressed the button (M45-45).
--
-- `changed_by` is an account id, and on PostgreSQL it is a UUID column: a lens change that arrived through a vote has no
-- account to name — the account that ran the decision is incidental, and the proposal is the cause. Storing
-- `proposal:<id>` in a UUID column is not possible, and storing the proposal's UUID there would make a proposal
-- indistinguishable from an account.
--
-- So the cause gets its own column: `operator` for a direct write, `proposal:<id>` for a lens the instance voted on.
ALTER TABLE instance_taste_settings_history ADD COLUMN cause TEXT NOT NULL DEFAULT 'operator';
