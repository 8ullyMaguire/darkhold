-- M69-21: a Saved Filter's public life — a number-key binding, subscriptions, and blends.
--
-- The object itself already exists (`saved_views`, 0009) and already has a slug (0173). What this adds is the part a reader can
-- act on: press a digit to open their filter, follow somebody else's public filter, and pin a blend of two filters as one
-- surface. None of the three is a second query engine and none introduces a second storage shape for a filter: `keybind` is a
-- column, and the two tables below are relations to views that already exist.

-- One digit, 1–9. `0` is excluded because it is the key beside backspace and would be pressed by accident; ten is not a number
-- key. The CHECK is here rather than only in the route because the route is not the only writer a migration or a script has.
ALTER TABLE saved_views ADD COLUMN keybind INTEGER
    CHECK (keybind IS NULL OR (keybind >= 1 AND keybind <= 9));

-- One binding per digit per account. A collision is a reader pressing one key and getting a surface they did not expect, and
-- the second filter then being unreachable in a way they cannot see — so the database refuses it rather than resolving it.
CREATE UNIQUE INDEX idx_saved_views_keybind
    ON saved_views (account_id, keybind)
    WHERE keybind IS NOT NULL;

-- Following somebody else's public filter.
--
-- Deliberately NOT `search_alerts`: an alert belongs to the pseud that owns the view (it is the mail the owner asked for), and a
-- subscription belongs to the READER who asked. One row per (reader, view): following twice is following once, and unfollowing
-- must not leave a row behind that silently re-follows.
CREATE TABLE saved_view_subscriptions (
    subscriber_account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    view_id               TEXT NOT NULL REFERENCES saved_views (id) ON DELETE CASCADE,
    created_at            TEXT NOT NULL,
    PRIMARY KEY (subscriber_account_id, view_id)
);

CREATE INDEX saved_view_subscriptions_view ON saved_view_subscriptions (view_id);

-- A pinned blend: two saved filters answering one question, in a stated mode.
--
-- `mode` is stored as text rather than a bit because a third mode must be additive; the CHECK names the two that exist so a
-- typo is refused at the write instead of becoming a blend that silently behaves as one of them.
--
-- The two sides are NOT foreign keys with ON DELETE CASCADE into the same row set twice: deleting a view that is half a blend
-- must delete the blend (it cannot answer its question any more), which cascading both sides gives for free.
CREATE TABLE saved_view_blends (
    id             TEXT PRIMARY KEY,
    account_id     TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    left_view_id   TEXT NOT NULL REFERENCES saved_views (id) ON DELETE CASCADE,
    right_view_id  TEXT NOT NULL REFERENCES saved_views (id) ON DELETE CASCADE,
    mode           TEXT NOT NULL DEFAULT 'interleave'
                   CHECK (mode IN ('interleave', 'union')),
    pinned         INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    CHECK (left_view_id <> right_view_id),
    UNIQUE (account_id, name)
);

CREATE INDEX saved_view_blends_account ON saved_view_blends (account_id, pinned, name);
