//! The public catalogue of syndicated works (M74-01).
//!
//! Metadata is public; body text is not. Imported items stay owned by the instance account in
//! `library_items` — no new table and no publication flag. A reader below trust 2 receives the same
//! shape as an item with no stored body: the `chapters` key is added only at trust 2, so the payload
//! cannot be used as an existence oracle for withheld prose.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::auth::MaybeSession;
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;

const MAX_ITEMS: i64 = 50;
const BODY_TRUST: i64 = 2;
const MAX_BODY_CHARS: usize = 200_000;

pub fn router() -> Router<AppState> {
    Router::new().route("/catalogue/syndicated", get(syndicated_catalogue))
}

fn fault(error: impl std::fmt::Display) -> ApiError {
    ApiError::from(anyhow::anyhow!("{error}"))
}

async fn trust_level(state: &AppState, viewer: Option<crate::auth::SessionUser>) -> i64 {
    let Some(viewer) = viewer else {
        return 0;
    };
    darkhold_db::governance::trust_for(state.db(), &viewer.account_id.to_string())
        .await
        .unwrap_or(0)
}

async fn syndicated_catalogue(
    State(state): State<AppState>,
    MaybeSession(viewer): MaybeSession,
) -> ApiResult<Json<Value>> {
    let email = &state.config().syndication.account_email;
    let Some(account) = darkhold_db::identity::find_account_by_email(state.db(), email)
        .await
        .map_err(fault)?
    else {
        return Ok(Json(json!({ "items": [] })));
    };
    let items = darkhold_db::imports::list_library_items(
        state.db(),
        &account.id.to_string(),
        MAX_ITEMS,
        None,
    )
    .await
    .map_err(fault)?;
    let trust = trust_level(&state, viewer).await;
    let may_read_body = trust >= BODY_TRUST;
    let store = darkhold_db::storage::BlobStore::new(state.config().storage.root.clone());
    let mut out = Vec::with_capacity(items.len());

    for item in items {
        let mut record = json!({
            "source_work_key": item.source_work_key,
            "title": item.title,
            "author": item.author_text,
            "source": item.source_key,
            "source_url": item.source_url,
            "summary": item.summary,
            "status": item.status,
            "word_count": item.word_count,
        });
        if may_read_body {
            let mut chapters = Vec::new();
            for unit in darkhold_db::imports::latest_units_for_item(state.db(), &item.id)
                .await
                .map_err(fault)?
            {
                if unit.state != "stored" {
                    continue;
                }
                let Some(checksum) = unit.content_blob_checksum else {
                    continue;
                };
                let Some(bytes) = store.get(state.db(), &checksum).await.map_err(fault)? else {
                    continue;
                };
                let mut body = String::from_utf8_lossy(&bytes).into_owned();
                if body.chars().count() > MAX_BODY_CHARS {
                    body = body.chars().take(MAX_BODY_CHARS).collect();
                    body.push('…');
                }
                chapters.push(json!({
                    "ordinal": unit.ordinal,
                    "title": unit.title,
                    "body": body,
                }));
            }
            record["chapters"] = json!(chapters);
        }
        out.push(record);
    }

    Ok(Json(json!({ "items": out })))
}
