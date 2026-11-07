-- M69-03: a signed taste passport, received and recorded as a CLAIM.
--
-- The row is a receipt, not an identity: it says "this account presented this document, signed by this key, and we checked
-- the signature". Nothing here proves the key belongs to the reader, and the imported taste signals it seeds are marked
-- `origin = 'imported'` (0101) so the local recomputation outvotes them the moment there is local evidence.
--
-- Keyed by (account, origin): a reader has one current passport per origin instance, and re-presenting a newer one replaces
-- the older claim rather than accumulating priors that would double-count the same history.

CREATE TABLE taste_passports (
    id          TEXT NOT NULL,               -- sha256 of the canonical document, first 32 hex chars
    account_id  TEXT NOT NULL,
    origin      TEXT NOT NULL,               -- the instance that issued it
    key         TEXT NOT NULL,               -- the ed25519 public key the document carries
    subject     TEXT NOT NULL,               -- the reader's handle at the origin: a claim, not an identity
    document    TEXT NOT NULL,               -- the signed document exactly as presented
    issued_at   TEXT NOT NULL,
    received_at TEXT NOT NULL,
    signals     INTEGER NOT NULL DEFAULT 0,  -- how many imported taste signals this document seeded
    PRIMARY KEY (account_id, origin)
);
CREATE INDEX taste_passports_document ON taste_passports (id);
