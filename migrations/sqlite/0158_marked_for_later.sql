-- M70-07 — Marked for Later: a private queue, deliberately not a private bookmark.
--
-- A flag on `bookmarks` would have been one column and no table. It is wrong for three reasons, and they are all
-- consequences of the same thing — a bookmark is a *record* and this is a *queue*:
--
--   * a bookmark contributes to counts and to the reader's taste signals; a queue must contribute to neither, or
--     "I will read this" quietly becomes "I liked this";
--   * finishing a work REMOVES the entry here, whereas a bookmark stays as the record that the work was read;
--   * the queue is private by definition, so it must be unreachable through any surface that lists bookmarks.
--
-- Keeping it in its own table makes each of those true by construction rather than by a condition somebody has to
-- remember at every call site.
CREATE TABLE IF NOT EXISTS marked_for_later (
    account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    work_id    TEXT NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    marked_at  TEXT NOT NULL,
    PRIMARY KEY (account_id, work_id)
);

-- The queue is read newest-first in one account's scope; that is the only query this table answers.
CREATE INDEX IF NOT EXISTS marked_for_later_recent
    ON marked_for_later (account_id, marked_at DESC);
