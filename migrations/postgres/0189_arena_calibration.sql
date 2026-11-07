-- M69-11: whether a ballot compared works that were still calibrating.
--
-- The arena pairs works whose ratings are close and whose deviation is high, and names the cards inside their first
-- ballots. This column is that name, kept with the vote: a consumer that ranks by the ladder can then leave the vote
-- out of the global number without having to re-derive which ballots were cast during a work's first five comparisons
-- (and it cannot re-derive it, because the count changes as more ballots arrive).
--
-- Recorded, not applied: nothing refuses the vote, and nothing here adjusts a rating. The window is a label on the
-- evidence.

-- A BOOLEAN on this engine and INTEGER 0/1 on SQLite, which is this repository's convention for every flag column
-- (`is_public`, `comments_enabled`, …): the two dialects declare the same fact and the Rust side binds it as a bool.
ALTER TABLE arena_ballots ADD COLUMN calibrating BOOLEAN NOT NULL DEFAULT FALSE;
