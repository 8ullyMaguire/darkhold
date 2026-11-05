-- M70-06 — reader skins: one stylesheet per reader, applied to their own session.
--
-- One row per account rather than a library of named skins: the feature is 'my interface, my way', and a library
-- turns it into a sharing surface (that is M70-10's business, with its own rules about attribution and ranking).
--
-- The css column holds text that already passed `darkhold_domain::reader_skin::check` — no @import, no remote
-- url(), no stylesheet code paths, no `</style`. That validation is not repeated here because a CHECK constraint
-- cannot see it; it is enforced at the one write path and re-asserted by the route test.
--
-- No `is_public`: a skin is the reader's own document. Another reader seeing it would be a statement about taste
-- nobody made, and the row would then need the same audience machinery as everything else.
CREATE TABLE IF NOT EXISTS reader_skins (
    account    TEXT PRIMARY KEY REFERENCES accounts (id) ON DELETE CASCADE,
    css        TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
