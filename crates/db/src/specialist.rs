//! M45-33 — the store behind specialist matchmaking.
//!
//! Every statement here is scoped by the pseud it names: there is no "list all profiles" query, because the
//! only way to learn that someone offers beta reading is the candidate query for a skill and a fandom.
//!
//! **`set_state` is conditional in SQL.** Two specialists can press accept on the same request at the same
//! moment; `WHERE state = 'open'` plus `rows_affected == 1` is what makes one of them win and the other read
//! "already taken", rather than both writing a `matched` row that names different people.

use darkhold_domain::specialist::{Candidate, Capacity, Skill, State};

use crate::{sql_owned, Backend, Database, Result};

/// A declared profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub pseud_id: String,
    pub skills: Vec<Skill>,
    pub fandoms: Vec<String>,
    pub capacity: Capacity,
    pub note: String,
    pub updated_at: String,
}

/// A request for a specialist.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub id: String,
    pub work_id: String,
    pub requester_pseud_id: String,
    pub skill: Skill,
    pub fandom: String,
    pub brief: String,
    pub state: State,
    pub matched_pseud_id: Option<String>,
    pub created_at: String,
    pub matched_at: Option<String>,
    pub completed_at: Option<String>,
}

fn split_list(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

fn join_list(items: &[String]) -> String {
    items
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>()
        .join(",")
}

/// Write (or replace) a profile and its skills.
///
/// # Errors
/// As the store.
pub async fn upsert_profile(
    db: &Database,
    pseud_id: &str,
    skills: &[Skill],
    fandoms: &[String],
    capacity: Capacity,
    note: &str,
) -> Result<Profile> {
    let now = crate::identity::now_rfc3339();
    let fandoms_text = join_list(fandoms);
    let upsert = sql_owned(
        db,
        "INSERT INTO specialist_profiles (pseud_id, fandoms, capacity, note, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT (pseud_id) DO UPDATE SET fandoms = excluded.fandoms, \
             capacity = excluded.capacity, note = excluded.note, updated_at = excluded.updated_at"
            .to_owned(),
        "INSERT INTO specialist_profiles (pseud_id, fandoms, capacity, note, created_at, updated_at) \
         VALUES ($1::uuid, $2, $3, $4, $5, $6) \
         ON CONFLICT (pseud_id) DO UPDATE SET fandoms = excluded.fandoms, \
             capacity = excluded.capacity, note = excluded.note, updated_at = excluded.updated_at"
            .to_owned(),
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&upsert)
                .bind(pseud_id)
                .bind(&fandoms_text)
                .bind(capacity.as_str())
                .bind(note)
                .bind(&now)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&upsert)
                .bind(pseud_id)
                .bind(&fandoms_text)
                .bind(capacity.as_str())
                .bind(note)
                .bind(&now)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }

    // Skills are replaced wholesale: a profile is the set a person claims *now*, and a delta would let an
    // old claim survive an edit that the person believes replaced it.
    let delete = sql_owned(
        db,
        "DELETE FROM specialist_skills WHERE pseud_id = ?".to_owned(),
        "DELETE FROM specialist_skills WHERE pseud_id = $1::uuid".to_owned(),
    );
    let insert = sql_owned(
        db,
        "INSERT INTO specialist_skills (pseud_id, skill) VALUES (?, ?)".to_owned(),
        "INSERT INTO specialist_skills (pseud_id, skill) VALUES ($1::uuid, $2)".to_owned(),
    );
    match db.backend() {
        Backend::Sqlite => {
            let pool = db.sqlite_pool().expect("sqlite handle");
            sqlx::query(&delete).bind(pseud_id).execute(pool).await?;
            for skill in skills {
                sqlx::query(&insert)
                    .bind(pseud_id)
                    .bind(skill.as_str())
                    .execute(pool)
                    .await?;
            }
        }
        Backend::Postgres => {
            let pool = db.postgres_pool().expect("postgres handle");
            sqlx::query(&delete).bind(pseud_id).execute(pool).await?;
            for skill in skills {
                sqlx::query(&insert)
                    .bind(pseud_id)
                    .bind(skill.as_str())
                    .execute(pool)
                    .await?;
            }
        }
    }

    profile(db, pseud_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the specialist profile upsert left no row"))
}

/// The profile a pseud wrote, if they wrote one.
///
/// # Errors
/// As the store.
pub async fn profile(db: &Database, pseud_id: &str) -> Result<Option<Profile>> {
    let sql = sql_owned(
        db,
        "SELECT pseud_id, fandoms, capacity, note, updated_at FROM specialist_profiles WHERE pseud_id = ?"
            .to_owned(),
        "SELECT pseud_id::text AS pseud_id, fandoms, capacity, note, updated_at \
         FROM specialist_profiles WHERE pseud_id = $1::uuid"
            .to_owned(),
    );
    type Row5 = (String, String, String, String, String);
    let row: Option<Row5> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(pseud_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(pseud_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let Some((pseud_id, fandoms, capacity, note, updated_at)) = row else {
        return Ok(None);
    };
    let skills = skills_for(db, &pseud_id).await?;
    Ok(Some(Profile {
        pseud_id,
        skills,
        fandoms: split_list(&fandoms),
        capacity: Capacity::parse(&capacity).unwrap_or(Capacity::Closed),
        note,
        updated_at,
    }))
}

async fn skills_for(db: &Database, pseud_id: &str) -> Result<Vec<Skill>> {
    let sql = sql_owned(
        db,
        "SELECT skill FROM specialist_skills WHERE pseud_id = ? ORDER BY skill".to_owned(),
        "SELECT skill FROM specialist_skills WHERE pseud_id = $1::uuid ORDER BY skill".to_owned(),
    );
    let rows: Vec<(String,)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(pseud_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(pseud_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .filter_map(|(skill,)| Skill::parse(&skill))
        .collect())
}

/// Forget a profile and its skills.
///
/// # Errors
/// As the store.
pub async fn delete_profile(db: &Database, pseud_id: &str) -> Result<bool> {
    let sql = sql_owned(
        db,
        "DELETE FROM specialist_profiles WHERE pseud_id = ?".to_owned(),
        "DELETE FROM specialist_profiles WHERE pseud_id = $1::uuid".to_owned(),
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(pseud_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(pseud_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

/// Everyone open for `skill`, with their load.
///
/// Closed profiles are excluded in SQL: a candidate list that included them would invite a request that can
/// never be accepted.
///
/// # Errors
/// As the store.
pub async fn candidates(db: &Database, skill: Skill) -> Result<Vec<Candidate>> {
    let sql = sql_owned(
        db,
        "SELECT p.pseud_id AS pseud_id, p.fandoms AS fandoms, \
                (SELECT COUNT(*) FROM specialist_requests r \
                  WHERE r.matched_pseud_id = p.pseud_id AND r.state = 'matched') AS live_engagements, \
                (SELECT COUNT(*) FROM specialist_requests r \
                  WHERE r.matched_pseud_id = p.pseud_id AND r.state = 'completed') AS completed \
           FROM specialist_profiles p \
           JOIN specialist_skills s ON s.pseud_id = p.pseud_id AND s.skill = ? \
          WHERE p.capacity = 'open' \
          ORDER BY p.pseud_id"
            .to_owned(),
        "SELECT p.pseud_id::text AS pseud_id, p.fandoms AS fandoms, \
                (SELECT COUNT(*) FROM specialist_requests r \
                  WHERE r.matched_pseud_id = p.pseud_id AND r.state = 'matched')::bigint AS live_engagements, \
                (SELECT COUNT(*) FROM specialist_requests r \
                  WHERE r.matched_pseud_id = p.pseud_id AND r.state = 'completed')::bigint AS completed \
           FROM specialist_profiles p \
           JOIN specialist_skills s ON s.pseud_id = p.pseud_id AND s.skill = $1 \
          WHERE p.capacity = 'open' \
          ORDER BY p.pseud_id"
            .to_owned(),
    );
    let mapped = |pseud_id: String, fandoms: String, live: i64, completed: i64| Candidate {
        pseud_id,
        fandoms: split_list(&fandoms),
        live_engagements: live,
        completed,
    };
    Ok(match db.backend() {
        Backend::Sqlite => {
            let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(&sql)
                .bind(skill.as_str())
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?;
            rows.into_iter()
                .map(|(id, fandoms, live, completed)| mapped(id, fandoms, live, completed))
                .collect()
        }
        Backend::Postgres => {
            let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(&sql)
                .bind(skill.as_str())
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?;
            rows.into_iter()
                .map(|(id, fandoms, live, completed)| mapped(id, fandoms, live, completed))
                .collect()
        }
    })
}

/// Write a request.
///
/// # Errors
/// As the store.
pub async fn create_request(
    db: &Database,
    id: &str,
    work_id: &str,
    requester: &str,
    skill: Skill,
    fandom: &str,
    brief: &str,
) -> Result<Request> {
    let now = crate::identity::now_rfc3339();
    let sql = sql_owned(
        db,
        "INSERT INTO specialist_requests \
         (id, work_id, requester_pseud_id, skill, fandom, brief, state, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, 'open', ?)"
            .to_owned(),
        "INSERT INTO specialist_requests \
         (id, work_id, requester_pseud_id, skill, fandom, brief, state, created_at) \
         VALUES ($1::uuid, $2::uuid, $3::uuid, $4, $5, $6, 'open', $7)"
            .to_owned(),
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(id)
                .bind(work_id)
                .bind(requester)
                .bind(skill.as_str())
                .bind(fandom)
                .bind(brief)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(id)
                .bind(work_id)
                .bind(requester)
                .bind(skill.as_str())
                .bind(fandom)
                .bind(brief)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    request(db, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the specialist request insert left no row"))
}

/// One request by id.
///
/// # Errors
/// As the store.
pub async fn request(db: &Database, id: &str) -> Result<Option<Request>> {
    let rows = requests_where(db, "id = ?", "id = $1::uuid", id).await?;
    Ok(rows.into_iter().next())
}

/// The requests on one work, newest first.
///
/// # Errors
/// As the store.
pub async fn requests_for_work(db: &Database, work_id: &str) -> Result<Vec<Request>> {
    requests_where(db, "work_id = ?", "work_id = $1::uuid", work_id).await
}

/// The open requests for `skill`, newest first — what a specialist browsing can take.
///
/// # Errors
/// As the store.
pub async fn open_requests(db: &Database, skill: Skill) -> Result<Vec<Request>> {
    requests_where(
        db,
        "state = 'open' AND skill = ?",
        "state = 'open' AND skill = $1",
        skill.as_str(),
    )
    .await
}

const COLUMNS: &str =
    "id, work_id, requester_pseud_id, skill, fandom, brief, state, matched_pseud_id, \
                       created_at, matched_at, completed_at";

async fn requests_where(
    db: &Database,
    sqlite_where: &str,
    postgres_where: &str,
    bound: &str,
) -> Result<Vec<Request>> {
    let sql = sql_owned(
        db,
        format!(
            "SELECT id, work_id, requester_pseud_id, skill, fandom, brief, state, matched_pseud_id, \
                    created_at, matched_at, completed_at \
               FROM specialist_requests WHERE {sqlite_where} ORDER BY created_at DESC, id DESC"
        ),
        format!(
            "SELECT id::text AS id, work_id::text AS work_id, \
                    requester_pseud_id::text AS requester_pseud_id, skill, fandom, brief, state, \
                    matched_pseud_id::text AS matched_pseud_id, created_at, matched_at, completed_at \
               FROM specialist_requests WHERE {postgres_where} ORDER BY created_at DESC, id DESC"
        ),
    );
    let _ = COLUMNS;
    type Row11 = (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<Row11> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(bound)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(bound)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .filter_map(
            |(
                id,
                work_id,
                requester_pseud_id,
                skill,
                fandom,
                brief,
                state,
                matched_pseud_id,
                created_at,
                matched_at,
                completed_at,
            )| {
                Some(Request {
                    id,
                    work_id,
                    requester_pseud_id,
                    skill: Skill::parse(&skill)?,
                    fandom,
                    brief,
                    state: State::parse(&state)?,
                    matched_pseud_id,
                    created_at,
                    matched_at,
                    completed_at,
                })
            },
        )
        .collect())
}

/// Move a request from `from` to `to`, naming the pseud when the transition is a match.
///
/// Returns `Ok(false)` when the row was not in `from` — the other specialist got there first.
///
/// # Errors
/// As the store.
pub async fn set_state(
    db: &Database,
    id: &str,
    from: State,
    to: State,
    matched: Option<&str>,
) -> Result<bool> {
    let now = crate::identity::now_rfc3339();
    let (sqlite, postgres) = match to {
        State::Matched => (
            "UPDATE specialist_requests SET state = 'matched', matched_pseud_id = ?, matched_at = ? \
             WHERE id = ? AND state = ?"
                .to_owned(),
            "UPDATE specialist_requests SET state = 'matched', matched_pseud_id = $1::uuid, \
                    matched_at = $2 WHERE id = $3::uuid AND state = $4"
                .to_owned(),
        ),
        State::Completed => (
            "UPDATE specialist_requests SET state = 'completed', completed_at = ? \
             WHERE id = ? AND state = ?"
                .to_owned(),
            "UPDATE specialist_requests SET state = 'completed', completed_at = $1 \
             WHERE id = $2::uuid AND state = $3"
                .to_owned(),
        ),
        _ => (
            "UPDATE specialist_requests SET state = ? WHERE id = ? AND state = ?".to_owned(),
            "UPDATE specialist_requests SET state = $1 WHERE id = $2::uuid AND state = $3".to_owned(),
        ),
    };
    let sql = sql_owned(db, sqlite, postgres);
    let affected = match (db.backend(), to) {
        (Backend::Sqlite, State::Matched) => sqlx::query(&sql)
            .bind(matched)
            .bind(&now)
            .bind(id)
            .bind(from.as_str())
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        (Backend::Sqlite, State::Completed) => sqlx::query(&sql)
            .bind(&now)
            .bind(id)
            .bind(from.as_str())
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        (Backend::Sqlite, _) => sqlx::query(&sql)
            .bind(to.as_str())
            .bind(id)
            .bind(from.as_str())
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        (Backend::Postgres, State::Matched) => sqlx::query(&sql)
            .bind(matched)
            .bind(&now)
            .bind(id)
            .bind(from.as_str())
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
        (Backend::Postgres, State::Completed) => sqlx::query(&sql)
            .bind(&now)
            .bind(id)
            .bind(from.as_str())
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
        (Backend::Postgres, _) => sqlx::query(&sql)
            .bind(to.as_str())
            .bind(id)
            .bind(from.as_str())
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected == 1)
}
