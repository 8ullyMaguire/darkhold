-- M69-10: Glicko-2 numbers for the roadmap arena, alongside the Elo rating the board still orders by.
--
-- Kept in its own table rather than new columns on `roadmap_cards` for two reasons. First, the switch: with `[roadmap]
-- glicko` off (the default) this table is empty and the board is byte-for-byte the Elo board. Second, the row is optional by
-- nature -- a card that has never been rated under Glicko-2 has no deviation, and a NULL deviation is indistinguishable from
-- "settled" to every reader that forgets to check.
--
-- `deviation` and `volatility` are Glicko-2's own numbers, in the units the paper uses (rating points and the 0.06-centred
-- volatility), not rescaled: the constant that converts them is defined by the algorithm and storing a rescaled copy would
-- invite a second conversion.

CREATE TABLE roadmap_card_glicko (
    card_id     TEXT PRIMARY KEY,
    deviation   REAL NOT NULL,
    volatility  REAL NOT NULL,
    diverged    INTEGER NOT NULL DEFAULT 0 CHECK (diverged IN (0, 1)),
    updated_at  TEXT NOT NULL
);
