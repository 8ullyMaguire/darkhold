-- The evaluation set: a temporal split, the popularity rung, and the refusals.
--
-- Dialect: PostgreSQL. One statement, same shape and same reasoning as the SQLite body; the
-- differences are `::text` casts (a UUID rendered by json_build_object is brace-wrapped, not bare)
-- and `json_agg`/`json_build_object` in place of `json_group_array`/`json_object`.
--
-- Readers are emitted as raw account ids and pseudonymised by `02-export-set.sh`.

WITH cutoff AS (
    SELECT created_at AS at
      FROM work_kudos
     ORDER BY created_at
     OFFSET (SELECT (count(*) * 8) / 10 FROM work_kudos) LIMIT 1
),
judged AS (
    SELECT k.account_id AS account_id, k.work_id::text AS work_id
      FROM work_kudos k, cutoff
     WHERE k.created_at > cutoff.at
    UNION
    SELECT b.account_id, b.subject_id::text
      FROM bookmarks b, cutoff
     WHERE b.subject_type = 'work' AND b.created_at > cutoff.at
),
scored AS (
    SELECT account_id FROM judged GROUP BY account_id HAVING count(*) >= 5
),
popularity AS (
    SELECT k.work_id::text AS work_id, count(*) AS n
      FROM work_kudos k, cutoff
     WHERE k.created_at <= cutoff.at
     GROUP BY 1
     ORDER BY 2 DESC, 1
     LIMIT 20
)
SELECT json_build_object(
    'catalogue_size', (SELECT count(*) FROM works),
    'cutoff', (SELECT at FROM cutoff),
    'groups', COALESCE((SELECT json_object_agg(work_id, grp) FROM (
        SELECT wt.work_id::text AS work_id, min(ce.canonical) AS grp
          FROM work_tags wt
          JOIN canonical_entities ce ON ce.id = wt.node_id
         WHERE ce.kind = 'fandom'
         GROUP BY 1) g), '{}'::json),
    'work_ids', COALESCE((SELECT json_agg(id) FROM (
        SELECT id::text AS id FROM works ORDER BY md5(id::text) LIMIT 5000) c), '[]'::json),
    'readers', COALESCE((SELECT json_agg(r) FROM (
        SELECT json_build_object(
            'reader', s.account_id::text,
            'judged', COALESCE((SELECT json_agg(json_build_object('work_id', j.work_id, 'grade', 1))
                                  FROM judged j WHERE j.account_id = s.account_id), '[]'::json),
            'rungs', json_build_array(
                json_build_object('name', 'popularity',
                    'ranking', COALESCE((SELECT json_agg(work_id) FROM popularity), '[]'::json)),
                json_build_object('name', 'tag_i2i', 'ranking', '[]'::json,
                    'refused', '0 of 1247 engaged rows carry a work_tag (M64-06)'),
                json_build_object('name', 'user_neighbourhood', 'ranking', '[]'::json,
                    'refused', 'co-liked works are too sparse to form a neighbourhood (M64-12)'),
                json_build_object('name', 'mf', 'ranking', '[]'::json,
                    'refused', 'no offline matrix factorisation exists (spec 12.7a)')
            )
        ) AS r
          FROM scored s) x), '[]'::json));
