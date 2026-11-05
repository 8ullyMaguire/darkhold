//! The public catalogue: search the fanfiction index by source and quality.
//!
//! ```text
//! GET /catalogue?source=sb&sort=quality&q=winter&min_likes=100&limit=20&offset=0
//! ```
//!
//! # Why this route is public and unauthenticated
//!
//! Every row is metadata about a story that lives somewhere else — title, author,
//! summary, word count, and the endorsement count the source itself publishes.
//! No story text is stored here, and no reader's shelf, tag or reading status is
//! reachable through it: `library_items` is per-account in the *writer's* sense,
//! and this route reads the catalogue accounts only. The estate rule that keeps
//! the catalogue metadata-only is what makes an unauthenticated read safe; if it
//! ever stopped being metadata-only, this route would have to stop being public.
//!
//! # The metric vocabulary
//!
//! `sort=` takes either a stored column (`likes`, `aggregate`, `views`, `words`,
//! `updated`) or a quality metric (`quality`, `source_pct`, `source_z`,
//! `engagement_density`, `consensus`, `crosspost`, `words_maturity`,
//! `source_coverage`, `cohort_pct`, `recency`, `unmeasured`) — see
//! [`darkhold_domain::quality`] for the maths of each.
//!
//! The distinction matters: a stored column is an `ORDER BY`, and ordering by
//! `likes` rewards whichever archive is largest. A metric is a percentile
//! computed *within the row's own source* before anything is compared, which is
//! what makes the ranking independent of archive size. A metric page therefore
//! answers with `ranked` of `total` — on the live catalogue, `1247 of 141317`,
//! because only 1,247 rows carry an endorsement count at all — and the rest of
//! the page is metadata triage, labelled `measured: false`.
//!
//! `likes` remains the default so that nothing changes for a caller that did not
//! ask, and `aggregate` still adds the same story's likes from every other
//! source plus the kudos it earned on darkhold.

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::{MaybeSession, RequireSession};
use crate::http::{ApiError, ApiResult};
use crate::state::AppState;
use darkhold_db::catalogue::{
    self, CatalogueQuery, CatalogueRow, CatalogueSort, FandomCount, SourceCount,
};
use darkhold_domain::AppError;

/// What a shell types for a flag.
///
/// `1`, `true`, `yes` and `on` are all true; absent or any empty string is false.
/// A value that is none of those is a `422` rather than a silent false, because a
/// caller who wrote `--exclude-read=maybe` meant something.
fn flag(raw: &str) -> ApiResult<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" => Ok(false),
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => Err(invalid(format!(
            "exclude_read: \"{other}\" is not a flag; use 1 or 0"
        ))),
    }
}

/// A query string the route cannot honour: a `422` that names the field.
fn invalid(message: impl Into<String>) -> ApiError {
    AppError::Validation {
        message: message.into(),
        field_errors: std::collections::BTreeMap::new(),
    }
    .into()
}

/// The query string, in the shape axum reads it.
#[derive(Debug, Deserialize)]
pub struct CatalogueParams {
    source: Option<String>,
    #[serde(default)]
    min_likes: Option<i64>,
    q: Option<String>,
    sort: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    offset: Option<i64>,
    /// Window on the source's clock: when the author last touched it.
    updated: Option<String>,
    /// Window on our clock: when the crawler last rechecked it.
    refreshed: Option<String>,
    /// Window on entry age: when it entered this catalogue.
    added: Option<String>,
    /// Restrict to one fandom, e.g. `naruto`. Case-insensitive; an unknown name
    /// is a `422` that names the field and suggests the nearest known fandom.
    fandom: Option<String>,
    /// Restrict to one author: exact, case-insensitive, trimmed (M63-04).
    author: Option<String>,
    /// Similar to this work: an id, a source URL, or a title (M63-04).
    seed: Option<String>,
    /// Drop rows this session is already reading. Needs a session (M63-04).
    ///
    /// Text rather than a `bool` because `exclude_read=1` is what a shell types
    /// and `serde` refuses it for a `bool`, answering `400 Failed to deserialize
    /// query string` — which reads as a syntax error rather than as the accepted
    /// spellings of a flag. `rec_run` learned this for `explain=1` already.
    #[serde(default)]
    exclude_read: Option<String>,
}

/// One row of a metric-ranked page.
#[derive(Debug, Serialize)]
pub struct MetricItem {
    #[serde(flatten)]
    row: CatalogueRow,
    /// 0..=1 for every ranked metric.
    score: f64,
    /// False when the metric could not measure the row and metadata triage
    /// ordered it. The two blocks are never mixed in a way that hides this.
    measured: bool,
    /// The components of the score, so a ranking can be argued with.
    signals: darkhold_domain::quality::Signals,
    /// 1-based position on this page.
    rank: usize,
    /// The source's last-update date, which [`CatalogueRow`] does not carry.
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
}

/// What a metric-ranked page answers: the page, plus the gate.
///
/// `ranked` of `total` is the line that keeps a thin ranking from reading as a
/// verdict: on the live catalogue it is `1247 of 141317`, because only 1,247 rows
/// carry any endorsement count at all.
#[derive(Debug, Serialize)]
pub struct MetricPage {
    items: Vec<MetricItem>,
    sort: String,
    metric: String,
    /// Engaged rows the metric could score, across the whole filtered set.
    ranked: i64,
    /// Rows the filters matched.
    total: i64,
    limit: i64,
    offset: i64,
    sources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fandom: Option<String>,
}

/// What the route answers.
#[derive(Debug, Serialize)]
pub struct CataloguePage {
    /// The rows on this page. `pub` because M63-14 renders them as a feed: one execution path, two clients.
    pub items: Vec<CatalogueRow>,
    sort: String,
    limit: i64,
    offset: i64,
    /// The sources present in this page, so a UI can label the filter honestly.
    sources: Vec<String>,
    /// The fandom this page answered for, spelled as the catalogue has it.
    ///
    /// Present only when the request carried `fandom=`. Echoing the resolved
    /// spelling back is what lets a caller that typed `naruto` show `Naruto`.
    #[serde(skip_serializing_if = "Option::is_none")]
    fandom: Option<String>,
    /// How many catalogue rows carry any fandom tag at all, and how many rows
    /// there are.
    ///
    /// Together they are the coverage line. `--fandom naruto` returning 265 rows
    /// is a complete answer only if the caller learns that 94.6% of the
    /// catalogue has no fandom metadata — otherwise a short page reads as a bad
    /// ranking instead of a missing tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    fandom_tagged_rows: Option<i64>,
    /// Of those tagged rows, how many publish a like count at all.
    ///
    /// The second half of the coverage line, and the one that explains a page of
    /// zeros: on the live catalogue the fandom tags come from one source whose
    /// rows carry no endorsements, so an ordered page is ordered by nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    fandom_tagged_with_likes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalogue_rows: Option<i64>,
}

/// One row of a seed search: the catalogue row, how similar it is, and the tokens
/// that carried it.
#[derive(Debug, Serialize)]
pub struct SimilarItem {
    #[serde(flatten)]
    row: CatalogueRow,
    /// 0..=1 similarity to the seed row.
    score: f64,
    /// Shared tokens, most informative first — the reason a reader can argue with.
    shared: Vec<String>,
    /// 1-based position on this page.
    rank: usize,
    /// The source's last-update date, which [`CatalogueRow`] does not carry.
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
}

/// What the seed the caller named resolved to.
#[derive(Debug, Serialize)]
pub struct SeedRef {
    id: String,
    title: String,
}

/// A page of "similar to this one".
///
/// `ranked` counts the neighbours in the *scanned* population and `total` is what
/// the filters matched, so a page cannot read as a statement about the whole
/// catalogue when the ranker only looked at 5,000 rows.
#[derive(Debug, Serialize)]
pub struct SimilarPage {
    items: Vec<SimilarItem>,
    sort: String,
    metric: String,
    seed: SeedRef,
    ranked: i64,
    total: i64,
    scanned: i64,
    limit: i64,
    offset: i64,
}

/// Mount the catalogue's read routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/catalogue", get(search))
        .route("/catalogue/sources", get(sources))
        .route("/catalogue/fandoms", get(fandoms))
        .route("/catalogue/export", get(export_manifest))
        .route("/catalogue/export.sqlite", get(export_sqlite))
        // M45-67: the works a reader wants more of are often not here. A list is resolved line by
        // line — catalogue row when it exists, stored evidence when it does not, a reason when it is
        // neither — and evidence is never turned into a work.
        .route(
            "/catalogue/seeds",
            post(resolve_external_seeds).get(list_external_seeds),
        )
        // M45-71: the shelf as a document — a judgement that can be reviewed, diffed and posted.
        .route("/catalogue/taste-brief", get(taste_brief))
}

async fn search(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    Query(params): Query<CatalogueParams>,
) -> ApiResult<Response> {
    // `seed` decides the order by itself, so it is tried before the metric
    // vocabulary: `--seed X --sort source_pct` would otherwise be ranked by the
    // metric and the seed would be silently ignored, which is the failure this
    // whole item exists to prevent.
    let seed_spec = params
        .seed
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    if let Some(spec) = seed_spec {
        return Ok(
            Json(similar_page(state, params, session.map(|s| s.account_id), &spec).await?)
                .into_response(),
        );
    }
    let exclude_read = match params.exclude_read.as_deref() {
        Some(raw) => flag(raw)?,
        None => false,
    };
    if exclude_read {
        // Without a session there is no reading history to exclude, and answering
        // 200 with nothing excluded would tell the caller the flag worked.
        return Err(invalid(
            "exclude_read: reading history belongs to a session, and this request has none \
             (send a token or a session cookie)",
        ));
    }
    // A quality metric is not a column, so it cannot be an `ORDER BY`: it is a
    // percentile computed over the filtered population, and the store ranks it in
    // memory (M63-03). The vocabulary is tried first so a metric name is never
    // mistaken for an unknown sort.
    let sort_word = params.sort.as_deref().map(str::trim).unwrap_or("");
    if let Some(metric) = darkhold_domain::quality::Metric::parse(sort_word) {
        use darkhold_domain::quality::Support;
        match metric.support() {
            Support::Blocked(reason) => {
                return Err(invalid(format!(
                    "sort: \"{}\" is not available yet — {reason}",
                    metric.refusal_name(sort_word)
                )))
            }
            Support::Raw => return Ok(Json(search_raw(state, params).await?).into_response()),
            Support::Ranked | Support::Triage => {
                let limit = params.limit.unwrap_or(20);
                if !(1..=100).contains(&limit) {
                    return Err(invalid("limit must be between 1 and 100"));
                }
                let offset = params.offset.unwrap_or(0).max(0);
                let query = build_query(&state, &params, None).await?;
                let page = darkhold_db::metrics::ranked(
                    state.db(),
                    &query,
                    metric,
                    limit as usize,
                    offset as usize,
                )
                .await?;
                let mut sources: Vec<String> = page
                    .items
                    .iter()
                    .map(|i| i.row.source_key.clone())
                    .collect();
                sources.sort();
                sources.dedup();
                return Ok(Json(MetricPage {
                    items: page
                        .items
                        .into_iter()
                        .map(|item| MetricItem {
                            row: item.row,
                            score: item.score,
                            measured: item.measured,
                            signals: item.signals,
                            rank: item.rank,
                            updated_at: item.updated_at,
                        })
                        .collect(),
                    sort: metric.as_str().to_owned(),
                    metric: metric.as_str().to_owned(),
                    ranked: page.ranked,
                    total: page.total,
                    limit,
                    offset,
                    sources,
                    fandom: query.fandom.clone(),
                })
                .into_response());
            }
        }
    }
    Ok(Json(search_raw(state, params).await?).into_response())
}

/// The catalogue as a plain `ORDER BY` page — every sort the store can do in SQL.
pub(crate) async fn search_raw(
    state: AppState,
    params: CatalogueParams,
) -> ApiResult<CataloguePage> {
    let sort = match params.sort.as_deref() {
        None => CatalogueSort::Likes,
        Some(raw) => {
            CatalogueSort::parse(raw).ok_or_else(|| invalid(format!("unknown sort: {raw}")))?
        }
    };
    let limit = params.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(invalid("limit must be between 1 and 100"));
    }
    let fandom = resolve_fandom(&state, params.fandom.as_deref()).await?;
    let (tagged, rankable, total) = match fandom.is_some() {
        true => {
            let coverage = catalogue::fandom_coverage(state.db()).await?;
            (
                Some(coverage.tagged),
                Some(coverage.with_likes),
                Some(catalogue::total_rows(state.db()).await?),
            )
        }
        false => (None, None, None),
    };
    let mut query = build_query(&state, &params, None).await?;
    query.sort = Some(sort);
    query.limit = Some(limit);
    let items = catalogue::search(state.db(), &query).await?;
    let mut sources: Vec<String> = items.iter().map(|row| row.source_key.clone()).collect();
    sources.sort();
    sources.dedup();
    Ok(CataloguePage {
        items,
        sort: sort.as_str().to_owned(),
        limit,
        offset: query.offset.unwrap_or(0),
        sources,
        fandom: query.fandom.clone(),
        fandom_tagged_rows: tagged,
        fandom_tagged_with_likes: rankable,
        catalogue_rows: total,
    })
}

/// Resolve the request into a [`CatalogueQuery`]: the fandom name against the
/// list the catalogue knows, and the three clocks through the one window parser
/// the CLI shares.
async fn build_query(
    state: &AppState,
    params: &CatalogueParams,
    exclude_account: Option<String>,
) -> ApiResult<CatalogueQuery> {
    let now = time::OffsetDateTime::now_utc();
    let window = |raw: &Option<String>, field: &str| match raw {
        None => Ok(None),
        Some(text) => darkhold_domain::window::parse(text, now)
            .map(Some)
            .map_err(|e| invalid(format!("{field}: {e}"))),
    };
    Ok(CatalogueQuery {
        source: params.source.clone(),
        min_likes: params.min_likes,
        q: params.q.clone(),
        sort: Some(CatalogueSort::Likes),
        limit: params.limit,
        offset: Some(params.offset.unwrap_or(0)),
        updated: window(&params.updated, "updated")?,
        refreshed: window(&params.refreshed, "refreshed")?,
        added: window(&params.added, "added")?,
        fandom: resolve_fandom(state, params.fandom.as_deref()).await?,
        author: params
            .author
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        // Filled by the seed path, which is the only caller that has a seed to
        // preselect against.
        tokens: Vec::new(),
        exclude_account,
    })
}

/// The tokens a seed search preselects its candidates with.
///
/// Longest first, because length is a cheap proxy for rarity and the rare tokens
/// are what make a candidate plausible at all; capped at six so the `OR` stays a
/// scan a reader can wait for. The summary only contributes when the title is too
/// short to identify anything — a one-word title like "Gamer" matches most of the
/// catalogue, and a seven-word one already narrows it to the story.
/// The words the candidate scan will look for: the seeds' titles first, then their summaries.
///
/// Built over a **set** (M45-66). The per-seed cap stays six title tokens, but the set gets a
/// budget too: twenty short seeds would otherwise preselect on a hundred words, which is the
/// catalogue rather than a taste. Six per seed up to twelve total, in the order the reader named
/// them, so the first seed they thought of still leads.
fn preselection_tokens(seeds: &[darkhold_db::metrics::SeedLookup]) -> Vec<String> {
    const TOTAL_CAP: usize = 12;
    let mut tokens: Vec<String> = Vec::new();
    for seed in seeds {
        let mut mine = darkhold_domain::similar::tokens(&seed.title);
        mine.sort_by_key(|token| std::cmp::Reverse(token.len()));
        mine.truncate(6);
        if mine.len() < 2 {
            let mut body = darkhold_domain::similar::tokens(&seed.summary);
            body.sort_by_key(|token| std::cmp::Reverse(token.len()));
            for token in body {
                if mine.contains(&token) {
                    continue;
                }
                mine.push(token);
                if mine.len() >= 6 {
                    break;
                }
            }
        }
        for token in mine {
            if tokens.len() >= TOTAL_CAP {
                return tokens;
            }
            if !tokens.contains(&token) {
                tokens.push(token);
            }
        }
    }
    tokens
}

/// Resolve one name in a set to exactly one row, refusing with the candidates when it cannot (M45-66).
///
/// The refusal names **which** name failed: a set query with one ambiguous seed would otherwise print
/// a list of candidates with no clue which of five names produced them.
async fn resolve_one_seed(
    state: &AppState,
    spec: &str,
    account: Option<&str>,
) -> ApiResult<darkhold_db::metrics::SeedLookup> {
    match darkhold_db::metrics::resolve_seed(state.db(), spec).await? {
        darkhold_db::metrics::SeedMatch::One(row) => Ok(row),
        darkhold_db::metrics::SeedMatch::None => {
            // M45-67: an id that names stored evidence ranks too — otherwise the list a reader
            // pasted would be a table nobody can use, and the point of storing it was to use it.
            if let Some(account_id) = account {
                if let Some(evidence) =
                    darkhold_db::external_seed::by_id(state.db(), account_id, spec).await?
                {
                    return Ok(darkhold_db::metrics::SeedLookup {
                        id: evidence.id,
                        title: evidence.title,
                        // No author and no summary: the instance has not seen this work, and
                        // inventing either would be a claim it cannot support.
                        author: String::new(),
                        summary: String::new(),
                        source_key: evidence.source_key,
                        likes: 0,
                    });
                }
            }
            Err(ApiError(AppError::NotFound {
                resource: Box::leak(format!("catalogue row for seed \"{spec}\"").into_boxed_str()),
            }))
        }
        darkhold_db::metrics::SeedMatch::Many(rows) => {
            let named: Vec<String> = rows
                .iter()
                .take(6)
                .map(|row| {
                    format!(
                        "{} [{}] {}L ({})",
                        row.title, row.source_key, row.likes, row.id
                    )
                })
                .collect();
            Err(invalid(format!(
                "seed \"{spec}\": {} rows match and none is dominant — pick one by id: {}",
                rows.len(),
                named.join("; ")
            )))
        }
    }
}

/// Answer `/catalogue?seed=…`: the rows most similar to one row (M63-04).
async fn similar_page(
    state: AppState,
    params: CatalogueParams,
    account: Option<darkhold_domain::ids::AccountId>,
    spec: &str,
) -> ApiResult<SimilarPage> {
    // A seed decides the order. Refusing the combination is the only honest
    // answer: accepting it and ordering by the metric would drop the seed
    // silently, and accepting it and ordering by similarity would drop the metric.
    if params.sort.as_deref().is_some_and(|s| !s.trim().is_empty()) {
        return Err(invalid(
            "sort: a seed already decides the order; ask for the tiebreak you want instead of \
             a second ordering",
        ));
    }
    let exclude_read = match params.exclude_read.as_deref() {
        Some(raw) => flag(raw)?,
        None => false,
    };
    let exclude_account =
        match (exclude_read, account) {
            (false, _) => None,
            (true, Some(account)) => Some(account.to_string()),
            (true, None) => return Err(invalid(
                "exclude_read: reading history belongs to a session, and this request has none \
                 (send a token or a session cookie)",
            )),
        };
    let limit = params.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(invalid("limit must be between 1 and 100"));
    }
    let offset = params.offset.unwrap_or(0).max(0);

    // M45-66: `seed=a,b,c` ranks by the set. The cap is a query-cost bound, not a taste limit: each
    // seed adds its vocabulary to one token union, and a hundred seeds is a search for "everything".
    const MAX_SEEDS: usize = 20;
    let specs: Vec<&str> = spec
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if specs.is_empty() {
        return Err(invalid("seed: name at least one work"));
    }
    if specs.len() > MAX_SEEDS {
        return Err(invalid(format!(
            "seed: {} works named, and a set query takes at most {MAX_SEEDS} — beyond that the \
             union stops describing a taste and starts describing the catalogue",
            specs.len()
        )));
    }
    let mut seeds = Vec::with_capacity(specs.len());
    for one in &specs {
        seeds.push(
            resolve_one_seed(&state, one, account.map(|id| id.to_string()).as_deref()).await?,
        );
    }
    let mut query = build_query(&state, &params, exclude_account).await?;
    query.tokens = preselection_tokens(&seeds);
    let page =
        darkhold_db::metrics::similar(state.db(), &query, &seeds, limit as usize, offset as usize)
            .await?;

    Ok(SimilarPage {
        items: page
            .items
            .into_iter()
            .map(|item| SimilarItem {
                row: item.row,
                score: item.score,
                shared: item.shared,
                rank: item.rank,
                updated_at: item.updated_at,
            })
            .collect(),
        sort: "similarity".to_owned(),
        metric: "similarity".to_owned(),
        seed: SeedRef {
            id: page.seed_id,
            title: page.seed_title,
        },
        ranked: page.ranked,
        total: page.total,
        scanned: page.scanned,
        limit,
        offset,
    })
}

/// The fandom this request names, spelled as the catalogue spells it.
///
/// An unknown name is a `422` that suggests the nearest known one: falling
/// through to the store would answer 200 with an empty list, which reads as
/// "this fandom has nothing" rather than "you spelled it differently".
async fn resolve_fandom(state: &AppState, raw: Option<&str>) -> ApiResult<Option<String>> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let known = catalogue::fandoms(state.db(), FANDOM_LIST_LIMIT).await?;
    match known.iter().find(|f| f.name.eq_ignore_ascii_case(raw)) {
        Some(found) => Ok(Some(found.name.clone())),
        None => {
            let hint = suggest(raw, &known)
                .map(|name| format!("; did you mean \"{name}\"?"))
                .unwrap_or_else(|| ".".to_owned());
            Err(invalid(format!(
                "fandom: no fandom called \"{raw}\"{hint} ({} fandoms known)",
                known.len()
            )))
        }
    }
}

/// How many fandoms the suggestion list is built from.
///
/// 500 covers the live catalogue's 233 with room for the tag backfill; a name
/// outside the top 500 is still an exact match, it just cannot be *suggested*.
const FANDOM_LIST_LIMIT: i64 = 500;

/// Every fandom the catalogue holds, largest first.
async fn fandoms(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
) -> ApiResult<Json<Vec<FandomCount>>> {
    Ok(Json(
        catalogue::fandoms(state.db(), FANDOM_LIST_LIMIT).await?,
    ))
}

/// The nearest known fandom to what someone typed.
///
/// Prefix and substring first, because those are what a truncated name looks
/// like (`narut`), then edit distance, because those are what a typo looks like
/// (`naruot`). Returns `None` rather than a bad guess: a suggestion nobody
/// recognises costs more attention than no suggestion.
fn suggest<'a>(raw: &str, known: &'a [FandomCount]) -> Option<&'a str> {
    let needle = raw.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }
    if let Some(hit) = known
        .iter()
        .filter(|f| f.name.to_lowercase().starts_with(&needle))
        .min_by_key(|f| f.name.len())
    {
        return Some(hit.name.as_str());
    }
    if let Some(hit) = known
        .iter()
        .filter(|f| f.name.to_lowercase().contains(&needle))
        .min_by_key(|f| f.name.len())
    {
        return Some(hit.name.as_str());
    }
    let budget = (needle.chars().count() / 3).max(2);
    known
        .iter()
        .map(|f| (edit_distance(&needle, &f.name.to_lowercase()), f))
        .filter(|(distance, _)| *distance <= budget)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, f)| f.name.as_str())
}

/// Levenshtein distance, insertion/deletion/substitution, no transposition.
///
/// Written out rather than pulled in: it is twelve lines, it is used only for a
/// suggestion, and a dependency for it would be the largest thing in the crate's
/// tree for the smallest feature in it.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// The distinct source keys the catalogue holds, with how many rows each has.
async fn sources(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
) -> ApiResult<Json<Vec<SourceCount>>> {
    let rows = catalogue::sources(state.db()).await?;
    Ok(Json(rows))
}

// ===== the downloadable snapshot =====
//
// `GET /catalogue/export.sqlite` and its manifest. The file is built once per
// catalogue fingerprint and cached on disk, because it is the same bytes for
// everyone: rebuilding per request would turn a free download into a denial of
// service on a $5 VPS.

/// Where snapshots live, under the configured storage root.
const EXPORT_DIR: &str = "catalogue-export";

/// How many fingerprints to keep. Two is enough for a client that started a
/// download just as the catalogue changed; older files are dead weight.
const EXPORT_KEEP: usize = 2;

/// Serialises builds inside this process, so ten simultaneous first requests
/// produce one snapshot rather than ten.
fn build_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// The manifest: what the file is, how to use it, and what is deliberately not
/// in it.
#[derive(Debug, Serialize)]
pub struct ExportManifest {
    #[serde(flatten)]
    summary: darkhold_db::catalogue_export::ExportSummary,
    /// The route that serves the file itself.
    url: String,
    /// The file name a download gets.
    filename: String,
    /// Schema, table by table, so a client can write SQL without downloading
    /// first.
    tables: Vec<ExportTable>,
    /// The privacy boundary, in the manifest as well as in the file's `meta`
    /// table — a download someone forgot they had should still be able to
    /// explain itself.
    privacy: String,
    /// Queries that answer the questions the route cannot: the point of a local
    /// database is the SQL nobody wrote for you.
    examples: Vec<ExportExample>,
}

/// One table in the snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct ExportTable {
    name: &'static str,
    columns: &'static [&'static str],
    /// One sentence on what a row means.
    means: &'static str,
}

/// One example query.
#[derive(Debug, Clone, Serialize)]
pub struct ExportExample {
    asks: &'static str,
    sql: &'static str,
}

const EXPORT_TABLES: &[ExportTable] = &[
    ExportTable {
        name: "works",
        columns: &[
            "work_key",
            "source_key",
            "source_work_key",
            "title",
            "author_text",
            "summary",
            "language",
            "word_count",
            "status",
            "source_url",
            "source_updated_at",
            "likes",
            "views",
            "on_darkhold",
            "total_likes",
            "source_count",
            "catalogue_updated_at",
        ],
        means: "One row per story per source. `likes` is the source's own endorsement count \
                (likes, kudos, followers) normalised by the crawler; `total_likes` sums the same \
                story across every source it appears on; `on_darkhold` says the catalogue row was \
                materialised into a work on this instance.",
    },
    ExportTable {
        name: "sources",
        columns: &["source_key", "works", "likes", "newest_update"],
        means: "One row per source: how many works it contributes, their likes in total, and the \
                newest catalogue update for that source.",
    },
    ExportTable {
        name: "meta",
        columns: &["key", "value"],
        means: "How the file was built: schema version, row and work counts, the catalogue \
                fingerprint, the build instant, and the privacy statement.",
    },
];

const EXPORT_EXAMPLES: &[ExportExample] = &[
    ExportExample {
        asks: "Naruto fics updated in the last week, best first",
        sql: "SELECT title, author_text, likes, total_likes, catalogue_updated_at \
              FROM works \
              WHERE lower(title) LIKE '%naruto%' OR lower(summary) LIKE '%naruto%' \
              ORDER BY catalogue_updated_at DESC",
    },
    ExportExample {
        asks: "Hidden gems: strong likes on low views, complete works only",
        sql: "SELECT title, source_key, likes, views, ROUND(1.0 * likes / views, 3) AS likes_per_view \
              FROM works \
              WHERE status = 'complete' AND views > 500 \
              ORDER BY likes_per_view DESC LIMIT 50",
    },
    ExportExample {
        asks: "Which fandoms gained the most catalogue entries (by source)",
        sql: "SELECT source_key, COUNT(*) AS works, SUM(likes) AS likes, MAX(catalogue_updated_at) \
              FROM works GROUP BY source_key ORDER BY works DESC",
    },
    ExportExample {
        asks: "Quality per thousand words rather than raw engagement",
        sql: "SELECT title, word_count, total_likes, \
                     ROUND(1000.0 * total_likes / NULLIF(word_count, 0), 2) AS likes_per_1k_words \
              FROM works WHERE word_count > 20000 AND total_likes > 100 \
              ORDER BY likes_per_1k_words DESC LIMIT 100",
    },
];

fn export_privacy() -> String {
    "Story metadata only. No account, pseudonym, email, import record or reading history is in \
     this file, and no column identifies a reader of this instance. Counts are exact rather than \
     suppressed: the k-anonymity floor protects aggregates about people, and there are none here."
        .to_owned()
}

fn snapshot_path(state: &AppState, fingerprint: &str) -> std::path::PathBuf {
    let short = &fingerprint[..16.min(fingerprint.len())];
    state
        .config()
        .storage
        .root
        .join(EXPORT_DIR)
        .join(format!("catalogue-{short}.sqlite"))
}

fn sidecar_path(snapshot: &std::path::Path) -> std::path::PathBuf {
    snapshot.with_extension("json")
}

/// The cached snapshot for the catalogue's current fingerprint, building it if
/// this fingerprint has never been built.
async fn snapshot(
    state: &AppState,
) -> ApiResult<(
    std::path::PathBuf,
    darkhold_db::catalogue_export::ExportSummary,
)> {
    let fingerprint = darkhold_db::catalogue_export::fingerprint(state.db()).await?;
    let path = snapshot_path(state, &fingerprint);
    if let Some(summary) = read_sidecar(&path).await {
        return Ok((path, summary));
    }

    let _guard = build_lock().lock().await;
    // Re-checked inside the lock: the holder before us may have just built it.
    if let Some(summary) = read_sidecar(&path).await {
        return Ok((path, summary));
    }

    let summary = darkhold_db::catalogue_export::build_sqlite(state.db(), &path).await?;
    if let Ok(json) = serde_json::to_vec(&summary) {
        let sidecar = sidecar_path(&path);
        if let Some(parent) = sidecar.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        let _ = tokio::fs::write(&sidecar, json).await;
    }
    prune_snapshots(&path).await;
    Ok((path, summary))
}

async fn read_sidecar(
    path: &std::path::Path,
) -> Option<darkhold_db::catalogue_export::ExportSummary> {
    if tokio::fs::metadata(path).await.is_err() {
        return None;
    }
    let bytes = tokio::fs::read(sidecar_path(path)).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Keep the newest few snapshots, drop the rest.
async fn prune_snapshots(newest: &std::path::Path) {
    let Some(dir) = newest.parent() else { return };
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    let mut database = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("sqlite") && path != newest {
            database.push(path);
        }
    }
    // Oldest first by mtime, so the tail of the list is what survives.
    let mut stamped = Vec::new();
    for path in database {
        let modified = tokio::fs::metadata(&path)
            .await
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        stamped.push((modified, path));
    }
    stamped.sort();
    let keep = EXPORT_KEEP.saturating_sub(1);
    for (_, path) in stamped.into_iter().rev().skip(keep) {
        let sidecar = sidecar_path(&path);
        let _ = tokio::fs::remove_file(&path).await;
        let _ = tokio::fs::remove_file(&sidecar).await;
    }
}

/// `GET /catalogue/export` — what the file is, before you download tens of MB.
async fn export_manifest(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
) -> ApiResult<Json<ExportManifest>> {
    let (path, summary) = snapshot(&state).await?;
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "catalogue.sqlite".to_owned());
    Ok(Json(ExportManifest {
        summary,
        url: "/api/v1/catalogue/export.sqlite".to_owned(),
        filename,
        tables: EXPORT_TABLES.to_vec(),
        privacy: export_privacy(),
        examples: EXPORT_EXAMPLES.to_vec(),
    }))
}

/// `GET /catalogue/export.sqlite` — the snapshot itself.
///
/// Served by `ServeFile`, not read into memory: the file is tens of megabytes on
/// a real catalogue, and a couple of simultaneous downloads should not be able to
/// take the instance down. `ServeFile` also gives conditional requests and Range
/// for free, which is what a resumable download needs.
async fn export_sqlite(
    State(state): State<AppState>,
    MaybeSession(_session): MaybeSession,
) -> ApiResult<axum::response::Response> {
    let (path, summary) = snapshot(&state).await?;
    let request = axum::http::Request::builder()
        .uri("/catalogue/export.sqlite")
        .body(axum::body::Body::empty())
        .map_err(|e| AppError::Internal(anyhow::anyhow!("export request: {e}")))?;
    let served = tower::ServiceExt::oneshot(tower_http::services::ServeFile::new(&path), request)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("serve export: {e}")))?;
    let mut response = served.map(axum::body::Body::new);
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/vnd.sqlite3"),
    );
    headers.insert(
        axum::http::header::CONTENT_DISPOSITION,
        axum::http::HeaderValue::from_str(&format!(
            "attachment; filename=\"{}\"",
            path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "catalogue.sqlite".to_owned())
        ))
        .map_err(|e| AppError::Internal(anyhow::anyhow!("content-disposition: {e}")))?,
    );
    if let Ok(value) = axum::http::HeaderValue::from_str(&summary.fingerprint) {
        headers.insert(axum::http::header::ETAG, value);
    }
    if let Ok(value) = axum::http::HeaderValue::from_str(&summary.rows.to_string()) {
        headers.insert("x-catalogue-rows", value);
    }
    Ok(response)
}

/// `POST /api/v1/catalogue/seeds` — resolve a pasted list, one line at a time (M45-67).
///
/// Three outcomes per line, and the response says which, because "3 of 8 resolved" is not something a
/// reader can act on: they need to know that line 5 is a QQ thread this instance cannot read and that
/// line 7 was refused for being a pasted chapter. Nothing here creates a work.
async fn resolve_external_seeds(
    State(state): State<AppState>,
    RequireSession(session): RequireSession,
    Json(body): Json<ResolveSeedsBody>,
) -> ApiResult<Json<serde_json::Value>> {
    const MAX_LINES: usize = 200;
    if body.lines.len() > MAX_LINES {
        return Err(invalid(format!(
            "lines: {} given and a list takes at most {MAX_LINES} — split it, because a list this \
             long is a file and files get imported, not typed",
            body.lines.len()
        )));
    }
    let account_id = session.account_id.to_string();
    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();

    let mut results = Vec::with_capacity(body.lines.len());
    let (mut resolved, mut external, mut refused, mut ambiguous) = (0usize, 0usize, 0usize, 0usize);
    for (index, line) in body.lines.iter().enumerate() {
        let number = index + 1;
        let outcome = match darkhold_domain::external_seed::classify(line) {
            darkhold_domain::external_seed::Line::Blank => {
                refused += 1;
                serde_json::json!({
                    "line": number, "status": "refused",
                    "reason": "empty line — nothing to resolve, and an empty seed would match the catalogue",
                })
            }
            darkhold_domain::external_seed::Line::TooLong { chars } => {
                refused += 1;
                serde_json::json!({
                    "line": number, "status": "refused",
                    "reason": format!("{chars} characters is a pasted paragraph, not a title (limit {})",
                        darkhold_domain::external_seed::MAX_TITLE_CHARS),
                })
            }
            darkhold_domain::external_seed::Line::Pointer {
                spec,
                source_key,
                source_url,
            } => {
                match darkhold_db::metrics::resolve_seed(state.db(), &spec).await? {
                    darkhold_db::metrics::SeedMatch::One(row) => {
                        resolved += 1;
                        serde_json::json!({
                            "line": number, "status": "resolved", "spec": spec,
                            "id": row.id, "title": row.title, "source_key": row.source_key,
                        })
                    }
                    darkhold_db::metrics::SeedMatch::Many(rows) => {
                        ambiguous += 1;
                        serde_json::json!({
                            "line": number, "status": "ambiguous", "spec": spec,
                            "reason": "several rows match and none is dominant",
                            "candidates": rows.iter().take(6).map(|row| serde_json::json!({
                                "id": row.id, "title": row.title,
                                "source_key": row.source_key, "likes": row.likes,
                            })).collect::<Vec<_>>(),
                        })
                    }
                    darkhold_db::metrics::SeedMatch::None => {
                        // Not here — kept as evidence, with the words the reader typed. No word
                        // count, no likes, no author: this instance has not seen the work and any
                        // number it printed would be invented.
                        let title = title_from(&spec, &source_key, source_url.as_deref());
                        let provenance = serde_json::json!({
                            "typed": line, "source_key": source_key,
                            "reason": "not in this instance's catalogue",
                        });
                        let claim = darkhold_db::external_seed::Claim {
                            spec: &spec,
                            title: &title,
                            source_key: &source_key,
                            source_url: source_url.as_deref(),
                            provenance: &provenance,
                        };
                        let stored = darkhold_db::external_seed::remember(
                            state.db(),
                            &account_id,
                            &claim,
                            &now,
                        )
                        .await?;
                        external += 1;
                        serde_json::json!({
                            "line": number, "status": "external", "spec": spec,
                            "id": stored.id, "title": stored.title,
                            "source_key": stored.source_key,
                            "note": "stored as taste evidence — it is not a catalogue row, and this \
                                     instance cannot report metrics for it",
                        })
                    }
                }
            }
        };
        results.push(outcome);
    }

    Ok(Json(serde_json::json!({
        "results": results,
        "counts": {
            "lines": body.lines.len(),
            "resolved": resolved,
            "external": external,
            "ambiguous": ambiguous,
            "refused": refused,
        },
    })))
}

/// The words to rank by when a line is evidence rather than a row.
///
/// A URL's last path segment is tried when it looks like a slug (`some-dark-fic`), because
/// `archiveofourown.org/works/123456` carries no title at all — and a numeric segment is an id, not
/// words, so ranking on it would be ranking on noise.
fn title_from(spec: &str, source_key: &str, source_url: Option<&str>) -> String {
    let Some(url) = source_url else {
        return spec.to_owned();
    };
    let segment = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let looks_like_words = segment.len() > 3
        && segment.chars().any(|c| c.is_alphabetic())
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if looks_like_words {
        segment.replace(['-', '_'], " ")
    } else {
        // A bare id: name it by where it points, so the row can still explain itself.
        format!("{source_key} {segment}")
    }
}

/// `GET /api/v1/catalogue/seeds` — the evidence this reader has supplied, newest first.
async fn list_external_seeds(
    State(state): State<AppState>,
    RequireSession(session): RequireSession,
) -> ApiResult<Json<serde_json::Value>> {
    let rows =
        darkhold_db::external_seed::list_for_account(state.db(), &session.account_id.to_string())
            .await?;
    Ok(Json(serde_json::json!({
        "items": rows.iter().map(|row| serde_json::json!({
            "id": row.id, "spec": row.spec, "title": row.title,
            "source_key": row.source_key, "source_url": row.source_url,
            "provenance": row.provenance_json, "created_at": row.created_at,
        })).collect::<Vec<_>>(),
        "total": rows.len(),
    })))
}

/// The body of `POST /catalogue/seeds`.
#[derive(Debug, serde::Deserialize)]
struct ResolveSeedsBody {
    lines: Vec<String>,
}

/// `GET /api/v1/catalogue/taste-brief?seed=a,b&format=json` — the shelf as markdown (M45-71).
///
/// The bytes are deterministic: same shelf, same document. That is what makes it committable — a brief
/// that changed on every read could not be reviewed, and this row exists so a judgement can be.
async fn taste_brief(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    Query(params): Query<BriefParams>,
) -> ApiResult<Response> {
    const MAX_SEEDS: usize = 20;
    let spec = params
        .seed
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("seed: a brief is generated from works you name"))?;
    let specs: Vec<&str> = spec
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if specs.is_empty() {
        return Err(invalid("seed: name at least one work"));
    }
    if specs.len() > MAX_SEEDS {
        return Err(invalid(format!(
            "seed: {} works named, and a brief takes at most {MAX_SEEDS}",
            specs.len()
        )));
    }

    let account = session.as_ref().map(|user| user.account_id.to_string());
    let mut facts = Vec::with_capacity(specs.len());
    for one in &specs {
        let seed = resolve_one_seed(&state, one, account.as_deref()).await?;
        let read = darkhold_db::taste_brief::facts_for_item(state.db(), &seed.id).await?;
        facts.push(darkhold_domain::taste_brief::SeedFacts {
            title: seed.title,
            fandoms: read.fandoms,
            tags: read.tags,
        });
    }
    let brief = darkhold_domain::taste_brief::build(&facts);

    let wants_json = params
        .format
        .as_deref()
        .is_some_and(|format| format.eq_ignore_ascii_case("json"));
    if wants_json {
        return Ok(Json(serde_json::json!(brief)).into_response());
    }
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            "text/markdown; charset=utf-8",
        )],
        brief.markdown,
    )
        .into_response())
}

/// The brief's own query string: `seed` and `format`, and nothing else.
///
/// Deliberately not `CatalogueParams`: a brief is not a search, and sharing the struct would put `sort`
/// and `limit` on a document that has neither.
#[derive(Debug, serde::Deserialize)]
struct BriefParams {
    seed: Option<String>,
    format: Option<String>,
}
