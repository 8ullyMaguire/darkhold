-- 0173_saved_view_slug — see the SQLite migration for the reasoning.
ALTER TABLE saved_views ADD COLUMN slug TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_saved_views_slug
    ON saved_views (account_id, slug)
    WHERE slug IS NOT NULL;
