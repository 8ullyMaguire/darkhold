-- 0173_saved_view_slug — a saved view gets an address (M69-21).
--
-- A saved filter is a view someone built and wants to come back to; the id is a UUID, which is fine to store and useless to
-- type, share or put in a keybinding. `slug` is the stable name a reader chooses. It is NULLABLE because a view without a slug
-- is still a view — every existing row stays valid, and a reader who never names one loses nothing.
--
-- Uniqueness is per account, and partial on NOT NULL: an account may have one `to-read`, two accounts may each have theirs, and
-- unset slugs do not collide with each other (a plain UNIQUE would allow only one NULL on some engines and none on others —
-- exactly the kind of two-engine divergence a partial index avoids).
ALTER TABLE saved_views ADD COLUMN slug TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_saved_views_slug
    ON saved_views (account_id, slug)
    WHERE slug IS NOT NULL;
