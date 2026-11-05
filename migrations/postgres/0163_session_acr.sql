-- M70-08 — the assurance level the issuer asserted for this session, and nothing invented locally.
--
-- `acr` is copied from the verified id_token at sign-in. It is NULL for a password session, because this site does
-- not manufacture an assurance claim: a password is whatever the operator configured, and calling it 'level 2'
-- without a second factor would be a claim nobody can audit.
--
-- The column is therefore a fact about how this session was authenticated, not a permission. The guard that reads it
-- (darkhold_domain::step_up) decides which operations need which level, and it only speaks when there is a level to
-- compare — absent means 'this session has no issuer-asserted assurance', which is a different statement from 'low'.
ALTER TABLE sessions ADD COLUMN acr TEXT;
