//! AST-based search: parse queries and render them to SQL with visibility
//! filtering. Both dialects.

use super::content_filter_sql::{self, FilterRule};
use super::query_error::SearchError;
use crate::search::SearchResult;
use crate::{sql_owned, Backend, Database};
use anyhow::Result;
use darkhold_domain::query::parse_query;
use darkhold_domain::query_sql::render_query;

/// What the reader asked the search to leave out, and in what order it wants what remains.
///
/// Every field is optional because every caller before M47-06b passed none of them, and an empty
/// value has to mean exactly the behaviour those callers had — the default ordering and no bound.
///
/// These are the same four values `GET /api/v1/search` reports in its `filters` object. Before
/// this type existed the route resolved them, echoed them, and passed none of them down, so a
/// reader who set "maximum rating: general" got a `200` claiming the ceiling was applied and
/// results that included explicit works.
#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    pub min_words: Option<i64>,
    pub max_words: Option<i64>,
    /// The highest rating the reader accepts, as a `ContentRating`'s serde name. An unrecognised
    /// value is read as the strictest ceiling: a typo must not leak the works it meant to hide.
    pub max_rating: Option<String>,
    /// `score` (the default) | `newest` | `title`. Anything else falls back to `score` rather
    /// than failing, because a stored default written by an older build must not turn search
    /// into an error for that reader.
    pub sort: Option<String>,
}

/// Search works using the AST parser and dialect-aware SQL.
pub async fn search_works_ast(
    db: &Database,
    query: &str,
    viewer_id: Option<&str>,
    limit: i64,
) -> Result<Vec<SearchResult>> {
    search_works_ast_impl(db, query, viewer_id, limit, &[], &SearchFilters::default()).await
}

/// Search with content-filter exclusion (spec §46.4 — a blocked tag never
/// reaches the client).
///
/// `filters` is a list of `(filter_type, value)` pairs, e.g. `("tag", "enemies
/// to lovers")`. The exclusion is rendered into the statement as a `NOT EXISTS`
/// against the work's taxonomy nodes, so it applies *before* `LIMIT` and costs
/// no extra round trips — see `content_filter_sql` for why both of those used
/// to be wrong.
pub async fn search_works_ast_filtered(
    db: &Database,
    query: &str,
    viewer_id: Option<&str>,
    limit: i64,
    filters: &[(String, String)],
) -> Result<Vec<SearchResult>> {
    search_works_ast_scoped(
        db,
        query,
        viewer_id,
        limit,
        filters,
        &SearchFilters::default(),
    )
    .await
}

/// The full door: content-filter exclusions *and* the reader's own bounds and ordering.
///
/// The four values in `scope` are the ones the search endpoint reports back, so this is the only
/// signature where the response cannot claim more than the query did.
pub async fn search_works_ast_scoped(
    db: &Database,
    query: &str,
    viewer_id: Option<&str>,
    limit: i64,
    filters: &[(String, String)],
    scope: &SearchFilters,
) -> Result<Vec<SearchResult>> {
    let rules: Vec<FilterRule> = filters
        .iter()
        .map(|(filter_type, value)| FilterRule::new(filter_type.clone(), value.clone()))
        .collect();
    search_works_ast_impl(db, query, viewer_id, limit, &rules, scope).await
}

/// Search works using the AST parser and dialect-aware SQL.
///
/// `rules` is the viewer's content filters. An empty slice excludes nothing, so
/// `search_works_ast` — which passes one — is exactly the unfiltered search and
/// the two paths cannot drift apart.
async fn search_works_ast_impl(
    db: &Database,
    query: &str,
    viewer_id: Option<&str>,
    limit: i64,
    rules: &[FilterRule],
    scope: &SearchFilters,
) -> Result<Vec<SearchResult>> {
    let (user_where, user_binds) = if query.trim().is_empty() {
        ("1=1".to_owned(), Vec::new())
    } else {
        // A bad query is the reader's mistake, not a server fault, so it gets a
        // typed error the route can turn into a 422 carrying the reason. It was
        // an `anyhow!` string before, which made every typo a 500.
        let ast = parse_query(query).map_err(|e| SearchError::parse(e.message, e.offset))?;
        let fragment = render_query(&ast).map_err(|e| SearchError::render(e.message))?;
        (fragment.sql, fragment.binds)
    };

    // Both statements are written with positional `?` and both go through
    // `sql_owned`, which renumbers only the PostgreSQL one. The old code
    // renumbered just the user fragment and hand-computed the `LIMIT` index,
    // then bound the viewer once for PostgreSQL and twice for SQLite because
    // the PostgreSQL form reused `$1` in the `blocks` subquery. Writing both
    // forms the same way removes that asymmetry: the bind list below is then
    // literally the same for either backend.
    let signed_in = viewer_id.is_some();
    let bound = scope_sql(scope);
    let sql = sql_owned(
        db,
        render_statement(signed_in, &user_where, rules, &bound, Dialect::Sqlite),
        render_statement(signed_in, &user_where, rules, &bound, Dialect::Postgres),
    );

    // Bind order, identical for both dialects: the viewer once per placeholder
    // that fills it, the user fragment's binds, the filter rules, then the
    // limit. sqlx binds in placeholder order, and `sql_owned` made that order
    // the same on both sides.
    let filter_binds = content_filter_sql::binds(rules);
    let rows: Vec<(String, String, String, i64, i64, String)> = match db.backend() {
        Backend::Sqlite => {
            let mut q = sqlx::query_as::<_, (String, String, String, i64, i64, String)>(&sql);
            if let Some(vid) = viewer_id {
                q = q.bind(vid); // visibility subquery
            }
            for b in &user_binds {
                q = q.bind(b.clone());
            }
            if let Some(vid) = viewer_id {
                q = q.bind(vid); // NOT EXISTS blocks
            }
            for b in &filter_binds {
                q = q.bind(b);
            }
            for b in &bound.binds {
                q = q.bind(*b);
            }
            q.bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            let mut q = sqlx::query_as::<_, (String, String, String, i64, i64, String)>(&sql);
            if let Some(vid) = viewer_id {
                q = q.bind(vid); // visibility subquery
            }
            for b in &user_binds {
                q = q.bind(b.clone());
            }
            if let Some(vid) = viewer_id {
                q = q.bind(vid); // NOT EXISTS blocks
            }
            for b in &filter_binds {
                q = q.bind(b);
            }
            for b in &bound.binds {
                q = q.bind(*b);
            }
            q.bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };

    Ok(rows
        .into_iter()
        .map(
            |(id, title, author_handle, word_count, score, _updated_at)| SearchResult {
                work_id: id,
                title,
                author_handle,
                word_count,
                score,
            },
        )
        .collect())
}

/// Which dialect to render for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Sqlite,
    Postgres,
}

/// Renders the search statement for one dialect.
///
/// `signed_in` selects the branch filter: a signed-in viewer sees their own
/// unlisted works and gets the `blocks` exclusion; an anonymous one sees only
/// public published works. `user_where` is the parsed query's SQL, already
/// written with `?`. `rules` is spliced in as a `NOT EXISTS` before the limit.
fn render_statement(
    signed_in: bool,
    user_where: &str,
    rules: &[FilterRule],
    bound: &BoundScope,
    dialect: Dialect,
) -> String {
    // Three type-level differences between the dialects, and only three. Each
    // is a *decoding* concern: the row is fetched into
    // `(String, String, String, i64, i64)`, and a column whose SQL type is not
    // exactly `TEXT`/`INT8` fails to decode even when the value is right.
    //
    //   * `works.id` is TEXT in SQLite, UUID in PostgreSQL. Needs `::text` to
    //     land in a `String`.
    //   * `SUM(cr.word_count)`: `bigint` in SQLite, but `numeric` in
    //     PostgreSQL -- `SUM` widens. `COALESCE(x, 0)` over a `numeric` is
    //     still `numeric`, so the `::bigint` cast goes *inside*, on the sum,
    //     not outside on the coalesce.
    //   * `COUNT(...)` is `bigint` in both, and needs no cast.
    //
    // Everything else, including every placeholder, is written identically and
    // renumbered by `sql_owned`. `COALESCE` is kept on both sides so an
    // empty aggregate is 0 rather than NULL.
    let (id_expr, sum_expr) = match dialect {
        Dialect::Sqlite => ("works.id", "CAST(SUM(cr.word_count) AS BIGINT)"),
        Dialect::Postgres => ("works.id::text", "CAST(SUM(cr.word_count) AS BIGINT)"),
    };

    // The work-id column, aliased. `work_count` and `score` must decode as
    // integers on both sides, so the aggregate is coalesced to 0 either way.
    // `rating` and `updated_at` are selected because the outer statement filters on the first and
    // orders on the second; both are single-valued per group, and `works.id` is the primary key,
    // so PostgreSQL accepts them beside the `GROUP BY works.id`. `updated_at` is fetched into the
    // row tuple and then dropped — it exists to make `sort=newest` expressible without a copy of
    // the query.
    let select = format!(
        "SELECT {id_expr}, works.title, pseuds.handle AS author_handle, \
         (SELECT COALESCE({sum_expr}, 0) FROM units c \
          JOIN revisions cr ON cr.id = c.current_revision_id \
          WHERE c.work_id = works.id AND c.deleted_at IS NULL) AS word_count, \
         COUNT(works_index_terms.term) AS score, works.rating, works.updated_at \
         FROM works \
         LEFT JOIN works_index_terms ON works_index_terms.work_id = works.id \
         LEFT JOIN works_index ON works_index.work_id = works.id \
         JOIN pseuds ON pseuds.id = works.owner_pseud_id "
    );

    // The visibility + block filter, which differs by whether there is a viewer.
    // `?` is used for the account id in both, and both the visibility subquery
    // and the `blocks` subquery take the viewer's id — two placeholders, two
    // binds, in that order.
    // `blocks.blocker` and `blocks.blocked` are `TEXT` in both dialects (see
    // 0013_community.sql), while `pseuds.account_id` is `UUID` in PostgreSQL.
    // Comparing them needs a cast on the pseuds side there, and none on the
    // SQLite side -- which is why this is per-dialect rather than shared.
    let blocked_account = match dialect {
        Dialect::Sqlite => "pseuds.account_id",
        Dialect::Postgres => "pseuds.account_id::text",
    };
    // Same reason on the other side of the comparison: the viewer's account id
    // arrives bound as a `String`, and `pseuds.account_id` is `UUID` in
    // PostgreSQL. A `?` bound as text against a UUID column is `operator does
    // not exist: uuid = text` -- the error that made the signed-in arm
    // unreachable on PostgreSQL, which is every authenticated search.
    let account_id = match dialect {
        Dialect::Sqlite => "account_id = ?",
        Dialect::Postgres => "account_id = ?::uuid",
    };
    let visibility = if signed_in {
        format!(
            "WHERE ((works.lifecycle = 'published' AND works.visibility != 'restricted') \
         OR works.owner_pseud_id IN (SELECT id FROM pseuds WHERE {account_id})) \
         AND ({user_where}) \
         AND NOT EXISTS (SELECT 1 FROM blocks \
                         WHERE blocks.blocker = ? \
                           AND blocks.blocked = {blocked_account} \
                           AND blocks.scope = 'all')"
        )
    } else {
        format!(
            "WHERE (works.lifecycle = 'published' AND works.visibility = 'public') \
         AND ({user_where})"
        )
    };

    // The content-filter exclusion, inside the paged statement so it applies
    // before the limit. This is the fix: the previous implementation applied
    // filters after paging, so a page could come back short.
    let exclusion = match dialect {
        Dialect::Sqlite => content_filter_sql::predicate(rules),
        Dialect::Postgres => content_filter_sql::predicate(rules),
    };
    // An empty predicate must not leave a stray `AND` behind.
    let exclusion = if exclusion.is_empty() {
        String::new()
    } else {
        format!("\n         AND {exclusion}")
    };

    // The bounds are applied OUTSIDE the grouped statement, and `ORDER BY`/`LIMIT` move out with
    // them. Two reasons, both the same reason:
    //
    //   * `word_count` is a correlated subquery in the select list, and neither dialect lets a
    //     `WHERE` (or a portable `HAVING`) reference a select alias — PostgreSQL rejects the
    //     alias outright, and repeating the subquery in the predicate would put a second copy of
    //     the word-count expression one edit away from drifting from the one the response
    //     reports.
    //   * a limit applied before the bound would return a page that the bound then shortens,
    //     which is the defect `content_filter_sql` was written for ("a page could come back
    //     short").
    //
    // PostgreSQL requires every non-aggregated column to be grouped, so `rating` joins the list.
    // SQLite tolerates the wider list and is given the same one rather than a special case.
    format!(
        "SELECT * FROM ({select}{visibility}{exclusion} \
         GROUP BY works.id, works.updated_at, works.rating, pseuds.handle) AS s \
         WHERE 1=1{where_clause} \
         ORDER BY {order} \
         LIMIT ?",
        where_clause = bound.where_clause,
        order = bound.order_by,
    )
}

/// The reader's bounds rendered for one statement, plus the integers they bind.
///
/// Built as fragments rather than as `(? IS NULL OR word_count >= ?)` predicates on purpose: a
/// `NULL` parameter compared against an integer needs a cast to keep PostgreSQL from reporting
/// "could not determine data type of parameter", and an absent bound is better expressed by not
/// writing the predicate at all — the statement then reads exactly as the one every caller had
/// before M47-06b.
#[derive(Debug, Clone)]
struct BoundScope {
    /// Empty, or one `AND …` per active bound, starting with a newline.
    where_clause: String,
    /// `score DESC, s.updated_at DESC` and friends; never empty.
    order_by: String,
    binds: Vec<i64>,
}

/// Rank a stored rating for the `max_rating` ceiling: `general` 0, `teen` 1, `mature` 2, else 3.
///
/// The fallthrough is 3 rather than an error so that a rating this build does not know about is
/// hidden by any ceiling, which is the direction that cannot leak. The same figure is written into
/// the SQL as a `CASE`, so a row whose stored value is unknown ranks 3 there too — the two halves
/// have to agree, and the test `max_rating_is_not_a_lie` is what holds them.
fn rating_rank(rating: &str) -> i64 {
    match rating {
        "general" => 0,
        "teen" => 1,
        "mature" => 2,
        _ => 3,
    }
}

fn scope_sql(scope: &SearchFilters) -> BoundScope {
    let mut where_clause = String::new();
    let mut binds = Vec::new();

    if let Some(min) = scope.min_words {
        where_clause.push_str("\n           AND s.word_count >= ?");
        binds.push(min);
    }
    if let Some(max) = scope.max_words {
        where_clause.push_str("\n           AND s.word_count <= ?");
        binds.push(max);
    }
    if let Some(rating) = scope.max_rating.as_deref() {
        // An unrecognised ceiling is read as the strictest one: a stored typo must not leak the
        // works the reader meant to hide. `config` validation rejects such a value on write, so
        // this is the belt to that braces.
        let ceiling = if rating_rank(rating) == 3 {
            rating_rank("general")
        } else {
            rating_rank(rating)
        };
        where_clause.push_str(
            "\n           AND (CASE s.rating \
             WHEN 'general' THEN 0 WHEN 'teen' THEN 1 WHEN 'mature' THEN 2 ELSE 3 END) <= ?",
        );
        binds.push(ceiling);
    }

    let order_by = match scope.sort.as_deref() {
        Some("newest") => "s.updated_at DESC, s.score DESC",
        Some("title") => "s.title ASC, s.score DESC",
        // `score` is the default, and every unrecognised value falls back to it rather than
        // failing: a stored default written by an older build must not make search an error.
        _ => "s.score DESC, s.updated_at DESC",
    }
    .to_owned();

    BoundScope {
        where_clause,
        order_by,
        binds,
    }
}
