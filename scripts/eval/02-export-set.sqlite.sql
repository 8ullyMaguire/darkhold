-- The evaluation set: a temporal split, the popularity rung, and the refusals.
--
-- Dialect: SQLite. One statement, because a per-reader loop over a 1.7 GB mirror is the cost class
-- that already cost this estate a day (30 s as one join versus minutes as a loop).
--
-- The split: the 80th percentile kudos timestamp is the cutoff, everything before it is training
-- and everything after it is scored. **Temporal, not a random mask** — every interaction in this
-- database carries a clock, so there is no reason to score the past with a model trained on the
-- future (spec §24.4).
--
-- A reader needs five test interactions to be scored. Fewer is a per-reader noise sample, and
-- including it is how one person's three kudos moves a mean.
--
-- Readers are emitted as raw account ids and pseudonymised by `02-export-set.sh`; SQLite has no
-- hash function, and doing it in one place beats doing it twice and differently.

WITH cutoff AS (
    SELECT created_at AS at
      FROM work_kudos
     ORDER BY created_at
     LIMIT 1 OFFSET (SELECT (count(*) * 8) / 10 FROM work_kudos)
),
judged AS (
    SELECT k.account_id AS account_id, k.work_id AS work_id
      FROM work_kudos k, cutoff
     WHERE k.created_at > cutoff.at
    UNION
    SELECT b.account_id, b.subject_id
      FROM bookmarks b, cutoff
     WHERE b.subject_type = 'work' AND b.created_at > cutoff.at
),
scored AS (
    SELECT account_id FROM judged GROUP BY account_id HAVING count(*) >= 5
),
popularity AS (
    SELECT k.work_id AS work_id, count(*) AS n
      FROM work_kudos k, cutoff
     WHERE k.created_at <= cutoff.at
     GROUP BY k.work_id
     ORDER BY n DESC, k.work_id
     LIMIT 20
)
SELECT json_object(
    'catalogue_size', (SELECT count(*) FROM works),
    'cutoff', (SELECT at FROM cutoff),
    'groups', (SELECT coalesce(json_group_object(work_id, grp), '{}') FROM (
        SELECT wt.work_id AS work_id, min(ce.canonical) AS grp
          FROM work_tags wt
          JOIN canonical_entities ce ON ce.id = wt.node_id
         WHERE ce.kind = 'fandom'
         GROUP BY wt.work_id)),
    'work_ids', (SELECT coalesce(json_group_array(id), '[]') FROM (
        SELECT id FROM works ORDER BY id LIMIT 5000)),
    'readers', (SELECT coalesce(json_group_array(json(r)), '[]') FROM (
        SELECT json_object(
            'reader', s.account_id,
            'judged', (SELECT coalesce(json_group_array(json_object('work_id', j.work_id, 'grade', 1)), '[]')
                         FROM judged j WHERE j.account_id = s.account_id),
            'rungs', json_array(
                json_object('name', 'popularity',
                    'ranking', (SELECT coalesce(json_group_array(work_id), '[]') FROM popularity)),
                json_object('name', 'tag_i2i', 'ranking', json_array(),
                    'refused', '0 of 1247 engaged rows carry a work_tag (M64-06)'),
                json_object('name', 'user_neighbourhood', 'ranking', json_array(),
                    'refused', 'co-liked works are too sparse to form a neighbourhood (M64-12)'),
                json_object('name', 'mf', 'ranking', json_array(),
                    'refused', 'no offline matrix factorisation exists (spec 12.7a)')
            )
        ) AS r
          FROM scored s)));
