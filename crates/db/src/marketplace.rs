//! M16 — Marketplace repository: listings, commissions, extensions, webhooks, gallery.

use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::{Backend, Database};
use darkhold_domain::extension::Capability;
use darkhold_domain::marketplace::{self, CommissionState, ListingKind};

// ---------------------------------------------------------------------------
// Listings
// ---------------------------------------------------------------------------

pub async fn create_listing(
    db: &Database,
    kind: ListingKind,
    owner: &str,
    work_id: Option<&str>,
    terms: &str,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO listings (id, kind, owner, work_id, terms, state, created_at)
                 VALUES (?, ?, ?, ?, ?, 'draft', ?)",
            )
            .bind(&id)
            .bind(kind.as_str())
            .bind(owner)
            .bind(work_id)
            .bind(terms)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO listings (id, kind, owner, work_id, terms, state, created_at)
                 VALUES ($1, $2, $3, $4, $5, 'draft', $6)",
            )
            .bind(&id)
            .bind(kind.as_str())
            .bind(owner)
            .bind(work_id)
            .bind(terms)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(id)
}

pub async fn list_listings(
    db: &Database,
    kind: Option<&str>,
    state: Option<&str>,
    limit: i64,
) -> Result<Vec<Value>, sqlx::Error> {
    let mut sqlite_sql =
        "SELECT id, kind, owner, work_id, terms, state, created_at FROM listings WHERE 1=1"
            .to_string();
    let mut pg_sql = "SELECT id::text, kind, owner, work_id::text, terms, state, created_at FROM listings WHERE 1=1".to_string();
    let mut param_idx = 1;
    if kind.is_some() {
        sqlite_sql.push_str(" AND kind = ?");
        pg_sql.push_str(&format!(" AND kind = ${}", param_idx));
        param_idx += 1;
    }
    if state.is_some() {
        sqlite_sql.push_str(" AND state = ?");
        pg_sql.push_str(&format!(" AND state = ${}", param_idx));
        param_idx += 1;
    }
    sqlite_sql.push_str(" ORDER BY created_at LIMIT ?");
    pg_sql.push_str(&format!(" ORDER BY created_at LIMIT ${}", param_idx));

    match db.backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query(&sqlite_sql);
            if let Some(k) = kind {
                q = q.bind(k);
            }
            if let Some(s) = state {
                q = q.bind(s);
            }
            let rows = q
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "kind": r.get::<String, _>("kind"),
                        "owner": r.get::<String, _>("owner"),
                        "work_id": r.get::<Option<String>, _>("work_id"),
                        "terms": r.get::<String, _>("terms"),
                        "state": r.get::<String, _>("state"),
                        "created_at": r.get::<String, _>("created_at"),
                    })
                })
                .collect())
        }
        Backend::Postgres => {
            let mut q = sqlx::query(&pg_sql);
            if let Some(k) = kind {
                q = q.bind(k);
            }
            if let Some(s) = state {
                q = q.bind(s);
            }
            let rows = q
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "kind": r.get::<String, _>("kind"),
                        "owner": r.get::<String, _>("owner"),
                        "work_id": r.get::<Option<String>, _>("work_id"),
                        "terms": r.get::<String, _>("terms"),
                        "state": r.get::<String, _>("state"),
                        "created_at": r.get::<String, _>("created_at"),
                    })
                })
                .collect())
        }
    }
}

// ---------------------------------------------------------------------------
// Commissions
// ---------------------------------------------------------------------------

pub async fn create_commission(
    db: &Database,
    listing_id: &str,
    client: &str,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO commissions (id, listing_id, client, state, created_at, updated_at)
                 VALUES (?, ?, ?, 'quoted', ?, ?)",
            )
            .bind(&id)
            .bind(listing_id)
            .bind(client)
            .bind(&now)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO commissions (id, listing_id, client, state, created_at, updated_at)
                 VALUES ($1, $2, $3, 'quoted', $4, $5)",
            )
            .bind(&id)
            .bind(listing_id)
            .bind(client)
            .bind(&now)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(id)
}

pub async fn transition_commission(
    db: &Database,
    commission_id: &str,
    from: &CommissionState,
    to: &CommissionState,
    ledger_ref: Option<&str>,
) -> Result<(), sqlx::Error> {
    if !marketplace::valid_commission_transition(from, to) {
        return Err(sqlx::Error::Protocol(format!(
            "invalid transition: {:?} -> {:?}",
            from, to
        )));
    }

    let now = crate::identity::now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            if let Some(ref ledger) = ledger_ref {
                sqlx::query("UPDATE commissions SET state = ?, updated_at = ?, quote_transaction = ? WHERE id = ? AND state = ?")
                    .bind(to.as_str()).bind(&now).bind(ledger).bind(commission_id).bind(from.as_str())
                    .execute(db.sqlite_pool().expect("sqlite")).await?;
            } else {
                sqlx::query(
                    "UPDATE commissions SET state = ?, updated_at = ? WHERE id = ? AND state = ?",
                )
                .bind(to.as_str())
                .bind(&now)
                .bind(commission_id)
                .bind(from.as_str())
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
            }
        }
        Backend::Postgres => {
            if let Some(ref ledger) = ledger_ref {
                sqlx::query("UPDATE commissions SET state = $1, updated_at = $2, quote_transaction = $3 WHERE id = $4 AND state = $5")
                    .bind(to.as_str()).bind(&now).bind(ledger).bind(commission_id).bind(from.as_str())
                    .execute(db.postgres_pool().expect("postgres")).await?;
            } else {
                sqlx::query("UPDATE commissions SET state = $1, updated_at = $2 WHERE id = $3 AND state = $4")
                    .bind(to.as_str()).bind(&now).bind(commission_id).bind(from.as_str())
                    .execute(db.postgres_pool().expect("postgres")).await?;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Extensions
// ---------------------------------------------------------------------------

pub async fn list_extensions(
    db: &Database,
    submitted_by: Option<&str>,
) -> Result<Vec<Value>, sqlx::Error> {
    let mut sqlite_sql =
        "SELECT id, version, document, submitted_by, state, created_at, derived_from FROM extension_manifests WHERE 1=1".to_string();
    let mut pg_sql = "SELECT id, version, document, submitted_by, state, created_at, derived_from FROM extension_manifests WHERE 1=1".to_string();
    if submitted_by.is_some() {
        sqlite_sql.push_str(" AND submitted_by = ?");
        pg_sql.push_str(" AND submitted_by = $1");
    }
    sqlite_sql.push_str(" ORDER BY created_at DESC");
    pg_sql.push_str(" ORDER BY created_at DESC");

    match db.backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query(&sqlite_sql);
            if let Some(by) = submitted_by {
                q = q.bind(by);
            }
            let rows = q.fetch_all(db.sqlite_pool().expect("sqlite")).await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "version": r.get::<String, _>("version"),
                        "document": r.get::<String, _>("document"),
                        "submitted_by": r.get::<String, _>("submitted_by"),
                        "state": r.get::<String, _>("state"),
                        "created_at": r.get::<String, _>("created_at"),
                        "derived_from": r.get::<Option<String>, _>("derived_from"),
                    })
                })
                .collect())
        }
        Backend::Postgres => {
            let mut q = sqlx::query(&pg_sql);
            if let Some(by) = submitted_by {
                q = q.bind(by);
            }
            let rows = q.fetch_all(db.postgres_pool().expect("postgres")).await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "version": r.get::<String, _>("version"),
                        "document": r.get::<String, _>("document"),
                        "submitted_by": r.get::<String, _>("submitted_by"),
                        "state": r.get::<String, _>("state"),
                        "created_at": r.get::<String, _>("created_at"),
                        "derived_from": r.get::<Option<String>, _>("derived_from"),
                    })
                })
                .collect())
        }
    }
}

pub async fn get_extension(db: &Database, id: &str) -> Result<Option<Value>, sqlx::Error> {
    match db.backend() {
        Backend::Sqlite => {
            // A package may have several versions now (migration 0183). The newest one is what "this package" means to a reader; an
            // unordered fetch_optional would fail on the second version, which is the bug this ordering fixes.
            let row = sqlx::query("SELECT id, version, document, submitted_by, state, created_at, derived_from FROM extension_manifests WHERE id = ? ORDER BY created_at DESC LIMIT 1")
                .bind(id)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(row.map(|r| {
                serde_json::json!({
                    "id": r.get::<String, _>("id"),
                    "version": r.get::<String, _>("version"),
                    "document": r.get::<String, _>("document"),
                    "submitted_by": r.get::<String, _>("submitted_by"),
                    "state": r.get::<String, _>("state"),
                    "created_at": r.get::<String, _>("created_at"),
                    "derived_from": r.get::<Option<String>, _>("derived_from"),
                })
            }))
        }
        Backend::Postgres => {
            let row = sqlx::query("SELECT id, version, document, submitted_by, state, created_at, derived_from FROM extension_manifests WHERE id = $1 ORDER BY created_at DESC LIMIT 1")
                .bind(id)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(row.map(|r| {
                serde_json::json!({
                    "id": r.get::<String, _>("id"),
                    "version": r.get::<String, _>("version"),
                    "document": r.get::<String, _>("document"),
                    "submitted_by": r.get::<String, _>("submitted_by"),
                    "state": r.get::<String, _>("state"),
                    "created_at": r.get::<String, _>("created_at"),
                    "derived_from": r.get::<Option<String>, _>("derived_from"),
                })
            }))
        }
    }
}

pub async fn list_my_grants(db: &Database, account: &str) -> Result<Vec<Value>, sqlx::Error> {
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query("SELECT account, manifest_id, version, capabilities, granted_at, revoked_at FROM extension_grants WHERE account = ?")
                .bind(account)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "account": r.get::<String, _>("account"),
                        "manifest_id": r.get::<String, _>("manifest_id"),
                        "version": r.get::<String, _>("version"),
                        "capabilities": r.get::<String, _>("capabilities"),
                        "granted_at": r.get::<String, _>("granted_at"),
                        "revoked_at": r.get::<Option<String>, _>("revoked_at"),
                        // Published rather than inferred by the reader: a revoked grant and a held one are the same shape of row, and the
                        // difference between "this extension may call the host" and "you uninstalled it" must not depend on a client
                        // deciding that `revoked_at != null` means something.
                        "active": r.get::<Option<String>, _>("revoked_at").is_none(),
                    })
                })
                .collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query("SELECT account, manifest_id, version, capabilities, granted_at, revoked_at FROM extension_grants WHERE account = $1")
                .bind(account)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "account": r.get::<String, _>("account"),
                        "manifest_id": r.get::<String, _>("manifest_id"),
                        "version": r.get::<String, _>("version"),
                        "capabilities": r.get::<String, _>("capabilities"),
                        "granted_at": r.get::<String, _>("granted_at"),
                        "revoked_at": r.get::<Option<String>, _>("revoked_at"),
                        // Published rather than inferred by the reader: a revoked grant and a held one are the same shape of row, and the
                        // difference between "this extension may call the host" and "you uninstalled it" must not depend on a client
                        // deciding that `revoked_at != null` means something.
                        "active": r.get::<Option<String>, _>("revoked_at").is_none(),
                    })
                })
                .collect())
        }
    }
}

pub async fn list_webhooks(db: &Database, owner: &str) -> Result<Vec<Value>, sqlx::Error> {
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(
                "SELECT id, owner, url, events, active, rate_per_minute, consecutive_failures, disabled_at, disabled_reason, created_at FROM webhook_endpoints WHERE owner = ?",
            )
                .bind(owner)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "owner": r.get::<String, _>("owner"),
                        "url": r.get::<String, _>("url"),
                        "events": r.get::<String, _>("events"),
                        // The secret has to travel with the row: `webhook_sender` signs with
                        // `wh["secret"]`, and a projection that omitted it signed every delivery with an
                        // empty key. The receiver — holding the real secret — could never verify, so every
                        // subscriber would have answered 400 and every webhook would have looked like a
                        // broken network instead of a broken query.
                        "secret": r.get::<String, _>("secret"),
                        "active": r.get::<i64, _>("active") == 1,
                        "rate_per_minute": r.get::<i64, _>("rate_per_minute"),
                        "consecutive_failures": r.get::<i64, _>("consecutive_failures"),
                        "disabled_at": r.get::<Option<String>, _>("disabled_at"),
                        "disabled_reason": r.get::<Option<String>, _>("disabled_reason"),
                        "created_at": r.get::<String, _>("created_at"),
                    })
                })
                .collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(
                "SELECT id, owner, url, events, CAST(active AS BIGINT) AS active, CAST(rate_per_minute AS BIGINT) AS rate_per_minute, CAST(consecutive_failures AS BIGINT) AS consecutive_failures, disabled_at, disabled_reason, created_at FROM webhook_endpoints WHERE owner = $1",
            )
                .bind(owner)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "owner": r.get::<String, _>("owner"),
                        "url": r.get::<String, _>("url"),
                        "events": r.get::<String, _>("events"),
                        // The secret has to travel with the row: `webhook_sender` signs with
                        // `wh["secret"]`, and a projection that omitted it signed every delivery with an
                        // empty key. The receiver — holding the real secret — could never verify, so every
                        // subscriber would have answered 400 and every webhook would have looked like a
                        // broken network instead of a broken query.
                        "secret": r.get::<String, _>("secret"),
                        "active": r.get::<i64, _>("active") == 1,
                        "rate_per_minute": r.get::<i64, _>("rate_per_minute"),
                        "consecutive_failures": r.get::<i64, _>("consecutive_failures"),
                        "disabled_at": r.get::<Option<String>, _>("disabled_at"),
                        "disabled_reason": r.get::<Option<String>, _>("disabled_reason"),
                        "created_at": r.get::<String, _>("created_at"),
                    })
                })
                .collect())
        }
    }
}

pub async fn delete_webhook(db: &Database, owner: &str, id: &str) -> Result<(), sqlx::Error> {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query("DELETE FROM webhook_endpoints WHERE owner = ? AND id = ?")
                .bind(owner)
                .bind(id)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query("DELETE FROM webhook_endpoints WHERE owner = $1 AND id = $2")
                .bind(owner)
                .bind(id)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    Ok(())
}

pub async fn grant_extension(
    db: &Database,
    account: &str,
    manifest_id: &str,
    version: &str,
    capabilities: &[Capability],
) -> Result<(), sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    let caps_json =
        serde_json::to_string(&capabilities.iter().map(|c| c.as_str()).collect::<Vec<_>>())
            .unwrap();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                // `ON CONFLICT` because the primary key is (account, manifest_id) and revocation is a column, not a delete: without it,
                // uninstalling and then installing again hit a unique violation and the extension could never be re-granted. A re-grant
                // is a new consent, so `revoked_at` is cleared and the capabilities are the ones just confirmed — not the old ones.
                "INSERT INTO extension_grants (account, manifest_id, version, capabilities, granted_at)
                 VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT(account, manifest_id) DO UPDATE SET version=excluded.version,
                 capabilities=excluded.capabilities, granted_at=excluded.granted_at, revoked_at=NULL"
            )
            .bind(account).bind(manifest_id).bind(version).bind(&caps_json).bind(&now)
            .execute(db.sqlite_pool().expect("sqlite")).await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO extension_grants (account, manifest_id, version, capabilities, granted_at)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT(account, manifest_id) DO UPDATE SET version=excluded.version,
                 capabilities=excluded.capabilities, granted_at=excluded.granted_at, revoked_at=NULL"
            )
            .bind(account).bind(manifest_id).bind(version).bind(&caps_json).bind(&now)
            .execute(db.postgres_pool().expect("postgres")).await?;
        }
    }
    Ok(())
}

/// One submitted version's manifest document, verbatim.
///
/// Separate from [`get_extension`], which answers "what is this package" (the newest version) — this answers "what does *this version*
/// declare", which is the question a permission comparison asks.
pub async fn manifest_document(
    db: &Database,
    id: &str,
    version: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query("SELECT document FROM extension_manifests WHERE id = ? AND version = ?")
                .bind(id)
                .bind(version)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?
                .map(|row| row.get("document"))
        }
        Backend::Postgres => {
            sqlx::query("SELECT document FROM extension_manifests WHERE id = $1 AND version = $2")
                .bind(id)
                .bind(version)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?
                .map(|row| row.get("document"))
        }
    };
    Ok(row)
}

/// The account that submitted a package (the seller), from its newest version.
///
/// `None` when the package does not exist. Used for two questions that must agree: "may this account price it" and "who is paid when it
/// sells" — reading the seller from the manifest rather than from a separate column is what makes them agree.
pub async fn submitted_by(db: &Database, id: &str) -> Result<Option<String>, sqlx::Error> {
    let row: Option<String> = match db.backend() {
        Backend::Sqlite => sqlx::query(
            "SELECT submitted_by FROM extension_manifests WHERE id = ? ORDER BY created_at DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(db.sqlite_pool().expect("sqlite"))
        .await?
        .map(|row| row.get("submitted_by")),
        Backend::Postgres => sqlx::query(
            "SELECT submitted_by FROM extension_manifests WHERE id = $1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(db.postgres_pool().expect("postgres"))
        .await?
        .map(|row| row.get("submitted_by")),
    };
    Ok(row)
}

/// The newest **approved** version's document, or `None` when the package has nothing published.
///
/// "Latest published" rather than "latest": a package under review has a newer row whose document nobody may install, and answering with it
/// would let a pending submission become the thing a reader grants.
pub async fn manifest_document_for_latest(
    db: &Database,
    id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<String> = match db.backend() {
        Backend::Sqlite => sqlx::query(
            "SELECT document FROM extension_manifests
             WHERE id = ? AND state = 'approved' ORDER BY created_at DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(db.sqlite_pool().expect("sqlite"))
        .await?
        .map(|row| row.get("document")),
        Backend::Postgres => sqlx::query(
            "SELECT document FROM extension_manifests
             WHERE id = $1 AND state = 'approved' ORDER BY created_at DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(db.postgres_pool().expect("postgres"))
        .await?
        .map(|row| row.get("document")),
    };
    Ok(row)
}

/// The capability set an account's active grant covers for one package.
///
/// Empty when there is no grant. Empty is also the honest answer for a grant whose capabilities do not parse: a corrupt grant covers
/// nothing, and treating it as covering everything is how a renewal check passes when it should refuse.
pub async fn granted_capabilities(
    db: &Database,
    account: &str,
    manifest_id: &str,
) -> Result<std::collections::BTreeSet<String>, sqlx::Error> {
    let row: Option<String> = match db.backend() {
        Backend::Sqlite => sqlx::query(
            "SELECT capabilities FROM extension_grants
             WHERE account = ? AND manifest_id = ? AND revoked_at IS NULL",
        )
        .bind(account)
        .bind(manifest_id)
        .fetch_optional(db.sqlite_pool().expect("sqlite"))
        .await?
        .map(|row| row.get("capabilities")),
        Backend::Postgres => sqlx::query(
            "SELECT capabilities FROM extension_grants
             WHERE account = $1 AND manifest_id = $2 AND revoked_at IS NULL",
        )
        .bind(account)
        .bind(manifest_id)
        .fetch_optional(db.postgres_pool().expect("postgres"))
        .await?
        .map(|row| row.get("capabilities")),
    };
    Ok(row
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default())
}

/// Insert a manifest that is entering review (`state = 'pending'`).
///
/// Returns `false` when that (id, version) already exists: a submission is an event, and re-submitting the same version of the same
/// package would restart a review somebody may already have half done. A new version is a new row and a new review.
pub async fn submit_manifest(
    db: &Database,
    id: &str,
    version: &str,
    document: &str,
    submitted_by: &str,
) -> Result<bool, sqlx::Error> {
    let exists: Option<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query("SELECT id FROM extension_manifests WHERE id = ? AND version = ?")
                .bind(id)
                .bind(version)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?
                .map(|row| row.get::<String, _>("id"))
        }
        Backend::Postgres => {
            sqlx::query("SELECT id FROM extension_manifests WHERE id = $1 AND version = $2")
                .bind(id)
                .bind(version)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?
                .map(|row| row.get::<String, _>("id"))
        }
    };
    if exists.is_some() {
        return Ok(false);
    }
    let now = crate::identity::now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO extension_manifests (id, version, document, submitted_by, state, created_at)
                 VALUES (?, ?, ?, ?, 'pending', ?)",
            )
            .bind(id)
            .bind(version)
            .bind(document)
            .bind(submitted_by)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO extension_manifests (id, version, document, submitted_by, state, created_at)
                 VALUES ($1, $2, $3, $4, 'pending', $5)",
            )
            .bind(id)
            .bind(version)
            .bind(document)
            .bind(submitted_by)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(true)
}

/// Fork a package, optionally with a document the caller has already rewritten.
///
/// `document` exists for one case: a fork published under a different licence. The default (`None`) copies the source's document
/// byte-for-byte, which is the rule — the manifest is what the author's review was about — and a caller that passes its own document has
/// taken responsibility for saying why (the route checks licence compatibility before it gets here).
pub async fn fork_manifest_document(
    db: &Database,
    source_id: &str,
    target_id: &str,
    version: &str,
    forked_by: &str,
    document: Option<&str>,
) -> Result<Value, sqlx::Error> {
    // Each arm resolves to plain values rather than handing back a row: the two pools are different types, and a `match` that returned
    // `Option<Row>` would not compile — which is the compiler pointing at the real thing, that these are two databases and only the
    // extracted values are common.
    let source: Option<(String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "SELECT version, document FROM extension_manifests WHERE id = ? ORDER BY created_at DESC LIMIT 1",
            )
            .bind(source_id)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?
            .map(|row| (row.get::<String, _>("version"), row.get::<String, _>("document")))
        }
        Backend::Postgres => {
            sqlx::query(
                "SELECT version, document FROM extension_manifests WHERE id = $1 ORDER BY created_at DESC LIMIT 1",
            )
            .bind(source_id)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?
            .map(|row| (row.get::<String, _>("version"), row.get::<String, _>("document")))
        }
    };
    let Some((source_version, source_document)) = source else {
        return Err(sqlx::Error::RowNotFound);
    };
    let document = document.map_or(source_document, str::to_owned);

    let exists: Option<String> = match db.backend() {
        Backend::Sqlite => sqlx::query("SELECT id FROM extension_manifests WHERE id = ?")
            .bind(target_id)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?
            .map(|row| row.get::<String, _>("id")),
        Backend::Postgres => sqlx::query("SELECT id FROM extension_manifests WHERE id = $1")
            .bind(target_id)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?
            .map(|row| row.get::<String, _>("id")),
    };
    if exists.is_some() {
        return Err(sqlx::Error::Protocol(format!(
            "a package called `{target_id}` already exists"
        )));
    }

    let now = crate::identity::now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO extension_manifests (id, version, document, submitted_by, state, created_at, derived_from)
                 VALUES (?, ?, ?, ?, 'pending', ?, ?)",
            )
            .bind(target_id)
            .bind(version)
            .bind(&document)
            .bind(forked_by)
            .bind(&now)
            .bind(source_id)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO extension_manifests (id, version, document, submitted_by, state, created_at, derived_from)
                 VALUES ($1, $2, $3, $4, 'pending', $5, $6)",
            )
            .bind(target_id)
            .bind(version)
            .bind(&document)
            .bind(forked_by)
            .bind(&now)
            .bind(source_id)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }

    Ok(serde_json::json!({
        "id": target_id,
        "version": version,
        "state": "pending",
        "derived_from": source_id,
        "derived_from_version": source_version,
        "submitted_by": forked_by,
        // Stated rather than implied: a caller that expected the original's permissions to come along needs to see that they did not.
        "grants": [],
    }))
}

/// The grants that still hold: revoked is a column, so "uninstall" means the row stops counting rather than disappearing.
///
/// A separate reader from [`list_my_grants`] on purpose. The product needs both questions — *what have I granted over time* (the reader's
/// settings page, where a revoked grant is history worth showing) and *may this package call the host right now* (the executor, where a
/// revoked grant must be invisible) — and one function that answered both would have to be trusted to know which caller it was serving.
pub async fn active_grants(db: &Database, account: &str) -> Result<Vec<Value>, sqlx::Error> {
    let all = list_my_grants(db, account).await?;
    Ok(all
        .into_iter()
        .filter(|grant| grant.get("revoked_at").is_some_and(Value::is_null))
        .collect())
}

/// Whether one extension currently holds a grant of `capability` for this account.
pub async fn holds_grant(
    db: &Database,
    account: &str,
    manifest_id: &str,
    capability: &str,
) -> Result<bool, sqlx::Error> {
    let active = active_grants(db, account).await?;
    for grant in active {
        if grant.get("manifest_id").and_then(Value::as_str) != Some(manifest_id) {
            continue;
        }
        let capabilities = grant
            .get("capabilities")
            .and_then(Value::as_str)
            .and_then(|text| serde_json::from_str::<Vec<String>>(text).ok())
            .unwrap_or_default();
        if capabilities.iter().any(|held| held == capability) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Get all active webhook endpoints.
pub async fn list_all_active_webhooks(db: &Database) -> Result<Vec<Value>, sqlx::Error> {
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(
                "SELECT id, owner, url, events, secret, previous_secret, rotated_at, active, rate_per_minute, consecutive_failures, created_at FROM webhook_endpoints WHERE active = 1"
            )
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "owner": r.get::<String, _>("owner"),
                        "url": r.get::<String, _>("url"),
                        "events": r.get::<String, _>("events"),
                        // The secret has to travel with the row: `webhook_sender` signs with
                        // `wh["secret"]`, and a projection that omitted it signed every delivery with an
                        // empty key. The receiver — holding the real secret — could never verify, so every
                        // subscriber would have answered 400 and every webhook would have looked like a
                        // broken network instead of a broken query.
                        "secret": r.get::<String, _>("secret"),
                        "active": r.get::<i64, _>("active") == 1,
                        "previous_secret": r.get::<Option<String>, _>("previous_secret"),
                        "rotated_at": r.get::<Option<String>, _>("rotated_at"),
                        "rate_per_minute": r.get::<i64, _>("rate_per_minute"),
                        "consecutive_failures": r.get::<i64, _>("consecutive_failures"),
                        "created_at": r.get::<String, _>("created_at"),
                    })
                })
                .collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(
                "SELECT id, owner, url, events, secret, previous_secret, rotated_at, CAST(active AS BIGINT) AS active, CAST(rate_per_minute AS BIGINT) AS rate_per_minute, CAST(consecutive_failures AS BIGINT) AS consecutive_failures, created_at FROM webhook_endpoints WHERE active = $1"
            )
            .bind(1i64)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "owner": r.get::<String, _>("owner"),
                        "url": r.get::<String, _>("url"),
                        "events": r.get::<String, _>("events"),
                        // The secret has to travel with the row: `webhook_sender` signs with
                        // `wh["secret"]`, and a projection that omitted it signed every delivery with an
                        // empty key. The receiver — holding the real secret — could never verify, so every
                        // subscriber would have answered 400 and every webhook would have looked like a
                        // broken network instead of a broken query.
                        "secret": r.get::<String, _>("secret"),
                        "active": r.get::<i64, _>("active") == 1,
                        "previous_secret": r.get::<Option<String>, _>("previous_secret"),
                        "rotated_at": r.get::<Option<String>, _>("rotated_at"),
                        "rate_per_minute": r.get::<i64, _>("rate_per_minute"),
                        "consecutive_failures": r.get::<i64, _>("consecutive_failures"),
                        "created_at": r.get::<String, _>("created_at"),
                    })
                })
                .collect())
        }
    }
}

pub async fn revoke_extension(
    db: &Database,
    account: &str,
    manifest_id: &str,
) -> Result<(), sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "UPDATE extension_grants SET revoked_at = ? WHERE account = ? AND manifest_id = ?",
            )
            .bind(&now)
            .bind(account)
            .bind(manifest_id)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query("UPDATE extension_grants SET revoked_at = $1 WHERE account = $2 AND manifest_id = $3")
                .bind(&now).bind(account).bind(manifest_id)
                .execute(db.postgres_pool().expect("postgres")).await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Webhooks
// ---------------------------------------------------------------------------

pub async fn create_webhook(
    db: &Database,
    owner: &str,
    url: &str,
    secret: &str,
    events: &[String],
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();
    let events_json = serde_json::to_string(events).unwrap();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO webhook_endpoints (id, owner, url, secret, events, created_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(owner)
            .bind(url)
            .bind(secret)
            .bind(&events_json)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO webhook_endpoints (id, owner, url, secret, events, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(&id)
            .bind(owner)
            .bind(url)
            .bind(secret)
            .bind(&events_json)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(id)
}

pub async fn record_delivery(
    db: &Database,
    endpoint_id: &str,
    event_id: &str,
    payload: &str,
    signature: &str,
    status: &str,
) -> Result<(), sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO webhook_deliveries (id, endpoint_id, event_id, payload, signature, attempted_at, status, attempts)
                 VALUES (?, ?, ?, ?, ?, ?, ?, 1)"
            )
            .bind(&id).bind(endpoint_id).bind(event_id).bind(payload).bind(signature).bind(&now).bind(status)
            .execute(db.sqlite_pool().expect("sqlite")).await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO webhook_deliveries (id, endpoint_id, event_id, payload, signature, attempted_at, status, attempts)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, 1)"
            )
            .bind(&id).bind(endpoint_id).bind(event_id).bind(payload).bind(signature).bind(&now).bind(status)
            .execute(db.postgres_pool().expect("postgres")).await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Gallery
// ---------------------------------------------------------------------------

pub async fn add_gallery_item(
    db: &Database,
    work_id: &str,
    owner: &str,
    media_type: &str,
    storage_key: &str,
    alt_text: &str,
    sanitized_document: &str,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = crate::identity::now_rfc3339();

    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO gallery_items (id, work_id, owner, media_type, storage_key, alt_text, sanitized_document, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&id).bind(work_id).bind(owner).bind(media_type).bind(storage_key).bind(alt_text).bind(sanitized_document).bind(&now)
            .execute(db.sqlite_pool().expect("sqlite")).await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO gallery_items (id, work_id, owner, media_type, storage_key, alt_text, sanitized_document, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
            )
            .bind(&id).bind(work_id).bind(owner).bind(media_type).bind(storage_key).bind(alt_text).bind(sanitized_document).bind(&now)
            .execute(db.postgres_pool().expect("postgres")).await?;
        }
    }
    Ok(id)
}

pub async fn list_gallery_items(db: &Database, work_id: &str) -> Result<Vec<Value>, sqlx::Error> {
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query("SELECT id, work_id, owner, media_type, storage_key, alt_text, sanitized_document, created_at FROM gallery_items WHERE work_id = ?")
                .bind(work_id)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "work_id": r.get::<String, _>("work_id"),
                        "owner": r.get::<String, _>("owner"),
                        "media_type": r.get::<String, _>("media_type"),
                        "storage_key": r.get::<String, _>("storage_key"),
                        "alt_text": r.get::<String, _>("alt_text"),
                        "sanitized_document": r.get::<String, _>("sanitized_document"),
                        "created_at": r.get::<String, _>("created_at"),
                    })
                })
                .collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query("SELECT id, work_id, owner, media_type, storage_key, alt_text, sanitized_document, created_at FROM gallery_items WHERE work_id = $1")
                .bind(work_id)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<String, _>("id"),
                        "work_id": r.get::<String, _>("work_id"),
                        "owner": r.get::<String, _>("owner"),
                        "media_type": r.get::<String, _>("media_type"),
                        "storage_key": r.get::<String, _>("storage_key"),
                        "alt_text": r.get::<String, _>("alt_text"),
                        "sanitized_document": r.get::<String, _>("sanitized_document"),
                        "created_at": r.get::<String, _>("created_at"),
                    })
                })
                .collect())
        }
    }
}
