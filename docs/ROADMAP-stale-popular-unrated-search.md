# ROADMAP — stale, popular, unrated search across sources

Status: **proposed 2026-11-24** · Ledger row `M45-75` · Spec:
`docs/spec-amendments/stale-popular-unrated-search.md` · Implementable plan:
`docs/plans/stale-popular-unrated-search.md`

## Why this exists

A reader's question — "system progression fantasy from qq/sv/sb/ffnet/ao3 that
has not updated in four months, by popularity, excluding what I already
rated" — is a single search in every other catalogue product. In darkhold it
is currently **five half-answers in three surfaces**:

- `trope:system-grind` names system/gamer/LitRPG/stat-grinding, but not
  celestial or CYOA (`crates/domain/src/tropes.rs`).
- `source:` does not exist in the works query grammar, so `?q=source:qq`
  answers 200 with zero rows (measured 2026-11-24).
- `updated:` reads `works.updated_at` (our import clock): 0 of 58,042 live
  works are older than 120 days. The author's clock lives on
  `library_items.source_updated_at`: 86,541 of 90,662 rows are older.
- `sort=popularity` is echoed by `GET /api/v1/search` but ignored by the SQL;
  only `newest` and `title` are recognised.
- `dh rec --exclude-read` excludes reading history, not ratings, and the
  catalogue surface takes one source and has no tag/trope grammar.

The fix is vocabulary, not a new product: add three fields and one sort to the
existing query language, plus one trope group. No migration, no scraper, no
new route.

## Phase 0 — can be done today, with caveats

Until the language lands, the closest honest answer is:

```
GET /api/v1/search?q=(trope:system-grind OR tag:celestial OR tag:cyoa)&limit=100
```

then filter the returned rows by `source_key` and `source_updated_at` from
`library_items`, order by `likes`, and drop work ids present in `rating` for
the reader. This is a **client-side join over a page**, so it is not equivalent:
the page is filled before filtering, which is exactly the defect the plan
refuses to ship. It is a stopgap, not the row.

## Phase 1 — name the subgenre (step 1 of the plan)

Add `trope:system-progression` with the seven tags a reader actually types.
Small, local, testable in `darkhold-domain` only. Unblocks the hand-spelled
`OR tag:celestial OR tag:cyoa` workaround.

## Phase 2 — the three missing predicates (steps 2–3)

`source:`, `source_updated:` and `me:rated` in the shared grammar; viewer bind
and two-engine search-store tests. This is the correctness core: without it
the question cannot be asked at all.

## Phase 3 — popularity as an ordering (step 3.3)

`sort=popularity` (aliases `likes`, `kudos`, `aggregate`) orders by the maximum
stored source endorsement, and unknown sort words are refused by name instead
of silently ranking by term count.

## Phase 4 — the terminal can type it (step 4)

`dh q --source qq,sv,sb,ffnet,ao3 --sort popularity --exclude-rated`, with the
cookie credential for the viewer-scoped clause. The expression remains
shareable; the flags are sugar.

## Phase 5 — prove and land (step 5–6)

Two-engine tests, route test, live probe on the deployed binary, ledger and
handoff. This row closes only when the canonical query returns rows on the
live instance and the anonymous `me:rated` probe answers 401.

## Dependencies and blockers

- **None for schema.** `library_items.source_key`,
  `library_items.source_updated_at` and `rating` already exist.
- **Ingestion coverage is a data fact, not a blocker.** The five named archives
  have 35,927 `sb`, 27,505 `sv`, 20,148 `qq`, 2,995 `ao3` and 8 `ffnet` rows;
  `ffnet` is nearly empty, so an ffnet-only query will honestly return nothing.
- **Reader credential.** `me:rated` needs a session; CLI use needs
  `DARKHOLD_COOKIE`. No new credential is introduced.
- **Ordering budget.** Seven trope tags + five sources + one date + one viewer
  term = 14 of the grammar's 24 terms.

## Non-goals

No semantic search, no new sources, no rewrite of `/api/v1/catalogue`, no
redefinition of `updated:`, no frontend redesign, no full-workspace gate. See
the amendment for the rejected alternatives in detail.

## Definition of done

The five clauses work in one request on both engines and through `dh q`; the
live probe returns a non-empty ordered page for a signed-in reader; an
anonymous `me:rated` request is refused; and ledger rows `M45-75.1`–`.5` carry
the commands and outputs that prove each clause.
