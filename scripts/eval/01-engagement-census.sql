-- Does the evidence a recommender needs actually exist?
--
-- The question this answers is not "how big is the database" but "which rungs of the baseline
-- ladder can be scored at all". A rung whose input is empty must be REFUSED by name rather than
-- scored zero, and this census is how the refusal gets its sentence.
--
--   sqlite3 -readonly data/darkhold.sqlite < scripts/eval/01-engagement-census.sql
--
-- `library_items` is the catalogue (141,317 rows on the deployed instance) and carries no counts of
-- its own: the endorsements live in `work_metric_aggregates`, one row per work, so "an engaged row"
-- is a library item whose work has any non-zero counter. Measured 2026-11-01: 1,247 of 141,317, and
-- **none of those 1,247 joined a `work_tag`** — which is why the tag rung is refused, not scored.
--
-- Exit code 1 with `no such table: work_ratings` means migration 0129 is not applied on this
-- database; every row above that line is still valid, and the ratings rung is refused by absence.

WITH engaged AS (
    SELECT DISTINCT li.work_id AS work_id
      FROM library_items li
      JOIN work_metric_aggregates m ON m.work_id = li.work_id
     WHERE coalesce(m.views, 0) + coalesce(m.complete_reads, 0) + coalesce(m.reactions, 0)
         + coalesce(m.kudos, 0) + coalesce(m.bookmarks, 0) + coalesce(m.collection_adds, 0)
         + coalesce(m.reviews, 0) > 0
)
SELECT 'works' AS measure, count(*) AS n FROM works
UNION ALL SELECT 'library_items', count(*) FROM library_items
UNION ALL SELECT 'library_items_with_work', count(*) FROM library_items WHERE work_id IS NOT NULL
UNION ALL SELECT 'engaged_works', count(*) FROM engaged
UNION ALL SELECT 'work_kudos', count(*) FROM work_kudos
UNION ALL SELECT 'kudos_readers', count(DISTINCT account_id) FROM work_kudos
UNION ALL SELECT 'kudos_works', count(DISTINCT work_id) FROM work_kudos
UNION ALL SELECT 'readers_with_5_or_more_kudos', count(*) FROM (
    SELECT account_id FROM work_kudos GROUP BY account_id HAVING count(*) >= 5
)
UNION ALL SELECT 'work_view_log', count(*) FROM work_view_log
UNION ALL SELECT 'distinct_viewers', count(DISTINCT viewer_hash) FROM work_view_log
UNION ALL SELECT 'reading_progress', count(*) FROM reading_progress
UNION ALL SELECT 'bookmarks', count(*) FROM bookmarks
UNION ALL SELECT 'recommendation_slots', count(*) FROM recommendation_slots
UNION ALL SELECT 'slots_with_a_propensity', count(*) FROM recommendation_slots WHERE propensity IS NOT NULL
UNION ALL SELECT 'work_tags', count(*) FROM work_tags
UNION ALL SELECT 'engaged_works_carrying_a_tag', count(DISTINCT wt.work_id)
    FROM work_tags wt JOIN engaged e ON e.work_id = wt.work_id;

-- Separate statement: the ratings ladder arrived in 0129, and a census that fails on an older
-- database is still a census of everything above.
SELECT 'work_ratings' AS measure, count(*) AS n FROM work_ratings;
