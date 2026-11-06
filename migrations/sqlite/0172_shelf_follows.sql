-- 0172_shelf_follows — a reader may follow a shelf another reader published.
--
-- A shelf is a list, so following one is a subscription: the follower wants to see what gets added to it. The row is per
-- (follower, shelf) and is idempotent; unfollowing deletes it.
--
-- `is_public` is enforced by the routes, not by a trigger: a shelf can be made private after it was followed, and a
-- constraint here would either refuse that patch (a reader cannot publish a list) or silently drop followers. The read
-- side joins `shelves.is_public = 1`, so a made-private shelf stops being visible to its followers without deleting their
-- rows — the subscription survives, the contents do not leak.
CREATE TABLE IF NOT EXISTS shelf_follows (
    follower_account_id TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    shelf_id TEXT NOT NULL REFERENCES shelves (id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    PRIMARY KEY (follower_account_id, shelf_id)
);

CREATE INDEX IF NOT EXISTS idx_shelf_follows_shelf ON shelf_follows (shelf_id);
