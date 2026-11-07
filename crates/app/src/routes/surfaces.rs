//! `GET /recommendation/surfaces` — the surface registry, with what each surface can actually run right now (M64-01).
//!
//! The registry itself lives in `darkhold_domain::recommendation_surfaces`, where it is closed and unit-tested. This file
//! answers the second half of the row: **an algorithm whose data is empty refuses by name**.
//!
//! # Why a refusal and not an empty list
//!
//! An empty ranking is ambiguous. It can mean "nothing matches", "the table is empty because nothing has been imported
//! yet", or "the code that fills this table was never written". The first is an answer; the second and third are bugs
//! wearing the first's clothes. Naming the algorithm and the milestone removes the ambiguity: `tag_cooccurrence` refusing
//! with `M64-06` is a work item, not a mystery.
//!
//! # Cost
//!
//! One `SELECT COUNT(*)` per distinct table the surfaces need — five today, and most of them answer from an index
//! header. The counts are taken once per request for the whole response, not once per surface.

use std::collections::BTreeMap;

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::Value;

use crate::auth::MaybeSession;
use darkhold_domain::error::AppError;

use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

async fn count_rows(state: &AppState, table: &str) -> Option<i64> {
    // The table name comes from `DataNeed::table()`, a closed set in the domain — never from a request. It is still
    // formatted rather than bound because a table name is not a bindable parameter in either dialect.
    let sqlite = format!("SELECT COUNT(*) FROM {table}");
    let postgres = format!("SELECT COUNT(*)::bigint FROM {table}");
    let sql = state.db().sql(&sqlite, &postgres);
    match state.db().backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(&sql)
            .fetch_one(state.db().sqlite_pool().expect("sqlite"))
            .await
            .ok(),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(&sql)
            .fetch_one(state.db().postgres_pool().expect("postgres"))
            .await
            .ok(),
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/recommendation/surfaces", get(list_surfaces))
        .route("/recommendation/experiments", get(list_experiments))
}

/// Per-arm numbers for one day (M64-03).
///
/// A promotion reads these, not the raw log: the numbers were fixed when the day was computed, so an argument about a
/// promotion is an argument about a record rather than about a query whose answer moves while it is being discussed.
#[derive(serde::Deserialize)]
struct ExperimentsQuery {
    /// `YYYY-MM-DD`. Absent means the most recent day with a snapshot.
    day: Option<String>,
}

async fn list_experiments(
    State(state): State<AppState>,
    MaybeSession(_viewer): MaybeSession,
    Query(params): Query<ExperimentsQuery>,
) -> ApiResult<Json<Value>> {
    let snapshots = match params.day.as_deref() {
        Some(day) => {
            if day.len() != 10 {
                return Err(ApiError(AppError::field("day", "is YYYY-MM-DD")));
            }
            let rows = darkhold_db::experiment_snapshots::day_snapshots(state.db(), day)
                .await
                .map_err(|error| {
                    ApiError(AppError::internal("reading experiment snapshots", error))
                })?;
            Some(rows)
        }
        None => darkhold_db::experiment_snapshots::latest_snapshots(state.db())
            .await
            .map_err(|error| ApiError(AppError::internal("reading experiment snapshots", error)))?,
    };

    // `null` and `[]` are different answers on purpose: `null` means no snapshot has ever been computed, `[]` means one
    // was computed and no arm had a single impression. Collapsing them would hide a broken job behind an empty list.
    Ok(Json(match snapshots {
        None => serde_json::json!({ "snapshots": Value::Null }),
        Some(rows) => serde_json::json!({ "snapshots": rows }),
    }))
}

async fn list_surfaces(
    State(state): State<AppState>,
    MaybeSession(_viewer): MaybeSession,
) -> ApiResult<Json<Value>> {
    use darkhold_domain::recommendation_surfaces::{algorithms_for, DataNeed, SURFACES};

    let configured = &state.config().discovery.surfaces;

    // One count per distinct table, not per surface.
    let mut counts: BTreeMap<&'static str, i64> = BTreeMap::new();
    for need in SURFACES.iter().map(|s| s.need) {
        if let Some(table) = need.table() {
            if !counts.contains_key(table) {
                let count = count_rows(&state, table).await.unwrap_or(0);
                counts.insert(table, count);
            }
        }
    }

    // M64-06: tags and engagement can both be plentiful and still **disjoint** — on the live instance `work_tags` joins none of
    // the engaged works, so a "table is empty" refusal never fires while the surface claims to rank. Measured once, not per
    // surface, and only when a WorkTags surface is actually in the registry.
    let tag_overlap = if SURFACES.iter().any(|s| s.need == DataNeed::WorkTags) {
        darkhold_db::tag_coverage::tag_engagement(state.db())
            .await
            .unwrap_or(darkhold_db::tag_coverage::TagEngagement {
                tagged: 0,
                engaged: 0,
                tagged_and_engaged: 0,
            })
    } else {
        darkhold_db::tag_coverage::TagEngagement {
            tagged: 0,
            engaged: 0,
            tagged_and_engaged: 0,
        }
    };

    let surfaces: Vec<Value> = SURFACES
        .iter()
        .map(|surface| {
            let algorithms = algorithms_for(surface, configured);
            let empty = surface
                .need
                .table()
                .map(|table| counts.get(table).copied().unwrap_or(0) == 0)
                .unwrap_or(false);

            // Two different refusals, and they are not the same claim:
            //
            //   * **no engine implements it** — the algorithm is named in the registry but no code runs under that name.
            //     This is a build fact, not a data fact, and it holds whether or not the table has rows.
            //   * **its data is empty** — the code exists and its table is empty, which is a work item with a milestone.
            //
            // Reporting them together is what makes the endpoint useful during the build-out: an operator sees which
            // surfaces are one import away and which are one milestone away.
            let unimplemented: Vec<Value> = algorithms
                .iter()
                .filter(|name| {
                    darkhold_domain::recommendation_surfaces::engine_reason(name).is_none()
                })
                .map(|name| {
                    serde_json::json!({
                        "algorithm": name,
                        "reason": "no engine implements this algorithm yet",
                        "milestone": "",
                    })
                })
                .collect();

            // A tag surface has a second way to be unusable that is not emptiness: rows exist on both sides and never meet.
            let disjoint_tags = surface.need == DataNeed::WorkTags && !tag_overlap.usable();
            let refused: Vec<Value> = if disjoint_tags && !empty {
                algorithms
                    .iter()
                    .map(|name| {
                        serde_json::json!({
                            "algorithm": name,
                            "reason": format!(
                                "`{}` has tags and engagement but no work carries both: {} tagged, {} engaged, 0 in common; M64-06 fills it",
                                name, tag_overlap.tagged, tag_overlap.engaged
                            ),
                            "milestone": surface.need.milestone(),
                        })
                    })
                    .collect()
            } else if empty {
                algorithms
                    .iter()
                    .map(|name| {
                        serde_json::json!({
                            "algorithm": name,
                            "reason": format!(
                                "`{}` has no rows yet; {} fills it",
                                surface.need.table().unwrap_or("its table"),
                                surface.need.milestone()
                            ),
                            "milestone": surface.need.milestone(),
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            };

            let mut refused = refused;
            refused.extend(unimplemented);

            serde_json::json!({
                "id": surface.id,
                "label": surface.label,
                "need": match surface.need {
                    DataNeed::None => "none",
                    DataNeed::WorkTags => "work_tags",
                    DataNeed::Shelves => "shelves",
                    DataNeed::Ratings => "ratings",
                    DataNeed::ArenaBallots => "arena_ballots",
                },
                "algorithms": algorithms,
                "refused": refused,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "surfaces": surfaces })))
}
