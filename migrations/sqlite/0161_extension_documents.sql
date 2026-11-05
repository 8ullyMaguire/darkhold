-- M70-09 — extension documents: what a reader can hand to another reader.
--
-- The row stores the CANONICAL document text, not its parts. Reassembling a document from columns would mean the
-- export path and the import path each had their own idea of the shape, and the first thing to drift would be the
-- one field neither test looks at. Export is therefore a read, and import is `extension_document::parse`.
--
-- `kind` and `name` are duplicated out of the document because a browse list needs them without parsing every
-- row, and they are written from the parsed document — never from the request — so they cannot disagree with it.
--
-- `is_public` defaults to 0: a document is private until its author says otherwise, and a shared document is
-- attributed to a pseud (ADR 0003), never to the account that owns the row.
CREATE TABLE IF NOT EXISTS extension_documents (
    id         TEXT PRIMARY KEY,
    account    TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL,
    document   TEXT NOT NULL,
    is_public  INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS extension_documents_account ON extension_documents (account, created_at);
CREATE INDEX IF NOT EXISTS extension_documents_public ON extension_documents (is_public, created_at);
