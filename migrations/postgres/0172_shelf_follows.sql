-- 0172_shelf_follows — see the SQLite migration for the reasoning.
CREATE TABLE IF NOT EXISTS shelf_follows (
    follower_account_id UUID NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    shelf_id UUID NOT NULL REFERENCES shelves (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (follower_account_id, shelf_id)
);

CREATE INDEX IF NOT EXISTS idx_shelf_follows_shelf ON shelf_follows (shelf_id);
