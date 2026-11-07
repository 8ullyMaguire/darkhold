-- M16-07: one row per (package, version), holding where review has got to.
--
-- The rules live in `darkhold_domain::extension_review`; this table only records what that module decided, so a
-- bug in a route cannot invent a stage the state machine would have refused. `checks` and `approvals` are JSON
-- arrays because they are lists whose length the workflow reads (did every check pass, are there QUORUM distinct
-- approvers) and whose contents are a report rather than a join target.
--
-- `reason` is kept for the two terminal moves -- rejection and emergency revocation -- because "why was this
-- pulled" is the question asked six weeks later, and a modlog line is not where a reader looks.
CREATE TABLE extension_reviews (
    manifest_id         TEXT NOT NULL,
    version             TEXT NOT NULL,
    stage               TEXT NOT NULL,
    submitter           TEXT NOT NULL,
    checks              TEXT NOT NULL DEFAULT '[]',
    permission_reviewer TEXT,
    security_reviewer   TEXT,
    approvals           TEXT NOT NULL DEFAULT '[]',
    reason              TEXT,
    updated_at          TEXT NOT NULL,
    PRIMARY KEY (manifest_id, version)
);

CREATE INDEX idx_extension_reviews_stage ON extension_reviews(stage);
