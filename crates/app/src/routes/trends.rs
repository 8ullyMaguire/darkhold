//! M45-32 — the trend radar, over HTTP.
//!
//! ```text
//! GET /catalogue/trends?recent_days=14&baseline_days=56&limit=20
//! ```
//!
//! Public: the answer is aggregate catalogue metadata (how many works gained a fandom tag, and when), and it
//! names no reader. It exists so an event or a bounty can be timed against a fandom that is actually
//! arriving, rather than against last year's biggest.
//!
//! The window arithmetic is here and the rule is in `commons_trend`; this module produces the two
//! RFC 3339 bounds, asks the store, and hands each fandom's counts to the rule. **The rule's inputs are
//! printed with the answer** — a caller that disagrees with `emerging` can see the counts and the ratio it
//! was decided on.

use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use commons_trend::{assess, Verdict};
use darkhold_domain::error::AppError;

use crate::auth::MaybeSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

/// Default recent window: two weeks.
const RECENT_DAYS: i64 = 14;
/// Default baseline: the eight weeks before the recent window.
const BASELINE_DAYS: i64 = 56;
/// How many fandoms an answer carries by default.
const LIMIT: usize = 20;
/// The widest window the route will compute, so a query cannot walk the whole table.
const MAX_DAYS: i64 = 365;

/// The route.
pub fn router() -> Router<AppState> {
    Router::new().route("/catalogue/trends", get(trends))
}

#[derive(Debug, Deserialize)]
struct TrendParams {
    /// Length of the window being judged.
    recent_days: Option<i64>,
    /// Length of the baseline before it.
    baseline_days: Option<i64>,
    /// How many fandoms to return.
    limit: Option<usize>,
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError(AppError::Validation {
        message: message.into(),
        field_errors: std::collections::BTreeMap::new(),
    })
}

async fn trends(
    State(state): State<AppState>,
    // Present and unused: this route answers the same for a signed-in reader as for a stranger, and the
    // extractor is what says so. The inventory test reads it as the proof of `Audience::Public`.
    MaybeSession(_session): MaybeSession,
    Query(params): Query<TrendParams>,
) -> ApiResult<Response> {
    let recent_days = params.recent_days.unwrap_or(RECENT_DAYS);
    let baseline_days = params.baseline_days.unwrap_or(BASELINE_DAYS);
    for (name, value) in [
        ("recent_days", recent_days),
        ("baseline_days", baseline_days),
    ] {
        if !(1..=MAX_DAYS).contains(&value) {
            return Err(invalid(format!(
                "{name}: expected 1..={MAX_DAYS}, got {value}"
            )));
        }
    }
    let limit = params.limit.unwrap_or(LIMIT).clamp(1, 200);

    let now = time::OffsetDateTime::now_utc();
    let recent_since = crate::format_rfc3339(now - time::Duration::days(recent_days));
    let baseline_since =
        crate::format_rfc3339(now - time::Duration::days(recent_days + baseline_days));

    let rows = darkhold_db::trend::counts(
        state.db(),
        &baseline_since,
        &recent_since,
        commons_trend::MIN_RECENT,
    )
    .await?;

    let baseline_weeks = baseline_days as f64 / 7.0;
    let recent_weeks = recent_days as f64 / 7.0;
    let mut judged: Vec<(i32, f64, serde_json::Value)> = rows
        .into_iter()
        .map(|row| {
            let counts = commons_trend::Counts {
                recent: row.recent,
                baseline: row.baseline,
                baseline_weeks,
                recent_weeks,
            };
            let assessment = assess(&counts);
            // Emerging first, then new, then the rest by ratio: an answer is read from the top.
            let rank = match assessment.verdict {
                Verdict::Emerging => 0,
                Verdict::New => 1,
                Verdict::Steady => 2,
                Verdict::Fading => 3,
                Verdict::NoVerdict => 4,
            };
            let ratio = assessment.ratio.unwrap_or(0.0);
            let value = serde_json::json!({
                "fandom": row.fandom,
                "recent": row.recent,
                "baseline": row.baseline,
                "ratio": assessment.ratio,
                "verdict": assessment.verdict.as_str(),
                "reason": assessment.reason,
            });
            (rank, ratio, value)
        })
        .collect();
    judged.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    let items: Vec<serde_json::Value> = judged.into_iter().take(limit).map(|row| row.2).collect();

    let body = serde_json::json!({
        "recent_days": recent_days,
        "baseline_days": baseline_days,
        "min_recent": commons_trend::MIN_RECENT,
        "ratio_threshold": commons_trend::RATIO,
        "counts_what": "works that gained this fandom tag in this instance, by the tag's arrival — not the source's publication date",
        "items": items,
    });
    // The answer moves as imports land, so a cache would serve a picture of an hour ago; a caller may
    // revalidate cheaply because the ETag changes with the body.
    let mut response = Json(body).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("public, max-age=300"),
    );
    Ok(response)
}
