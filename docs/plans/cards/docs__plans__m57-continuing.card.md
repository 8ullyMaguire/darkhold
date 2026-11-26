# docs/plans/m57-continuing.md — plan card

Source: `docs/plans/m57-continuing.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Status:** spec written (`docs/spec.md` §57, commit `fd4becc`). Not yet implemented.
`docs/plans/100-ideas-scope.md` cluster one: the reader-retention features that
| 57.1 Continue Reading | `reading_progress` (`migrations/sqlite/0004_reading.sql:38`): `account_id`, `pseud_id`, `subject_type`, `subject_id`, `chapter_id`, `content_revision`, `paragraph_anchor`, **`position_permille`**, `device_id`, `version` |
| 57.2 length | `chapter_revisions.word_count` (`migrations/sqlite/0003_works.sql:111`) |
`words_per_minute` does **not** exist in `crates/app/src/config.rs` yet. §38.1
**File:** `crates/domain/src/continuing.rs` (new)
**File:** `crates/db/src/continuing.rs` (new)
Three, all recorded in `docs/plans/REMAINING-2026-10-03.md`:
  (three occurrences: `hit_rate.rs`, `analytics.rs`, `media_resilience.rs`).
  alias it. `scripts/check-pg-subquery-alias.py` catches it.
  `crates/app/tests/flow_dashboard.rs` already does it. This cost four 500s during
Register `GET /me/continue` in `crates/app/tests/route_inventory.rs` so it is

## Copy-pasteable code / commands

-- reader_work_status: the reader's private disposition toward a work.
-- DNF lives here rather than in a new table because it is the same SHAPE as the
-- rating: one row per (reader, work), set by that reader, never public.
CREATE TABLE reader_work_status (
    id              TEXT/UUUID PRIMARY KEY,
    pseud_id        TEXT/UUUID NOT NULL REFERENCES pseuds(id) ON DELETE CASCADE,
    work_id         TEXT/UUUID NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    -- 'dnf' is the only value today. Not an enum column, not a boolean: the
    -- obvious next value is 'dnf_again' (read it, gave up twice) and a schema
    -- that cannot hold it forces a migration.
    status          TEXT NOT NULL,
    -- PRIVATE. Never aggregated, never in a public API response, never shown to
    -- the author. The test that enforces this is the reason the column exists in
    -- this form.
    private_reason  TEXT,
    created_at      TEXT/TIMESTAMPTZ NOT NULL,
    UNIQUE (pseud_id, work_id)
);
CREATE INDEX reader_work_status_work ON reader_work_status (work_id);

pub enum WorkCompletion { InProgress, Complete, Hiatus, Abandoned }

/// §57.1. `progress` is a fraction of the WORK, not of the chapter.
pub struct ContinueReading {
    pub work_id: String,
    pub work_title: String,
    pub chapter_id: String,
    pub chapter_title: String,
    /// 0.0..=1.0 across the whole work.
    pub progress: f64,
    pub chapters_read: i64,
    pub chapters_total: i64,
    /// §9.3: when devices disagree, present a choice rather than picking silently.
    pub conflicted: bool,
    pub updated_at: String,
}

pub struct WorkLength {
    pub word_count: i64,
    /// word_count / Config.words_per_minute, labelled an ESTIMATE.
    pub reading_minutes: i64,
    /// False when a rating ceiling suppressed the estimate (§57.2).
    pub estimate_available: bool,
}

GET  /me/continue            57.1, 200 with {continue: null} when nothing
PUT  /me/continue/dismissed  57.1, stays dismissed
GET  /works/:id/length       57.2
PUT  /works/:id/dnf          57.4
DELETE /works/:id/dnf        57.4

## Verification command and expected output

    -- the author. The test that enforces this is the reason the column exists in
  PostgreSQL.** Any test helper that reads an id needs per-engine decode, the way
  the M45-23 pass because the masked error named no column.
**`PUT /works/:id/dnf` must not return the reason in any response.** §57.4's test is
- Progress rendered as a **work-level** fraction. The test asserts finishing a
`frontend/src/routes/ContinueBanner.test.ts`
Case 7 is the one that needs the derivation to actually run in the test — drive
`updated_at` far enough into the past for §8.8 to mark the work dormant, then assert
python3 scripts/check-pg-subquery-alias.py --self-test
cargo test -p darkhold-app --test continuing --test dnf
cargo test -p darkhold-db --test migration_catalogue
sudo docker start lh-pg-test
  cargo test -p darkhold-app --test continuing --test dnf
**Always `./scripts/fe.sh e2e`, never `playwright test` directly.** The binary embeds
in the last pass that appeared to prove nothing.
attempts in an earlier pass failed to compile.

## RED/GREEN evidence

attempts in an earlier pass failed to compile.
