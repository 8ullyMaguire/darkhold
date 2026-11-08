//! A peer's public picks, as this instance subscribed to them (M45-44, spec §0.4).
//!
//! Two lifetimes, two tables: a subscription is what an operator asked for, a pick is what the peer answered last time. The
//! reading side never touches the network — the sweep is the only fetcher — so a page render cannot block on a peer being down.

use serde::Serialize;

use crate::{Backend, Database};

/// One subscription, as the operator sees it.
#[derive(Debug, Clone, Serialize)]
pub struct Subscription {
    pub id: String,
    pub peer_host: String,
    pub collection_url: String,
    pub enabled: bool,
    pub created_at: String,
    pub last_fetched_at: Option<String>,
    pub last_status: Option<String>,
    pub last_error_at: Option<String>,
}

/// One pick, as a reader sees it.
#[derive(Debug, Clone, Serialize)]
pub struct PeerPick {
    pub peer_host: String,
    pub collection_url: String,
    pub work_url: String,
    pub title: String,
    pub author: Option<String>,
    pub fetched_at: String,
}

/// Add a subscription, or re-point an existing one for the same host.
///
/// Re-pointing is an update rather than a second row: the host is the identity, and two feeds from one host would show every pick
/// twice with no way for a reader to tell the rows apart. Returns the id.
pub async fn subscribe(
    db: &Database,
    peer_host: &str,
    collection_url: &str,
) -> Result<String, sqlx::Error> {
    let existing = db.sql(
        "SELECT id FROM federation_pick_subscriptions WHERE peer_host = ?1",
        "SELECT id::text FROM federation_pick_subscriptions WHERE peer_host = $1",
    );
    let found: Option<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&existing)
                .bind(peer_host)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&existing)
                .bind(peer_host)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    if let Some(id) = found {
        let update = db.sql(
            "UPDATE federation_pick_subscriptions SET collection_url = ?1, enabled = 1 WHERE id = ?2",
            "UPDATE federation_pick_subscriptions SET collection_url = $1, enabled = TRUE WHERE id = $2",
        );
        match db.backend() {
            Backend::Sqlite => {
                sqlx::query(&update)
                    .bind(collection_url)
                    .bind(&id)
                    .execute(db.sqlite_pool().expect("sqlite handle"))
                    .await?;
            }
            Backend::Postgres => {
                sqlx::query(&update)
                    .bind(collection_url)
                    .bind(&id)
                    .execute(db.postgres_pool().expect("postgres handle"))
                    .await?;
            }
        };
        return Ok(id);
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();
    let insert = db.sql(
        "INSERT INTO federation_pick_subscriptions (id, peer_host, collection_url, enabled, created_at)
         VALUES (?1, ?2, ?3, 1, ?4)",
        "INSERT INTO federation_pick_subscriptions (id, peer_host, collection_url, enabled, created_at)
         VALUES ($1::uuid, $2, $3, TRUE, $4)",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&insert)
                .bind(&id)
                .bind(peer_host)
                .bind(collection_url)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&insert)
                .bind(&id)
                .bind(peer_host)
                .bind(collection_url)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    };
    Ok(id)
}

/// Every subscription, newest host last — the operator's list.
pub async fn subscriptions(db: &Database) -> Result<Vec<Subscription>, sqlx::Error> {
    let sql = db.sql(
        // `enabled` is read through `CASE`, not straight: SQLite stores 0/1 and PostgreSQL stores a boolean, and decoding the
        // PostgreSQL column into an `i64` fails at runtime — the same dialect trap that has bitten this workspace before. The
        // `CASE` makes both engines answer an integer, so the row type is one type.
        "SELECT id, peer_host, collection_url, CASE WHEN enabled = 1 THEN 1 ELSE 0 END, created_at, last_fetched_at, last_status, last_error_at
         FROM federation_pick_subscriptions ORDER BY peer_host",
        "SELECT id::text, peer_host, collection_url, CASE WHEN enabled THEN 1 ELSE 0 END::bigint, created_at, last_fetched_at, last_status, last_error_at
         FROM federation_pick_subscriptions ORDER BY peer_host",
    );
    type Row = (
        String,
        String,
        String,
        i64,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<Row> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                peer_host,
                collection_url,
                enabled,
                created_at,
                last_fetched_at,
                last_status,
                last_error_at,
            )| Subscription {
                id,
                peer_host,
                collection_url,
                enabled: enabled != 0,
                created_at,
                last_fetched_at,
                last_status,
                last_error_at,
            },
        )
        .collect())
}

/// The subscriptions the sweep should fetch, least recently fetched first.
///
/// Ordered by `last_fetched_at` **with NULLs first**: a subscription that has never been fetched is the one whose backlog is
/// unknown, and a peer added five minutes ago should not wait behind one that was fetched five minutes ago.
pub async fn due_subscriptions(
    db: &Database,
    limit: i64,
) -> Result<Vec<(String, String, String)>, sqlx::Error> {
    let sql = db.sql(
        "SELECT id, peer_host, collection_url FROM federation_pick_subscriptions
         WHERE enabled = 1
         ORDER BY last_fetched_at IS NULL DESC, last_fetched_at, peer_host
         LIMIT ?1",
        "SELECT id::text, peer_host, collection_url FROM federation_pick_subscriptions
         WHERE enabled = TRUE
         ORDER BY last_fetched_at IS NULL DESC, last_fetched_at, peer_host
         LIMIT $1",
    );
    // Wrapped in `Ok` because this is the tail expression: a bare `match` with `?` inside yields the row vector, not a result.
    Ok(match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    })
}

/// Replace a subscription's picks with what the peer just answered, returning how many are stored now.
///
/// **Replace, not merge.** The collection is the peer's *current* list: a work they stopped recommending must stop being shown
/// here, or the surface becomes an archive of what a peer once liked and a reader cannot tell which is which. The delete and the
/// inserts are one transaction so a failure mid-write cannot leave a peer with no picks at all.
pub async fn replace_picks(
    db: &Database,
    subscription_id: &str,
    picks: &[(String, String, Option<String>)],
) -> Result<usize, sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            let mut tx = db.sqlite_pool().expect("sqlite handle").begin().await?;
            sqlx::query("DELETE FROM federation_peer_picks WHERE subscription_id = ?1")
                .bind(subscription_id)
                .execute(&mut *tx)
                .await?;
            for (work_url, title, author) in picks {
                sqlx::query(
                    "INSERT INTO federation_peer_picks (id, subscription_id, work_url, title, author, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(subscription_id)
                .bind(work_url)
                .bind(title)
                .bind(author)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
        }
        Backend::Postgres => {
            let mut tx = db.postgres_pool().expect("postgres handle").begin().await?;
            sqlx::query("DELETE FROM federation_peer_picks WHERE subscription_id = $1::uuid")
                .bind(subscription_id)
                .execute(&mut *tx)
                .await?;
            for (work_url, title, author) in picks {
                sqlx::query(
                    "INSERT INTO federation_peer_picks (id, subscription_id, work_url, title, author, fetched_at)
                     VALUES ($1::uuid, $2::uuid, $3, $4, $5, $6)",
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(subscription_id)
                .bind(work_url)
                .bind(title)
                .bind(author)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
        }
    }
    Ok(picks.len())
}

/// Record the outcome of a fetch. `status` is one line, so the operator list stays readable.
pub async fn record_fetch(
    db: &Database,
    subscription_id: &str,
    status: &str,
    ok: bool,
) -> Result<(), sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    // A failure bumps `last_fetched_at` too: the sweep's ordering is "least recently *attempted*", and a failing peer that
    // never advances would be retried every pass forever, at the head of the queue.
    let sql = db.sql(
        "UPDATE federation_pick_subscriptions
         SET last_fetched_at = ?1, last_status = ?2, last_error_at = CASE WHEN ?3 = 1 THEN NULL ELSE ?1 END
         WHERE id = ?4",
        "UPDATE federation_pick_subscriptions
         SET last_fetched_at = $1, last_status = $2, last_error_at = CASE WHEN $3 THEN NULL ELSE $1 END
         WHERE id = $4::uuid",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&now)
                .bind(status)
                .bind(i64::from(ok))
                .bind(subscription_id)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&now)
                .bind(status)
                .bind(ok)
                .bind(subscription_id)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    };
    Ok(())
}

/// Remove a subscription and its picks (the cascade does the picks).
pub async fn unsubscribe(db: &Database, peer_host: &str) -> Result<bool, sqlx::Error> {
    let sql = db.sql(
        "DELETE FROM federation_pick_subscriptions WHERE peer_host = ?1",
        "DELETE FROM federation_pick_subscriptions WHERE peer_host = $1",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(peer_host)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(peer_host)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

/// Recent picks across every enabled subscription, newest first.
///
/// Joined to the subscription so a disabled peer's picks stop being shown without anything being deleted: turning a peer off is
/// reversible, and a reader must not keep seeing a feed the operator switched off.
pub async fn recent_picks(db: &Database, limit: i64) -> Result<Vec<PeerPick>, sqlx::Error> {
    let sql = db.sql(
        "SELECT s.peer_host, s.collection_url, p.work_url, p.title, p.author, p.fetched_at
         FROM federation_peer_picks p
         JOIN federation_pick_subscriptions s ON s.id = p.subscription_id
         WHERE s.enabled = 1
         ORDER BY p.fetched_at DESC, p.title
         LIMIT ?1",
        "SELECT s.peer_host, s.collection_url, p.work_url, p.title, p.author, p.fetched_at
         FROM federation_peer_picks p
         JOIN federation_pick_subscriptions s ON s.id = p.subscription_id
         WHERE s.enabled = TRUE
         ORDER BY p.fetched_at DESC, p.title
         LIMIT $1",
    );
    let rows: Vec<(String, String, String, String, Option<String>, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(
            |(peer_host, collection_url, work_url, title, author, fetched_at)| PeerPick {
                peer_host,
                collection_url,
                work_url,
                title,
                author,
                fetched_at,
            },
        )
        .collect())
}
