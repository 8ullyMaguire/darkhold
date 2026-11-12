//! GS-5's store: the eligible pool, the drawn panel, and the verdicts that close it.
//!
//! The rule lives in `darkhold_domain::jury` and knows nothing about databases: it takes candidates, exclusions, rules and a seed, and returns the
//! panel or refuses. This module is the seam — it reads the pool, writes the panel the rule chose, and records what each juror said. Keeping the two
//! apart is what makes "same seed, same panel" checkable in a unit test while the panel that actually decided something is still re-readable.
//!
//! Two decisions worth naming:
//!
//! * **The pool is `trust_levels` inner-joined to live, non-guest accounts.** A trust row outlives the account it describes, and a guest is a cookie
//!   with a pseudonym — seating either produces a jury that cannot vote or does not exist. The level is the stored one: recomputing it here would
//!   make the draw depend on when it ran.
//! * **A juror's verdict is written once.** `record_verdict` refuses a second answer from the same account; a jury whose members can revise after
//!   seeing each other's votes is not a jury, it is a poll. Changing a verdict is an operator act on `review_tasks`, not a silent rewrite.

use anyhow::Result;
use darkhold_domain::jury::{self, Juror, JuryRules};
use sqlx::Row;

use crate::{Backend, Database};

/// One account eligible to sit on a jury.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolMember {
    pub account: String,
    pub level: i64,
}

/// A drawn panel: who sits, what drew them, and what they said.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: String,
    pub task_id: String,
    pub seed: String,
    pub size: i64,
    pub level_floor: i64,
    pub drawn_at: String,
    pub appeal_of: Option<String>,
    pub members: Vec<PanelMember>,
}

#[derive(Debug, Clone)]
pub struct PanelMember {
    pub account: String,
    pub verdict: String,
    pub verdict_at: Option<String>,
}

impl Panel {
    /// Did every seated juror answer?
    pub fn closed(&self) -> bool {
        !self.members.is_empty() && self.members.iter().all(|m| !m.verdict.is_empty())
    }

    /// How many answered, and how they split. The counts are what a decision surface renders; the identities stay in `members` for the appeal.
    pub fn tally(&self) -> (i64, i64) {
        let uphold = self
            .members
            .iter()
            .filter(|m| m.verdict.eq_ignore_ascii_case("uphold"))
            .count() as i64;
        let overturn = self
            .members
            .iter()
            .filter(|m| m.verdict.eq_ignore_ascii_case("overturn"))
            .count() as i64;
        (uphold, overturn)
    }
}

/// Every account that may sit on a jury: active, not a guest, with a stored level at or above `min_level`, newest-levels first is irrelevant because
/// the draw sorts by id itself.
pub async fn pool(db: &Database) -> Result<Vec<PoolMember>> {
    // **No level filter here.** The rule filters by `min_level` and, when too few reach it, falls back to the top of the *actual* ladder (§14's
    // elasticity). A store that pre-filtered by the floor would hand the fallback an empty list, so the fallback could never fire — measured on
    // PostgreSQL: a pool of level-1 accounts drew `PoolTooSmall { available: 0 }` instead of the honest three-at-the-top answer.
    let sql = db.sql(
        "SELECT t.account, t.level FROM trust_levels t
           JOIN accounts a ON a.id = t.account
          WHERE a.is_guest = 0",
        // `trust_levels.level` is INT4 on PostgreSQL and `i64` on the Rust side: the cast is the difference, and without it every draw on PG panics
        // with "mismatched types; Rust type i64 (as SQL type INT8) is not compatible with SQL type INT4".
        "SELECT t.account, CAST(t.level AS BIGINT) FROM trust_levels t
           JOIN accounts a ON a.id::text = t.account
          WHERE a.is_guest = 0",
    );

    // Mapped inside each arm: `sqlx` rows are `SqliteRow` and `PgRow`, which do not unify, so a shared `Vec<row>` is impossible — the same reason
    // every other dialect-split query in this crate maps per arm.
    let members: Vec<PoolMember> = match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(&sql)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?;
            rows.into_iter()
                .map(|r| PoolMember {
                    account: r.get::<String, _>(0),
                    level: r.get::<i64, _>(1),
                })
                .collect()
        }
        Backend::Postgres => {
            let rows = sqlx::query(&sql)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?;
            rows.into_iter()
                .map(|r| PoolMember {
                    account: r.get::<String, _>(0),
                    level: r.get::<i64, _>(1),
                })
                .collect()
        }
    };
    Ok(members)
}

/// What a draw attempt produced. A refusal is an answer, not an error: the caller answers 422 with the numbers and writes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drawn {
    Panel { members: Vec<String> },
    TooSmall { available: usize, required: usize },
}

/// The seed a caller chose, as the number the rule shuffles with.
///
/// FNV-1a over the seed string, in the same shape `m66-04` uses for its daily pick: the *string* is what gets stored and shown, and this is the
/// one-way map from it to a shuffle key. A seed that is only a number would be a seed somebody can also read backwards from the panel.
#[must_use]
pub fn seed_key(seed: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in seed.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Draw a panel for `task_id` and persist it.
///
/// The draw is the domain's; this decides what goes in as candidates and exclusions and writes the answer down. `exclude` is the caller's exclusions
/// (the reporter and the reported) plus, on an appeal, the members of the panel being appealed — passed in rather than read here so the appeal rule is
/// visible at the call site instead of buried in the store.
pub async fn draw_and_save(
    db: &Database,
    panel_id: &str,
    task_id: &str,
    rules: JuryRules,
    exclude: &[String],
    seed: &str,
    appeal_of: Option<&str>,
) -> Result<Drawn> {
    let candidates: Vec<Juror> = pool(db)
        .await?
        .into_iter()
        .map(|member| Juror {
            account: member.account,
            level: member.level,
        })
        .collect();

    let borrowed: Vec<&str> = exclude.iter().map(String::as_str).collect();
    let members = match jury::draw(&candidates, &borrowed, rules, seed_key(seed)) {
        Ok(members) => members,
        Err(jury::JuryError::PoolTooSmall {
            available,
            required,
        }) => {
            // Nothing is written. A panel that could not be drawn must not leave a row behind, or the next attempt sees "already drawn" and the
            // report can never get one.
            return Ok(Drawn::TooSmall {
                available,
                required,
            });
        }
        Err(jury::JuryError::EmptyPanel) => {
            return Err(anyhow::anyhow!(
                "governance.jury_size is 0, so no jury can be drawn: set it in the instance config"
            ));
        }
    };

    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "INSERT INTO jury_panels (id, task_id, seed, size, level_floor, drawn_at, appeal_of)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        "INSERT INTO jury_panels (id, task_id, seed, size, level_floor, drawn_at, appeal_of)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(panel_id)
                .bind(task_id)
                .bind(seed)
                .bind(members.len() as i64)
                .bind(rules.min_level)
                .bind(&now)
                .bind(appeal_of)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(panel_id)
                .bind(task_id)
                .bind(seed)
                .bind(members.len() as i64)
                .bind(rules.min_level)
                .bind(&now)
                .bind(appeal_of)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }

    let members_sql = db.sql(
        "INSERT INTO jury_panel_members (panel_id, account, verdict) VALUES (?, ?, '')",
        "INSERT INTO jury_panel_members (panel_id, account, verdict) VALUES ($1, $2, '')",
    );
    for member in &members {
        match db.backend() {
            Backend::Sqlite => {
                sqlx::query(&members_sql)
                    .bind(panel_id)
                    .bind(member)
                    .execute(db.sqlite_pool().expect("sqlite handle"))
                    .await?;
            }
            Backend::Postgres => {
                sqlx::query(&members_sql)
                    .bind(panel_id)
                    .bind(member)
                    .execute(db.postgres_pool().expect("postgres handle"))
                    .await?;
            }
        };
    }

    Ok(Drawn::Panel { members })
}

/// The newest panel for a task, with its members and their verdicts.
pub async fn panel_for_task(db: &Database, task_id: &str) -> Result<Option<Panel>> {
    // **`drawn_at` alone can tie.** `now_rfc3339` is second precision, and an appeal is drawn seconds after the panel it replaces — sometimes in the
    // same second — so ordering by time alone returns the ORIGINAL panel and the appeal looks like it never happened. An appeal outranks the panel it
    // replaces explicitly, and ties inside the same kind fall back to id.
    let sql = db.sql(
        "SELECT id, task_id, seed, CAST(size AS BIGINT), CAST(level_floor AS BIGINT), drawn_at, appeal_of FROM jury_panels
          WHERE task_id = ?1
          ORDER BY (CASE WHEN appeal_of IS NULL THEN 0 ELSE 1 END) DESC, drawn_at DESC, id DESC LIMIT 1",
        "SELECT id, task_id, seed, CAST(size AS BIGINT), CAST(level_floor AS BIGINT), drawn_at, appeal_of FROM jury_panels
          WHERE task_id = $1
          ORDER BY (CASE WHEN appeal_of IS NULL THEN 0 ELSE 1 END) DESC, drawn_at DESC, id DESC LIMIT 1",
    );
    let found: Option<Panel> = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(task_id)
            .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .map(|row| Panel {
                id: row.get(0),
                task_id: row.get(1),
                seed: row.get(2),
                size: row.get(3),
                level_floor: row.get(4),
                drawn_at: row.get(5),
                appeal_of: row.get(6),
                members: Vec::new(),
            }),
        Backend::Postgres => sqlx::query(&sql)
            .bind(task_id)
            .fetch_optional(db.postgres_pool().expect("postgres handle"))
            .await?
            .map(|row| Panel {
                id: row.get(0),
                task_id: row.get(1),
                seed: row.get(2),
                size: row.get(3),
                level_floor: row.get(4),
                drawn_at: row.get(5),
                appeal_of: row.get(6),
                members: Vec::new(),
            }),
    };
    let Some(mut panel) = found else {
        return Ok(None);
    };
    panel.members = members(db, &panel.id).await?;
    Ok(Some(panel))
}

/// A panel's members, in the order they were seated (by account id, which is the order the draw produced).
pub async fn members(db: &Database, panel_id: &str) -> Result<Vec<PanelMember>> {
    let sql = db.sql(
        "SELECT account, verdict, verdict_at FROM jury_panel_members WHERE panel_id = ? ORDER BY account",
        "SELECT account, verdict, verdict_at FROM jury_panel_members WHERE panel_id = $1 ORDER BY account",
    );
    let seated: Vec<PanelMember> = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(panel_id)
            .fetch_all(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .into_iter()
            .map(|r| PanelMember {
                account: r.get(0),
                verdict: r.get(1),
                verdict_at: r.get(2),
            })
            .collect(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(panel_id)
            .fetch_all(db.postgres_pool().expect("postgres handle"))
            .await?
            .into_iter()
            .map(|r| PanelMember {
                account: r.get(0),
                verdict: r.get(1),
                verdict_at: r.get(2),
            })
            .collect(),
    };
    Ok(seated)
}

/// Record one juror's verdict. Returns `false` when the account is not seated on the panel **or** has already spoken: both are "this vote was not
/// counted", and the caller answers 404 for the first (it does not confirm the panel to a stranger) and 409 for the second.
pub async fn record_verdict(
    db: &Database,
    panel_id: &str,
    account: &str,
    verdict: &str,
) -> Result<bool> {
    if !matches!(verdict, "uphold" | "overturn") {
        return Ok(false);
    }
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "UPDATE jury_panel_members SET verdict = ?, verdict_at = ?
          WHERE panel_id = ? AND account = ? AND verdict = ''",
        "UPDATE jury_panel_members SET verdict = $1, verdict_at = $2
          WHERE panel_id = $3 AND account = $4 AND verdict = ''",
    );
    // `rows_affected` per arm: the two dialects' result types do not unify either.
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(verdict)
            .bind(&now)
            .bind(panel_id)
            .bind(account)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(verdict)
            .bind(&now)
            .bind(panel_id)
            .bind(account)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected == 1)
}

/// The panels an account sits on, newest first, with the task they belong to. A juror has to be able to find their assignment; nothing else tells
/// them a decision is waiting on them.
pub async fn panels_for_account(
    db: &Database,
    account: &str,
) -> Result<Vec<(String, String, String, Option<String>)>> {
    let sql = db.sql(
        "SELECT p.id, p.task_id, m.verdict, p.drawn_at FROM jury_panel_members m
           JOIN jury_panels p ON p.id = m.panel_id
          WHERE m.account = ? ORDER BY p.drawn_at DESC",
        "SELECT p.id, p.task_id, m.verdict, p.drawn_at FROM jury_panel_members m
           JOIN jury_panels p ON p.id = m.panel_id
          WHERE m.account = $1 ORDER BY p.drawn_at DESC",
    );
    let assignments = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(account)
            .fetch_all(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .into_iter()
            .map(|r| {
                (
                    r.get::<String, _>(0),
                    r.get::<String, _>(1),
                    r.get::<String, _>(2),
                    r.get::<Option<String>, _>(3),
                )
            })
            .collect(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(account)
            .fetch_all(db.postgres_pool().expect("postgres handle"))
            .await?
            .into_iter()
            .map(|r| {
                (
                    r.get::<String, _>(0),
                    r.get::<String, _>(1),
                    r.get::<String, _>(2),
                    r.get::<Option<String>, _>(3),
                )
            })
            .collect(),
    };
    Ok(assignments)
}

/// The reporter and the reported behind a review task, which are the draw's exclusions.
///
/// `None` means there is no such task — the route answers 404 rather than drawing a panel nobody asked for. A report whose subject is not an account
/// (a work, a comment) yields an empty reported string: the rule filters exclusions by comparing account ids, and an empty string matches nobody,
/// which is the right answer for a subject that is not a person.
pub async fn task_parties(db: &Database, task_id: &str) -> Result<Option<(String, String)>> {
    let sql = db.sql(
        "SELECT r.reporter, CASE WHEN r.subject_type = 'account' THEN r.subject_id ELSE '' END
           FROM review_tasks t JOIN reports r ON r.id = t.report_id
          WHERE t.id = ?",
        "SELECT r.reporter, CASE WHEN r.subject_type = 'account' THEN r.subject_id ELSE '' END
           FROM review_tasks t JOIN reports r ON r.id = t.report_id
          WHERE t.id = $1",
    );
    match db.backend() {
        Backend::Sqlite => Ok(sqlx::query(&sql)
            .bind(task_id)
            .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .map(|row| (row.get::<String, _>(0), row.get::<String, _>(1)))),
        Backend::Postgres => Ok(sqlx::query(&sql)
            .bind(task_id)
            .fetch_optional(db.postgres_pool().expect("postgres handle"))
            .await?
            .map(|row| (row.get::<String, _>(0), row.get::<String, _>(1)))),
    }
}

/// Whatever the panel decided, written onto the report's review task.
///
/// **A jury that votes into a void is the half-row trap.** The verdicts are recorded, but until this runs, nothing about the report changes: the task
/// stays undecided and the report stays open, so a panel could deliberate and the queue would never move. This is the write that makes the draw mean
/// something.
///
/// Majority decides; a tie does not. A tie can only happen on an even panel, which the rules do not draw (5, or 3 elastically), but config is a place
/// where an instance can make an even panel — and "the panel split, so the decision is undo" is the honest answer rather than letting the first vote
/// break it. `undecided` leaves the report open for a human, which is what a tie is for.
///
/// The outcome vocabulary is the one `review_tasks` already uses: the task's own `decide_task` door writes `uphold`/`overturn`, and this writes the
/// same words so the queue, the statement of reasons and the transparency report read one vocabulary rather than two.
pub async fn apply_panel_outcome(db: &Database, panel: &Panel) -> Result<Option<String>> {
    let (uphold, overturn) = panel.tally();
    let outcome = match uphold.cmp(&overturn) {
        std::cmp::Ordering::Greater => "uphold",
        std::cmp::Ordering::Less => "overturn",
        std::cmp::Ordering::Equal => "undecided",
    };

    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "UPDATE review_tasks SET outcome = ?, decided_at = ? WHERE id = ?",
        "UPDATE review_tasks SET outcome = $1, decided_at = $2 WHERE id = $3",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(outcome)
                .bind(&now)
                .bind(&panel.task_id)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(outcome)
                .bind(&now)
                .bind(&panel.task_id)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }

    // The report behind the task follows the decision. A tie leaves it `open`: the state a human needs to find it.
    if outcome != "undecided" {
        let report_sql = db.sql(
            "UPDATE reports SET state = 'resolved', resolved_at = ?, resolution = ?
              WHERE id = (SELECT report_id FROM review_tasks WHERE id = ?)",
            "UPDATE reports SET state = 'resolved', resolved_at = $1, resolution = $2
              WHERE id = (SELECT report_id FROM review_tasks WHERE id = $3)",
        );
        match db.backend() {
            Backend::Sqlite => {
                sqlx::query(&report_sql)
                    .bind(&now)
                    .bind(outcome)
                    .bind(&panel.task_id)
                    .execute(db.sqlite_pool().expect("sqlite handle"))
                    .await?;
            }
            Backend::Postgres => {
                sqlx::query(&report_sql)
                    .bind(&now)
                    .bind(outcome)
                    .bind(&panel.task_id)
                    .execute(db.postgres_pool().expect("postgres handle"))
                    .await?;
            }
        }
    }

    Ok(Some(outcome.to_owned()))
}
