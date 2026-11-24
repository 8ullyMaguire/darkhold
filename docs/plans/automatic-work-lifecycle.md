# M45-76 — implementation plan: automatic WIP/abandoned lifecycle

**Status:** ready to implement
**Spec:** `docs/spec-amendments/automatic-work-lifecycle.md`
**Roadmap:** `docs/ROADMAP-automatic-work-lifecycle.md`
**Depends on:** `M4-01` (`derived_activity` columns, migration 0119), `M45-28`
(WIP adoption, `lifecycle_events`), `M45-75` (the search work uses the same
`source_updated_at` clock; this feature does not depend on it).

## Conventions this plan obeys

- Every store statement is written twice: `db.sql(sqlite, postgres)`, or a
  `match db.backend()`. Never one literal with `?` for PostgreSQL by hand —
  `Database::sql` rewrites placeholders, so the PostgreSQL arm uses `?` too.
- Every new test runs on SQLite **and** PostgreSQL. Set
  `DARKHOLD_TEST_PG_URL` and forward the container port first:
  `ssh -f -N -L 127.0.0.1:55433:127.0.0.1:55433 thinkcentre`.
- Targeted tests only. No `gate.sh all`, no full-workspace run mid-turn.
- Unset `DARKHOLD_BASE_URL DARKHOLD_TOKEN DARKHOLD_COOKIE` before CLI tests
  that must not inherit a live session.
- Build and test on `thinkcentre`. The workstation has no Rust toolchain for
  this workspace.
- No new `RouteEntry` for a route that does not exist. This plan adds two
  routes, so both get an inventory row and the inventory test stays 5/5.
- `completion` values stay `in_progress | complete | hiatus | abandoned`.
  "WIP" is the UI label for `in_progress`, never a fifth value.

## Step 0 — Preconditions

```bash
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && git status -sb'
cd ~/code-local/rust/darkhold && git status -sb
```

Expected: both clean, or only the docs commit from this task. If the deploy
checkout is ahead, stop and reconcile before writing code. Run the existing
targeted suites that this change touches:

```bash
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && \
  env -u DATABASE_URL -u DARKHOLD_TEST_PG_URL \
  cargo test -p darkhold-db --test content_state --no-fail-fast'
```

Expected: the current suite is green. A red suite is noted before the first
edit, not after.

## Step 1 — Migration 0218, both engines

Files:

- `migrations/sqlite/0218_work_completion_marks.sql`
- `migrations/postgres/0218_work_completion_marks.sql`

SQLite arm:

```sql
-- 0218: automatic completion lifecycle (M45-76).
--
-- `completion_source` says who last moved completion. `import` is the honest
-- default for every row that existed before this migration: the pawchive
-- importer wrote 15,220 abandoned rows and this tree has no record of a human
-- decision for any of them.
--
-- `completion_marked_at` is the comparison point for revival. It is seeded from
-- `updated_at` for the existing abandoned rows: an import that loaded a work
-- yesterday did not see a story update after it.

ALTER TABLE works ADD COLUMN completion_source TEXT NOT NULL DEFAULT 'import'
    CHECK (completion_source IN ('import', 'owner', 'review', 'sweep', 'update'));

ALTER TABLE works ADD COLUMN completion_marked_at TEXT;

UPDATE works
   SET completion_marked_at = updated_at
 WHERE completion = 'abandoned' AND completion_marked_at IS NULL;

CREATE TABLE IF NOT EXISTS work_completion_marks (
    id                     TEXT PRIMARY KEY,
    work_id                TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    from_completion        TEXT NOT NULL,
    to_completion          TEXT NOT NULL,
    source                 TEXT NOT NULL
                           CHECK (source IN ('import', 'owner', 'review', 'sweep', 'update')),
    actor_account_id       TEXT REFERENCES accounts(id) ON DELETE SET NULL,
    note                   TEXT,
    observed_last_activity TEXT,
    marked_at              TEXT NOT NULL,
    CHECK (from_completion IN ('in_progress', 'complete', 'hiatus', 'abandoned')),
    CHECK (to_completion IN ('in_progress', 'complete', 'hiatus', 'abandoned'))
);

CREATE INDEX IF NOT EXISTS work_completion_marks_work_idx
    ON work_completion_marks (work_id, marked_at DESC);
```

PostgreSQL arm: identical statements with `id UUID PRIMARY KEY`, `work_id UUID
NOT NULL REFERENCES works(id)`, `actor_account_id UUID REFERENCES accounts(id)`.
Use the `0217_user_lists.sql` arm as the template. The `UPDATE ... SET
completion_marked_at` runs before the table is created, which is fine; it only
touches `works`.

Verification:

```bash
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && \
  env -u DATABASE_URL -u DARKHOLD_TEST_PG_URL \
  cargo test -p darkhold-db --test migrations --no-fail-fast'
```

Expected: migration applies on SQLite; the PostgreSQL arm is exercised by the
same test with `DARKHOLD_TEST_PG_URL` set, expected 0 pending and 0 failed.

## Step 2 — Domain module and its unit tests

Files:

- `crates/domain/src/work_lifecycle.rs` (new)
- `crates/domain/src/lib.rs` (add `pub mod work_lifecycle;` beside
  `pub mod wip_adoption;`)

Complete module:

```rust
//! Automatic WIP/abandoned lifecycle (M45-76).
//!
//! The decision is pure: the store supplies the clocks and the current state,
//! this module says what should happen. Keeping it here means the boundary
//! cases (exactly four months, an update after the mark, a source-complete
//! work) are tested without a database.

use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{Duration, OffsetDateTime, PrimitiveDateTime};

/// The default number of months without an update before abandonment.
pub const DEFAULT_ABANDON_AFTER_MONTHS: u32 = 4;

/// Who last moved `works.completion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionSource {
    Import,
    Owner,
    Review,
    Sweep,
    Update,
}

impl CompletionSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Owner => "owner",
            Self::Review => "review",
            Self::Sweep => "sweep",
            Self::Update => "update",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "import" => Some(Self::Import),
            "owner" => Some(Self::Owner),
            "review" => Some(Self::Review),
            "sweep" => Some(Self::Sweep),
            "update" => Some(Self::Update),
            _ => None,
        }
    }
}

/// What the sweep should do with one work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleAction {
    /// Nothing to do, or the work is already in the target state.
    None,
    /// There is no clock to judge; the work is reported, never marked.
    Unmeasured,
    /// `in_progress`/`hiatus` -> `abandoned`.
    Abandon,
    /// `abandoned` -> `in_progress`, because a story update followed the mark.
    Revive,
    /// A linked source says complete; the state follows the source.
    Conclude,
}

/// The facts the decision needs. `last_activity` is the maximum of the author
/// clocks, never `works.updated_at`.
#[derive(Debug, Clone, Copy)]
pub struct Reading<'a> {
    pub completion: &'a str,
    pub source_says_complete: bool,
    pub completion_marked_at: Option<OffsetDateTime>,
    pub last_activity: Option<OffsetDateTime>,
}

#[must_use]
pub fn decide(
    reading: Reading<'_>,
    now: OffsetDateTime,
    abandon_after_months: u32,
) -> LifecycleAction {
    // A source that says "complete" outranks both the clock and the old state.
    if reading.source_says_complete {
        return if reading.completion == "complete" {
            LifecycleAction::None
        } else {
            LifecycleAction::Conclude
        };
    }

    match reading.completion {
        // The author's completed work is never swept, however old the last
        // chapter is.
        "complete" => LifecycleAction::None,
        "abandoned" => match (reading.last_activity, reading.completion_marked_at) {
            // Only an update AFTER the mark revives. An update before it is
            // the reason it was marked abandoned in the first place.
            (Some(last), Some(marked)) if last > marked => LifecycleAction::Revive,
            _ => LifecycleAction::None,
        },
        "in_progress" | "hiatus" => match reading.last_activity {
            None => LifecycleAction::Unmeasured,
            Some(last) if is_stale(now, last, abandon_after_months) => {
                LifecycleAction::Abandon
            }
            Some(_) => LifecycleAction::None,
        },
        // A value this build does not know is left alone; the vocabulary is
        // closed by the CHECK constraints, not guessed at here.
        _ => LifecycleAction::None,
    }
}

/// `X` months is `X * 30` days, the same arithmetic `updated:<=-120d` uses.
#[must_use]
pub fn is_stale(now: OffsetDateTime, last: OffsetDateTime, months: u32) -> bool {
    last + Duration::days(i64::from(months) * 30) <= now
}

/// Parse every shape the three clocks store.
///
/// `source_updated_at` is RFC 3339 with `Z`; `occurred_at` is RFC 3339 with
/// nanoseconds; `published_at` is naive and means UTC. A parser that accepted
/// only RFC 3339 would read the 27,472 imported works with a `published_at` as
/// "never updated" and abandon them all on the first sweep.
#[must_use]
pub fn parse_activity_timestamp(raw: &str) -> Option<OffsetDateTime> {
    let raw = raw.trim();
    if let Ok(parsed) = OffsetDateTime::parse(raw, &Rfc3339) {
        return Some(parsed);
    }
    PrimitiveDateTime::parse(
        raw,
        &format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]"),
    )
    .ok()
    .map(PrimitiveDateTime::assume_utc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(raw: &str) -> OffsetDateTime {
        parse_activity_timestamp(raw).expect("fixture timestamp")
    }

    fn reading<'a>(
        completion: &'a str,
        source_says_complete: bool,
        marked: Option<OffsetDateTime>,
        last: Option<OffsetDateTime>,
    ) -> Reading<'a> {
        Reading {
            completion,
            source_says_complete,
            completion_marked_at: marked,
            last_activity: last,
        }
    }

    #[test]
    fn four_months_is_the_boundary_and_a_day_short_is_not() {
        let now = at("2026-11-24T00:00:00Z");
        assert_eq!(
            decide(reading("in_progress", false, None, Some(at("2026-07-27T00:00:00Z"))), now, 4),
            LifecycleAction::Abandon,
            "120 days is stale"
        );
        assert_eq!(
            decide(reading("in_progress", false, None, Some(at("2026-07-28T00:00:00Z"))), now, 4),
            LifecycleAction::None,
            "119 days is not"
        );
    }

    #[test]
    fn hiatus_is_swept_because_the_owner_chose_only_complete_as_the_exemption() {
        let now = at("2026-11-24T00:00:00Z");
        assert_eq!(
            decide(reading("hiatus", false, None, Some(at("2026-01-01T00:00:00Z"))), now, 4),
            LifecycleAction::Abandon
        );
    }

    #[test]
    fn an_update_after_the_mark_revives_and_one_before_it_does_not() {
        let now = at("2026-11-24T00:00:00Z");
        assert_eq!(
            decide(
                reading("abandoned", false, Some(at("2026-06-01T00:00:00Z")), Some(at("2026-10-01T00:00:00Z"))),
                now,
                4
            ),
            LifecycleAction::Revive
        );
        assert_eq!(
            decide(
                reading("abandoned", false, Some(at("2026-06-01T00:00:00Z")), Some(at("2026-05-01T00:00:00Z"))),
                now,
                4
            ),
            LifecycleAction::None
        );
    }

    #[test]
    fn a_source_that_says_complete_concludes_instead_of_abandoning() {
        let now = at("2026-11-24T00:00:00Z");
        assert_eq!(
            decide(reading("abandoned", true, Some(at("2026-06-01T00:00:00Z")), Some(at("2026-01-01T00:00:00Z"))), now, 4),
            LifecycleAction::Conclude
        );
        assert_eq!(
            decide(reading("in_progress", true, None, Some(at("2026-01-01T00:00:00Z"))), now, 4),
            LifecycleAction::Conclude
        );
    }

    #[test]
    fn no_clock_is_unmeasured_not_abandoned() {
        let now = at("2026-11-24T00:00:00Z");
        assert_eq!(
            decide(reading("in_progress", false, None, None), now, 4),
            LifecycleAction::Unmeasured
        );
    }

    #[test]
    fn all_three_stored_shapes_parse() {
        assert_eq!(
            parse_activity_timestamp("2026-08-01T00:00:05").unwrap(),
            at("2026-08-01T00:00:05Z")
        );
        assert_eq!(
            parse_activity_timestamp("2001-08-24T00:00:00Z").unwrap(),
            at("2001-08-24T00:00:00Z")
        );
        assert_eq!(
            parse_activity_timestamp("2026-09-20T08:42:00.771205161Z").unwrap(),
            at("2026-09-20T08:42:00.771205161Z")
        );
    }
}
```

Verification:

```bash
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && \
  env -u DATABASE_URL -u DARKHOLD_TEST_PG_URL \
  cargo test -p darkhold-domain work_lifecycle --no-fail-fast'
```

Expected: 7 passed, 0 failed. Then break the four-month test by changing
`months * 30` to `months * 31`; it must go red. Restore.

## Step 3 — Store module, both engines

File: `crates/db/src/work_lifecycle.rs` (new), and `pub mod work_lifecycle;`
in `crates/db/src/lib.rs` beside `pub mod wip_adoption;`.

### 3.1 The candidate query

```rust
use crate::Database;
use darkhold_domain::work_lifecycle::CompletionSource;

/// One row the sweep is allowed to judge.
pub struct Candidate {
    pub work_id: String,
    pub completion: String,
    pub completion_source: String,
    pub completion_marked_at: Option<String>,
    pub published_at: Option<String>,
    pub last_event: Option<String>,
    pub last_source: Option<String>,
    pub source_says_complete: bool,
}

const CANDIDATES_SQLITE: &str = "
WITH ev AS (
  SELECT work_id, MAX(occurred_at) AS last_event
    FROM publication_events
   WHERE action IN ('publish', 'update', 'restore_revision')
   GROUP BY work_id
),
src AS (
  SELECT work_id, MAX(source_updated_at) AS last_source
    FROM library_items
   WHERE source_updated_at IS NOT NULL
   GROUP BY work_id
),
has_lib AS (
  SELECT DISTINCT work_id FROM library_items
),
done AS (
  SELECT DISTINCT work_id FROM library_items WHERE status = 'complete'
)
SELECT w.id,
       w.completion,
       w.completion_source,
       w.completion_marked_at,
       w.published_at,
       CASE WHEN hl.work_id IS NOT NULL THEN NULL ELSE ev.last_event END AS last_event,
       src.last_source,
       CAST(CASE WHEN done.work_id IS NULL THEN 0 ELSE 1 END AS INTEGER) AS source_says_complete
  FROM works w
  LEFT JOIN ev      ON ev.work_id = w.id
  LEFT JOIN src     ON src.work_id = w.id
  LEFT JOIN has_lib hl ON hl.work_id = w.id
  LEFT JOIN done    ON done.work_id = w.id
 WHERE w.lifecycle = 'published'
   AND w.deleted_at IS NULL
   AND w.completion IN ('in_progress', 'hiatus', 'abandoned')
   AND w.id > COALESCE(?1, '')
 ORDER BY w.id
 LIMIT ?2";

const CANDIDATES_POSTGRES: &str = "
WITH ev AS (
  SELECT work_id, MAX(occurred_at) AS last_event
    FROM publication_events
   WHERE action IN ('publish', 'update', 'restore_revision')
   GROUP BY work_id
),
src AS (
  SELECT work_id, MAX(source_updated_at) AS last_source
    FROM library_items
   WHERE source_updated_at IS NOT NULL
   GROUP BY work_id
),
has_lib AS (
  SELECT DISTINCT work_id FROM library_items
),
done AS (
  SELECT DISTINCT work_id FROM library_items WHERE status = 'complete'
)
SELECT w.id::text,
       w.completion,
       w.completion_source,
       w.completion_marked_at,
       w.published_at,
       CASE WHEN hl.work_id IS NOT NULL THEN NULL ELSE ev.last_event END AS last_event,
       src.last_source,
       CAST(CASE WHEN done.work_id IS NULL THEN 0 ELSE 1 END AS BIGINT) AS source_says_complete
  FROM works w
  LEFT JOIN ev      ON ev.work_id = w.id
  LEFT JOIN src     ON src.work_id = w.id
  LEFT JOIN has_lib hl ON hl.work_id = w.id
  LEFT JOIN done    ON done.work_id = w.id
 WHERE w.lifecycle = 'published'
   AND w.deleted_at IS NULL
   AND w.completion IN ('in_progress', 'hiatus', 'abandoned')
   AND w.id > COALESCE(?1::uuid, '00000000-0000-0000-0000-000000000000'::uuid)
 ORDER BY w.id
 LIMIT ?2";

pub async fn candidates(
    db: &Database,
    after: Option<&str>,
    limit: i64,
) -> Result<Vec<Candidate>, sqlx::Error> {
    let sql = db.sql(CANDIDATES_SQLITE, CANDIDATES_POSTGRES);
    let rows: Vec<(String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, i64)> =
        match db.backend() {
            crate::Backend::Sqlite => sqlx::query_as(&sql)
                .bind(after)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?,
            crate::Backend::Postgres => sqlx::query_as(&sql)
                .bind(after)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?,
        };
    Ok(rows
        .into_iter()
        .map(|(work_id, completion, completion_source, completion_marked_at, published_at, last_event, last_source, source_says_complete)| Candidate {
            work_id,
            completion,
            completion_source,
            completion_marked_at,
            published_at,
            last_event,
            last_source,
            source_says_complete: source_says_complete != 0,
        })
        .collect())
}
```

The `MAX` is over each clock separately and the maximum is taken in Rust. Do
not take a SQL `GREATEST` of the three: `published_at` is naive, `occurred_at`
has nanoseconds, and a textual `GREATEST` would order them by shape, not by
instant.

**The `has_lib` case is the load-bearing part.** A work with any `library_items`
row is imported, and its `publication_events.publish` carries an import date
(2026-09-20 to 2026-10-27), not the author's. The `CASE` nulls `last_event` for
those works so `source_updated_at` is the clock. A hosted work (no library item)
keeps the event clock. The first version of this query counted events for every
work and reported zero eligible works on the live database — the rule looked
implemented and did nothing.

### 3.2 The conditional transition, in one transaction

```rust
/// Apply one transition and write its history row atomically.
///
/// `from` is part of the WHERE clause, not a check in Rust: the author can
/// change completion between the sweep's SELECT and this UPDATE, and a
/// zero-row result is the correct answer to that race, not an error.
#[allow(clippy::too_many_arguments)]
pub async fn apply_transition(
    db: &Database,
    work_id: &str,
    from: &str,
    to: &str,
    source: CompletionSource,
    actor_account_id: Option<&str>,
    note: Option<&str>,
    observed_last_activity: Option<&str>,
    now: &str,
) -> Result<bool, sqlx::Error> {
    let update = db.sql(
        "UPDATE works
            SET completion = ?1,
                completion_source = ?2,
                completion_marked_at = ?3
          WHERE id = ?4 AND completion = ?5 AND deleted_at IS NULL",
        "UPDATE works
            SET completion = ?1,
                completion_source = ?2,
                completion_marked_at = ?3
          WHERE id = ?4::uuid AND completion = ?5 AND deleted_at IS NULL",
    );
    let insert = db.sql(
        "INSERT INTO work_completion_marks
             (id, work_id, from_completion, to_completion, source,
              actor_account_id, note, observed_last_activity, marked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        "INSERT INTO work_completion_marks
             (id, work_id, from_completion, to_completion, source,
              actor_account_id, note, observed_last_activity, marked_at)
         VALUES (?1::uuid, ?2::uuid, ?3, ?4, ?5, ?6::uuid, ?7, ?8, ?9)",
    );
    let mark_id = uuid::Uuid::new_v4().to_string();

    match db.backend() {
        crate::Backend::Sqlite => {
            let mut tx = db.sqlite_pool().expect("sqlite handle").begin().await?;
            let changed = sqlx::query(&update)
                .bind(to)
                .bind(source.as_str())
                .bind(now)
                .bind(work_id)
                .bind(from)
                .execute(&mut *tx)
                .await?
                .rows_affected()
                > 0;
            if changed {
                sqlx::query(&insert)
                    .bind(&mark_id)
                    .bind(work_id)
                    .bind(from)
                    .bind(to)
                    .bind(source.as_str())
                    .bind(actor_account_id)
                    .bind(note)
                    .bind(observed_last_activity)
                    .bind(now)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            Ok(changed)
        }
        crate::Backend::Postgres => {
            let mut tx = db.postgres_pool().expect("postgres handle").begin().await?;
            let changed = sqlx::query(&update)
                .bind(to)
                .bind(source.as_str())
                .bind(now)
                .bind(work_id)
                .bind(from)
                .execute(&mut *tx)
                .await?
                .rows_affected()
                > 0;
            if changed {
                sqlx::query(&insert)
                    .bind(&mark_id)
                    .bind(work_id)
                    .bind(from)
                    .bind(to)
                    .bind(source.as_str())
                    .bind(actor_account_id)
                    .bind(note)
                    .bind(observed_last_activity)
                    .bind(now)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            Ok(changed)
        }
    }
}
```

`completion_marked_at = now` is written even for a reviewer's completion; the
mark time is the comparison point if the work is ever abandoned later.

### 3.3 Authority

```rust
/// Whether an account may set completion on a work it does not own.
///
/// `trust_for` is the earned level; `accounts.is_curator` is the explicit
/// instance grant. `curator_roles` is deliberately NOT read: it is the
/// media-resilience opt-in (§32.7.5), not an editorial power.
pub async fn may_review_completion(
    db: &Database,
    account: &str,
    min_trust: i64,
) -> Result<bool, sqlx::Error> {
    if crate::governance::trust_for(db, account).await? >= min_trust {
        return Ok(true);
    }
    let sql = db.sql(
        "SELECT CAST(CASE WHEN COALESCE(is_curator, 0) THEN 1 ELSE 0 END AS INTEGER)
           FROM accounts WHERE id = ?",
        "SELECT CAST(CASE WHEN COALESCE(is_curator, false) THEN 1 ELSE 0 END AS BIGINT)
           FROM accounts WHERE id = ?::uuid",
    );
    let row: Option<(i64,)> = match db.backend() {
        crate::Backend::Sqlite => sqlx::query_as(&sql)
            .bind(account)
            .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
            .await?,
        crate::Backend::Postgres => sqlx::query_as(&sql)
            .bind(account)
            .fetch_optional(db.postgres_pool().expect("postgres handle"))
            .await?,
    };
    Ok(row.is_some_and(|(value,)| value != 0))
}
```

### 3.4 Store tests

File: `crates/db/tests/work_lifecycle.rs`. Use the repo's test harness
(`test_support::TestDb`) and seed:

1. A work with `completion='in_progress'`, no publication update, one
   `library_items` row with `source_updated_at='2026-01-01T00:00:00Z'` and
   `status='ongoing'`; expect the candidate's `last_source` to be that value and
   `last_event` to be `None` (the import event must not mask the source clock).
2. A work with **no library item** and only a `publication_events` `update`
   row; expect `last_event` to be that value.
3. A work with only `published_at='2025-01-01T00:00:00'`; expect it parses as
   UTC and is stale.
4. A work with `library_items.status='complete'`; expect
   `source_says_complete == true`.
5. A work with two library items; expect the later source date wins.
6. `apply_transition` with `from='in_progress'`, then a second call with the
   same `from`; first returns true, second false, and exactly one mark row
   exists.
7. A PostgreSQL-only assertion that `actor_account_id` round-trips as a UUID
   and `work_id` is not truncated.

Verification:

```bash
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && \
  env -u DATABASE_URL -u DARKHOLD_TEST_PG_URL \
  cargo test -p darkhold-db --test work_lifecycle --no-fail-fast'
DARKHOLD_TEST_PG_URL=postgres://darkhold:darkhold@127.0.0.1:55433/darkhold \
  cargo test -p darkhold-db --test work_lifecycle --no-fail-fast
```

Expected: 7 passed on each engine. If the PostgreSQL run cannot connect, the
tunnel is not up; do not call the test passed.

## Step 4 — The sweep and the operator door

### 4.1 App module

File: `crates/app/src/work_lifecycle.rs` (new), declared in
`crates/app/src/lib.rs` (or wherever the other modules such as `rating_watch`
are declared).

```rust
use darkhold_domain::work_lifecycle::{
    decide, parse_activity_timestamp, CompletionSource, LifecycleAction, Reading,
};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, serde::Serialize)]
pub struct SweepReport {
    pub scanned: i64,
    pub abandoned: i64,
    pub revived: i64,
    pub concluded: i64,
    pub raced: i64,
    pub unmeasured: i64,
    pub next_cursor: Option<String>,
    pub done: bool,
    pub dry_run: bool,
}

pub async fn sweep_page(
    state: &crate::AppState,
    cursor: Option<String>,
    limit: i64,
    dry_run: bool,
) -> Result<SweepReport, darkhold_domain::AppError> {
    let limit = limit.clamp(1, 500);
    let candidates =
        darkhold_db::work_lifecycle::candidates(state.db(), cursor.as_deref(), limit)
            .await
            .map_err(|e| darkhold_domain::AppError::Internal(e.into()))?;
    let now = OffsetDateTime::now_utc();
    let now_text = now.format(&Rfc3339).expect("UTC formats as RFC 3339");
    let months = state.config().works.auto_abandon_after_months;

    let mut report = SweepReport {
        scanned: candidates.len() as i64,
        abandoned: 0,
        revived: 0,
        concluded: 0,
        raced: 0,
        unmeasured: 0,
        next_cursor: None,
        done: candidates.len() < usize::try_from(limit).unwrap_or(0),
        dry_run,
    };

    for candidate in &candidates {
        let last = [&candidate.last_event, &candidate.last_source, &candidate.published_at]
            .into_iter()
            .flatten()
            .filter_map(|raw| parse_activity_timestamp(raw))
            .max();
        let marked = candidate
            .completion_marked_at
            .as_deref()
            .and_then(parse_activity_timestamp);
        let action = decide(
            Reading {
                completion: &candidate.completion,
                source_says_complete: candidate.source_says_complete,
                completion_marked_at: marked,
                last_activity: last,
            },
            now,
            months,
        );
        let observed = last.map(|value| value.format(&Rfc3339).expect("RFC 3339"));

        let (to, source) = match action {
            LifecycleAction::None => continue,
            LifecycleAction::Unmeasured => {
                report.unmeasured += 1;
                continue;
            }
            LifecycleAction::Abandon => ("abandoned", CompletionSource::Sweep),
            LifecycleAction::Revive => ("in_progress", CompletionSource::Update),
            LifecycleAction::Conclude => ("complete", CompletionSource::Update),
        };

        if dry_run {
            match action {
                LifecycleAction::Abandon => report.abandoned += 1,
                LifecycleAction::Revive => report.revived += 1,
                LifecycleAction::Conclude => report.concluded += 1,
                LifecycleAction::None | LifecycleAction::Unmeasured => {}
            }
            continue;
        }

        let changed = darkhold_db::work_lifecycle::apply_transition(
            state.db(),
            &candidate.work_id,
            &candidate.completion,
            to,
            source,
            None,
            None,
            observed.as_deref(),
            &now_text,
        )
        .await
        .map_err(|e| darkhold_domain::AppError::Internal(e.into()))?;
        if changed {
            match action {
                LifecycleAction::Abandon => report.abandoned += 1,
                LifecycleAction::Revive => report.revived += 1,
                LifecycleAction::Conclude => report.concluded += 1,
                LifecycleAction::None | LifecycleAction::Unmeasured => {}
            }
        } else {
            report.raced += 1;
        }
    }

    if !report.done {
        report.next_cursor = candidates.last().map(|candidate| candidate.work_id.clone());
    }
    Ok(report)
}
```

`auto_abandon = false` is checked by the route, not inside the function, so the
operator's explicit `POST` can still run a dry run while writes are off.

### 4.2 Route

File: `crates/app/src/routes/work_lifecycle.rs` (new), plus a mount in
`crates/app/src/server.rs` beside `routes::works::router()`, and a
`RouteEntry` in `crates/app/src/routes/inventory.rs`.

```rust
#[derive(Debug, serde::Deserialize)]
pub struct SweepBody {
    #[serde(default = "default_true")]
    pub dry_run: bool,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub cursor: Option<String>,
}

fn default_true() -> bool { true }
fn default_limit() -> i64 { 500 }

pub async fn sweep(
    RequireSession(user): RequireSession,
    State(state): State<crate::AppState>,
    Json(body): Json<SweepBody>,
) -> crate::ApiResult<Json<crate::work_lifecycle::SweepReport>> {
    crate::routes::discovery::require_operator(&state, &user)?;
    if !body.dry_run && !state.config().works.auto_abandon {
        return Err(crate::ApiError(darkhold_domain::AppError::field(
            "dry_run",
            "automatic abandonment is disabled by [works].auto_abandon = false; \
             a write would contradict the configured state",
        )));
    }
    let report = crate::work_lifecycle::sweep_page(&state, body.cursor.clone(), body.limit, body.dry_run)
        .await?;
    Ok(Json(report))
}
```

Mount `POST /api/v1/lifecycle/sweep` (not under `/operator`; that path is a nest
whose subtree swallowing is already recorded in `docs/known-gaps.md`). Route
inventory entry: `Audience::AdminOf { site: None }`, method `POST`, path
`/api/v1/lifecycle/sweep`.

### 4.3 CLI

File: `crates/cli/src/lifecycle.rs` (new), command enum entry in
`crates/cli/src/main.rs`.

```
dh lifecycle sweep [--apply] [--all] [--limit 500]
```

- No `--apply` means `dry_run: true`; the command prints the report.
- `--all` loops with `cursor` until `done`, printing one line per page.
- The route needs an operator session: the existing `DARKHOLD_COOKIE` is used
  exactly as `dh supply recompute` uses it.
- Exit codes: `0` when the last page is `done`; `2` for a usage error; `1` for
  a refused or unreachable route. Use the CLI's existing `Failure`/`Exit`
  helpers rather than a new convention.

Verification:

```bash
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && \
  env -u DARKHOLD_BASE_URL -u DARKHOLD_TOKEN -u DARKHOLD_COOKIE \
  cargo test -p darkhold-app --test m45_76_work_lifecycle --no-fail-fast'
```

Expected: the route test has not been written yet; this command is the step
after 4.2. It should fail to compile with "no test target" until Step 6.

## Step 5 — Reviewer completion route and owner provenance

### 5.1 Reviewer route

Add to `crates/app/src/routes/works.rs` (it already has `parse_work_id` and the
work lookup) or to the new `work_lifecycle.rs` route module. Prefer the new
module and reuse `parse_work_id` via `crate::routes::works::parse_work_id`.

```rust
#[derive(Debug, serde::Deserialize)]
pub struct CompletionBody {
    pub completion: String,
    #[serde(default)]
    pub note: Option<String>,
}

pub async fn set_completion(
    RequireSession(user): RequireSession,
    State(state): State<crate::AppState>,
    Path(id): Path<String>,
    Json(body): Json<CompletionBody>,
) -> crate::ApiResult<Json<serde_json::Value>> {
    let work_id = crate::routes::works::parse_work_id(&id)?;
    let Some(work) = darkhold_db::content::find_work(state.db(), work_id)
        .await?
    else {
        return Err(crate::ApiError(darkhold_domain::AppError::NotFound { resource: "work" }));
    };
    if work.lifecycle != "published" {
        return Err(crate::ApiError(darkhold_domain::AppError::field(
            "completion",
            "only a published work carries a completion state",
        )));
    }
    if !matches!(body.completion.as_str(), "complete" | "in_progress") {
        return Err(crate::ApiError(darkhold_domain::AppError::field(
            "completion",
            "a reviewer sets complete or in_progress; abandoned and hiatus are the author's states",
        )));
    }
    let account = user.account_id.to_string();
    let may = darkhold_db::work_lifecycle::may_review_completion(
        state.db(),
        &account,
        state.config().works.completion_review_min_trust,
    )
    .await
    .map_err(|e| darkhold_domain::AppError::Internal(e.into()))?;
    if !may {
        return Err(crate::ApiError(darkhold_domain::AppError::AccessDenied));
    }
    if body.completion == work.completion {
        return Ok(Json(serde_json::json!({
            "work_id": work.id.to_string(),
            "completion": work.completion,
            "changed": false,
        })));
    }
    let now = darkhold_db::identity::now_rfc3339();
    let changed = darkhold_db::work_lifecycle::apply_transition(
        state.db(),
        &work.id.to_string(),
        &work.completion,
        &body.completion,
        darkhold_domain::work_lifecycle::CompletionSource::Review,
        Some(&account),
        body.note.as_deref(),
        None,
        &now,
    )
    .await
    .map_err(|e| darkhold_domain::AppError::Internal(e.into()))?;
    if !changed {
        // The author (or another reviewer) moved it between the read and the
        // conditional update. Answer the state that is actually stored rather
        // than inventing a version the caller never sent.
        let current = darkhold_db::content::find_work(state.db(), work_id)
            .await?
            .ok_or_else(|| crate::ApiError(darkhold_domain::AppError::NotFound { resource: "work" }))?;
        return Ok(Json(serde_json::json!({
            "work_id": current.id.to_string(),
            "completion": current.completion,
            "changed": false,
        })));
    }
    Ok(Json(serde_json::json!({
        "work_id": work.id.to_string(),
        "completion": body.completion,
        "changed": true,
    })))
}
```

Route inventory: `POST /api/v1/works/{id}/completion`, `Audience::Authenticated`.

### 5.2 Owner provenance in `update_work`

In `crates/db/src/content.rs::update_work`, add to both `SET` clauses:

```sql
completion_source = CASE WHEN ? IS NULL THEN completion_source ELSE 'owner' END,
completion_marked_at = CASE WHEN ? IS NULL THEN completion_marked_at ELSE ? END,
```

and bind `patch.completion`, `patch.completion`, `&now` before the `updated_at`
bind. Then, in `crates/app/src/routes/works.rs::update_work`, after the existing
completion/resurrection event block, write the history row best-effort:

```rust
if let Some(completion) = &patch.completion {
    if *completion != work.completion {
        let from = work.completion.clone();
        let to = completion.clone();
        let work_id = work.id.to_string();
        let account = user.account_id.to_string();
        if let Err(error) = darkhold_db::work_lifecycle::record_owner_mark(
            state.db(),
            &work_id,
            &from,
            &to,
            &account,
            &darkhold_db::identity::now_rfc3339(),
        )
        .await
        {
            tracing::warn!(work = %work_id, %error, "failed to record owner completion mark");
        }
    }
}
```

`record_owner_mark` is a two-line INSERT helper over
`work_completion_marks`; it is best-effort because the columns written in the
same `UPDATE` are the authoritative provenance. If the mark row is lost, the
state is still correct; if the state write is lost, the route returns a conflict
and no mark is written.

## Step 6 — Tests

File: `crates/app/tests/m45_76_work_lifecycle.rs`.

Seed at least six works in one test database. Use the existing test harness and
the `operator()` helper; assert through HTTP, not by calling store functions.

Test list:

1. `a_stale_in_progress_work_is_abandoned_by_a_write_sweep` — seed
   `source_updated_at = now - 121 days`, run `POST /api/v1/lifecycle/sweep`
   with `{"dry_run":false}`, expect `abandoned:1` and the row
   `completion='abandoned'`, `completion_source='sweep'`.
2. `a_dry_run_writes_nothing` — same seed, `{"dry_run":true}`, expect
   `abandoned:1` but the row is still `in_progress` and
   `work_completion_marks` is empty.
3. `a_complete_work_is_never_swept` — expect `abandoned:0`.
4. `a_source_complete_work_is_concluded` — `library_items.status='complete'`,
   `completion='in_progress'`, stale date; expect `concluded:1` and
   `completion='complete'`.
5. `a_hiatus_work_is_swept` — expect `abandoned:1`.
6. `a_new_source_date_revives_an_abandoned_work` — seed abandoned with
   `completion_marked_at = now - 10 days`, `source_updated_at = now - 1 day`;
   expect `revived:1`, `completion='in_progress'`.
7. `a_metadata_edit_does_not_revive` — abandoned with a mark older than
   `updated_at`, but no new source date; expect `revived:0`.
8. `tl3_may_complete_a_work_it_does_not_own` — `POST
   /api/v1/works/{id}/completion`, expect 200 and `completion_source='review'`.
9. `tl2_is_refused_with_the_bar_named` — expect 403; the body names `3`.
10. `an_explicit_curator_at_tl0_may_complete` — set `accounts.is_curator =
    true`, expect 200.
11. `a_draft_is_refused` — expect 422.
12. `a_reviewer_cannot_set_abandoned` — expect 422.
13. `the_second_identical_completion_is_idempotent` — expect `changed:false`
    and one mark row.

Run both engines:

```bash
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && \
  env -u DATABASE_URL -u DARKHOLD_TEST_PG_URL \
  cargo test -p darkhold-app --test m45_76_work_lifecycle --no-fail-fast'
DARKHOLD_TEST_PG_URL=postgres://darkhold:darkhold@127.0.0.1:55433/darkhold \
  cargo test -p darkhold-app --test m45_76_work_lifecycle --no-fail-fast
```

Expected: 13 passed on SQLite, 13 passed on PostgreSQL, 0 failed.

Mutation checks, each from a `/tmp` copy (never `git checkout` in a dirty tree):

- Remove `source_says_complete` from `decide`; test 4 goes red.
- Change `completion IN ('in_progress','hiatus','abandoned')` to only
  `in_progress`; test 5 goes red.
- Remove `AND completion = ?5` from the update; the race test goes red.
- Remove `last > marked` in `decide`; test 6 goes red.
- Remove the `is_curator` branch; test 10 goes red.

## Step 7 — Config and deployment

Files:

- `crates/app/src/config.rs`: add to `WorksConfig`:
  `auto_abandon: bool`, `auto_abandon_after_months: u32`,
  `completion_review_min_trust: i64`; add the three `Option` fields to
  `WorksSection`; parse them in the `let works = WorksConfig { ... }` block.
  Defaults: `true`, `4`, `3`. Refuse `auto_abandon_after_months == 0` at load
  with a clear message naming `auto_abandon = false`.
- `docs/config-reference.md`: add a `[works]` subsection (or extend the existing
  one) with the three keys and their defaults. The config test
  `config_sections` must stay green.
- `deploy/darkhold-lifecycle.service` and `.timer`:

```ini
[Unit]
Description=Darkhold automatic WIP/abandoned lifecycle sweep
After=network-online.target postgresql.service

[Service]
Type=oneshot
EnvironmentFile=/personal/secrets/estate/darkhold.env
WorkingDirectory=/personal/documents/code/rust/darkhold
ExecStart=/personal/documents/code/rust/darkhold/target/release/dh lifecycle sweep --apply --all
```

```ini
[Unit]
Description=Run the Darkhold lifecycle sweep daily

[Timer]
OnCalendar=*-*-* 04:45:00
Persistent=true
Unit=darkhold-lifecycle.service

[Install]
WantedBy=timers.target
```

Mirror `darkhold-archive-bodies.service` for the binary path and environment
file. **Do not enable the timer on the first deploy.** Run the dry-run route
first, read `scanned/abandoned/revived/unmeasured`, then enable:

```bash
ssh thinkcentre 'sudo systemctl daemon-reload && sudo systemctl enable --now darkhold-lifecycle.timer'
```

Expected on a dry run against the live database: 9,722 eligible
`in_progress` works, 30,570 `unmeasured`, and zero writes.

## Step 8 — UI provenance

Files:

- `frontend/src/lib/api.ts`: add `completion_source?: string` and
  `completion_marked_at?: string | null` to `WorkSummary` and the work-view
  types that already carry `completion`.
- `frontend/src/lib/components/WorkCard.svelte`: when
  `work.completion === 'abandoned' && work.completion_source === 'sweep'`, the
  `MetadataChip` label becomes `Abandoned · no update in 4 months` (read the
  configured number from the API, not a hard-coded four). Otherwise keep
  `Abandoned`.
- `frontend/src/routes/WorkPage.svelte`: the same qualifier in the header.

No new component and no new design token: this is one label and one optional
tooltip on the existing chip.

Verification:

```bash
cd frontend && node node_modules/vite/bin/vite.js build
node --test src/lib/components/WorkCard.test.ts
```

Expected: build succeeds; the WorkCard test asserts the qualifier for
`completion_source='sweep'` and the plain label for `completion_source='owner'`.

## Definition of done

- Migration 0218 applies cleanly on both engines.
- Domain 7 tests, db 7 tests, app 13 tests — each on SQLite and PostgreSQL.
- `route_inventory` 5/5 with the two new rows.
- `config_sections` green with the three new keys.
- Mutation checks above are red before restore.
- Live dry run reports 9,722 eligible / 30,570 unmeasured and writes nothing.
- The timer is installed but **disabled** until the operator has read the dry
  run and run one bounded `--apply` batch by hand.
- The `docs/spec-amendments/automatic-work-lifecycle.md` acceptance list is
  satisfied item by item, with the command and output recorded in the handoff.
