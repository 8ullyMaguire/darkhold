use crate::auth::{MaybeSession, RequirePseud};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use darkhold_domain::browse::Sort;
use darkhold_domain::AppError;
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/browse/surfaces", get(list_surfaces))
        .route(
            "/browse/sort/{surface}",
            get(get_sort).put(set_sort).delete(delete_sort),
        )
        .route("/people", get(list_people))
        .route("/tags", get(list_tags))
        .route("/tags/{tag}", get(list_works_by_tag))
        // M69-19: related tags ranked by NPMI, not by raw co-occurrence. A tag that is on everything relates to
        // nothing, and raw counts cannot say that.
        .route("/tags/{tag}/related", get(list_related_tags))
        .route("/fandoms", get(list_fandoms))
        .route("/fandoms/{fandom}", get(list_works_by_fandom))
}

// --- Surfaces -----------------------------------------------------------

#[derive(Debug, Serialize)]
struct SurfaceInfo {
    key: String,
    label: String,
    default_sort: String,
}

async fn list_surfaces(
    State(_state): State<AppState>,
    MaybeSession(_session): MaybeSession,
) -> ApiResult<Json<Vec<SurfaceInfo>>> {
    let surfaces = vec![
        SurfaceInfo {
            key: "discover".into(),
            label: "Discover".into(),
            default_sort: Sort::ForYou.as_str().into(),
        },
        SurfaceInfo {
            key: "people".into(),
            label: "People".into(),
            default_sort: Sort::Az.as_str().into(),
        },
        SurfaceInfo {
            key: "library".into(),
            label: "Library".into(),
            default_sort: Sort::New.as_str().into(),
        },
        SurfaceInfo {
            key: "tags".into(),
            label: "Tags".into(),
            default_sort: Sort::Az.as_str().into(),
        },
        SurfaceInfo {
            key: "fandoms".into(),
            label: "Fandoms".into(),
            default_sort: Sort::Az.as_str().into(),
        },
        SurfaceInfo {
            key: "collections".into(),
            label: "Collections".into(),
            default_sort: Sort::New.as_str().into(),
        },
        SurfaceInfo {
            key: "series".into(),
            label: "Series".into(),
            default_sort: Sort::New.as_str().into(),
        },
        SurfaceInfo {
            key: "authors".into(),
            label: "Authors".into(),
            default_sort: Sort::Az.as_str().into(),
        },
    ];
    Ok(Json(surfaces))
}

// --- Sort state ---------------------------------------------------------

#[derive(Debug, Serialize)]
struct SortStateResponse {
    surface: String,
    sort: String,
    source: String,
}

async fn get_sort(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    Path(surface): Path<String>,
) -> ApiResult<Json<SortStateResponse>> {
    let surface_default = default_for(&surface);

    let Some(user) = session else {
        return Ok(Json(SortStateResponse {
            surface,
            sort: surface_default.as_str().into(),
            source: "default".into(),
        }));
    };

    let Some(pseud_id) = user.pseud_id else {
        return Ok(Json(SortStateResponse {
            surface,
            sort: surface_default.as_str().into(),
            source: "default".into(),
        }));
    };

    let pref =
        darkhold_db::browse::get_sort_preference(state.db(), &pseud_id.to_string(), &surface)
            .await
            .map_err(|e| ApiError(AppError::internal("reading sort preference", e)))?;

    let (sort, source) = match pref {
        Some(pref) if Sort::parse(&pref.sort_value).is_some() => {
            (pref.sort_value, "preference".to_string())
        }
        _ => (surface_default.as_str().into(), "default".into()),
    };
    Ok(Json(SortStateResponse {
        surface,
        sort,
        source,
    }))
}

#[derive(Debug, Deserialize)]
struct SetSortBody {
    sort: String,
}

async fn set_sort(
    State(state): State<AppState>,
    RequirePseud { pseud_id, .. }: RequirePseud,
    Path(surface): Path<String>,
    Json(body): Json<SetSortBody>,
) -> ApiResult<Json<SortStateResponse>> {
    let sort = Sort::parse(&body.sort).ok_or_else(|| {
        ApiError(AppError::field(
            "sort",
            format!(
                "unknown sort `{}`; accepted: {}",
                body.sort,
                Sort::accepted_set()
            ),
        ))
    })?;

    darkhold_db::browse::set_sort_preference(
        state.db(),
        &pseud_id.to_string(),
        &surface,
        sort.as_str(),
    )
    .await
    .map_err(|e| ApiError(AppError::internal("saving sort preference", e)))?;

    Ok(Json(SortStateResponse {
        surface,
        sort: sort.as_str().into(),
        source: "preference".into(),
    }))
}

async fn delete_sort(
    State(state): State<AppState>,
    RequirePseud { pseud_id, .. }: RequirePseud,
    Path(surface): Path<String>,
) -> ApiResult<Json<SortStateResponse>> {
    darkhold_db::browse::delete_sort_preference(state.db(), &pseud_id.to_string(), &surface)
        .await
        .map_err(|e| ApiError(AppError::internal("deleting sort preference", e)))?;

    let sort = default_for(&surface).as_str().to_string();
    Ok(Json(SortStateResponse {
        surface,
        sort,
        source: "default".into(),
    }))
}

fn default_for(surface: &str) -> Sort {
    match surface {
        "discover" => Sort::ForYou,
        "people" | "tags" | "fandoms" | "authors" | "moods" => Sort::Az,
        "library" | "collections" | "series" | "reading-paths" => Sort::New,
        _ => Sort::New,
    }
}

// --- People -------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PeopleQuery {
    #[serde(default)]
    sort: Option<String>,
    #[serde(default = "default_page_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}

fn default_page_limit() -> i64 {
    50
}

#[derive(Debug, Serialize)]
struct PersonItem {
    id: String,
    handle: String,
    display_name: String,
    work_count: i64,
}

async fn list_people(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    Query(params): Query<PeopleQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let sort =
        resolve_surface_sort(&state, session.as_ref(), params.sort.as_deref(), "people").await;

    let people = darkhold_db::identity::list_discoverable_pseuds(
        state.db(),
        params.limit.clamp(1, 200),
        params.offset.max(0),
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?;

    let mut items: Vec<PersonItem> = people
        .into_iter()
        .map(|(id, handle, display_name, work_count)| PersonItem {
            id,
            handle,
            display_name,
            work_count,
        })
        .collect();

    apply_sort(&mut items, &sort);

    Ok(Json(serde_json::json!({ "items": items, "sort": sort })))
}

fn apply_sort(items: &mut [PersonItem], sort: &str) {
    match sort {
        "new" => items.sort_by(|a, b| b.id.cmp(&a.id)),
        "updated" => items.sort_by(|a, b| b.id.cmp(&a.id)),
        "trending" => items.sort_by_key(|i| std::cmp::Reverse(i.work_count)),
        "top" => items.sort_by_key(|i| std::cmp::Reverse(i.work_count)),
        "az" => items.sort_by(|a, b| {
            a.display_name
                .to_lowercase()
                .cmp(&b.display_name.to_lowercase())
        }),
        "for-you" => items.sort_by_key(|i| std::cmp::Reverse(i.work_count)),
        _ => {}
    }
}

// --- Tags ---------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TagListQuery {
    #[serde(default)]
    sort: Option<String>,
    #[serde(default = "default_page_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}

#[derive(Debug, Serialize)]
struct TagItem {
    id: String,
    canonical: String,
    kind: String,
    work_count: i64,
}

async fn list_tags(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    Query(params): Query<TagListQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let sort = resolve_surface_sort(&state, session.as_ref(), params.sort.as_deref(), "tags").await;

    // **The browse shows the TREE, and the tree is the entities above the
    // floor** (spec §6.4, R-23). An entity below it is an assertion: the work
    // says "this is a Slow Burn", and the taxonomy does not claim there is a
    // Slow Burn *node*. The assertion is still stored, still searchable and
    // still countable -- the floor decides what appears in the tree and not
    // what the instance knows.
    let (tags, total) = darkhold_db::entity_floor::browsable(
        state.db(),
        "tag",
        params.limit.clamp(1, 200),
        params.offset.max(0),
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e.into())))?;

    let mut items: Vec<TagItem> = tags
        .into_iter()
        .map(|(id, canonical, kind, work_count)| TagItem {
            id,
            canonical,
            kind,
            work_count,
        })
        .collect();

    apply_tag_sort(&mut items, &sort);

    Ok(Json(serde_json::json!({
        "items": items,
        "sort": sort,
        // **The total is the count of the entities ABOVE THE FLOOR**, not the
        // count of the rows in the table. A reader who sees "12 of 900" learns
        // something, and a reader who sees "900" learns nothing.
        "total": total,
        "floor": state.config().taxonomy.entity_floor,
    })))
}

fn apply_tag_sort(items: &mut [TagItem], sort: &str) {
    match sort {
        "new" => items.sort_by(|a, b| b.id.cmp(&a.id)),
        "updated" => items.sort_by(|a, b| b.id.cmp(&a.id)),
        "trending" => items.sort_by_key(|i| std::cmp::Reverse(i.work_count)),
        "top" => items.sort_by_key(|i| std::cmp::Reverse(i.work_count)),
        "az" => items.sort_by_key(|a| a.canonical.to_lowercase()),
        "for-you" => items.sort_by_key(|i| std::cmp::Reverse(i.work_count)),
        _ => {}
    }
}

#[derive(Debug, Deserialize)]
pub struct WorksByTagQuery {
    #[serde(default)]
    sort: Option<String>,
    #[serde(default = "default_page_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
    /// §11.4's `+children` expansion: include works tagged with any descendant of this tag.
    ///
    /// Off by default. A browse surface that silently widened what a reader asked for would be answering a question they did not
    /// ask, and the expansion is a property of the *query* (`+children`), not of the tag.
    #[serde(default)]
    children: bool,
}

#[derive(Debug, Serialize)]
struct WorksByTagItem {
    work_id: String,
    title: String,
    author_handle: String,
    updated_at: String,
    /// Where the text lives, in the API's own vocabulary (M71-04). Filled after the list is built, in one grouped
    /// query, so the badge a reader sees and the word an API client reads are the same fact.
    provenance: String,
}

async fn list_works_by_tag(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    Path(tag): Path<String>,
    Query(params): Query<WorksByTagQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let sort = resolve_surface_sort(&state, session.as_ref(), params.sort.as_deref(), "tags").await;

    // **The reader's own filters**, and they are applied INSIDE the pool
    // query (spec §10.2, §12.7). A warning is information and never a gate on
    // the whole archive: a reader who filters "graphic violence" sees a
    // *filtered list*, and the filter is visible and reversible.
    let filters = reader_content_filters(state.db(), session.as_ref()).await?;

    // §11.4's expansion, when the query asked for it: the tag's descendants come from the materialised closure (one join, not a
    // recursion), and the listing is then a single statement over [this tag] ∪ [its descendants] so that paging and the total are
    // properties of one result.
    //
    // An unknown tag is **not** an error and does not become one here: the single-tag listing has always answered an empty page
    // for a name the vocabulary does not hold, and `children=true` on such a name must answer the same empty page. The lookup is
    // therefore allowed to fail into the plain listing rather than turning a 200 into a 404.
    let listed = match (params.children, darkhold_db::taxonomy::node_id_by_norm(state.db(), "tag", &tag).await)
    {
        (true, Ok(Some(node_id))) => {
            let mut names = vec![tag.clone()];
            match darkhold_db::entity_closure::descendants_of(state.db(), &node_id).await {
                Ok(descendants) if !descendants.is_empty() => {
                    let ids: Vec<String> = descendants.into_iter().map(|(id, _)| id).collect();
                    match darkhold_db::taxonomy::norms_for_node_ids(state.db(), &ids).await {
                        Ok(found) => names.extend(found),
                        Err(error) => {
                            tracing::warn!(%error, "the +children expansion could not resolve its nodes");
                        }
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(%error, "the +children expansion could not read the closure");
                }
            }
            darkhold_db::taxonomy::works_by_tags_filtered(
                state.db(),
                &names,
                params.limit.clamp(1, 200),
                params.offset.max(0),
                &filters,
            )
            .await
        }
        _ => {
            darkhold_db::taxonomy::works_by_tag_filtered(
                state.db(),
                &tag,
                params.limit.clamp(1, 200),
                params.offset.max(0),
                &filters,
            )
            .await
        }
    }
    .map_err(|e| ApiError(AppError::Internal(e)))?;

    let mut items: Vec<WorksByTagItem> = listed
        .works
        .into_iter()
        .map(
            |(work_id, title, author_handle, updated_at)| WorksByTagItem {
                work_id,
                title,
                author_handle,
                updated_at,
                provenance: "known".to_owned(),
            },
        )
        .collect();

    apply_works_sort(&mut items, &sort);

    // One query for the page, not one per row (M71-04).
    let ids: Vec<String> = items.iter().map(|item| item.work_id.clone()).collect();
    let provenance = super::provenance::for_work_ids(&state, &ids).await;
    for item in items.iter_mut() {
        item.provenance = provenance
            .get(&item.work_id)
            .copied()
            .unwrap_or(super::provenance::Provenance::Known)
            .as_str()
            .to_owned();
    }

    // M64-10: the facets over exactly this tag's works, so the reader narrows rather than starts over.
    let facets = darkhold_db::facets::all_facets(
        state.db(),
        &darkhold_db::facets::FacetFilter {
            tag: Some(tag.clone()),
            tag_kind: Some("tag".to_owned()),
            title_contains: None,
            min_words: None,
            max_words: None,
            max_rating: None,
        },
    )
    .await
    .unwrap_or_default();

    // M71-13: how much of the archive this tag describes. "12 works" reads as "this is the tag"; "12 of 3,400 works in this archive
    // are tagged slow burn (0.4%)" is the fact the page has.
    let coverage = darkhold_db::taxonomy::dimension_coverage(state.db(), "tag", &tag)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;

    Ok(Json(serde_json::json!({
        "items": items,
        "sort": sort,
        "tag": tag,
        "facets": facets,
        "coverage": coverage,
        // The count is the count of what SURVIVED the filter, not the count
        // before it. A reader told there are forty works and shown twelve,
        // with nothing saying which twelve, has been told a lie.
        "total": listed.total,
        // **The filter is visible.** A reader who cannot see why a work is
        // missing cannot undo the thing that hid it, and "the filter is
        // visible and reversible" is the whole of spec §10.2.
        "filters": filters
            .iter()
            .map(|filter| serde_json::json!({ "kind": filter.kind, "value": filter.norm }))
            .collect::<Vec<serde_json::Value>>(),
    })))
}

/// Tags that co-occur with this one, ranked by NPMI (M69-19).
async fn list_related_tags(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
    Path(tag): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let norm = darkhold_domain::exchange::normalise(&tag);
    let related = darkhold_db::taxonomy::related_tags_by_npmi(state.db(), &norm, 20)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    Ok(Json(serde_json::json!({
        "tag": tag,
        "items": related
            .into_iter()
            .map(|(name, npmi, count)| serde_json::json!({
                "tag": name,
                "npmi": npmi,
                "cooccurrences": count,
            }))
            .collect::<Vec<serde_json::Value>>(),
    })))
}

/// The reader's content filters, as the pool query needs them.
///
/// **Per reader, per face** (spec §10.2): the filters live in
/// `content_filters`, keyed by the pseud, and they are not a global setting.
/// A signed-out reader has none, and a reader whose filters are empty sees
/// everything.
async fn reader_content_filters(
    db: &darkhold_db::Database,
    session: Option<&crate::auth::SessionUser>,
) -> ApiResult<Vec<darkhold_db::taxonomy::ContentFilter>> {
    let Some(user) = session else {
        return Ok(Vec::new());
    };
    let Some(pseud) = user.pseud_id else {
        return Ok(Vec::new());
    };
    let rows = darkhold_db::settings::list_content_filters(db, pseud.as_uuid())
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;
    Ok(rows
        .into_iter()
        .map(|row| darkhold_db::taxonomy::ContentFilter {
            kind: row.filter_type,
            norm: row.value,
        })
        .collect())
}

// --- Fandoms ------------------------------------------------------------

#[derive(Debug, Serialize)]
struct FandomItem {
    slug: String,
    name: String,
    work_count: i64,
}

async fn list_fandoms(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    Query(params): Query<TagListQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let sort =
        resolve_surface_sort(&state, session.as_ref(), params.sort.as_deref(), "fandoms").await;

    let fandoms = darkhold_db::taxonomy::list_fandoms(
        state.db(),
        params.limit.clamp(1, 200),
        params.offset.max(0),
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?;

    let mut items: Vec<FandomItem> = fandoms
        .into_iter()
        .map(|(slug, name, work_count)| FandomItem {
            slug,
            name,
            work_count,
        })
        .collect();

    apply_fandom_sort(&mut items, &sort);

    Ok(Json(serde_json::json!({ "items": items, "sort": sort })))
}

fn apply_fandom_sort(items: &mut [FandomItem], sort: &str) {
    match sort {
        "new" => items.sort_by(|a, b| b.slug.cmp(&a.slug)),
        "updated" => items.sort_by(|a, b| b.slug.cmp(&a.slug)),
        "trending" => items.sort_by_key(|i| std::cmp::Reverse(i.work_count)),
        "top" => items.sort_by_key(|i| std::cmp::Reverse(i.work_count)),
        "az" => items.sort_by_key(|a| a.name.to_lowercase()),
        "for-you" => items.sort_by_key(|i| std::cmp::Reverse(i.work_count)),
        _ => {}
    }
}

async fn list_works_by_fandom(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    Path(fandom): Path<String>,
    Query(params): Query<WorksByTagQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let sort =
        resolve_surface_sort(&state, session.as_ref(), params.sort.as_deref(), "fandoms").await;

    let works = darkhold_db::taxonomy::works_by_fandom(
        state.db(),
        &fandom,
        params.limit.clamp(1, 200),
        params.offset.max(0),
    )
    .await
    .map_err(|e| ApiError(AppError::Internal(e)))?;

    let mut items: Vec<WorksByTagItem> = works
        .into_iter()
        .map(
            |(work_id, title, author_handle, updated_at)| WorksByTagItem {
                work_id,
                title,
                author_handle,
                updated_at,
                provenance: "known".to_owned(),
            },
        )
        .collect();

    apply_works_sort(&mut items, &sort);

    // M64-10: the same facets under the fandom, which is the same join with a different `kind`.
    let facets = darkhold_db::facets::all_facets(
        state.db(),
        &darkhold_db::facets::FacetFilter {
            tag: Some(fandom.clone()),
            tag_kind: Some("fandom".to_owned()),
            title_contains: None,
            min_words: None,
            max_words: None,
            max_rating: None,
        },
    )
    .await
    .unwrap_or_default();

    // M71-13: the same coverage fact as the tag page, because a fandom page with "9 works" says the same misleading thing.
    let coverage = darkhold_db::taxonomy::dimension_coverage(state.db(), "fandom", &fandom)
        .await
        .map_err(|e| ApiError(AppError::Internal(e)))?;

    Ok(Json(serde_json::json!({
        "items": items,
        "sort": sort,
        "fandom": fandom,
        "facets": facets,
        "coverage": coverage,
    })))
}

// --- Works (shared) -----------------------------------------------------

fn apply_works_sort(items: &mut [WorksByTagItem], sort: &str) {
    match sort {
        "new" => items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at)),
        "updated" => items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at)),
        "trending" => items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at)),
        "top" => items.sort_by(|a, b| a.title.cmp(&b.title)),
        "az" => items.sort_by_key(|a| a.title.to_lowercase()),
        "for-you" => items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at)),
        _ => {}
    }
}

// --- Shared sort resolver ----------------------------------------------

async fn resolve_surface_sort(
    state: &AppState,
    session: Option<&crate::auth::SessionUser>,
    query_sort: Option<&str>,
    surface: &str,
) -> String {
    // 1. Query param takes precedence.
    if let Some(sort_str) = query_sort {
        if Sort::parse(sort_str).is_some() {
            return sort_str.to_string();
        }
    }

    // 2. Stored preference.
    if let Some(user) = session {
        if let Some(pseud_id) = &user.pseud_id {
            if let Ok(Some(pref)) =
                darkhold_db::browse::get_sort_preference(state.db(), &pseud_id.to_string(), surface)
                    .await
            {
                if Sort::parse(&pref.sort_value).is_some() {
                    return pref.sort_value;
                }
            }
        }
    }

    // 3. Default for the surface.
    default_for(surface).as_str().to_string()
}
