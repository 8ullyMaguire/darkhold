//! Payments, refunds, prices and the operator's tax profile (M16-11).
//!
//! The arithmetic is in `darkhold_domain::payments`; this module records what it decided. Two properties are the schema's rather than a
//! handler's:
//!
//! * **One payment per (account, idempotency key).** A retried purchase after a timeout must not charge twice, so the unique index is the
//!   guard and `record_payment` inserts `ON CONFLICT DO NOTHING` then reads the winner back.
//! * **`refunded_minor` is cumulative.** The refunds rows record events; this column is what the check compares against `amount_minor`, so
//!   there is exactly one number that could be wrong and it is written by one statement.

use serde_json::{json, Value};
use sqlx::Row;

use darkhold_domain::payments::{PaymentState, TaxProfile};

use crate::{Backend, Database};

/// The twelve columns every read selects, without a WHERE or a placeholder.
///
/// Spelled once so a projection cannot drift between queries; each call appends its own `WHERE`, because the placeholder syntax is the one
/// thing the two engines disagree about here and a shared string would have to lie about it.
const COLUMNS: &str = "SELECT id, account, subject_kind, subject_id, subject_version, amount_minor, currency, \
     state, processor, processor_ref, tax_minor, refunded_minor FROM payments";

/// A payment, as the store keeps it.
#[derive(Debug, Clone)]
pub struct Payment {
    pub id: String,
    pub account: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub subject_version: Option<String>,
    pub amount_minor: i64,
    pub currency: String,
    pub state: PaymentState,
    pub processor: String,
    pub processor_ref: Option<String>,
    pub tax_minor: i64,
    pub refunded_minor: i64,
}

impl Payment {
    /// The JSON a route publishes. `processor_ref` is included because reconciliation is an operator's job, and it is not a secret.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "subject_kind": self.subject_kind,
            "subject_id": self.subject_id,
            "subject_version": self.subject_version,
            "amount_minor": self.amount_minor,
            "currency": self.currency,
            "state": self.state.as_str(),
            "processor": self.processor,
            "processor_ref": self.processor_ref,
            "tax_minor": self.tax_minor,
            "refunded_minor": self.refunded_minor,
        })
    }
}

/// A payment about to be recorded.
///
/// A struct rather than eight positional arguments: three of them are `&str` and two are minor amounts, and a caller that swapped
/// `subject_id` for `idempotency` would still compile. Naming them at the call site is the check.
#[derive(Debug, Clone)]
pub struct NewPayment<'a> {
    pub account: &'a str,
    pub subject_kind: &'a str,
    pub subject_id: &'a str,
    pub subject_version: Option<&'a str>,
    pub amount_minor: i64,
    pub currency: &'a str,
    pub tax_minor: i64,
    pub idempotency: &'a str,
    pub processor: &'a str,
}

/// Insert a payment, returning the existing one when the idempotency key has been used.
pub async fn record_payment(db: &Database, new: &NewPayment<'_>) -> Result<Payment, sqlx::Error> {
    let NewPayment {
        account,
        subject_kind,
        subject_id,
        subject_version,
        amount_minor,
        currency,
        tax_minor,
        idempotency,
        processor,
    } = *new;
    let now = crate::identity::now_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO payments
                     (id, account, subject_kind, subject_id, subject_version, amount_minor, currency,
                      state, processor, idempotency, tax_minor, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?)
                 ON CONFLICT(account, idempotency) DO NOTHING",
            )
            .bind(&id)
            .bind(account)
            .bind(subject_kind)
            .bind(subject_id)
            .bind(subject_version)
            .bind(amount_minor)
            .bind(currency)
            .bind(processor)
            .bind(idempotency)
            .bind(tax_minor)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO payments
                     (id, account, subject_kind, subject_id, subject_version, amount_minor, currency,
                      state, processor, idempotency, tax_minor, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', $8, $9, $10, $11)
                 ON CONFLICT(account, idempotency) DO NOTHING",
            )
            .bind(&id)
            .bind(account)
            .bind(subject_kind)
            .bind(subject_id)
            .bind(subject_version)
            .bind(amount_minor)
            .bind(currency)
            .bind(processor)
            .bind(idempotency)
            .bind(tax_minor)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    payment_by_idempotency(db, account, idempotency)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

/// One payment by id.
pub async fn payment(db: &Database, id: &str) -> Result<Option<Payment>, sqlx::Error> {
    let row: Option<P> = match db.backend() {
        Backend::Sqlite => sqlx::query(&format!("{COLUMNS} WHERE id = ?"))
            .bind(id)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?
            .map(|row| payment_row_sqlite(&row)),
        Backend::Postgres => sqlx::query(&format!("{COLUMNS} WHERE id = $1"))
            .bind(id)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?
            .map(|row| payment_row_pg(&row)),
    };
    Ok(row.map(Into::into))
}

async fn payment_by_idempotency(
    db: &Database,
    account: &str,
    idempotency: &str,
) -> Result<Option<Payment>, sqlx::Error> {
    match db.backend() {
        Backend::Sqlite => {
            let row: Option<P> = sqlx::query(
                "SELECT id, account, subject_kind, subject_id, subject_version, amount_minor, currency,
                        state, processor, processor_ref, tax_minor, refunded_minor
                 FROM payments WHERE account = ? AND idempotency = ?",
            )
            .bind(account)
            .bind(idempotency)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await?
            .map(|row| payment_row_sqlite(&row));
            Ok(row.map(Into::into))
        }
        Backend::Postgres => {
            let row: Option<P> = sqlx::query(
                "SELECT id, account, subject_kind, subject_id, subject_version, amount_minor, currency,
                        state, processor, processor_ref, tax_minor, refunded_minor
                 FROM payments WHERE account = $1 AND idempotency = $2",
            )
            .bind(account)
            .bind(idempotency)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await?
            .map(|row| payment_row_pg(&row));
            Ok(row.map(Into::into))
        }
    }
}

/// Mark a payment captured and record what the processor said.
///
/// `WHERE state = 'pending'` is the guard: a double capture is a no-op, not a second charge, so the affected-row count is what the caller
/// reads to know whether it captured or found it already captured.
pub async fn capture(
    db: &Database,
    id: &str,
    processor_ref: &str,
) -> Result<bool, sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(
            "UPDATE payments SET state = 'captured', processor_ref = ?, captured_at = ?
             WHERE id = ? AND state = 'pending'",
        )
        .bind(processor_ref)
        .bind(&now)
        .bind(id)
        .execute(db.sqlite_pool().expect("sqlite"))
        .await?
        .rows_affected(),
        Backend::Postgres => sqlx::query(
            "UPDATE payments SET state = 'captured', processor_ref = $1, captured_at = $2
             WHERE id = $3 AND state = 'pending'",
        )
        .bind(processor_ref)
        .bind(&now)
        .bind(id)
        .execute(db.postgres_pool().expect("postgres"))
        .await?
        .rows_affected(),
    };
    Ok(affected > 0)
}

/// Record a refund and update the cumulative total in one statement.
///
/// `refunded_minor + amount` is written rather than a computed absolute value, and the guard `refunded_minor + ? <= amount_minor` makes an
/// over-refund impossible even if a caller skipped the domain check — the database is the last place a negative balance can be prevented.
/// The state settles to `refunded` when the total reaches the amount.
pub async fn record_refund(
    db: &Database,
    payment_id: &str,
    amount_minor: i64,
    reason: &str,
    actor: &str,
) -> Result<bool, sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    let refund_id = uuid::Uuid::new_v4().to_string();
    let affected = match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "UPDATE payments
                 SET refunded_minor = refunded_minor + ?,
                     state = CASE WHEN refunded_minor + ? >= amount_minor THEN 'refunded' ELSE state END
                 WHERE id = ? AND state = 'captured' AND refunded_minor + ? <= amount_minor",
            )
            .bind(amount_minor)
            .bind(amount_minor)
            .bind(payment_id)
            .bind(amount_minor)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?
            .rows_affected()
        }
        Backend::Postgres => {
            sqlx::query(
                "UPDATE payments
                 SET refunded_minor = refunded_minor + $1,
                     state = CASE WHEN refunded_minor + $2 >= amount_minor THEN 'refunded' ELSE state END
                 WHERE id = $3 AND state = 'captured' AND refunded_minor + $4 <= amount_minor",
            )
            .bind(amount_minor)
            .bind(amount_minor)
            .bind(payment_id)
            .bind(amount_minor)
            .execute(db.postgres_pool().expect("postgres"))
            .await?
            .rows_affected()
        }
    };
    if affected == 0 {
        return Ok(false);
    }
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO refunds (id, payment_id, amount_minor, reason, actor, created_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&refund_id)
            .bind(payment_id)
            .bind(amount_minor)
            .bind(reason)
            .bind(actor)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO refunds (id, payment_id, amount_minor, reason, actor, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(&refund_id)
            .bind(payment_id)
            .bind(amount_minor)
            .bind(reason)
            .bind(actor)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(true)
}

/// A buyer's own payments, newest first. The only payment read that names an account, and it is the caller's own.
pub async fn my_payments(db: &Database, account: &str, limit: i64) -> Result<Vec<Value>, sqlx::Error> {
    let payments: Vec<Payment> = match db.backend() {
        Backend::Sqlite => sqlx::query(
            "SELECT id, account, subject_kind, subject_id, subject_version, amount_minor, currency,
                    state, processor, processor_ref, tax_minor, refunded_minor
             FROM payments WHERE account = ? ORDER BY created_at DESC LIMIT ?",
        )
        .bind(account)
        .bind(limit)
        .fetch_all(db.sqlite_pool().expect("sqlite"))
        .await?
        .iter()
        .map(payment_row_sqlite)
        .map(Into::into)
        .collect(),
        Backend::Postgres => sqlx::query(
            "SELECT id, account, subject_kind, subject_id, subject_version, amount_minor, currency,
                    state, processor, processor_ref, tax_minor, refunded_minor
             FROM payments WHERE account = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(account)
        .bind(limit)
        .fetch_all(db.postgres_pool().expect("postgres"))
        .await?
        .iter()
        .map(payment_row_pg)
        .map(Into::into)
        .collect(),
    };
    Ok(payments.iter().map(Payment::to_json).collect())
}

/// Set a package's price.
pub async fn set_price(
    db: &Database,
    manifest_id: &str,
    price_minor: i64,
    currency: &str,
    enabled: bool,
    set_by: &str,
) -> Result<(), sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO extension_prices (manifest_id, price_minor, currency, enabled, set_by, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?)
                 ON CONFLICT(manifest_id) DO UPDATE SET
                     price_minor=excluded.price_minor, currency=excluded.currency,
                     enabled=excluded.enabled, set_by=excluded.set_by, updated_at=excluded.updated_at",
            )
            .bind(manifest_id)
            .bind(price_minor)
            .bind(currency)
            .bind(i64::from(enabled))
            .bind(set_by)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO extension_prices (manifest_id, price_minor, currency, enabled, set_by, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT(manifest_id) DO UPDATE SET
                     price_minor=excluded.price_minor, currency=excluded.currency,
                     enabled=excluded.enabled, set_by=excluded.set_by, updated_at=excluded.updated_at",
            )
            .bind(manifest_id)
            .bind(price_minor)
            .bind(currency)
            .bind(i64::from(enabled))
            .bind(set_by)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(())
}

/// A package's price, if one has been set and is enabled.
pub async fn enabled_price(
    db: &Database,
    manifest_id: &str,
) -> Result<Option<(i64, String)>, sqlx::Error> {
    let row: Option<(i64, String)> = match db.backend() {
        Backend::Sqlite => sqlx::query(
            "SELECT price_minor, currency FROM extension_prices
             WHERE manifest_id = ? AND enabled = 1",
        )
        .bind(manifest_id)
        .fetch_optional(db.sqlite_pool().expect("sqlite"))
        .await?
        .map(|row| (row.get("price_minor"), row.get("currency"))),
        Backend::Postgres => sqlx::query(
            "SELECT price_minor, currency FROM extension_prices
             WHERE manifest_id = $1 AND enabled = 1",
        )
        .bind(manifest_id)
        .fetch_optional(db.postgres_pool().expect("postgres"))
        .await?
        .map(|row| (row.get("price_minor"), row.get("currency"))),
    };
    Ok(row)
}

/// A seller's revenue over their captured, unrefunded sales of one kind of subject.
///
/// Computed from the payments rather than stored: a running total is a second copy of the truth, and the one question a payout asks is
/// "what is captured and not refunded right now".
pub async fn seller_net(
    db: &Database,
    seller_account: &str,
    currency: &str,
) -> Result<i64, sqlx::Error> {
    // The payments table does not carry the seller: it is the paid package's submitter. The join is written out in both arms because the
    // schema of `extension_manifests` is keyed on (id, version) and only one row per version exists on either engine.
    match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(
                "SELECT COALESCE(SUM(p.amount_minor - p.refunded_minor), 0) AS net
                 FROM payments p
                 JOIN extension_manifests m ON m.id = p.subject_id AND (m.version = p.subject_version OR p.subject_version IS NULL)
                 WHERE m.submitted_by = ? AND p.currency = ? AND p.state IN ('captured','refunded')",
            )
            .bind(seller_account)
            .bind(currency)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(row.get::<i64, _>("net"))
        }
        Backend::Postgres => {
            let row = sqlx::query(
                "SELECT COALESCE(SUM(p.amount_minor - p.refunded_minor), 0)::BIGINT AS net
                 FROM payments p
                 JOIN extension_manifests m ON m.id = p.subject_id AND (m.version = p.subject_version OR p.subject_version IS NULL)
                 WHERE m.submitted_by = $1 AND p.currency = $2 AND p.state IN ('captured','refunded')",
            )
            .bind(seller_account)
            .bind(currency)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(row.get::<i64, _>("net"))
        }
    }
}

/// The stored tax profile, if the operator has set one.
pub async fn tax_profile(db: &Database) -> Result<Option<TaxProfile>, sqlx::Error> {
    let row: Option<(String, String, Option<String>, i64, String)> = match db.backend() {
        Backend::Sqlite => sqlx::query(
            "SELECT legal_name, country, tax_id, rate_bp, invoice_prefix
             FROM operator_tax_profile WHERE id = 'default'",
        )
        .fetch_optional(db.sqlite_pool().expect("sqlite"))
        .await?
        .map(|row| {
            (
                row.get("legal_name"),
                row.get("country"),
                row.get("tax_id"),
                row.get("rate_bp"),
                row.get("invoice_prefix"),
            )
        }),
        Backend::Postgres => sqlx::query(
            "SELECT legal_name, country, tax_id, rate_bp, invoice_prefix
             FROM operator_tax_profile WHERE id = 'default'",
        )
        .fetch_optional(db.postgres_pool().expect("postgres"))
        .await?
        .map(|row| {
            (
                row.get("legal_name"),
                row.get("country"),
                row.get("tax_id"),
                row.get("rate_bp"),
                row.get("invoice_prefix"),
            )
        }),
    };
    Ok(row.map(|(legal_name, country, tax_id, rate_bp, invoice_prefix)| TaxProfile {
        legal_name,
        country,
        tax_id,
        rate_bp,
        invoice_prefix,
    }))
}

/// Store the tax profile (one row; setting it twice replaces it).
pub async fn set_tax_profile(db: &Database, profile: &TaxProfile) -> Result<(), sqlx::Error> {
    let now = crate::identity::now_rfc3339();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO operator_tax_profile (id, legal_name, country, tax_id, rate_bp, invoice_prefix, updated_at)
                 VALUES ('default', ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(id) DO UPDATE SET
                     legal_name=excluded.legal_name, country=excluded.country, tax_id=excluded.tax_id,
                     rate_bp=excluded.rate_bp, invoice_prefix=excluded.invoice_prefix,
                     updated_at=excluded.updated_at",
            )
            .bind(&profile.legal_name)
            .bind(&profile.country)
            .bind(&profile.tax_id)
            .bind(profile.rate_bp)
            .bind(&profile.invoice_prefix)
            .bind(&now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO operator_tax_profile (id, legal_name, country, tax_id, rate_bp, invoice_prefix, updated_at)
                 VALUES ('default', $1, $2, $3, $4, $5, $6)
                 ON CONFLICT(id) DO UPDATE SET
                     legal_name=excluded.legal_name, country=excluded.country, tax_id=excluded.tax_id,
                     rate_bp=excluded.rate_bp, invoice_prefix=excluded.invoice_prefix,
                     updated_at=excluded.updated_at",
            )
            .bind(&profile.legal_name)
            .bind(&profile.country)
            .bind(&profile.tax_id)
            .bind(profile.rate_bp)
            .bind(&profile.invoice_prefix)
            .bind(&now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(())
}

/// The twelve columns, extracted once per engine so both arms hand back one type.
type P = (
    String,
    String,
    String,
    String,
    Option<String>,
    i64,
    String,
    String,
    String,
    Option<String>,
    i64,
    i64,
);

fn payment_row_sqlite(row: &sqlx::sqlite::SqliteRow) -> P {
    (
        row.get("id"),
        row.get("account"),
        row.get("subject_kind"),
        row.get("subject_id"),
        row.get("subject_version"),
        row.get("amount_minor"),
        row.get("currency"),
        row.get("state"),
        row.get("processor"),
        row.get("processor_ref"),
        row.get("tax_minor"),
        row.get("refunded_minor"),
    )
}

fn payment_row_pg(row: &sqlx::postgres::PgRow) -> P {
    (
        row.get("id"),
        row.get("account"),
        row.get("subject_kind"),
        row.get("subject_id"),
        row.get("subject_version"),
        row.get("amount_minor"),
        row.get("currency"),
        row.get("state"),
        row.get("processor"),
        row.get("processor_ref"),
        row.get("tax_minor"),
        row.get("refunded_minor"),
    )
}

impl From<P> for Payment {
    fn from(
        (
            id,
            account,
            subject_kind,
            subject_id,
            subject_version,
            amount_minor,
            currency,
            state,
            processor,
            processor_ref,
            tax_minor,
            refunded_minor,
        ): P,
    ) -> Self {
        Self {
            id,
            account,
            subject_kind,
            subject_id,
            subject_version,
            amount_minor,
            currency,
            state: PaymentState::from_str(&state).unwrap_or_default(),
            processor,
            processor_ref,
            tax_minor,
            refunded_minor,
        }
    }
}
