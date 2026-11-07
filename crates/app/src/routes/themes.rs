//! M16-13 — the theme doors.
//!
//! ```text
//! GET    /themes                  what is stored for me (drafts and the published one) + what renders
//! GET    /themes/effective        the palette and layout that actually render, and why
//! POST   /themes                  save a draft (validation errors come back, nothing is published)
//! POST   /themes/{id}/publish     publish a draft; contrast failures refuse here
//! DELETE /themes                  reset: delete what is published for me
//! POST   /themes/{id}/preview     preview a theme in this session for 15 minutes
//! POST   /operator/themes/safe-mode  switch safe mode on or off
//! ```
//!
//! # Who a theme speaks for
//!
//! A reader's door always speaks for the session's own account and always saves a reader-scoped theme, which
//! may set a palette and nothing else. The instance's theme is the operator's, and the same `POST /themes`
//! writes it when — and only when — the caller asks for `"scope": "instance"` and passes the operator check.
//! That is the one place in this module where the body changes which door it is, and it is checked before
//! anything is written.
//!
//! # Reset is reachable by construction
//!
//! `DELETE /themes` is not themed and not routed through anything a theme can restyle: it answers with core
//! values and deletes the published row. Safe mode is the same argument one level up — when it is on, no
//! theme is consulted at all, so the surface that says "safe mode is on" cannot be styled by the theme that
//! broke.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_domain::theme_model::{self, Effective, Placement, SafeMode, Scope, Slot, Theme, Token};
use darkhold_domain::AppError;

/// How long a preview lasts. Long enough to look at a page twice, short enough that a forgotten preview is a
/// curiosity rather than an appearance.
const PREVIEW_SECONDS: i64 = 15 * 60;

/// The theme routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/themes", get(list_themes).post(save_theme).delete(reset_theme))
        .route("/themes/effective", get(effective_theme))
        .route("/themes/{id}/publish", post(publish_theme))
        .route("/themes/{id}/preview", post(preview_theme))
        .route("/operator/themes/safe-mode", post(set_safe_mode))
}

#[derive(Debug, Deserialize)]
struct SaveBody {
    /// The theme's name; also its identity.
    name: String,
    /// Token name (as emitted, e.g. `--dh-ink`) to value.
    #[serde(default)]
    palette: BTreeMap<String, String>,
    /// Slot name to placement. Refused for a reader theme.
    #[serde(default)]
    layout: BTreeMap<String, String>,
    /// `instance` (operator only) or `reader` (the default: the caller's own).
    #[serde(default)]
    scope: Option<String>,
}

impl SaveBody {
    /// Turn the wire shapes into the model, ignoring names the model does not define.
    fn into_theme(self) -> Theme {
        let mut palette = BTreeMap::new();
        for (name, value) in &self.palette {
            if let Some(token) = Token::all().iter().find(|t| t.css_name() == name) {
                palette.insert(*token, value.clone());
            }
        }
        let mut layout = BTreeMap::new();
        for (name, value) in &self.layout {
            if let (Some(slot), Some(placement)) =
                (Slot::all().iter().find(|s| s.name() == name), Placement::parse(value))
            {
                layout.insert(*slot, placement);
            }
        }
        Theme {
            name: self.name,
            palette,
            layout,
        }
    }
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// `"instance"` needs the operator check; anything else is the caller's own reader scope.
fn scope_for(
    state: &AppState,
    user: &crate::auth::SessionUser,
    requested: Option<&str>,
) -> ApiResult<(Scope, &'static str, String)> {
    if requested == Some("instance") {
        crate::routes::experiment_sweep::require_operator(state, user)?;
        return Ok((Scope::Instance, "instance", darkhold_db::theme_profiles::INSTANCE_OWNER.to_owned()));
    }
    let account = user.account_id.to_string();
    Ok((Scope::Reader(account.clone()), "reader", account))
}

/// Store what is stored for the caller, plus what renders.
async fn list_themes(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
) -> ApiResult<Json<Value>> {
    let account = user.account_id.to_string();
    let mut rows = darkhold_db::theme_profiles::list(state.db(), "reader", &account)
        .await
        .map_err(ApiError::from)?;
    // An operator also sees the instance's themes; a reader does not need to know they exist.
    if crate::routes::experiment_sweep::require_operator(&state, &user).is_ok() {
        rows.extend(
            darkhold_db::theme_profiles::list(
                state.db(),
                "instance",
                darkhold_db::theme_profiles::INSTANCE_OWNER,
            )
            .await
            .map_err(ApiError::from)?,
        );
    }
    let effective = resolve(&state, &user).await?;
    Ok(Json(json!({ "themes": rows, "effective": effective })))
}

/// Save a draft. Invalid themes are saved-free: the refusals are the answer.
async fn save_theme(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Json(body): Json<SaveBody>,
) -> ApiResult<Json<Value>> {
    let (scope, scope_name, owner) = scope_for(&state, &user, body.scope.as_deref())?;
    let theme = body.into_theme();

    let refusals = theme.validate(&scope);
    if !refusals.is_empty() {
        let reasons: Vec<String> = refusals.iter().map(|r| r.message()).collect();
        return Err(ApiError(AppError::field(
            "theme",
            reasons.join("; "),
        )));
    }

    let id = darkhold_db::theme_profiles::save(state.db(), scope_name, &owner, &theme, &now_rfc3339())
        .await
        .map_err(ApiError::from)?;
    Ok(Json(json!({
        "id": id,
        "state": "draft",
        "scope": scope_name,
        "declarations": theme.declarations(),
        "contrast": "every required pair passes",
    })))
}

/// Publish a theme the caller owns.
async fn publish_theme(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    // Publishing is the moment a theme becomes an appearance, so the check runs here rather than only on
    // save: a draft written when the rule was looser must not be published by a later call.
    let rows = darkhold_db::theme_profiles::list(state.db(), "reader", &user.account_id.to_string())
        .await
        .map_err(ApiError::from)?;
    let mut candidates: Vec<(String, Value)> = rows
        .iter()
        .filter(|r| r["id"] == json!(id))
        .map(|r| ("reader".to_owned(), r.clone()))
        .collect();
    if crate::routes::experiment_sweep::require_operator(&state, &user).is_ok() {
        let rows = darkhold_db::theme_profiles::list(
            state.db(),
            "instance",
            darkhold_db::theme_profiles::INSTANCE_OWNER,
        )
        .await
        .map_err(ApiError::from)?;
        candidates.extend(
            rows.iter()
                .filter(|r| r["id"] == json!(id))
                .map(|r| ("instance".to_owned(), r.clone())),
        );
    }
    let Some((scope_name, row)) = candidates.into_iter().next() else {
        return Err(ApiError(AppError::field("id", "no theme with that id belongs to you")));
    };

    let scope = if scope_name == "instance" {
        Scope::Instance
    } else {
        Scope::Reader(user.account_id.to_string())
    };
    let owner = if scope_name == "instance" {
        darkhold_db::theme_profiles::INSTANCE_OWNER.to_owned()
    } else {
        user.account_id.to_string()
    };

    // Rebuild the theme from the stored row and validate it: what is published is what was checked here.
    let theme = theme_from_row(&row);
    let refusals = theme.validate(&scope);
    if !refusals.is_empty() {
        let reasons: Vec<String> = refusals.iter().map(|r| r.message()).collect();
        return Err(ApiError(AppError::field("theme", reasons.join("; "))));
    }

    let published = darkhold_db::theme_profiles::publish(
        state.db(),
        &scope_name,
        &owner,
        &id,
        &now_rfc3339(),
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(json!({
        "id": id,
        "published": published,
        "scope": scope_name,
        "declarations": theme.declarations(),
        "layout": wire_layout(&theme.layout_with_core()),
    })))
}

/// Slots and placements as the wire spells them (`nav` -> `inline`), which is what the console sends back.
fn wire_layout(layout: &BTreeMap<Slot, Placement>) -> BTreeMap<&str, &str> {
    layout
        .iter()
        .map(|(slot, placement)| (slot.name(), placement.name()))
        .collect()
}

fn theme_from_row(row: &Value) -> Theme {
    let mut palette = BTreeMap::new();
    if let Some(map) = row["palette"].as_object() {
        for token in Token::all() {
            if let Some(value) = map.get(token.css_name()).and_then(Value::as_str) {
                palette.insert(*token, value.to_owned());
            }
        }
    }
    let mut layout = BTreeMap::new();
    if let Some(map) = row["layout"].as_object() {
        for slot in Slot::all() {
            if let Some(placement) = map.get(slot.name()).and_then(Value::as_str) {
                if let Some(parsed) = Placement::parse(placement) {
                    layout.insert(*slot, parsed);
                }
            }
        }
    }
    Theme {
        name: row["name"].as_str().unwrap_or_default().to_owned(),
        palette,
        layout,
    }
}

/// What renders for the caller, and why.
async fn effective_theme(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
) -> ApiResult<Json<Value>> {
    Ok(Json(resolve(&state, &user).await?))
}

async fn resolve(state: &AppState, user: &crate::auth::SessionUser) -> ApiResult<Value> {
    let safe_mode = darkhold_db::theme_profiles::safe_mode(state.db())
        .await
        .map_err(ApiError::from)?;
    let instance = darkhold_db::theme_profiles::published(
        state.db(),
        "instance",
        darkhold_db::theme_profiles::INSTANCE_OWNER,
    )
    .await
    .map_err(ApiError::from)?
    .map(|stored| stored.theme);
    let reader = darkhold_db::theme_profiles::published(state.db(), "reader", &user.account_id.to_string())
        .await
        .map_err(ApiError::from)?
        .map(|stored| stored.theme);

    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let preview = darkhold_db::theme_profiles::preview_for(state.db(), &user.session_id.to_string(), now)
        .await
        .map_err(ApiError::from)?
        .map(|stored| stored.theme);

    let effective = theme_model::effective(
        &safe_mode,
        instance.as_ref(),
        reader.as_ref(),
        preview.as_ref(),
    );
    Ok(render(&effective, &safe_mode))
}

fn wire_palette(palette: &BTreeMap<Token, String>) -> BTreeMap<&str, &str> {
    palette
        .iter()
        .map(|(token, value)| (token.css_name(), value.as_str()))
        .collect()
}

fn render(effective: &Effective, safe_mode: &SafeMode) -> Value {
    match effective {
        Effective::Core { reason } => json!({
            "source": "core",
            "safe_mode": true,
            "safe_mode_reason": reason,
            "palette": wire_palette(&theme_model::core_palette()),
            "layout": wire_layout(&theme_model::core_layout()),
            "declarations": Theme { name: String::new(), palette: BTreeMap::new(), layout: BTreeMap::new() }.declarations(),
        }),
        Effective::Themed { palette, layout, source } => json!({
            "source": source,
            "safe_mode": safe_mode.active,
            "palette": wire_palette(palette),
            "layout": wire_layout(layout),
            "declarations": palette
                .iter()
                .map(|(token, value)| format!("{}: {value};", token.css_name()))
                .collect::<Vec<String>>()
                .join("\n"),
            "protected_surfaces": darkhold_domain::theme_model::Surface::all()
                .iter()
                .map(|surface| json!({ "surface": format!("{surface:?}"), "mechanism": surface.mechanism() }))
                .collect::<Vec<Value>>(),
        }),
    }
}

/// Reset: delete whatever is published for the caller's own scope.
async fn reset_theme(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
) -> ApiResult<Json<Value>> {
    let removed = darkhold_db::theme_profiles::reset(state.db(), "reader", &user.account_id.to_string())
        .await
        .map_err(ApiError::from)?;
    Ok(Json(json!({
        "reset": true,
        "removed": removed,
        "effective": "core",
        "note": "reset deletes the theme rather than writing the default, so a later change to the default is not a theme drift",
    })))
}

/// Preview a theme in this session only.
async fn preview_theme(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let now = time::OffsetDateTime::now_utc();
    let expires = now + time::Duration::seconds(PREVIEW_SECONDS);
    let expires_at = expires
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();

    darkhold_db::theme_profiles::start_preview(
        state.db(),
        &id,
        &user.session_id.to_string(),
        &expires_at,
    )
    .await
    .map_err(ApiError::from)?;

    Ok(Json(json!({
        "id": id,
        "preview": true,
        "expires_at": expires_at,
        "seconds": PREVIEW_SECONDS,
        "note": "this preview is visible to this session only; safe mode overrides it, because the theme on preview is the one most likely to be broken",
    })))
}

#[derive(Debug, Deserialize)]
struct SafeModeBody {
    active: bool,
    #[serde(default)]
    reason: Option<String>,
}

/// Operator: enter or leave safe mode.
async fn set_safe_mode(
    State(state): State<AppState>,
    crate::auth::RequireSession(user): crate::auth::RequireSession,
    Json(body): Json<SafeModeBody>,
) -> ApiResult<Json<Value>> {
    crate::routes::experiment_sweep::require_operator(&state, &user)?;

    let mode = if body.active {
        let Some(reason) = body.reason.filter(|r| !r.trim().is_empty()) else {
            return Err(ApiError(AppError::field(
                "reason",
                "safe mode needs a reason: a state nobody can explain is a bug report with no subject",
            )));
        };
        SafeMode::on(reason)
    } else {
        SafeMode::off()
    };

    darkhold_db::theme_profiles::set_safe_mode(state.db(), &mode, &now_rfc3339())
        .await
        .map_err(ApiError::from)?;
    Ok(Json(json!({
        "active": mode.active,
        "reason": mode.reason,
        "effective": if mode.active { "core" } else { "themes" },
    })))
}
