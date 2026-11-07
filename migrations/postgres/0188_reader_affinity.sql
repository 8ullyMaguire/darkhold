-- M69-09: one stored affinity score per reader, recomputed in a sweep rather than in a request.
--
-- The unit is the reader. `operator_affinities` already stores a per-WORK score (M11-03); summing those cannot
-- answer "how close is this reader to the instance", because ten liked and ten hated works total the same as
-- twenty mediocre ones. This table is the second question, and the columns keep the components beside the
-- result so a reader can see which evidence produced the number.

CREATE TABLE reader_affinity (
    account_id TEXT PRIMARY KEY,
    score_bp INTEGER NOT NULL,              -- 0..10000
    evidence_bp INTEGER NOT NULL,           -- the weight of the signals that were present, 0..10000
    signals INTEGER NOT NULL,               -- how many of the three contributed
    favourite_overlap_bp INTEGER,           -- NULL: no evidence, not a zero
    tag_similarity_bp INTEGER,
    arena_agreement_bp INTEGER,
    computed_at TEXT NOT NULL
);

CREATE INDEX idx_reader_affinity_score ON reader_affinity (score_bp DESC);
