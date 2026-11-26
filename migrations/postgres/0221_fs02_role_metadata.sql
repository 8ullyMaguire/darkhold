-- 0221: reviewed role metadata for three works (FS-02). PostgreSQL arm.
--
-- Mirrors the SQLite arm exactly. work_id is UUID here, so every work literal
-- is cast; node ids remain TEXT on both engines.

INSERT INTO canonical_entities
    (id, kind, canonical, norm, created_at, review_status, status)
VALUES
    ('5f3819e5-8391-50c0-a3eb-28da9e40d9c9', 'role', 'narrator', 'narrator', '2026-11-26T00:00:00Z', 'curated', 'active'),
    ('63c312ef-7c11-5f63-83d7-6d1b9de2c500', 'role', 'mentor', 'mentor', '2026-11-26T00:00:00Z', 'curated', 'active'),
    ('bcf4319f-1c02-58cc-bc9f-d837d04e0462', 'role', 'love interest', 'love_interest', '2026-11-26T00:00:00Z', 'curated', 'active'),
    ('030cd99e-e46d-5c93-b00e-5ba4ca96c544', 'role', 'friend', 'friend', '2026-11-26T00:00:00Z', 'curated', 'active'),
    ('262ac5ce-9f64-56f3-a607-06a276437e38', 'role', 'parent', 'parent', '2026-11-26T00:00:00Z', 'curated', 'active'),
    ('19386f25-6856-5377-ac21-334456845ca1', 'role', 'attendant', 'attendant', '2026-11-26T00:00:00Z', 'curated', 'active')
ON CONFLICT (kind, norm) DO NOTHING;

INSERT INTO work_characters
    (work_id, character_node_id, prominence, is_pov, added_at)
SELECT '84b7455c-9166-4415-9aee-3f105b7c73b7'::uuid, '77d9c8a4-356d-4b81-b503-61a1456a1baf', 'protagonist', 1, '2026-11-26T00:00:00Z'
 WHERE EXISTS (SELECT 1 FROM works WHERE id = '84b7455c-9166-4415-9aee-3f105b7c73b7'::uuid)
ON CONFLICT DO NOTHING;

INSERT INTO work_characters
    (work_id, character_node_id, prominence, is_pov, added_at)
SELECT '8f841724-f791-437f-8c38-d89cd336886a'::uuid, '7c955608-4e63-48c9-8f84-b4222aad5d1e', 'supporting', 0, '2026-11-26T00:00:00Z'
 WHERE EXISTS (SELECT 1 FROM works WHERE id = '8f841724-f791-437f-8c38-d89cd336886a'::uuid)
UNION ALL
SELECT '8f841724-f791-437f-8c38-d89cd336886a'::uuid, '4b6d4099-875d-47f9-8d52-b1f36294b22d', 'supporting', 0, '2026-11-26T00:00:00Z'
 WHERE EXISTS (SELECT 1 FROM works WHERE id = '8f841724-f791-437f-8c38-d89cd336886a'::uuid)
ON CONFLICT DO NOTHING;

UPDATE work_characters
   SET prominence = 'protagonist', is_pov = 1
 WHERE work_id IN ('3569bf92-d6e3-4441-9163-75328e3bc349'::uuid,
                   '84b7455c-9166-4415-9aee-3f105b7c73b7'::uuid,
                   '8f841724-f791-437f-8c38-d89cd336886a'::uuid)
   AND character_node_id = '77d9c8a4-356d-4b81-b503-61a1456a1baf';

INSERT INTO work_character_roles
    (work_id, character_node_id, role_node_id, added_at)
SELECT w.id, '77d9c8a4-356d-4b81-b503-61a1456a1baf', '5f3819e5-8391-50c0-a3eb-28da9e40d9c9', '2026-11-26T00:00:00Z'
  FROM works w
 WHERE w.id IN ('3569bf92-d6e3-4441-9163-75328e3bc349'::uuid,
                '84b7455c-9166-4415-9aee-3f105b7c73b7'::uuid,
                '8f841724-f791-437f-8c38-d89cd336886a'::uuid)
   AND EXISTS (SELECT 1 FROM work_characters wc
                WHERE wc.work_id = w.id AND wc.character_node_id = '77d9c8a4-356d-4b81-b503-61a1456a1baf')
ON CONFLICT DO NOTHING;

INSERT INTO work_character_roles
    (work_id, character_node_id, role_node_id, added_at)
SELECT w.id, tc.character_node_id, tc.role_node_id, '2026-11-26T00:00:00Z'
  FROM works w
  JOIN (SELECT '3569bf92-d6e3-4441-9163-75328e3bc349'::uuid AS work_id, '69a9f0bf-abba-49d9-bc4c-86cad9f6a627' AS character_node_id, '63c312ef-7c11-5f63-83d7-6d1b9de2c500' AS role_node_id
        UNION ALL SELECT '3569bf92-d6e3-4441-9163-75328e3bc349'::uuid, '69a9f0bf-abba-49d9-bc4c-86cad9f6a627', 'bcf4319f-1c02-58cc-bc9f-d837d04e0462'
        UNION ALL SELECT '84b7455c-9166-4415-9aee-3f105b7c73b7'::uuid, 'fa52bf68-6218-4e3d-b5d3-bb9663aa9edb', '030cd99e-e46d-5c93-b00e-5ba4ca96c544'
        UNION ALL SELECT '8f841724-f791-437f-8c38-d89cd336886a'::uuid, 'fa52bf68-6218-4e3d-b5d3-bb9663aa9edb', '030cd99e-e46d-5c93-b00e-5ba4ca96c544'
        UNION ALL SELECT '8f841724-f791-437f-8c38-d89cd336886a'::uuid, '7c955608-4e63-48c9-8f84-b4222aad5d1e', '262ac5ce-9f64-56f3-a607-06a276437e38'
        UNION ALL SELECT '8f841724-f791-437f-8c38-d89cd336886a'::uuid, '4b6d4099-875d-47f9-8d52-b1f36294b22d', '262ac5ce-9f64-56f3-a607-06a276437e38'
        UNION ALL SELECT '8f841724-f791-437f-8c38-d89cd336886a'::uuid, '932e99fd-59b5-4f6f-b3f8-857f5418afa7', '19386f25-6856-5377-ac21-334456845ca1'
        UNION ALL SELECT '8f841724-f791-437f-8c38-d89cd336886a'::uuid, '60bc3811-0cd4-44d9-b837-a0cfac3f6320', '19386f25-6856-5377-ac21-334456845ca1'
       ) tc ON tc.work_id = w.id
 WHERE EXISTS (SELECT 1 FROM work_characters wc
                WHERE wc.work_id = w.id AND wc.character_node_id = tc.character_node_id)
ON CONFLICT DO NOTHING;
