-- M70-10 — publishing is attributed to a pseud, never to an account.
--
-- A shared document needs an author a reader can recognise. The account is not one: ADR 0003 keeps the account and
-- the pseud apart so that a pseud can be deleted or changed without the account's history following it, and a
-- browse list that showed accounts would undo that in the one place strangers look.
--
-- Nullable because a document may exist unpublished; `publish` requires a pseud to be chosen, and `list_public`
-- skips rows without one rather than showing an anonymous entry that looks like a bug.
--
-- ON DELETE SET NULL, not CASCADE: deleting a pseud must not silently delete the documents that others may have
-- imported, but the row must stop being attributed to a person who is gone.
ALTER TABLE extension_documents ADD COLUMN pseud_id TEXT REFERENCES pseuds (id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS extension_documents_browse ON extension_documents (is_public, pseud_id, created_at);
