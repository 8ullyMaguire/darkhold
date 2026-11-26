# docs/plans/m47-06-search-applies-its-filters.md — plan card

Source: `docs/plans/m47-06-search-applies-its-filters.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`crates/app/src/routes/search.rs:163` passes `q`, the viewer, the limit and the content filters. The four
`crates/db/src/search/ast_search.rs` has no parameter for a word bound, a rating or an ordering.
This is the `docs/goal.md` failure the repository keeps naming: a mechanism nothing reaches.
1. **`crates/db/src/search/ast_search.rs`** — a public `SearchFilters` value:
5. **`crates/app/src/routes/search.rs`** — pass the resolved four into `search_works_ast_scoped`.

## Copy-pasteable code / commands

Query params: min_words, max_words, sort, max_rating
  ↓
resolve_search_defaults()  ← explicit param, else the pseud's search_settings row
  ↓
search_works_ast_filtered(db, &params.q, viewer_id, limit, &filters)   ← q, viewer, limit, content filters ONLY
  ↓
200 {"items": [...], "filters": {"min_words": …, "max_words": …, "sort": …, "max_rating": …}}

   /// What the reader asked the search to leave out, and in what order it wants what remains.
   ///
   /// Every field is optional because every caller before M47-06 passed none of them, and an
   /// empty value must mean exactly the behaviour those callers had.
   #[derive(Debug, Clone, Default)]
   pub struct SearchFilters {
       pub min_words: Option<i64>,
       pub max_words: Option<i64>,
       /// The highest rating the reader accepts, as a `ContentRating`'s serde name.
       pub max_rating: Option<String>,
       /// `score` (default) | `newest` | `title`; anything else falls back to `score`.
       pub sort: Option<String>,
   }
   

   AND (? IS NULL OR s.word_count >= ?)
   AND (? IS NULL OR s.word_count <= ?)
   -- ContentRating::rank(): general 0, teen 1, mature 2, explicit 3.
   AND (? IS NULL OR (CASE s.rating WHEN 'general' THEN 0 WHEN 'teen' THEN 1
                                    WHEN 'mature' THEN 2 ELSE 3 END) <= ?)
   

## Verification command and expected output

explicit and 500k-word works. `M47-05` is marked `implemented-fully-tested`, and the test it was
   `search_works_ast` and `search_works_ast_filtered` keep their signatures and pass
5. **`crates/app/src/routes/search.rs`** — pass the resolved four into `search_works_ast_scoped`.
| step | command | expected |
| RED 1 | `cargo test -p darkhold-app --test search_filters_applied max_words_is_not_a_lie` | fails: a 40k-word work is returned under `max_words=10000` |
| GREEN | the three above | pass |
| Regression | `cargo test -p darkhold-db --test ast_search` and `cargo test -p darkhold-app --test search_routes` | unchanged, all pass |
| Both engines | the same three with `DARKHOLD_TEST_PG_URL` set | pass — the CASE ranks and the wrapper are dialect-neutral, the casts are already in the inner select |

## RED/GREEN evidence

   /// Every field is optional because every caller before M47-06 passed none of them, and an
| RED 1 | `cargo test -p darkhold-app --test search_filters_applied max_words_is_not_a_lie` | fails: a 40k-word work is returned under `max_words=10000` |
| RED 2 | `… max_rating_is_not_a_lie` | fails: an explicit work is returned under `max_rating=general` |
| RED 3 | `… sort_newest_orders_by_update` | fails: order is score-first |
| GREEN | the three above | pass |
