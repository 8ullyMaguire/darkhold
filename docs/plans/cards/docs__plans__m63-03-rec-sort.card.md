# docs/plans/m63-03-rec-sort.md — plan card

Source: `docs/plans/m63-03-rec-sort.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`docs/ideas/2026-11-01-idea-sweep-power-users.md` §"The flagship", the `[S] --sort accepts any
`crates/domain/src/quality.rs`, the `Metric` enum (after `Wilson`):
in parentheses only when it differs, and both routes (`catalogue.rs`,
`rec_run.rs`) format their `Blocked` refusal with it.
`crates/db/src/catalogue.rs`, `CatalogueSort`:
The existing `an_unknown_sort_is_refused`-style assertion at `catalogue.rs:681`
`crates/cli/src/rec.rs`, `metrics_list()`, after the loop:
`crates/app/tests/m63_03_metrics.rs`, three tests: `sort=completion` returns a plain
`docs/requirements.csv` row `M63-03` → `implemented-fully-tested`, through `scripts/ledger.py` only,
with the evidence and the gates this run actually produced. `docs/backlog/` and
`docs/ideas-catalogue.json` regenerated. `README.md`'s `dh rec --sort` paragraph gains the eight names
4. **Step 7's propagation no longer applies.** `~/code/rust/darkhold` is at `d1c28cce` (a frontend/estate

## Copy-pasteable code / commands

    /// `quality` multiplied by `recency` — the same composite, discounted by age.
    FreshnessWeightedQuality,
    /// `library_items.status`: complete, ongoing, hiatus, cancelled. A stored
    /// column, so the routes order it in SQL.
    Completion,
    /// Δkudos per hour. Blocked with `Velocity`: a rate needs two snapshots.
    KudosPerHour,
    /// How often an author ships what they start. Blocked: no update history.
    UpdaterReliability,

        Self::FreshnessWeightedQuality,
        Self::Completion,
        Self::KudosPerHour,
        Self::UpdaterReliability,

            "freshness_weighted_quality" | "fresh_quality" | "fq" => Some(Self::FreshnessWeightedQuality),
            "completion" | "status" => Some(Self::Completion),
            "kudos_per_hour" | "likes_per_hour" => Some(Self::KudosPerHour),
            "updater_reliability" | "reliability" => Some(Self::UpdaterReliability),
            // AO3's word for the imported count, and the row names it.
            "kudos" => Some(Self::Likes),
            "kudos_per_1k_views" => Some(Self::LikesPerView),

## Verification command and expected output

`ranked N of M` gate, `--list-metrics`, both routes. Nothing here is a rewrite. The defect is the
  `recency`, whose tests already assert an absent date is "absent rather than old". Rejected:
    #[test]
    #[test]
        assert!((score("fresh") - score("stale") / (-1.0f64).exp()).abs() < 1e-9,
Expected RED: `error[E0599]: no variant or associated item named 'FreshnessWeightedQuality' found for
`CataloguePage` whose first row is the `complete` one (the seed writes `ongoing`; the test updates one
cargo test -p darkhold-domain --lib quality        # 24 passed (22 + 2 new)
cargo test -p darkhold-db --test quality_metrics   # 4 passed + the completion arm
cargo test -p darkhold-db --lib catalogue          # the sort vocabulary
cargo test -p darkhold-app --test m63_03_metrics   # 8 passed (5 + 3)
cargo test -p darkhold-cli --lib rec               # 17 passed
- `CatalogueSort::Completion => "likes DESC, id DESC"` must fail the route test's first row.
dh --base-url http://127.0.0.1:8099 rec --sort kudos_per_hour ; echo "exit=$?"   # 422 naming M63-32
   copied into `--sort`. The column is now measured from the vocabulary (26 + marker + 2), and the test
   asserts both long names print whole. Found by reading the live output, not by a test — the list was

## RED/GREEN evidence

### Step 1 — domain: three answered names (RED first)
### Step 2 — domain tests (the RED state)
Expected RED: `error[E0599]: no variant or associated item named 'FreshnessWeightedQuality' found for
cargo test -p darkhold-domain --lib quality        # 24 passed (22 + 2 new)
cargo test -p darkhold-db --test quality_metrics   # 4 passed + the completion arm
cargo test -p darkhold-app --test m63_03_metrics   # 8 passed (5 + 3)
cargo test -p darkhold-cli --lib rec               # 17 passed
Mutation proof, both red then restored:
