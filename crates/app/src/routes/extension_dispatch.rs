//! M16-12's dispatch point: running a package's declarative recipe, and the consent rule that governs
//! everything that runs instead of composing.
//!
//! ```text
//! POST /extensions/{slug}/apply-recipe   copy an installed recipe package into my own recipe library
//! ```
//!
//! # Why one door and not a runner
//!
//! §16 defines a `recipe` as "a JSON composition of existing parts" and §28 repeats it: recipes are
//! declarative configuration, and scripting lives in the WASM sandbox. So there is nothing to execute here —
//! the honest implementation of "run this recipe" is to validate the composition and store it where recipes
//! live, owned by the reader who asked. That is also what makes the opt-out question answerable: a document
//! that may only carry strategy names cannot hand a reader's text anywhere, so `custom_processing: refuse`
//! does not block a recipe, and blocking it would be a false positive that teaches readers the setting is
//! noise.
//!
//! # The consent check is here, not in the engine
//!
//! [`requires_custom_processing`] lives in the domain and this route asks it *before* doing anything with the
//! package. A check placed after the call would be an audit, not a permission. For a `recommendation_engine`
//! package the answer is "yes, consent applies" and an opted-out reader is refused with the setting's own
//! name; for a `declarative_recipe` the answer is "no" and the reader is not asked for permission to do
//! something that never leaves the process.

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::extension::Category;
use darkhold_domain::recipe_document::{self, Processing};
use darkhold_domain::AppError;

/// The dispatch routes.
pub fn router() -> Router<AppState> {
    Router::new().route("/extensions/{slug}/apply-recipe", post(apply_recipe))
}

#[derive(Debug, Deserialize)]
struct ApplyBody {
    /// The recipe document. Validated against this instance's own strategies.
    recipe: Value,
    /// What to call the reader's copy. Defaults to the package's own name.
    #[serde(default)]
    name: Option<String>,
}

/// Whether running a package of this category may hand the reader's data outside the process.
///
/// Re-exported here so a future runner has one function to ask, and so the route and any test agree about
/// where the answer comes from.
pub fn requires_custom_processing(category: Category) -> bool {
    recipe_document::processing(category) == Processing::Runs
}

async fn apply_recipe(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Path(slug): Path<String>,
    Json(body): Json<ApplyBody>,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();

    let lookup = darkhold_db::marketplace::get_extension(state.db(), &slug)
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    let Some(extension) = lookup else {
        return Err(ApiError(AppError::field("slug", "no such package")));
    };

    // Only an approved package may be applied, and only by somebody who installed it. Installing is not the
    // same act as running: a reader may hold a package they have not switched on.
    if extension["state"].as_str() != Some("approved") {
        return Err(ApiError(AppError::field(
            "slug",
            "this package is not approved, so nothing of it may be applied",
        )));
    }
    // Installs are per acting pseud (M16-08), because "installed" is a statement about a face a reader
    // wears, not about the account behind it.
    let Some(pseud) = user.pseud_id else {
        return Err(ApiError(AppError::field(
            "session",
            "choose a pseud before applying a package",
        )));
    };
    let installed = darkhold_db::gallery::my_installations(state.db(), &pseud.to_string())
        .await
        .map_err(|e| ApiError(AppError::Internal(e.into())))?;
    if !installed
        .iter()
        .any(|row| row["manifest_id"] == json!(slug))
    {
        return Err(ApiError(AppError::field(
            "slug",
            "install this package before applying its recipe",
        )));
    }

    let document = extension["document"].as_str().unwrap_or_default();
    let manifest: darkhold_domain::extension::Manifest =
        serde_json::from_str(document).map_err(|e| {
            ApiError(AppError::field(
                "slug",
                format!("this package's manifest does not parse: {e}"),
            ))
        })?;

    // The consent gate, before anything is read out of the body beyond its shape.
    if requires_custom_processing(manifest.category) {
        let allowed = crate::privacy::custom_processing_allowed(state.db(), &account)
            .await
            .map_err(ApiError::from)?;
        if !allowed {
            return Err(ApiError(AppError::field(
                "custom_processing",
                "this package runs code or drives an engine, so applying it is custom processing; \
                 it is switched off for your account (Settings → Privacy → custom_processing)",
            )));
        }
    }

    if manifest.category != Category::DeclarativeRecipe {
        return Err(ApiError(AppError::field(
            "slug",
            format!(
                "this package is a {}; only a declarative recipe can be applied this way",
                manifest.category.as_str()
            ),
        )));
    }

    let known = crate::rec_engine::available_strategies();
    let recipe = recipe_document::parse(&body.recipe, &known).map_err(|refusals| {
        ApiError(AppError::field(
            "recipe",
            refusals
                .iter()
                .map(recipe_document::Refusal::message)
                .collect::<Vec<String>>()
                .join("; "),
        ))
    })?;

    let name = body.name.unwrap_or_else(|| manifest.id.clone());
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    // Stored from the validated value, never echoed from the request: a recipe that carried something the
    // validator did not look at cannot reach the table.
    darkhold_db::discovery::save_recipe(
        state.db(),
        &id,
        &account,
        &name,
        &recipe.document(),
        false,
        &created_at,
    )
    .await
    .map_err(AppError::Internal)?;

    Ok(Json(json!({
        "id": id,
        "name": name,
        "package": slug,
        "stages": recipe.stages.iter().map(|s| json!({
            "strategy": s.strategy,
            "weight": s.weight,
        })).collect::<Vec<Value>>(),
        "total_weight": recipe.total_weight(),
        "custom_processing_required": false,
        "note": "a recipe composes strategies the instance already runs, so it processes nothing of yours outside the host",
    })))
}
