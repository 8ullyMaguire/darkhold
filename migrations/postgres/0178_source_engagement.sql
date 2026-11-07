-- 0178_source_engagement — see the SQLite migration for the reasoning.
CREATE TABLE IF NOT EXISTS library_item_engagement (
    library_item_id UUID NOT NULL REFERENCES library_items (id) ON DELETE CASCADE,
    metric          TEXT NOT NULL CHECK (metric IN ('likes', 'hits', 'views', 'replies', 'bookmarks',
                                                    'kudos', 'follows', 'favourites')),
    value           BIGINT NOT NULL CHECK (value >= 0),
    reported_at     TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (library_item_id, metric)
);
