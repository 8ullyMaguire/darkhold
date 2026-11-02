# M47-05 (again): search applies the filters it reports

**Ledger row:** new, `M47-06`. **Milestone:** M47. **Status:** implemented 2026-11-02 (`implemented-fully-tested`, both engines).

## The defect, measured

`GET /api/v1/search` resolves the reader's stored search defaults and returns them:

```
Query params: min_words, max_words, sort, max_rating
  ↓
resolve_search_defaults()  ← explicit param, else the pseud's search_settings row
  ↓
search_works_ast_filtered(db, &params.q, viewer_id, limit, &filters)   ← q, viewer, limit, content filters ONLY
  ↓
200 {"items": [...], "filters": {"min_words": …, "max_words": …, "sort": …, "max_rating": …}}
```

`crates/app/src/routes/search.rs:163` passes `q`, the viewer, the limit and the content filters. The four
resolved values are **never used by the query** — they are echoed back in the response's `filters`
object, which is the whole of their existence. `search_works_ast_impl` in
`crates/db/src/search/ast_search.rs` has no parameter for a word bound, a rating or an ordering.

So a reader who sets "maximum rating: general" in content preferences, or "short works only" in
search defaults, gets a `200` whose `filters` claims the limit was applied, and results that include
explicit and 500k-word works. `M47-05` is marked `implemented-fully-tested`, and the test it was
closed on asserts the *echo*.

This is the `docs/goal.md` failure the repository keeps naming: a mechanism nothing reaches.
It is not a missing feature — every piece exists and is wired to the response — it is a query that
never asks.

## What to build

Apply all four, so the echo becomes true, and add the one the reader actually needs to be able to
turn off (`sort`, which is currently a fixed `score DESC`).

1. **`crates/db/src/search/ast_search.rs`** — a public `SearchFilters` value:

   ```rust
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
   ```

   `search_works_ast` and `search_works_ast_filtered` keep their signatures and pass
   `&SearchFilters::default()`; a new `search_works_ast_scoped(db, query, viewer_id, limit, rules,
   filters)` is the only implementation. No caller loses its old behaviour, and the two paths cannot
   drift.

2. **The statement gains a wrapper**, because the word count is a correlated subquery in the select
   list and neither dialect lets `WHERE` reference a select alias. `render_statement` becomes
   `SELECT * FROM ( … existing select, visibility, exclusion, GROUP BY … ) AS s WHERE … ORDER BY …
   LIMIT ?`. Two consequences to write down in the code: the inner statement keeps `GROUP BY`, and
   the outer one owns `ORDER BY`/`LIMIT` so the filter applies before the page is cut (the same
   reason the content-filter exclusion lives inside the statement).

3. **The predicates**, portable across both dialects and bound as integers:

   ```sql
   AND (? IS NULL OR s.word_count >= ?)
   AND (? IS NULL OR s.word_count <= ?)
   -- ContentRating::rank(): general 0, teen 1, mature 2, explicit 3.
   AND (? IS NULL OR (CASE s.rating WHEN 'general' THEN 0 WHEN 'teen' THEN 1
                                    WHEN 'mature' THEN 2 ELSE 3 END) <= ?)
   ```

   `rating` must join the inner select (it is `works.rating`, `TEXT NOT NULL DEFAULT 'general'`).
   An unrecognised stored rating ranks 3 — the conservative direction: an unknown row is treated as
   the highest rating, so a `max_rating` filter hides it rather than leaking it.

4. **Ordering** from `sort`: `score` → `score DESC, works.updated_at DESC` (today's), `newest` →
   `works.updated_at DESC`, `title` → `title ASC`. An unknown value falls back to `score` rather
   than `422`: a stored default written by an older build must not make search fail for that reader.

5. **`crates/app/src/routes/search.rs`** — pass the resolved four into `search_works_ast_scoped`.
   The response body does not change: it already reports them, and now reports the truth.

## Verification

| step | command | expected |
|---|---|---|
| RED 1 | `cargo test -p darkhold-app --test search_filters_applied max_words_is_not_a_lie` | fails: a 40k-word work is returned under `max_words=10000` |
| RED 2 | `… max_rating_is_not_a_lie` | fails: an explicit work is returned under `max_rating=general` |
| RED 3 | `… sort_newest_orders_by_update` | fails: order is score-first |
| GREEN | the three above | pass |
| Regression | `cargo test -p darkhold-db --test ast_search` and `cargo test -p darkhold-app --test search_routes` | unchanged, all pass |
| Both engines | the same three with `DARKHOLD_TEST_PG_URL` set | pass — the CASE ranks and the wrapper are dialect-neutral, the casts are already in the inner select |

Fixtures: three published works — one 2k words `general`, one 40k words `mature`, one 8k words
`explicit` — each with one unit and one revision, so `SUM(cr.word_count)` is the number written.

## Out of scope, named

* `M47-05`'s *frontend* half (a preset button for "complete works under 10k", item 22 of the ideas
  audit) becomes honest only after this and is a separate row: with the filter inert, such a button
  would set two parameters and change nothing.
* Ranking by rating, or any new index. The word count is still a correlated subquery per row; if the
  filter proves slow on a large library, that is a separate, measurable change (a denormalised
  `works.word_count` maintained by the revision trigger).
