# M63-03 — `--sort` answers every name the row lists

Status: implemented (`9164636e`, plus a follow-on for the name column). Row: `M63-03` (power-user).
Idea source:
`docs/ideas/2026-11-01-idea-sweep-power-users.md` §"The flagship", the `[S] --sort accepts any
registered metric by name` bullet.

## The problem, measured first

The row's requirement names eight words. The tree, read before anything was written:

| # | word in the row | today | why |
|---|---|---|---|
| 1 | `wilson` | `Metric::Wilson`, `Support::Blocked` | views are null on all 141,317 rows (M63-32 imports them) |
| 2 | `hidden_gem` | `Metric::HiddenGem`, `Support::Blocked` | needs two snapshots of the same row (M63-32) |
| 3 | `kudos` | **`unknown sort: kudos`** (422) | `likes` is the stored column; `kudos` is AO3's word for it and nothing maps the two |
| 4 | `kudos_per_1k_views` | **`unknown sort`** | `likes_per_view` is registered and blocked; the spelling in the row is not |
| 5 | `kudos_per_hour` | **`unknown sort`** | Δkudos/time — the same second snapshot `velocity` needs, under another name |
| 6 | `completion` | **`unknown sort`** | `library_items.status` is stored (`complete|ongoing|hiatus|cancelled|unknown`) and no sort reads it |
| 7 | `updater_reliability` | **`unknown sort`** | needs per-author update history; nothing records it |
| 8 | `freshness_weighted_quality` | **`unknown sort`** | both inputs already exist: `quality` and `recency` |

So four of the eight names are answered, and four are a dead end. `unknown sort: kudos` is the worst
shape of refusal: it names neither the reason nor the work that unblocks it, so a caller reads it as
"this product cannot sort by kudos", which is false — `likes` is kudos.

**What is NOT the problem.** The metric machinery is complete and correct: 21 names, the ranker, the
`ranked N of M` gate, `--list-metrics`, both routes. Nothing here is a rewrite. The defect is the
vocabulary — the set of words that has a decision attached.

## The rule this row settles

Every name a caller can type gets **one of two** answers, never `unknown sort`:

1. **an answer** — computed in memory (`Ranked`), ordered in SQL (`Raw`), or triaged (`Triage`); or
2. **a refusal by name** — `Support::Blocked(reason)`, where the reason names the missing input and
   the ledger row that supplies it.

A word that is neither stays a 422, because `--sort wilson2` must not be silently `likes`. The
vocabulary grows; the strictness does not.

## Decisions (rejected alternatives named)

- **`kudos` is an alias of `likes`, not a new column.** The imported count *is* AO3 kudos; darkhold's
  own count is `darkhold_kudos`, already folded into `aggregate`. Adding a second stored column would
  be a second source of truth for one number. Rejected: a separate `Metric::Kudos` that scores
  `darkhold_kudos` — it would silently mean "kudos *on this instance*" for a catalogue whose rows are
  99% imports with no work behind them.
- **`completion` is a stored-column sort (`Support::Raw` → `CatalogueSort`), not an in-memory metric.**
  It is a `CHECK`-constrained `TEXT` column; a ranker over 141k rows to reproduce an `ORDER BY` would
  be slower and no more correct. Rejected: scoring `complete`=1.0 / `ongoing`=0.5 as a 0..1 signal —
  it would put a fabricated number in `score` for a field that is categorical.
- **`freshness_weighted_quality` is `quality × recency`, and an undated row scores 0.** That matches
  `recency`, whose tests already assert an absent date is "absent rather than old". Rejected:
  renormalising it like the composite does — freshness is the whole point of the metric, so dropping
  it when absent would answer the question nobody asked.
- **`kudos_per_hour` gets its own blocked variant rather than aliasing `velocity`.** A refusal must
  name the word the caller typed; aliasing would answer `sort: "velocity"` to `--sort kudos_per_hour`.

## Steps

### Step 1 — domain: three answered names (RED first)

`crates/domain/src/quality.rs`, the `Metric` enum (after `Wilson`):

```rust
    /// `quality` multiplied by `recency` — the same composite, discounted by age.
    FreshnessWeightedQuality,
    /// `library_items.status`: complete, ongoing, hiatus, cancelled. A stored
    /// column, so the routes order it in SQL.
    Completion,
    /// Δkudos per hour. Blocked with `Velocity`: a rate needs two snapshots.
    KudosPerHour,
    /// How often an author ships what they start. Blocked: no update history.
    UpdaterReliability,
```

`Metric::ALL` gains the four, in the same order, before `Self::Likes`:

```rust
        Self::FreshnessWeightedQuality,
        Self::Completion,
        Self::KudosPerHour,
        Self::UpdaterReliability,
```

`parse`, three arms added and two aliases:

```rust
            "freshness_weighted_quality" | "fresh_quality" | "fq" => Some(Self::FreshnessWeightedQuality),
            "completion" | "status" => Some(Self::Completion),
            "kudos_per_hour" | "likes_per_hour" => Some(Self::KudosPerHour),
            "updater_reliability" | "reliability" => Some(Self::UpdaterReliability),
            // AO3's word for the imported count, and the row names it.
            "kudos" => Some(Self::Likes),
            "kudos_per_1k_views" => Some(Self::LikesPerView),
```

`as_str`: `freshness_weighted_quality`, `completion`, `kudos_per_hour`, `updater_reliability`.

`support`:

```rust
            Self::Completion => Support::Raw,
            Self::KudosPerHour => Support::Blocked(
                "Δkudos needs two snapshots of the same row; the recheck (M63-32) creates the second",
            ),
            Self::UpdaterReliability => Support::Blocked(
                "needs the author's update history, which nothing records yet (M63-32 rechecks create it)",
            ),
```

and `FreshnessWeightedQuality` joins the `Ranked` arm.

`help`: one line each — `"quality × exp(-days/180); an undated row scores 0"`,
`"complete, then ongoing, hiatus, unknown, cancelled"`, `"kudos gained per hour"`,
`"how often an author finishes what they start"`.

`score_of` — one computed arm; `Completion` joins the `=> 0.0` tail because a
`Raw` metric is ordered in SQL, and the ranker's `Candidate` carries no status
(the ranker is reachable directly, so a fabricated score would be a number nobody
could check):

```rust
        // Freshness is what the metric is for, so a missing date scores zero
        // rather than being renormalised away — the same choice `Recency` makes.
        Metric::FreshnessWeightedQuality => {
            score_of(Metric::Quality, signals) * signals.recency.unwrap_or(0.0)
        }
```

The two new blocked variants join the `=> 0.0` tail with `Completion`.

### Step 2b — a refusal quotes the word the caller typed

Found while probing live: `--sort kudos_per_1k_views` answered `sort:
"likes_per_view" is not available yet`, naming a word the caller did not type.
`Metric::refusal_name(typed)` returns the typed word, appending the canonical name
in parentheses only when it differs, and both routes (`catalogue.rs`,
`rec_run.rs`) format their `Blocked` refusal with it.

### Step 2 — domain tests (the RED state)

```rust
    #[test]
    fn the_kudos_and_completion_spellings_reach_their_metric() {
        assert_eq!(Metric::parse("kudos"), Some(Metric::Likes));
        assert_eq!(Metric::parse("kudos_per_1k_views"), Some(Metric::LikesPerView));
        assert_eq!(Metric::parse("completion"), Some(Metric::Completion));
        assert_eq!(Metric::parse("kudos2"), None, "a typo is still refused");
    }

    #[test]
    fn a_freshness_weighted_quality_prefers_the_fresher_of_two_equal_rows() {
        let mut fresh = candidate("fresh", "sb", 500, Some(60_000));
        fresh.freshness_days = Some(0.0);
        let mut stale = candidate("stale", "sb", 500, Some(60_000));
        stale.freshness_days = Some(180.0);
        let mut undated = candidate("undated", "sb", 500, Some(60_000));
        undated.freshness_days = None;
        let ranked = rank(Metric::FreshnessWeightedQuality, &[fresh, stale, undated], 5, 0);
        let score = |id: &str| ranked.hits.iter().find(|h| h.id == id).expect("ranked").score;
        assert!((score("fresh") - score("stale") / (-1.0f64).exp()).abs() < 1e-9,
            "180 days is one half-life: fresh {} vs stale {}", score("fresh"), score("stale"));
        assert_eq!(score("undated"), 0.0, "no date, no freshness to weight by");
    }
```

Expected RED: `error[E0599]: no variant or associated item named 'FreshnessWeightedQuality' found for
enum 'Metric'` (and the same for the other three), then, once the variants exist but `score_of` does
not, `non-exhaustive patterns: 'FreshnessWeightedQuality' not covered`.

### Step 3 — store: `completion` is a column

`crates/db/src/catalogue.rs`, `CatalogueSort`:

```rust
    /// Reading state: complete first, then ongoing, hiatus, unknown, cancelled.
    Completion,
```

`parse`: `"completion" | "status" => Some(Self::Completion)`, and `"kudos" => Some(Self::Likes)`
(the metric vocabulary sends `kudos` down the raw path, so the raw vocabulary has to know it or the
alias dies in `search_raw` with `unknown sort: kudos`).

`as_str`: `"completion"`.

`order_by`:

```rust
        // A categorical column, ranked rather than alphabetical: `cancelled` sorts
        // before `complete` by any string order, which is the opposite of useful.
        CatalogueSort::Completion => {
            "CASE status WHEN 'complete' THEN 0 WHEN 'ongoing' THEN 1 \
             WHEN 'hiatus' THEN 2 WHEN 'unknown' THEN 3 ELSE 4 END, likes DESC, id DESC"
        }
```

The existing `an_unknown_sort_is_refused`-style assertion at `catalogue.rs:681`
(`CatalogueSort::parse("kudos"), None`) becomes the alias assertion:

```rust
        assert_eq!(CatalogueSort::parse("kudos"), Some(CatalogueSort::Likes));
        assert_eq!(CatalogueSort::parse("kudos2"), None);
```

and the `every_sort_round_trips` list gains `CatalogueSort::Completion`.

### Step 4 — routes: no change

`Support::Raw` already forwards to the SQL path in both `/catalogue` (`search_raw`) and `/rec/run`
(the `(None, Some(raw))` arm). This is the wiring the plan checks rather than assumes: **if
`completion` reached a metric page instead of a raw page, step 3's variant would be dead code.**

### Step 5 — CLI: the list must not lie

`crates/cli/src/rec.rs`, `metrics_list()`, after the loop:

```rust
    out.push_str(
        "\naliases: kudos = likes, kudos_per_1k_views = likes_per_view, status = completion\n",
    );
```

Nothing else: `validate_sort` already asks `Metric::parse`, so `--sort kudos` stops being refused
locally the moment step 1 lands, and `--list-metrics` prints the four new rows because they are in
`Metric::ALL`.

### Step 6 — route tests

`crates/app/tests/m63_03_metrics.rs`, three tests: `sort=completion` returns a plain
`CataloguePage` whose first row is the `complete` one (the seed writes `ongoing`; the test updates one
row to `complete`), `sort=kudos` returns the same ids in the same order as `sort=likes`, and
`sort=kudos_per_hour` is a 422 whose message contains `M63-32`.

## Verification, in order

```bash
cargo test -p darkhold-domain --lib quality        # 24 passed (22 + 2 new)
cargo test -p darkhold-db --test quality_metrics   # 4 passed + the completion arm
cargo test -p darkhold-db --lib catalogue          # the sort vocabulary
cargo test -p darkhold-app --test m63_03_metrics   # 8 passed (5 + 3)
cargo test -p darkhold-cli --lib rec               # 17 passed
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Mutation proof, both red then restored:
- `Metric::FreshnessWeightedQuality => quality` (drop the `* recency`) must fail
  `a_freshness_weighted_quality_prefers_the_fresher_of_two_equal_rows`.
- `CatalogueSort::Completion => "likes DESC, id DESC"` must fail the route test's first row.

Live probe on a scratch server, the words a caller actually types:

```bash
dh --base-url http://127.0.0.1:8099 q ...            # unrelated surface, sanity only
dh --base-url http://127.0.0.1:8099 rec --sort kudos --limit 2
dh --base-url http://127.0.0.1:8099 rec --sort completion --limit 2
dh --base-url http://127.0.0.1:8099 rec --sort freshness_weighted_quality --limit 2
dh --base-url http://127.0.0.1:8099 rec --sort kudos_per_hour ; echo "exit=$?"   # 422 naming M63-32
dh --base-url http://127.0.0.1:8099 rec --list-metrics | grep -E "kudos|completion|reliability"
```

## Step 7 — the record

`docs/requirements.csv` row `M63-03` → `implemented-fully-tested`, through `scripts/ledger.py` only,
with the evidence and the gates this run actually produced. `docs/backlog/` and
`docs/ideas-catalogue.json` regenerated. `README.md`'s `dh rec --sort` paragraph gains the eight names
and the two kinds of answer. `~/secondbrain/10-Projects/darkhold/STANDING-PROMPT.md`'s state block
re-derived (row counts, `Next:`).

## Deviations, and what the run found

1. **`score_of` gained one arm, not two.** The plan's draft scored `Completion` from a
   `signals.status_rank` field that does not exist and should not: `Completion` is `Support::Raw`, so the
   routes order it in SQL, and the ranker's `Candidate` carries no status. A fabricated score would be a
   number nobody could check.
2. **`Metric::refusal_name` was added mid-run** (step 2b). The live probe showed the refusal quoting
   `likes_per_view` to a caller who typed `kudos_per_1k_views`.
3. **`--list-metrics` truncated the names it exists to publish.** The name column was fixed at twenty
   characters, so `freshness_weighted_quality` printed as `freshness_weighted_…` — a name that cannot be
   copied into `--sort`. The column is now measured from the vocabulary (26 + marker + 2), and the test
   asserts both long names print whole. Found by reading the live output, not by a test — the list was
   covered only for which names it *contains*.
4. **Step 7's propagation no longer applies.** `~/code/rust/darkhold` is at `d1c28cce` (a frontend/estate
   commit) and `merge --ff-only local/master` fails: `fatal: Not possible to fast-forward`. The push to
   both forges went through; the propagation merge was left alone rather than forced.
