-- M45-52 step 6: a copyright notice is not a DSA notice.
--
-- Three disjunctive elements live in the DSA's Article 16 notice and four statutory ones in 17 U.S.C. 512(c)(3). Folding
-- them into one form makes one of the two non-compliant, so the DMCA arm keeps its own record: what is claimed, what is
-- said to infringe, the good-faith statement and the signature. The report row stays the queue's object; this table is the
-- notice's own text.
CREATE TABLE dmca_notices (
    report_id TEXT PRIMARY KEY,
    copyrighted_work TEXT NOT NULL,
    infringing_material TEXT NOT NULL,
    good_faith_statement TEXT NOT NULL,
    signature TEXT NOT NULL,
    created_at TEXT NOT NULL
);
