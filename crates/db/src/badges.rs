//! §9.7.6 — badge awards (M47-14).
//!
//! An award is two writes that must not disagree: the `badge_award` row (unique per account and
//! badge, so the badge itself is one-time) and the credit bonus (which goes through the economy's
//! idempotent `post_transaction`). The row is written first and the ledger second, and the ledger's
//! idempotency key names the badge and the account — so a crash between them leaves a badge with no
//! bonus, which a retry repairs, rather than a bonus with no badge, which nothing would ever notice.

use anyhow::Result;

use darkhold_domain::badges::{self, Badge};

use crate::{Backend, Database};

/// An award as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Award {
    /// The badge's slug.
    pub badge: String,
    /// The badge's name, resolved from the catalogue; the slug when this build no longer knows it.
    pub name: String,
    /// What earned it — a work id, a pseud id, or nothing.
    pub context: Option<String>,
    /// The credits it paid, as recorded at the time rather than re-read from a constant.
    pub credits: i64,
    /// When it was awarded, RFC 3339.
    pub awarded_at: String,
}

/// Award a badge to an account, once, with its credit bonus.
///
/// Returns `true` when this call is what awarded it, `false` when the account already held it. A
/// caller can tell "congratulate" from "nothing happened" without a second read, and a replay is
/// silent rather than an error.
pub async fn award(
    db: &Database,
    account_id: &str,
    badge: Badge,
    context: Option<&str>,
) -> Result<bool> {
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "INSERT INTO badge_award (id, account_id, badge, context, credits, awarded_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (account_id, badge) DO NOTHING",
        "INSERT INTO badge_award (id, account_id, badge, context, credits, awarded_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (account_id, badge) DO NOTHING",
    );
    let affected = match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(account_id)
                .bind(badge.slug)
                .bind(context)
                .bind(badge.credits)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?
                .rows_affected()
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(account_id)
                .bind(badge.slug)
                .bind(context)
                .bind(badge.credits)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?
                .rows_affected()
        }
    };
    if affected == 0 {
        return Ok(false);
    }

    // The bonus, under a key that names the badge and the account. `post_transaction` answers with
    // the existing transaction on a replay instead of paying twice, so a retry after a crash between
    // these two writes tops the ledger up exactly once.
    if badge.credits > 0 {
        let key = badges::credit_key(badge.slug, account_id);
        crate::economy::post_transaction(
            db,
            darkhold_domain::economy::TxnType::Grant,
            &key,
            &format!("badge:{}", badge.slug),
            &[(account_id.to_owned(), "granted".to_owned(), badge.credits)],
        )
        .await
        .map_err(|error| anyhow::anyhow!("posting the badge bonus: {error}"))?;
    }

    Ok(true)
}

/// Every badge an account holds, newest first.
pub async fn for_account(db: &Database, account_id: &str) -> Result<Vec<Award>> {
    let sql = db.sql(
        "SELECT badge, context, credits, awarded_at FROM badge_award
          WHERE account_id = ? ORDER BY awarded_at DESC, badge ASC",
        "SELECT badge, context, credits, awarded_at FROM badge_award
          WHERE account_id = ? ORDER BY awarded_at DESC, badge ASC",
    );
    let rows: Vec<(String, Option<String>, i64, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(account_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(badge, context, credits, awarded_at)| Award {
            name: badges::find(&badge).map_or_else(|| badge.clone(), |found| found.name.to_owned()),
            badge,
            context,
            credits,
            awarded_at,
        })
        .collect())
}

/// Whether an account holds a badge.
pub async fn holds(db: &Database, account_id: &str, slug: &str) -> Result<bool> {
    let sql = db.sql(
        "SELECT 1 FROM badge_award WHERE account_id = ? AND badge = ?",
        "SELECT 1 FROM badge_award WHERE account_id = ? AND badge = ?",
    );
    let found: Option<i32> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(account_id)
                .bind(slug)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(account_id)
                .bind(slug)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(found.is_some())
}

/// How many works this pseud has published.
///
/// The debut signal, derived. A stored `is_new_author` flag would go stale the moment a second work
/// was published, and a flag nobody remembers to update is worse than no flag at all.
pub async fn published_work_count(db: &Database, pseud_id: &str) -> Result<i64> {
    let sql = db.sql(
        "SELECT COUNT(*) FROM works
          WHERE owner_pseud_id = ? AND lifecycle = 'published' AND deleted_at IS NULL",
        "SELECT COUNT(*)::bigint FROM works
          WHERE owner_pseud_id::text = ? AND lifecycle = 'published' AND deleted_at IS NULL",
    );
    let count: Option<i64> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(pseud_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(pseud_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(count.unwrap_or(0))
}

/// The account that owns a pseud, if the pseud exists.
pub async fn account_for_pseud(db: &Database, pseud_id: &str) -> Result<Option<String>> {
    let sql = db.sql(
        "SELECT account_id FROM pseuds WHERE id = ? AND deleted_at IS NULL",
        // `::text` on the SELECT as well as the predicate: PostgreSQL will not decode a UUID column
        // into a `String`, and the account travels as text because `badge_award.account_id` is text.
        "SELECT account_id::text FROM pseuds WHERE id::text = ? AND deleted_at IS NULL",
    );
    let account: Option<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(pseud_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(pseud_id)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(account)
}
