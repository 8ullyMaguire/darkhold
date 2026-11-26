# docs/plans/m45-28-wip-adoption-draft-2026-10-04.md — plan card

Source: `docs/plans/m45-28-wip-adoption-draft-2026-10-04.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Status:** spec written (`docs/spec.md` §56, commit `c1837b3`). Not yet implemented. This
`docs/requirements.csv` M45-28:
Read `docs/spec.md` §56 before designing anything. Three clauses, and two of them are
`docs/plans/m45-23-north-star.md` opened with: **the prerequisite is already built.**
  `migrations/sqlite/0003_works.sql:48`). `abandoned` is what §56.1 gates on, and it
- `chapter_revisions.created_by_pseud_id` (`0003_works.sql:114`) already records who wrote
**File:** `crates/domain/src/wip_adoption.rs` (new)
**File:** `crates/db/src/wip_adoption.rs` (new)
  is `42883`, and `docs/plans/REMAINING-2026-10-03.md` records three of those.
**Files:** `crates/app/tests/wip_adoption.rs`, `crates/app/tests/closure_notes.rs`
`scripts/check-pg-subquery-alias.py` is the gate that would catch the `contributors()`
python3 scripts/check-pg-subquery-alias.py crates

## Copy-pasteable code / commands

-- adoption_offers
CREATE TABLE adoption_offers (
    id              TEXT/UUUID  PRIMARY KEY,
    work_id         TEXT/UUUID  NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    offered_by_pseud_id TEXT/UUUID NOT NULL REFERENCES pseuds(id),
    -- The author who stopped. Kept alongside `work_id` so withdrawing an offer is a
    -- state change on the offer rather than a permission check against the work's
    -- current owner -- see "Why both pseuds" below.
    author_pseud_id TEXT/UUUID  NOT NULL REFERENCES pseuds(id),
    -- NULL means "anyone at the publishing trust level may adopt".
    adopted_by_pseud_id TEXT/UUUID REFERENCES pseuds(id),
    status          TEXT        NOT NULL DEFAULT 'open',  -- open | withdrawn | adopted
    created_at      TEXT/TIMESTAMPTZ NOT NULL,
    updated_at      TEXT/TIMESTAMPTZ NOT NULL
);
CREATE INDEX adoption_offers_work ON adoption_offers (work_id, status);
CREATE INDEX adoption_offers_open ON adoption_offers (status, created_at);

-- closure_notes
CREATE TABLE closure_notes (
    id              TEXT/UUUID  PRIMARY KEY,
    work_id         TEXT/UUUID  NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    -- §56.2: the note is versioned WITH THE WORK. One row per revision number, like
    -- chapter_revisions, so restoring an old note is a normal revision restore.
    revision_number  INTEGER     NOT NULL,
    body            TEXT        NOT NULL,
    author_pseud_id TEXT/UUUID  NOT NULL REFERENCES pseuds(id),
    created_at      TEXT/TIMESTAMPTZ NOT NULL
);
CREATE UNIQUE INDEX closure_notes_revision ON closure_notes (work_id, revision_number);

pub enum AdoptionStatus { Open, Withdrawn, Adopted }

pub struct AdoptionOffer {
    pub id: String,
    pub work_id: String,
    pub offered_by_pseud_id: String,
    pub author_pseud_id: String,
    pub adopted_by_pseud_id: Option<String>,
    pub status: AdoptionStatus,
    pub created_at: String,
    pub updated_at: String,
}

pub struct Contributor {
    pub pseud_id: String,
    pub handle: String,
    /// §56.3: FIRST contribution, so an adopter does not leapfrog chapter one's author.
    pub first_revision_at: String,
    pub revisions: i64,
}

pub struct ClosureNote {
    pub work_id: String,
    pub revision_number: i64,
    pub body: String,
    pub author_pseud_id: String,
    pub created_at: String,
}

POST /works/:id/adoption-offer          open an offer (work's owner pseud)
DELETE /works/:id/adoption-offer        withdraw it (no notice period)
GET /adoption-offers                    list open offers
POST /adoption-offers/:id/adopt         adopt (trust level ≥ publishing)
GET /works/:id/contributors             lineage, §56.3 ordering
PUT /works/:id/closure-note             create or edit, versioned
GET /works/:id/closure-note             latest

## Verification command and expected output

update to `works` anywhere in this module, and the acceptance test asserts the column value
Cases that will not pass by accident, each paired with the implementation it rules out:
| 1 | adopting does not change `works.owner_pseud_id` — assert the column **before and after** | any implementation that transfers ownership |
| 10 | an unaliased subquery would be caught — see "the gate" below | nothing; belt and braces |
the only way to test §56.3's "lineage never affects ranking": you cannot assert an absence
## The gate
`scripts/check-pg-subquery-alias.py` is the gate that would catch the `contributors()`
hazard, and it is already wired into CI with its self-test. Run it:
python3 scripts/check-pg-subquery-alias.py --self-test
Expected: `OK: every FROM subquery is aliased` and `10 cases passed`. A finding in
test run is what proves it.
cargo test -p darkhold-app --test wip_adoption --test closure_notes
cargo test -p darkhold-db --test migration_catalogue
sudo docker start lh-pg-test
  cargo test -p darkhold-app --test wip_adoption --test closure_notes
ran**. Three of four mutation attempts in the previous pass failed to compile and proved

## RED/GREEN evidence

Expected: `OK: every FROM subquery is aliased` and `10 cases passed`. A finding in
ran**. Three of four mutation attempts in the previous pass failed to compile and proved
