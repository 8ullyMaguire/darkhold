-- 0178_source_engagement — engagement as each archive reports it (M63-37).
--
-- `library_items.likes` is one normalised number, which is fine for ranking and useless for a rate: AO3 publishes hits and kudos, the
-- XenForo boards publish replies and threadmark likes, FFNet publishes favourites and follows. Dividing a normalised count by another
-- archive's normalised count produces a number that looks like a rate and is not one.
--
-- This table keeps each metric under the name its source published it, so a rate is computed from two metrics of the **same archive**
-- and a missing one is missing rather than zero.
CREATE TABLE IF NOT EXISTS library_item_engagement (
    library_item_id TEXT NOT NULL REFERENCES library_items (id) ON DELETE CASCADE,
    -- `likes` | `hits` | `views` | `replies` | `bookmarks` | `kudos` | `follows` | `favourites`.
    metric          TEXT NOT NULL CHECK (metric IN ('likes', 'hits', 'views', 'replies', 'bookmarks',
                                                    'kudos', 'follows', 'favourites')),
    value           INTEGER NOT NULL CHECK (value >= 0),
    -- When the *source* last reported it, not when we wrote the row: an engagement number ages, and a rate is only meaningful
    -- against the counter it was read with.
    reported_at     TEXT NOT NULL,
    PRIMARY KEY (library_item_id, metric)
);
