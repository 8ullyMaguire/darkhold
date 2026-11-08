//! Preservation over HTTP — the doors M59-11…16 were built behind and never got (M70-13).
//!
//! Four surfaces, and the split is the design:
//!
//! * **the work's own answer** — `GET /works/{id}/preservation` says which destinations carry this work, which of them are
//!   verified, how many the instance enables at all, and whether that is enough. One request, because "how preserved am I"
//!   is one question and a reader who has to assemble it from three has been handed the schema.
//! * **the author's claim** — `PUT /works/{id}/preservation/{destination}` records "this work also exists there". A claim
//!   pays nothing: §2.2 pays for *verified live* preservation, and a claim is a person's word.
//! * **the redistribution assertion** — `PUT /works/{id}/preservation/redistribution`. `yes` pays full, `ask` pays less,
//!   `no` refuses by name. It gates the crosspost rather than the credit, so a refusal is recorded as a refusal.
//! * **the operator's configuration** — the destinations themselves. An archive is instance policy: its base URL, the rule
//!   that identifies one of its item pages, and whether this instance may drive it automatically.
//!
//! And one public read: `/preservation/preservers`, the leaderboard §2.4 asks for. It counts **live** targets, grouped by
//! account — a reader who writes under three pseudonyms is one person on a leaderboard.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use darkhold_domain::preservation::{PreservationEligibility, Redistribution};
use darkhold_domain::AppError;

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/preservation/preservers", get(preservers))
        .route("/works/{id}/preservation", get(work_preservation))
        .route(
            "/works/{id}/preservation/redistribution",
            put(set_redistribution),
        )
        .route("/works/{id}/preservation/{destination}", put(record_target))
        .route("/admin/preservation/destinations", get(list_destinations))
        .route(
            "/admin/preservation/destinations/{slug}",
            put(write_destination),
        )
        .route(
            "/admin/preservation/destinations/{slug}/enabled",
            post(set_destination_enabled),
        )
}

/// The instance's archives, and how many of them are actually enabled.
///
/// Operator-only. A destination is not a link a reader follows; it is a machine this instance will fetch from on a schedule,
/// so who may add one is a policy question and the answer is "not the public".
async fn list_destinations(
    State(state): State<AppState>,
    session: RequireSession,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &session.0)?;
    let destinations = darkhold_db::preservation::list_destinations(state.db())
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let enabled = darkhold_db::preservation::enabled_destination_count(state.db())
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    Ok(Json(json!({
        "enabled": enabled,
        "total": destinations.len(),
        "destinations": destinations
            .iter()
            .map(|d| json!({
                "id": d.id,
                "name": d.name,
                "base_url": d.base_url,
                "match_rule": d.match_rule,
                "accepts_automated": d.accepts_automated,
                "enabled": d.enabled,
                "updated_at": d.updated_at,
            }))
            .collect::<Vec<Value>>(),
    })))
}

#[derive(Debug, Deserialize)]
struct DestinationBody {
    name: String,
    /// Origin only, no trailing slash: `check_one` builds the item URL by appending the record's path, and a trailing slash
    /// makes every fetch a `//`. Refused here rather than normalised, because a base URL that needed editing to be usable is a
    /// base URL whose `match_rule` was probably written against the unedited form.
    base_url: String,
    /// The substring that identifies a *record* page rather than a search or tag page. Required because "does this page name
    /// the work" and "is this page really the work's page" are different questions (§11.12a).
    match_rule: String,
    #[serde(default)]
    accepts_automated: bool,
    #[serde(default = "yes")]
    enabled: bool,
}

fn yes() -> bool {
    true
}

async fn write_destination(
    State(state): State<AppState>,
    session: RequireSession,
    Path(slug): Path<String>,
    Json(body): Json<DestinationBody>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &session.0)?;
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ApiError(AppError::field(
            "id",
            "a destination id is lowercase, digits and hyphens",
        )));
    }
    if body.base_url.trim_end_matches('/').len() != body.base_url.len() {
        return Err(ApiError(AppError::field(
            "base_url",
            "no trailing slash — the item path is appended to it",
        )));
    }
    if !body.base_url.starts_with("https://") && !body.base_url.starts_with("http://") {
        return Err(ApiError(AppError::field(
            "base_url",
            "an absolute http(s) URL",
        )));
    }
    if body.match_rule.trim().is_empty() {
        return Err(ApiError(AppError::field(
            "match_rule",
            "the substring that identifies one of this archive's item pages",
        )));
    }
    let written = darkhold_db::preservation::write_destination(
        state.db(),
        &slug,
        &body.name,
        &body.base_url,
        &body.match_rule,
        body.accepts_automated,
        body.enabled,
    )
    .await
    .map_err(|error| ApiError(AppError::Internal(error)))?;
    Ok(Json(json!({
        "id": written.id,
        "name": written.name,
        "base_url": written.base_url,
        "match_rule": written.match_rule,
        "accepts_automated": written.accepts_automated,
        "enabled": written.enabled,
    })))
}

#[derive(Debug, Deserialize)]
struct EnabledBody {
    enabled: bool,
}

/// Switch a destination on or off.
///
/// Off is not a deletion: the targets recorded against it stand, the recheck skips it, and switching it back on resumes
/// checking. That distinction is why this is its own door rather than a field on the write — an operator turning an archive
/// off during an outage should not have to re-state its URL to do it.
async fn set_destination_enabled(
    State(state): State<AppState>,
    session: RequireSession,
    Path(slug): Path<String>,
    Json(body): Json<EnabledBody>,
) -> ApiResult<Json<Value>> {
    crate::routes::discovery::require_operator(&state, &session.0)?;
    let changed =
        darkhold_db::preservation::set_destination_enabled(state.db(), &slug, body.enabled)
            .await
            .map_err(|error| ApiError(AppError::Internal(error)))?;
    if !changed {
        return Err(ApiError(AppError::NotFound {
            resource: "preservation destination",
        }));
    }
    Ok(Json(json!({ "id": slug, "enabled": body.enabled })))
}

/// The work's owner, or `404`.
///
/// `404` rather than `403` for the reason ADR 0030 gives: whether a work exists is public, but whether *this* account owns it
/// is not something a refusal should confirm.
async fn require_owner(state: &AppState, work_id: &str, account: &str) -> ApiResult<()> {
    let owned = darkhold_db::orphan::owned_by(state.db(), work_id, account)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    if !owned {
        return Err(ApiError(AppError::NotFound { resource: "work" }));
    }
    Ok(())
}

/// How preserved this work is, and what would make it more so.
async fn work_preservation(
    State(state): State<AppState>,
    // Named but unused: the answer is the same for every caller, and the extractor says so — a reader may be signed in, and
    // nothing here varies by who. Removing it would be the more honest simplification, but the route inventory requires an
    // explicit audience for every door and `Public` is the one that fits.
    MaybeSession(_viewer): MaybeSession,
    Path(work_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let targets = darkhold_db::preservation::targets_for_work(state.db(), &work_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let verified = darkhold_db::preservation::verified_count_for_work(state.db(), &work_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let eligible = darkhold_db::preservation::enabled_destination_count(state.db())
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let redistribution = darkhold_db::preservation::read_redistribution(state.db(), &work_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let config = &state.config().preservation;

    // Both live numbers, from the tables rather than from the target list: a dead destination is not
    // eligible-and-verified, and counting it would let a reader keep a badge's worth of progress on archives that no longer
    // answer.
    let eligibility = PreservationEligibility::evaluate(
        usize::try_from(eligible.max(0)).unwrap_or(usize::MAX),
        usize::try_from(verified.max(0)).unwrap_or(usize::MAX),
        usize::try_from(config.threshold.max(1)).unwrap_or(usize::MAX),
    );

    Ok(Json(json!({
        "work_id": work_id,
        "verified": verified,
        "eligible_destinations": eligible,
        "threshold": config.threshold,
        "fully_preserved": eligibility.is_reached(),
        "shortfall": eligibility.reason(),
        "redistribution": redistribution.as_str(),
        "targets": targets
            .iter()
            .map(|t| json!({
                "destination_id": t.destination_id,
                "state": t.state.as_str(),
                "verified_at": t.verified_at,
                "dead_at": t.dead_at,
                "credits_paid": t.credits_paid,
                "external_url": t.external_url,
                "external_record_id": t.external_record_id,
            }))
            .collect::<Vec<Value>>(),
    })))
}

async fn set_redistribution(
    State(state): State<AppState>,
    session: RequireSession,
    Path(work_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    let account = session.0.account_id.to_string();
    require_owner(&state, &work_id, &account).await?;
    let asked = body
        .get("redistribution")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError(AppError::field("redistribution", "yes · ask · no")))?;
    let Some(value) = Redistribution::parse_stored(Some(asked)) else {
        return Err(ApiError(AppError::field(
            "redistribution",
            "yes · ask · no",
        )));
    };
    darkhold_db::preservation::write_redistribution(state.db(), &work_id, value)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    Ok(Json(json!({
        "work_id": work_id,
        "redistribution": value.as_str(),
        "permits_crosspost": value.permits_crosspost(),
    })))
}

#[derive(Debug, Deserialize)]
struct RecordBody {
    /// The destination's own id for the record — not this instance's id for the work. Two archives name the same work
    /// differently, and storing ours twice would make them look like one crosspost.
    external_record_id: String,
    #[serde(default)]
    external_url: Option<String>,
}

/// Record that this work also exists at a destination.
///
/// Pays nothing, deliberately: §2.2 pays for verified live preservation, and this is one person's word. The recheck job turns
/// a claim into a verified target — or into a dead one — and the credits follow that.
async fn record_target(
    State(state): State<AppState>,
    session: RequireSession,
    Path((work_id, destination)): Path<(String, String)>,
    Json(body): Json<RecordBody>,
) -> ApiResult<Json<Value>> {
    let account = session.0.account_id.to_string();
    require_owner(&state, &work_id, &account).await?;

    // The author's assertion gates the claim, not only the credit: an author who said `no` has refused the crosspost, and
    // recording one anyway would store a refusal and a claim about the same work.
    let redistribution = darkhold_db::preservation::read_redistribution(state.db(), &work_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    if !redistribution.permits_crosspost() {
        return Err(ApiError(AppError::field(
            "redistribution",
            "this work's author has refused redistribution; no crosspost can be recorded against it",
        )));
    }

    // §2.7: a work this instance *imported* from somewhere else is already preserved elsewhere, by the archive that hosts it —
    // recording this instance as its preserver would claim credit for somebody else's copy. A query rather than a column, so
    // that deleting the import makes the work local again.
    let imported = darkhold_db::preservation::is_imported(state.db(), &work_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    if imported {
        return Err(ApiError(AppError::field(
            "work",
            "this work was imported from another instance and is preserved there; §2.7 refuses a second claim",
        )));
    }

    let Some(destination_row) = darkhold_db::preservation::list_destinations(state.db())
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?
        .into_iter()
        .find(|d| d.id == destination)
    else {
        return Err(ApiError(AppError::NotFound {
            resource: "preservation destination",
        }));
    };

    // A work's identity is how "this work, elsewhere" is expressed exactly once (A0): the member row hangs off it, so it has to
    // exist before a crosspost can be recorded.
    let identity = darkhold_db::story_identity::identity_for_work(state.db(), &work_id)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    let identity_id = match identity {
        Some(identity) => identity.id,
        None => {
            let title = darkhold_db::preservation::published_title(state.db(), &work_id)
                .await
                .map_err(|error| ApiError(AppError::Internal(error)))?
                .ok_or_else(|| ApiError(AppError::NotFound { resource: "work" }))?;
            darkhold_db::story_identity::ensure_identity_for_work(state.db(), &work_id, &title)
                .await
                .map_err(|error| ApiError(AppError::Internal(error)))?
        }
    };

    let record = darkhold_db::preservation::record_target(
        state.db(),
        &identity_id,
        &work_id,
        &destination_row.id,
        &body.external_record_id,
        body.external_url.as_deref(),
        Some(uuid::Uuid::from(session.0.account_id)),
    )
    .await
    .map_err(|error| ApiError(AppError::Internal(error)))?;

    Ok(Json(json!({
        "member_id": record.member_id,
        "destination_id": record.destination_id,
        "state": record.state.as_str(),
        "external_record_id": record.external_record_id,
        "external_url": record.external_url,
        "credits_paid": record.credits_paid,
    })))
}

#[derive(Debug, Deserialize)]
struct PreserversQuery {
    /// Weeks back to count. Defaults to four.
    #[serde(default)]
    days: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

/// Who has preserved most, by live verified target.
async fn preservers(
    State(state): State<AppState>,
    Query(query): Query<PreserversQuery>,
) -> ApiResult<Json<Value>> {
    let days = query.days.unwrap_or(28).clamp(1, 365);
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let since = (time::OffsetDateTime::now_utc() - time::Duration::days(days))
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| ApiError(AppError::Internal(anyhow::anyhow!(error.to_string()))))?;
    let rows = darkhold_db::preservation::top_preservers(state.db(), &since, limit)
        .await
        .map_err(|error| ApiError(AppError::Internal(error)))?;
    Ok(Json(json!({
        "since": since,
        "days": days,
        "preservers": rows
            .iter()
            // The account id is the identity the store groups by, and a leaderboard of opaque ids is a leaderboard nobody
            // can read; the handle is looked up where the page needs it. Not expanded here: this is a list of thirty
            // people, and thirty lookups per request is the shape of N+1 that turns a cheap page into a slow one.
            .map(|(account, destinations)| json!({
                "account_id": account,
                "destinations": destinations,
            }))
            .collect::<Vec<Value>>(),
    })))
}
