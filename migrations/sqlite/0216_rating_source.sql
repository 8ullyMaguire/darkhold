-- Row 407 — tell a seeded rating from a human one in the table itself.
--
-- The seeder writes through reading::upsert_rating, the same function the
-- rating route calls. A side table would need a second write and would drift
-- from the row it describes; a nullable column on the row is one fact in one
-- place. NULL means a person rated this work — the overwhelming majority of
-- rows, and the value every existing INSERT keeps without a change of intent.
-- 'seed.read_list' is the only value this release writes.
ALTER TABLE rating ADD COLUMN source TEXT;
CREATE INDEX rating_source ON rating (source) WHERE source IS NOT NULL;
