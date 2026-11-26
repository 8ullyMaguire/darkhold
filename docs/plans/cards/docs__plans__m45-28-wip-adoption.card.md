# docs/plans/m45-28-wip-adoption.md — plan card

Source: `docs/plans/m45-28-wip-adoption.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`docs/plans/m45-28-wip-adoption-draft-2026-10-04.md`; it proposes migration `0116`, which is now
`0116_account_darkhold_columns.sql`. Migration `0137`.
| §56 exists with three clauses | `docs/spec.md` 10548–10617 (`56.1`–`56.4` + Acceptance) | holds |
| something writes revisions | `grep -rn chapter_revisions crates/` finds no writer, but the table arrives **populated** (an import) | the read path has supply; the write path is elsewhere |
## Step 1 — migration `0137_wip_adoption.sql`, both engines
  erasure fail. Both facts declared in `docs/snapshot-column-policy.json`.
## Step 2 — domain (`crates/domain/src/wip_adoption.rs`, pure)
## Step 3 — store (`crates/db/src/wip_adoption.rs`)
## Step 4 — routes (`crates/app/src/routes/wip_adoption.rs`), module `wip_adoption`
`crates/db/tests/wip_adoption.rs` (both engines), 11 cases: offer opens on an abandoned work the
`crates/app/tests/m45_28_wip_adoption.rs`, 7 cases: anonymous writes `401`; the module is registered
| 7 | `python3 scripts/ideas_catalogue.py` | 0 homeless |

## Copy-pasteable code / commands

CREATE TABLE adoption_offers (
    id                  TEXT   PRIMARY KEY,
    work_id             TEXT   NOT NULL,   -- FK works(id) ON DELETE CASCADE
    offered_by_pseud_id TEXT   NOT NULL,   -- no FK: §51 rekeys pseuds
    author_pseud_id     TEXT   NOT NULL,   -- authority for withdrawal, kept as evidence
    adopted_by_pseud_id TEXT,
    status              TEXT   NOT NULL DEFAULT 'open',   -- open | withdrawn | adopted
    created_at          TEXT   NOT NULL,
    updated_at          TEXT   NOT NULL,
    CHECK (status IN ('open','withdrawn','adopted'))
);
CREATE UNIQUE INDEX adoption_offers_open_work ON adoption_offers (work_id) WHERE status = 'open';
CREATE INDEX adoption_offers_status ON adoption_offers (status, created_at);

CREATE TABLE closure_notes (
    id              TEXT   PRIMARY KEY,
    work_id         TEXT   NOT NULL,
    revision_number BIGINT NOT NULL,
    body            TEXT   NOT NULL,
    author_pseud_id TEXT   NOT NULL,
    created_at      TEXT   NOT NULL
);
CREATE UNIQUE INDEX closure_notes_revision ON closure_notes (work_id, revision_number);

pub enum OfferStatus { Open, Withdrawn, Adopted }
pub enum Opening { Opened, NotOwner, NotAbandoned, AlreadyOpen }
pub enum Adoption { Adopted, NotOpen, NotAbandoned, BelowBar { bar: i64, caller: i64 } }
pub enum Withdrawal { Withdrawn, NotTheAuthor, NotOpen }
pub struct Contributor { pub pseud_id: String, pub handle: String, pub first_revision_at: String, pub revisions: i64 }

pub fn may_adopt(min_trust_level: i64, caller_trust: i64) -> bool;
pub fn order_contributors(list: Vec<Contributor>) -> Vec<Contributor>;  // first_revision_at ASC, then pseud_id
pub fn next_revision(current: Option<i64>) -> i64;                      // 1, then +1

## Verification command and expected output

| a publishing trust level exists to gate adoption | no publish route, and no canonical level: `trust_levels` is `(account, level, computed_at, basis)` with no seed; `Tier` is a *reading* tier (`House/Personal/Full`) | must be **configured**, not inferred |
- `revision_number` is `BIGINT` on both engines so the migration-catalogue test compares like with like.
| # | command | expected |
| 1 | `cargo test -p darkhold-domain --lib wip_adoption` | 5 passed |
| 2 | `cargo test -p darkhold-db --test wip_adoption` | 11 passed |
| 3 | `DARKHOLD_TEST_PG_URL=… cargo test -p darkhold-db --test wip_adoption` | 11 passed |
| 4 | `cargo test -p darkhold-app --test m45_28_wip_adoption` | 7 passed |

## RED/GREEN evidence

| 1 | `cargo test -p darkhold-domain --lib wip_adoption` | 5 passed |
| 2 | `cargo test -p darkhold-db --test wip_adoption` | 11 passed |
| 3 | `DARKHOLD_TEST_PG_URL=… cargo test -p darkhold-db --test wip_adoption` | 11 passed |
| 4 | `cargo test -p darkhold-app --test m45_28_wip_adoption` | 7 passed |
