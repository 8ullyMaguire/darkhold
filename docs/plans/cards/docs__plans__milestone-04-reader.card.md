# docs/plans/milestone-04-reader.md — plan card

Source: `docs/plans/milestone-04-reader.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

* `crates/db/src/content.rs` — `find_work`, `chapters_for_work`,
* `crates/app/src/routes/works.rs::read_router()` — `/works/:id` and
* `crates/domain/src/document.rs` — `to_sanitized_html()` is what the reader
* `frontend/src/routes/ChapterRead.svelte` and `WorkRead.svelte` — the pages
Files: `migrations/sqlite/0004_reading.sql`, `migrations/postgres/0004_reading.sql`.
File: `crates/domain/src/reading.rs` (new; export it from `lib.rs`).
### Task 3 — Repository: `crates/db/src/reading.rs`
  `crates/domain/src/content.rs` (`MIN_PUBLIC_RATINGS: i64 = 5`) and have the
### Task 4 — Routes: `crates/app/src/routes/reading.rs`
Register the mutable ones in `crates/app/src/routes/mod.rs` and in
Acceptance tests to add to `crates/app/tests/milestone_4.rs` (copy the harness
from `milestone_3.rs`):

## Copy-pasteable code / commands

reading_progress
    id, account_id, pseud_id (nullable), subject_type ('work'|'library_item'),
    subject_id, chapter_id (nullable), content_revision (nullable),
    paragraph_anchor (nullable), position_fraction (REAL, 0..1),
    device_id (nullable), created_at, updated_at, version
    UNIQUE (account_id, pseud_id, subject_type, subject_id, device_id)
    -- device_id nullable: SQLite and PostgreSQL both treat NULLs as distinct
    -- in a UNIQUE index, so a reader with no device id keeps one row per row
    -- rather than colliding. Document that in the migration comment.

rating
    id, account_id, pseud_id, work_id, stars (1..5), created_at, updated_at,
    version, deleted_at
    UNIQUE (pseud_id, work_id)
    -- Private by construction: no `is_public` column yet. §9.4 adds the
    -- public aggregate in a later migration only when explicitly published.

review
    id, account_id, pseud_id, work_id, body, contains_spoilers (0/1),
    is_public (0/1, default 0), published_at, created_at, updated_at, version,
    deleted_at
    UNIQUE (pseud_id, work_id)

reading_history_entry
    id, account_id, pseud_id, subject_type, subject_id, last_read_at,
    revision_seen, created_at
    UNIQUE (account_id, pseud_id, subject_type, subject_id)

reader_note
    id, account_id, pseud_id, subject_type, subject_id, anchor, body,
    created_at, updated_at, version, deleted_at

typography_preference
    account_id PRIMARY KEY, font_scale REAL, line_height REAL, measure INTEGER,
    reader_theme TEXT, distraction_free (0/1), created_at, updated_at, version

pub struct ReadingPosition { pub revision: Option<RevisionId>, pub anchor: Option<String>,
                             pub fraction: u16 /* permille */, pub device: Option<String> }

pub enum ProgressResolution { UseStored(ReadingPosition), AskTheReader { a: ReadingPosition,
                             b: ReadingPosition }, NoPosition }

/// Spec §9.3: "When devices disagree, present a choice rather than always
/// taking the furthest position."
pub fn resolve_progress(rows: &[ReadingPosition]) -> ProgressResolution;

/// Whether a stored position still points at content the reader saw.
pub fn position_is_reliable(position: &ReadingPosition, current_revision: RevisionId) -> bool;

pub struct ReadingTime { pub minutes: u32 }
/// Spec §9.4 §9.6: 200 words per minute, floored at one minute for non-empty.
pub fn estimate_reading_time(word_count: u32) -> ReadingTime;

save_progress(db, account, pseud, subject, position) -> ()   // upsert on the unique key
progress_for(db, account, subject) -> Vec<ReadingPosition>   // one per device
touch_history(db, account, pseud, subject, revision) -> ()   // upsert last_read_at
history_for(db, account, limit) -> Vec<HistoryRow>           // joined with work metadata
delete_history_entry(db, account, entry) -> bool
clear_history(db, account) -> u64
upsert_rating(db, account, pseud, work, stars) -> i64        // returns new version
rating_for(db, pseud, work) -> Option<Rating>
public_rating_summary(db, work) -> Option<RatingSummary>     // count + mean, ≥5 public ratings only
upsert_review(...) / review_for(...)
notes_for(db, pseud, subject) -> Vec<Note>  /  save_note(...) / delete_note(...)
typography_for(db, account) -> Typography                    // row or defaults
save_typography(db, account, expected_version, …) -> bool

## Verification command and expected output

Then run `cargo test -p darkhold-db`: `the_two_dialects_define_the_same_migration_ids`
must pass, and `darkhold migrate` on a scratch database must apply 0004.
  aggregate query with `WHERE is_public = 1` and write a test that a private
frontend/src/lib/reading.test.ts
  script will fail the same way again. `src/lib/prepaint.test.ts` now fails if
### Task 6 — Verify, then write it down
* `cargo test --workspace`, `cargo clippy --all-targets --all-features -- -D warnings`,
* `bash frontend/scripts/fe.sh build`, `… test`.
Spec §9 `Acceptance` requires, and these must each be a named test:
| Requirement | Test |
   `RequireSession` here would silently break it, and the test suite will not
   notice unless you write the anonymous test first.

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
