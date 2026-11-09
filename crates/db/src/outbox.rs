//! The transactional outbox (spec §8.4, §10.1).
//!
//! Every side effect that must happen exactly once — a notification, a search
//! index update, a cache invalidation — is written as a row in the *same
//! transaction* as the change that caused it. The worker (Milestone 5) drains
//! this table later. Nothing in a transaction ever sends mail or calls out.
//!
//! Idempotency comes from `dedupe_key`: the unique index refuses a second row
//! with the same key, so a retried publication cannot notify twice, no matter
//! how many times the client retries (spec §8 acceptance).

use anyhow::{Context, Result};
use sqlx::FromRow;

use darkhold_domain::OutboxEventId;

use crate::identity::now_rfc3339;
use crate::{Backend, Database};

/// A queued side effect.
#[derive(Debug, Clone, FromRow)]
pub struct OutboxEvent {
    /// Identifier.
    pub id: String,
    /// What kind of side effect this is.
    pub topic: String,
    /// Its JSON payload.
    pub payload: String,
    /// How many times delivery has been attempted.
    pub attempts: i64,
    /// Creation time, RFC 3339.
    pub created_at: String,
}

/// Record a side effect to be delivered later.
///
/// `dedupe_key` makes the write idempotent: the second insert of the same key
/// changes nothing, and the caller is told so rather than being handed an
/// error it cannot act on.
pub async fn enqueue(
    db: &Database,
    topic: &str,
    payload: &str,
    dedupe_key: Option<&str>,
) -> Result<bool> {
    let id = OutboxEventId::new();
    let now = now_rfc3339();

    let sql = db.sql(
        "INSERT INTO outbox_events (id, topic, payload, dedupe_key, created_at, available_at, attempts)
         VALUES (?, ?, ?, ?, ?, ?, 0)
         ON CONFLICT DO NOTHING",
        "INSERT INTO outbox_events (id, topic, payload, dedupe_key, created_at, available_at, attempts)
         VALUES (?::uuid, ?, ?, ?, ?, ?, 0)
         ON CONFLICT (dedupe_key) DO NOTHING",
    );

    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(id.to_string())
            .bind(topic)
            .bind(payload)
            .bind(dedupe_key)
            .bind(&now)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await
            .context("enqueueing outbox event")?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(id.to_string())
            .bind(topic)
            .bind(payload)
            .bind(dedupe_key)
            .bind(&now)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await
            .context("enqueueing outbox event")?
            .rows_affected(),
    };

    Ok(affected > 0)
}

/// Undelivered events that are due.
pub async fn pending(db: &Database, limit: i64) -> Result<Vec<OutboxEvent>> {
    let sql = db.sql(
        "SELECT id, topic, payload, attempts, created_at FROM outbox_events
          WHERE delivered_at IS NULL AND claimed_at IS NULL AND available_at <= ?
          ORDER BY created_at ASC LIMIT ?",
        "SELECT id::text AS id, topic, payload, attempts, created_at FROM outbox_events
          WHERE delivered_at IS NULL AND claimed_at IS NULL AND available_at <= ?
          ORDER BY created_at ASC LIMIT ?",
    );

    let rows: Vec<OutboxEvent> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(now_rfc3339())
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(now_rfc3339())
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    Ok(rows)
}

/// The oldest undelivered events a worker **that can handle them** may take.
///
/// `pending` answers a different question: "what is waiting?", oldest first, whatever the topic. A worker that took
/// that batch could be handed a topic it has no handler for, and the honest response is to defer it — which is what
/// this estate did for months. `outbox_events` accumulated 40,664 rows for topics nothing consumes
/// (`work.published`, `publish.index`), every pass took the same oldest fifty, deferred all fifty, and a fresh
/// `comment.posted` sat behind 40k unclaimable rows: `attempts` stayed 0 and a webhook never fired even after the
/// producer existed. A queue ordered oldest-first is only fair if the worker can take what it is handed.
///
/// Empty `topics` means "nothing is claimable here" — not "everything", which is how a starved worker is built.
pub async fn claimable(db: &Database, topics: &[String], limit: i64) -> Result<Vec<OutboxEvent>> {
    if topics.is_empty() || limit <= 0 {
        return Ok(Vec::new());
    }

    // Postgres numbers its parameters and SQLite does not, and `now` is bound first, so Postgres topics start at
    // $2. Getting that off by one is a 500 at runtime and a green compile, which is why the gate exercises the
    // deployment and not only the unit tests.
    let statement = match db.backend() {
        Backend::Sqlite => format!(
            "SELECT id, topic, payload, attempts, created_at FROM outbox_events
              WHERE delivered_at IS NULL AND claimed_at IS NULL AND available_at <= ?
                AND topic IN ({})
              ORDER BY created_at ASC LIMIT ?",
            vec!["?"; topics.len()].join(", ")
        ),
        Backend::Postgres => format!(
            "SELECT id::text AS id, topic, payload, attempts, created_at FROM outbox_events
              WHERE delivered_at IS NULL AND claimed_at IS NULL AND available_at <= $1
                AND topic IN ({})
              ORDER BY created_at ASC LIMIT ${}",
            (2..=topics.len() + 1)
                .map(|index| format!("${index}"))
                .collect::<Vec<_>>()
                .join(", "),
            topics.len() + 2
        ),
    };

    let rows: Vec<OutboxEvent> = match db.backend() {
        Backend::Sqlite => {
            let mut query = sqlx::query_as::<_, OutboxEvent>(&statement);
            query = query.bind(now_rfc3339());
            for topic in topics {
                query = query.bind(topic.clone());
            }
            query
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            let mut query = sqlx::query_as::<_, OutboxEvent>(&statement);
            query = query.bind(now_rfc3339());
            for topic in topics {
                query = query.bind(topic.clone());
            }
            query
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows)
}

/// Mark an event delivered.
pub async fn mark_delivered(db: &Database, id: OutboxEventId) -> Result<()> {
    let sql = db.sql(
        "UPDATE outbox_events SET delivered_at = ?, claimed_at = NULL, attempts = attempts + 1
          WHERE id = ?",
        "UPDATE outbox_events SET delivered_at = ?, claimed_at = NULL, attempts = attempts + 1
          WHERE id::text = ?",
    );
    let now = now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&now)
                .bind(id.to_string())
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&now)
                .bind(id.to_string())
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(())
}

/// Record a failed delivery attempt and when to try again.
pub async fn mark_failed(
    db: &Database,
    id: OutboxEventId,
    error: &str,
    retry_at: &str,
) -> Result<()> {
    let sql = db.sql(
        "UPDATE outbox_events
            SET attempts = attempts + 1, claimed_at = NULL, available_at = ?, last_error = ?
          WHERE id = ?",
        "UPDATE outbox_events
            SET attempts = attempts + 1, claimed_at = NULL, available_at = ?, last_error = ?
          WHERE id::text = ?",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(retry_at)
                .bind(error)
                .bind(id.to_string())
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(retry_at)
                .bind(error)
                .bind(id.to_string())
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(())
}

/// How many events are still undelivered.
pub async fn undelivered_count(db: &Database) -> Result<i64> {
    let sql = "SELECT COUNT(*) FROM outbox_events WHERE delivered_at IS NULL";
    match db.backend() {
        Backend::Sqlite => Ok(sqlx::query_scalar(sql)
            .fetch_one(db.sqlite_pool().expect("sqlite handle"))
            .await?),
        Backend::Postgres => Ok(sqlx::query_scalar(sql)
            .fetch_one(db.postgres_pool().expect("postgres handle"))
            .await?),
    }
}

/// When the oldest undelivered event was created, if any are waiting.
///
/// The companion to `undelivered_count`, and the reason both exist: a count cannot tell "five just enqueued" from "five stuck
/// since yesterday". The deployed instance makes the point sharply — it holds ~40k `work.published` rows **on purpose**, held
/// back until the ATProto bridge is configured, so a bare count is permanently large and means nothing. Age separates a
/// deliberately deferred backlog from a queue that stopped moving.
pub async fn oldest_undelivered_at(db: &Database) -> Result<Option<String>> {
    let sql = "SELECT MIN(created_at) FROM outbox_events WHERE delivered_at IS NULL";
    match db.backend() {
        Backend::Sqlite => Ok(sqlx::query_scalar(sql)
            .fetch_one(db.sqlite_pool().expect("sqlite handle"))
            .await?),
        Backend::Postgres => Ok(sqlx::query_scalar(sql)
            .fetch_one(db.postgres_pool().expect("postgres handle"))
            .await?),
    }
}

/// Topics recorded for a work, for introspection in tests and `doctor`.
pub async fn topics_for_work(db: &Database, work: darkhold_domain::WorkId) -> Result<Vec<String>> {
    let sql = db.sql(
        "SELECT topic FROM outbox_events WHERE payload LIKE ? ORDER BY created_at ASC",
        "SELECT topic FROM outbox_events WHERE payload LIKE ? ORDER BY created_at ASC",
    );
    let pattern = format!("%\"work_id\":\"{work}\"%");

    let rows: Vec<(String,)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(pattern)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(pattern)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    Ok(rows.into_iter().map(|(topic,)| topic).collect())
}
