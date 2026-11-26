# docs/plans/crawling-retention-preservation.md — plan card

Source: `docs/plans/crawling-retention-preservation.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Spec:** `docs/spec-amendments/crawling-retention-and-preservation.md`
| `robots_gate` as a pure function | §11.5 | built, `safety.rs:1316` | Phase A extends the signature — **done, A.3** |
   `crates/app/src/routes/roadmap.rs:80`, `:136` and `:250` all read
`migrations/sqlite/0085_story_identity.sql` and
`migrations/postgres/0085_story_identity.sql`, identical ids:
`docs/requirements.csv` can seed a row for each. Nothing writes to them in A0.
New file `crates/scrapers/src/robots.rs` addition (the rules live there; the
`crates/app/src/config.rs` (`ImportsConfig`, around line 1104):
function in `crates/domain/src/policy.rs`:
2. **`resolve_posture` lives in `crates/scrapers/src/robots.rs`, not
   `crates/domain/src/policy.rs`.** `scrapers` does not depend on `domain`, and
`crates/scrapers/src/safety.rs:1316`, currently

## Copy-pasteable code / commands

CREATE TABLE IF NOT EXISTS story_identities (
    id                TEXT PRIMARY KEY,
    -- The local work this identity is about. A crosspost this instance
    -- performed always has one; an identity with no local member is a
    -- purely-external grouping, which A0 does not create.
    work_id           TEXT NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    -- Canonical metadata is a COPY of the work's, not the authority. §11.10's
    -- merges are reversible, so an identity must be able to be dissolved back
    -- into its members without a work losing its title.
    canonical_title   TEXT NOT NULL DEFAULT '',
    status            TEXT NOT NULL DEFAULT 'active',
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    version           INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS story_identity_members (
    id                  TEXT PRIMARY KEY,
    identity_id         TEXT NOT NULL REFERENCES story_identities (id) ON DELETE CASCADE,
    -- Exactly one of the two. A local member names a work; an external member
    -- names a record at a site. Both are CHECK-enforced, because a row with
    -- both or neither is a row that means two things.
    work_id             TEXT REFERENCES works (id) ON DELETE CASCADE,
    external_record_id  TEXT,
    -- One value ships: 'cross_posted'. See below before adding a second.
    edition_relation    TEXT NOT NULL DEFAULT 'cross_posted',
    external_source_key TEXT,
    external_url        TEXT,
    created_at          TEXT NOT NULL,
    UNIQUE (identity_id, work_id),
    UNIQUE (identity_id, external_source_key, external_url),
    CHECK ((work_id IS NULL) <> (external_record_id IS NULL))
);

an_identity_member_cannot_be_created_by_similarity_of_title_author_or_url

cd ~/code-local/rust/darkhold
cargo test -p darkhold-app story_identity 2>&1 | tail -30

## Verification command and expected output

| Crossposting | §11.12 | not built (no `crosspost` symbol outside one test file name) | Phase D |
test to write:
A grep for the matcher is a second net, but the test is the one that matters:
assert that every member row's `created_by` is a performed crosspost.
cargo test -p darkhold-app story_identity 2>&1 | tail -30
The last-but-one and the parity test are the two that catch the failure modes
The gate itself reads `FetchPolicy::robots_posture`; `honour_robots` is
pre-existing `the_import_policy_carries_the_instances_robots_answer` test caught
three postures — **asserted by a test, not by a comment**, because that is the
| M12 | the gate ignores the class | `metadata_only_reads_a_disallowed_metadata_path_and_discards_it` |
M14 is a real bug this phase shipped, caught by a test written before the change
M6, M7 and M10 **survived the first pass** and each found a real hole rather than
a bad assertion. M6 and M7 passed only because no test read the field the gate
reads — the existing test asserted the *deprecated* bool, which `policy_for`
gate returns, so routing the discarded read through `record_robots_override`
satisfied every gate assertion. The three tests that closed them are named in
Two of the first-pass runs were also untrustworthy and are worth recording: M3
exactly like one that survived. Both scripts now assert the anchor count and
as distinct from a pass.
`SafeFetcher::label_fetched` applies the gate's answer to the page as it comes

## RED/GREEN evidence

**Status:** Phases A0, A.1–A.5 and B built and green. Phases C1, C, D, E not
Twelve mutations, each of which must turn the suite red. Every one is a way the
a bad assertion. M6 and M7 passed only because no test read the field the gate
happens to keep in step. M10 passed because the counter is not part of what the
green because they all tested `max_bytes_for` and none looked at the seam. The
**A.4 mutations** (all turn the suite red):
other passed. The script now hands the discarded page real bytes
**A.5 mutations** (`scratch/prove_a5.sh`, all nine turn the suite red):
`persistent` check turned the suite red — but only because a test asserted the
`if let Ok(...)` never matched, the test passed, and it would have kept passing
**Phase B mutations** (`scratch/prove_b.sh`, all seven turn the suite red):
send path passed it a constant. This is the same seam as D6 (the ceiling) and the
