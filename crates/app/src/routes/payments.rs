//! §21.9's paid marketplace over HTTP (M16-11).
//!
//! ```text
//! POST /api/v1/extensions/{slug}/purchase        buy a package; records a pending payment
//! GET  /api/v1/me/payments                        the caller's own payments
//! POST /api/v1/operator/payments/{id}/capture     take the money (operator, or a processor's webhook)
//! POST /api/v1/operator/payments/{id}/refund      return some or all of it
//! PUT  /api/v1/operator/extensions/{slug}/price   what a package costs
//! GET  /api/v1/operator/tax-profile               the invoice identity
//! PUT  /api/v1/operator/tax-profile               set it
//! ```
//!
//! # Why capture is a separate step
//!
//! There is no payment provider. A purchase therefore records what was *asked* for and grants nothing; capture is the moment money exists, and
//! it is the moment access is granted. Skipping the step would be a host that hands out paid packages in exchange for a form submission —
//! and it would hide the one state an operator most needs to see, which is "this reader says they paid".
//!
//! # What a refund does
//!
//! A refund of the whole amount revokes the grant and reverses the seller's revenue. A partial refund does neither: the reader keeps what
//! they bought, and the ledger records that they paid less. Both decisions are in `darkhold_domain::payments`, so this file cannot drift from
//! the arithmetic.

use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::payments::{
    self, is_full_refund, PaymentRefusal, PaymentState, Processor, Split, TaxProfile,
};
use darkhold_domain::AppError;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/extensions/{slug}/purchase", post(purchase))
        .route("/me/payments", get(my_payments))
        .route("/operator/payments/{id}/capture", post(capture))
        .route("/operator/payments/{id}/refund", post(refund))
        .route("/operator/extensions/{slug}/price", put(set_price))
        .route(
            "/operator/tax-profile",
            get(read_tax_profile).put(write_tax_profile),
        )
}

fn refused(message: String) -> ApiError {
    ApiError(AppError::Validation {
        message,
        field_errors: Default::default(),
    })
}

#[derive(Debug, Deserialize)]
pub struct PurchaseBody {
    /// The buyer's own retry key. Absent, one is derived from the account and the package, which means "one purchase per package per
    /// account" — the same answer a retry needs, without asking the client to invent a key it will lose.
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

async fn purchase(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(slug): Path<String>,
    Json(body): Json<PurchaseBody>,
) -> ApiResult<Json<Value>> {
    let Some((price_minor, currency)) = darkhold_db::payments::enabled_price(state.db(), &slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?
    else {
        return Err(refused(format!("`{slug}` is not for sale")));
    };
    let money = payments::Money::new(price_minor, currency.clone());
    if !money.is_chargeable() {
        return Err(refused(PaymentRefusal::NotChargeable.message()));
    }

    // The seller is the package's submitter; buying your own package is self-dealing (§20.9.3) and is refused before a payment row exists.
    let seller = darkhold_db::marketplace::submitted_by(state.db(), &slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    let buyer = user.account_id.to_string();
    if seller.as_deref() == Some(buyer.as_str()) {
        return Err(refused(
            "a package's own author cannot buy it; a sale to yourself is not a sale".to_owned(),
        ));
    }

    // Tax comes from the stored profile: an invoice must be able to say what was tax, and computing it later means computing it twice.
    let tax_minor = match darkhold_db::payments::tax_profile(state.db())
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?
    {
        Some(profile) => profile.tax_on(price_minor),
        // No profile is not a failure to sell: a host that has not registered for tax charges none, and the invoice says so by omission.
        None => 0,
    };

    let idempotency = body
        .idempotency_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map_or_else(|| format!("extension:{buyer}:{slug}"), str::to_owned);
    if idempotency.len() > 200 {
        return Err(ApiError(AppError::field(
            "idempotency_key",
            "an idempotency key is at most 200 characters",
        )));
    }

    let version = darkhold_db::gallery::published_versions(state.db(), &slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?
        .into_iter()
        .next();

    let payment = darkhold_db::payments::record_payment(
        state.db(),
        &darkhold_db::payments::NewPayment {
            account: &buyer,
            subject_kind: "extension",
            subject_id: &slug,
            subject_version: version.as_deref(),
            amount_minor: price_minor,
            currency: &currency,
            tax_minor,
            idempotency: &idempotency,
            processor: Processor::Manual.as_str(),
        },
    )
    .await
    .map_err(|error| ApiError(AppError::Internal(error.into())))?;

    // Asked, and honestly reported as not taken. The receipt is what a provider-backed processor would return with `captured: true`.
    let receipt = Processor::Manual.capture(&payment.id, payment.amount_minor);

    Ok(Json(json!({
        "payment": payment.to_json(),
        "net_minor": payment.amount_minor - payment.tax_minor,
        "tax_minor": payment.tax_minor,
        "purchase_state": payment.state.as_str(),
        "processor": {
            "name": Processor::Manual.as_str(),
            "captured": receipt.captured,
            "reference": receipt.reference,
            "note": "no payment provider is configured: an operator captures this payment once the money arrives",
        },
    })))
}

async fn my_payments(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    let payments =
        darkhold_db::payments::my_payments(state.db(), &user.account_id.to_string(), 100)
            .await
            .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    // What the seller is owed, per currency, computed from the payments rather than kept in a running total: the only
    // question a payout asks is "what is captured and not refunded right now", and a stored total would be a second copy
    // of the truth that drifts the first time a refund path forgets it (M70-13).
    let account = user.account_id.to_string();
    let mut currencies: Vec<String> = payments
        .iter()
        .filter_map(|payment| payment.get("currency").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    currencies.sort();
    currencies.dedup();
    let mut net = serde_json::Map::new();
    for currency in currencies {
        let amount = darkhold_db::payments::seller_net(state.db(), &account, &currency)
            .await
            .map_err(|error| ApiError(AppError::Internal(error.into())))?;
        net.insert(currency, Value::from(amount));
    }
    Ok(Json(json!({ "payments": payments, "net": net })))
}

#[derive(Debug, Deserialize, Default)]
pub struct CaptureBody {
    /// A bank or invoice reference, so a captured payment can be reconciled against a statement.
    #[serde(default)]
    pub reference: Option<String>,
}

async fn capture(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(body): Json<CaptureBody>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &user)?;
    let payment = darkhold_db::payments::payment(state.db(), &id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?
        .ok_or_else(|| {
            ApiError(AppError::NotFound {
                resource: "payment",
            })
        })?;
    if payment.state != PaymentState::Pending {
        return Err(refused(format!(
            "this payment is already `{}`",
            payment.state.as_str()
        )));
    }

    let reference = body
        .reference
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map_or_else(
            || {
                Processor::Manual
                    .capture(&payment.id, payment.amount_minor)
                    .reference
            },
            str::to_owned,
        );
    let captured = darkhold_db::payments::capture(state.db(), &id, &reference)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    if !captured {
        // Somebody else captured it between the read and the write; the state is what matters, not who won.
        return Err(refused(
            "this payment was captured by another request".to_owned(),
        ));
    }

    // Capture is the moment the package becomes the buyer's: the grant names the version that was paid for.
    let mut granted = false;
    if let Some(version) = payment.subject_version.as_deref() {
        let document =
            darkhold_db::marketplace::manifest_document(state.db(), &payment.subject_id, version)
                .await
                .map_err(|error| ApiError(AppError::Internal(error.into())))?;
        let permissions = document
            .as_deref()
            .and_then(|text| {
                serde_json::from_str::<darkhold_domain::extension::Manifest>(text).ok()
            })
            .map(|manifest| manifest.permissions)
            .unwrap_or_default();
        darkhold_db::marketplace::grant_extension(
            state.db(),
            &payment.account,
            &payment.subject_id,
            version,
            &permissions,
        )
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
        granted = true;
    }

    let split = Split::reference(payment.amount_minor - payment.tax_minor)
        .ok_or_else(|| refused(PaymentRefusal::BadFee(payments::reference_fee_bp()).message()))?;
    let seller = darkhold_db::marketplace::submitted_by(state.db(), &payment.subject_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    if let Some(seller) = seller {
        // The seller's share is posted at capture, not at purchase: revenue that has not been collected is not revenue.
        let _ = darkhold_db::monetization::post_earnings(
            state.db(),
            Some(&seller),
            split.developer,
            &payment.currency,
            "extension_sale",
            Some(&format!("extension_sale:{}", payment.id)),
            Some(&format!("payment:{}", payment.id)),
        )
        .await;
    }

    Ok(Json(json!({
        "payment": darkhold_db::payments::payment(state.db(), &id)
            .await
            .map_err(|error| ApiError(AppError::Internal(error.into())))?
            .map(|payment| payment.to_json()),
        "granted": granted,
        "split": { "developer_minor": split.developer, "host_minor": split.host },
        "reference": reference,
    })))
}

#[derive(Debug, Deserialize)]
pub struct RefundBody {
    /// Absent means "all that remains", which is the case an operator means nine times in ten.
    #[serde(default)]
    pub amount_minor: Option<i64>,
    pub reason: String,
}

async fn refund(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(body): Json<RefundBody>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &user)?;
    let payment = darkhold_db::payments::payment(state.db(), &id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?
        .ok_or_else(|| {
            ApiError(AppError::NotFound {
                resource: "payment",
            })
        })?;
    if body.reason.trim().is_empty() {
        // A refund with no reason is the one ledger entry nobody can explain later.
        return Err(ApiError(AppError::field(
            "reason",
            "a refund needs a reason",
        )));
    }

    let remaining = payments::refundable(payment.amount_minor, payment.refunded_minor);
    let amount = body.amount_minor.unwrap_or(remaining);
    let cumulative = payments::apply_refund(
        payment.amount_minor,
        payment.refunded_minor,
        amount,
        payment.state,
    )
    .map_err(|refusal| refused(refusal.message()))?;

    let recorded = darkhold_db::payments::record_refund(
        state.db(),
        &id,
        amount,
        body.reason.trim(),
        &user.account_id.to_string(),
    )
    .await
    .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    if !recorded {
        return Err(refused(
            "this payment is not in a state that allows a refund of that amount".to_owned(),
        ));
    }

    let full = is_full_refund(payment.amount_minor, cumulative);
    if full {
        // Access goes with the money. Revoking the grant is version-scoped, so a buyer who paid for 1.0.0 keeps nothing of it and can buy
        // 1.1.0 separately.
        if let Some(version) = payment.subject_version.as_deref() {
            let _ = darkhold_db::extension_review::require_renewal(
                state.db(),
                &payment.subject_id,
                version,
            )
            .await;
            let _ = darkhold_db::marketplace::revoke_extension(
                state.db(),
                &payment.account,
                &payment.subject_id,
            )
            .await;
        }
        // And the seller's revenue is reversed by the same amount they were credited for the net.
        let net = payment.amount_minor - payment.tax_minor;
        if let Some(seller) =
            darkhold_db::marketplace::submitted_by(state.db(), &payment.subject_id)
                .await
                .map_err(|error| ApiError(AppError::Internal(error.into())))?
        {
            if let Some(split) = Split::reference(net) {
                let _ = darkhold_db::monetization::post_earnings(
                    state.db(),
                    Some(&seller),
                    -split.developer,
                    &payment.currency,
                    "extension_refund",
                    Some(&format!("extension_refund:{}", payment.id)),
                    Some(&format!("refund:{}", payment.id)),
                )
                .await;
            }
        }
    }

    Ok(Json(json!({
        "refunded_minor": amount,
        "refunded_total_minor": cumulative,
        "full": full,
        "access_revoked": full,
        "payment": darkhold_db::payments::payment(state.db(), &id)
            .await
            .map_err(|error| ApiError(AppError::Internal(error.into())))?
            .map(|payment| payment.to_json()),
    })))
}

#[derive(Debug, Deserialize)]
pub struct PriceBody {
    pub price_minor: i64,
    pub currency: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

async fn set_price(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(slug): Path<String>,
    Json(body): Json<PriceBody>,
) -> ApiResult<Json<Value>> {
    let seller = darkhold_db::marketplace::submitted_by(state.db(), &slug)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?
        .ok_or_else(|| {
            ApiError(AppError::NotFound {
                resource: "extension",
            })
        })?;
    let is_operator = crate::routes::discovery::require_operator(&state, &user).is_ok();
    // The seller sets the price of their own package; an operator may set it for anyone (a host sometimes has to correct a listing). Anyone
    // else gets a 403 rather than a 404, because the package is public and pretending it is not would be a lie a reader can check.
    if !is_operator && seller != user.account_id.to_string() {
        return Err(ApiError(AppError::AccessDenied));
    }
    if body.price_minor < 0 {
        return Err(ApiError(AppError::field(
            "price_minor",
            "a price cannot be negative",
        )));
    }
    if !(3..=3).contains(&body.currency.len())
        || !body.currency.chars().all(|c| c.is_ascii_uppercase())
    {
        return Err(ApiError(AppError::field(
            "currency",
            "a currency is a three-letter upper-case code",
        )));
    }
    if body.enabled && body.price_minor == 0 {
        return Err(ApiError(AppError::field(
            "price_minor",
            "a package for sale needs a price; leave it disabled to give it away",
        )));
    }
    darkhold_db::payments::set_price(
        state.db(),
        &slug,
        body.price_minor,
        &body.currency,
        body.enabled,
        &user.account_id.to_string(),
    )
    .await
    .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    Ok(Json(json!({
        "manifest_id": slug,
        "price_minor": body.price_minor,
        "currency": body.currency,
        "enabled": body.enabled,
    })))
}

async fn read_tax_profile(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &user)?;
    let profile = darkhold_db::payments::tax_profile(state.db())
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    Ok(Json(json!({
        "profile": profile.map(|profile| json!({
            "legal_name": profile.legal_name,
            "country": profile.country,
            "tax_id": profile.tax_id,
            "rate_bp": profile.rate_bp,
            "invoice_prefix": profile.invoice_prefix,
        })),
    })))
}

#[derive(Debug, Deserialize)]
pub struct TaxProfileBody {
    pub legal_name: String,
    pub country: String,
    #[serde(default)]
    pub tax_id: Option<String>,
    pub rate_bp: i64,
    pub invoice_prefix: String,
}

async fn write_tax_profile(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Json(body): Json<TaxProfileBody>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &user)?;
    let profile = TaxProfile {
        legal_name: body.legal_name.trim().to_owned(),
        country: body.country.trim().to_uppercase(),
        tax_id: body
            .tax_id
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned),
        rate_bp: body.rate_bp,
        invoice_prefix: body.invoice_prefix.trim().to_owned(),
    };
    profile.validate().map_err(refused)?;
    darkhold_db::payments::set_tax_profile(state.db(), &profile)
        .await
        .map_err(|error| ApiError(AppError::Internal(error.into())))?;
    Ok(Json(json!({
        "legal_name": profile.legal_name,
        "country": profile.country,
        "tax_id": profile.tax_id,
        "rate_bp": profile.rate_bp,
        "invoice_prefix": profile.invoice_prefix,
        // Said out loud: storing a profile is not registering for tax. Nothing here files anything.
        "note": "this records the invoice identity and rate; it does not register, file or remit tax",
    })))
}
