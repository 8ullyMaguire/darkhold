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

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::Value;

use crate::auth::MaybeSession;
use crate::http::ApiResult;
use crate::state::AppState;

async fn count_rows(state: &AppState, table: &str) -> Option<i64> {
    // The table name comes from `DataNeed::table()`, a closed set in the domain — never from a request. It is still
    // formatted rather than bound because a table name is not a bindable parameter in either dialect.
    let sqlite = format!("SELECT COUNT(*) FROM {table}");
    let postgres = format!("SELECT COUNT(*)::bigint FROM {table}");
    let sql = state.db().sql(&sqlite, &postgres);
    match state.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .fetch_one(state.db().sqlite_pool().expect("sqlite"))
                .await
                .ok()
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .fetch_one(state.db().postgres_pool().expect("postgres"))
                .await
                .ok()
        }
    }
}

pub fn router() -> Router<AppState> {
    Router::new().route("/recommendation/surfaces", get(list_surfaces))
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

    let surfaces: Vec<Value> = SURFACES
        .iter()
        .map(|surface| {
            let algorithms = algorithms_for(surface, configured);
            let empty = surface
                .need
                .table()
                .map(|table| counts.get(table).copied().unwrap_or(0) == 0)
                .unwrap_or(false);

            // A refusal names the algorithm and what would fill it. All algorithms on a surface share the surface's
            // declared need, so a surface is refused whole or not at all — a per-algorithm need is a future refinement,
            // and inventing one now would be a claim about data nobody has measured.
            let refused: Vec<Value> = if empty {
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
