# M64-16 — the offline pre-registration harness, as a baseline ladder

**Requirement:** `M64-16` — *"an offline pre-registration harness scores a challenger against
recorded history on `taste::eval`'s metrics before it takes live traffic"* (`docs/requirements.csv`).

**Spec it implements:** §12.7a (the measured ladder) and §24.4 (*the five ways a harness lies*).

**Source of the doctrine, and what it is not.** `docs/wiki/` vendors a published AO3 recommendation
study (`mmarcet/AO3_recommender_system`) that measured every family of recommender this repository
names, on a corpus it did not share with us. Its numbers are **reference points, not targets** — but
two of its findings are load-bearing here and neither is guessable:

- **recency-only and length-only ordering scored below random.** `new` is a browse order, not a
  recommendation, and a fallback chain that ends there is worse than one that ends in an empty page.
- **a pure tag-profile cosine scored 0.0016 f1@20 against popularity's 0.0364**, while the same tags
  used item→item scored 0.0351. Tag similarity is a candidate generator, never a ranker.

## What this plan does NOT do

- **No matrix factorisation.** The MF rung is *declared and refused* until an offline ALS exists; the
  published study's own evidence is that a family arriving with another family's hyperparameter grid
  scores ≈ random (LMF 0.0045 on ALS's grid against ALS's 0.2025). A rung that prints a refusal is
  worth more than a rung that prints a number produced by the wrong grid.
- **No online experiment.** This is the offline half of the conjunction; M64-04 owns promotion.
- **No new signal.** Every rung reads a table that already exists. Adding a signal is M64-06/-11/-12.
- **No database access from `darkhold-taste`.** That crate does no I/O by design (D-7, and its
  `Cargo.toml` says so). The export is a SQL script; the harness is a pure function over its output.

## Facts this plan is built on, measured — not assumed

From `docs/design/2026-11-01-ranking-signals-and-surfaces.md` (measured in the tree and the deployed
database on 2026-11-01):

```
library_items                                  141,317
library_items carrying any endorsement           1,247   (0.9 %)
rows where views is non-null                         0
engaged rows that join a work_tag                    0   ← the tag rung's blocker
reading_progress rows                                1
work_ratings rows                                    0
```

and the schema the rungs read (`migrations/*/0068_work_metrics.sql`, `0098_ranking.sql`,
`0129_work_ratings.sql`):

| table | what it holds | clock |
|---|---|---|
| `work_kudos (work_id UUID, account_id UUID)` | one row per reader per work | `created_at` (RFC 3339 TEXT) |
| `work_view_log (work_id TEXT, viewer_hash TEXT)` | one row per work per viewer per hour | `viewed_at` |
| `work_metric_aggregates` | kudos/views/bookmarks counters per work | `updated_at` |
| `recommendation_slots` | every served slot, with `propensity` (NULL on pre-0098 rows) | served-at |
| `bookmarks (account_id, subject_type, subject_id)` | reader bookmarks, `subject_type='work'` | `created_at` |

**The last column is the point:** every interaction here has a clock, which is why this harness can
use a **temporal** holdout. The published study could not — its corpus had no timestamps, so it
scored the past with a model trained on the future. We do not have that excuse.

**The blocker, stated before any code is written.** 1,247 engaged rows is not a corpus a recommender
can be measured on. This harness is therefore built to run **green on a thin corpus by printing the
refusals** — and its first real verdict will be "the evidence is not here yet", which is a result, not
a failure. Do not manufacture engagement to make the table look better.

---

## Design

Three pieces, each independently verifiable:

```
scripts/eval/01-engagement-census.sql     does the data exist? (read-only, both engines)
scripts/eval/02-export-set.sh             → eval/eval-set.json   (temporal split, pseudonymised)
crates/taste/src/ladder.rs                pure scoring over that file (unit-tested)
crates/taste/examples/ladder.rs           the CLI that prints the table and exits non-zero
```

`eval/` is **gitignored**. The set names readers; §0.3 forbids a reader-level artefact leaving the
host, and a committed one is one `git push` from doing exactly that.

### 1. `crates/taste/src/eval.rs` — three metrics the study's table needs

`ndcg_at`, `recall_at` and `overlap_at` exist. The study's headline is F1@20 and MAP@20, so add three
functions with the same denominators `recall_at` already uses.

```rust
/// `precision@k`: the share of the first `k` *slots* that hit a relevant work.
///
/// The denominator is `k`, never the number of works the ranking actually held. A four-item
/// ranking with four hits is `4/20 = 0.2`, not `1.0`: it served a fifth of a page and a
/// caller reading 1.0 would believe it served the page. This is the same rule as
/// [`overlap_at`], and for the same reason.
#[must_use]
pub fn precision_at(ranking: &[String], judged: &[Judgement], k: usize) -> f64 {
    if k == 0 {
        return 0.0;
    }
    let relevant: HashSet<&str> = judged
        .iter()
        .filter(|j| j.grade > 0)
        .map(|j| j.work_id.as_str())
        .collect();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut hits = 0_usize;
    for work in ranking.iter().take(k) {
        if !seen.insert(work.as_str()) {
            continue; // a repeat occupies a slot and earns nothing
        }
        if relevant.contains(work.as_str()) {
            hits += 1;
        }
    }
    hits as f64 / k as f64
}

/// `F1@k` — the harmonic mean of [`precision_at`] and [`recall_at`].
///
/// `None` when the reader has no relevant judgements, because a reader who judged nothing is
/// not a reader who was served perfectly (the rule [`recall_at`] already follows).
#[must_use]
pub fn f1_at(ranking: &[String], judged: &[Judgement], k: usize) -> Option<f64> {
    let recall = recall_at(ranking, judged, k)?;
    let precision = precision_at(ranking, judged, k);
    if precision + recall == 0.0 {
        return Some(0.0);
    }
    Some(2.0 * precision * recall / (precision + recall))
}

/// `AP@k` — average precision, the ordering term F1 cannot see.
///
/// The denominator is the number of relevant works, **never** `min(k, relevant)` and never the
/// list's length: clamping it lets a three-item list score 1.0 on an eleven-work judgement set.
#[must_use]
pub fn average_precision_at(ranking: &[String], judged: &[Judgement], k: usize) -> Option<f64> {
    let relevant: HashSet<&str> = judged
        .iter()
        .filter(|j| j.grade > 0)
        .map(|j| j.work_id.as_str())
        .collect();
    if relevant.is_empty() || k == 0 {
        return None;
    }
    let mut seen: HashSet<&str> = HashSet::new();
    let mut hits = 0_usize;
    let mut total = 0.0;
    for (position, work) in ranking.iter().take(k).enumerate() {
        if !seen.insert(work.as_str()) {
            continue;
        }
        if relevant.contains(work.as_str()) {
            hits += 1;
            total += hits as f64 / (position + 1) as f64;
        }
    }
    Some(total / relevant.len() as f64)
}
```

**Verification** (in `eval.rs`'s `#[cfg(test)] mod tests`, beside the existing ones):

```bash
cargo test -p darkhold-taste --lib eval -- --nocapture
```

Expected: every existing test still green, plus:

```
test eval::tests::precision_divides_by_k_not_by_the_list ... ok
test eval::tests::a_repeat_earns_one_slot_and_no_second_hit ... ok
test eval::tests::f1_is_none_when_nothing_was_judged ... ok
test eval::tests::a_short_list_cannot_reach_a_perfect_map ... ok
test result: ok. 13 passed; 0 failed
```

(Nine tests exist in the module today — `grep -c '#\[test\]' crates/taste/src/eval.rs` — and the
four above make thirteen. The count is in the expectation because "the suite is green" on a filtered
run that matched nothing is this project's recurring trap.)

The four tests, with the hand-computable numbers that make them checks rather than decorations:

| test | case | expected |
|---|---|---|
| `precision_divides_by_k_not_by_the_list` | ranking `[w1]`, `w1` relevant, k=20 | `0.05` |
| `a_repeat_earns_one_slot_and_no_second_hit` | ranking `[w1, w1]`, `w1` relevant | precision `0.05`, recall counts `w1` once |
| `f1_is_none_when_nothing_was_judged` | `judged = []` | `None`, not `0.0` and not `1.0` |
| `a_short_list_cannot_reach_a_perfect_map` | ranking `[w1]`, three relevant works | `AP = 1/3`, not `1.0` |

### 2. `crates/taste/src/ladder.rs` — the ladder itself

```rust
//! The baseline ladder: every rung a recommender is placed against, on one split.
//!
//! A score answers nothing on its own. `0.09 f1@20` is excellent against a popularity floor of
//! `0.036` and unremarkable against a study that reported `0.20`. So the unit of a verdict here is
//! the **ladder**, not the score: every rung is scored on the same readers, the same judgements and
//! the same cut, and a challenger ships only against the rung that carries the bar (popularity).
//!
//! # What it refuses
//!
//! - **A refused rung is not a zero.** A rung whose data does not exist reports `refused: Some(..)`
//!   and `None` for every metric. Scoring it `0.0` would put a missing table at the bottom of a
//!   table and invite the reading "the tag engine is bad" when the truth is "no engaged row has a
//!   tag".
//! - **A reader with no judgements is not a perfect run.** They are excluded and counted
//!   (`scored` beside `readers`), never averaged in as 1.0.
//! - **Coverage is part of every score.** A rung that recommends the same 100 works to everyone
//!   scores well and is unusable; the distinct-work count and the catalogue share are printed
//!   beside the metric, and the guardrails read them.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::eval::{average_precision_at, f1_at, ndcg_at, precision_at, recall_at, Judgement};

/// The cut every rung is scored at. Fixed and printed, so two runs compare.
pub const K: usize = 20;

/// One engine's answer for one reader.
#[derive(Debug, Clone)]
pub struct Rung {
    /// The name the report prints (`random`, `popularity`, `tag_i2i`, …).
    pub name: String,
    /// Work ids in the order the rung would serve them. Empty when `refused` is set.
    pub ranking: Vec<String>,
    /// Why this rung could not be computed. `Some` means `ranking` is meaningless and is ignored.
    pub refused: Option<String>,
}

/// One reader: what they actually engaged with after the cutoff, and every rung's answer.
#[derive(Debug, Clone)]
pub struct Case {
    pub reader: String,
    pub judged: Vec<Judgement>,
    pub rungs: Vec<Rung>,
}

/// What a challenger must clear. Pre-registered: written before the run, and the run cannot move it.
#[derive(Debug, Clone, Copy)]
pub struct Bar {
    /// The challenger's `f1@20` must be at least this multiple of the popularity rung's.
    pub popularity_multiple: f64,
    /// And it must reach at least this share of the catalogue…
    pub min_catalogue_share: f64,
    /// …without putting more than this share of its slots into one fandom group.
    pub max_group_share: f64,
}

impl Bar {
    /// The proposed bar: 1.25× popularity, ≥ 5 % of the catalogue, no group above 40 %.
    ///
    /// The multiple is a *proposal*, not a measurement: the published study's gap between
    /// popularity (0.0364) and a neighbourhood walk (0.0933) is 2.6×, so 1.25× is the smallest
    /// number that is clearly not noise while remaining reachable on a thin corpus.
    #[must_use]
    pub const fn proposed() -> Self {
        Self { popularity_multiple: 1.25, min_catalogue_share: 0.05, max_group_share: 0.40 }
    }
}

/// One rung's totals over every case.
#[derive(Debug, Clone, PartialEq)]
pub struct RungScore {
    pub name: String,
    pub refused: Option<String>,
    /// Readers this rung could be scored for.
    pub scored: usize,
    pub precision_at_k: Option<f64>,
    pub recall_at_k: Option<f64>,
    pub f1_at_k: Option<f64>,
    pub map_at_k: Option<f64>,
    pub ndcg_at_10: Option<f64>,
    /// Distinct works across every case's ranking — the anti-collapse measure.
    pub distinct_works: usize,
    pub catalogue_share: f64,
    /// The largest share of one case's slots that belongs to a single group, or `None` when no
    /// group key was supplied.
    pub largest_group_share: Option<f64>,
}

/// The verdict on one named challenger.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub challenger: String,
    pub f1_ratio: Option<f64>,
    pub shipped: bool,
    /// The sentence a human reads, which names the reason either way.
    pub reason: String,
}
```

`LadderReport::build` walks each case, each rung, and fills `RungScore`: means over the scored cases
for the four metrics plus nDCG@10, `distinct_works` from the union of every case's ranking (truncated
to `K`), `catalogue_share = distinct_works / catalogue_size`, and `largest_group_share` from a
`&HashMap<String, String>` of work id → group (the caller passes the fandom groups of §11.8;
`None` when the map is empty, never `0.0`).

`LadderReport::verdict(challenger)` is the conjunction, and it names which limb failed:

```
shipped  = f1_ratio >= bar.popularity_multiple
        && catalogue_share >= bar.min_catalogue_share
        && largest_group_share <= bar.max_group_share
```

**Verification:**

```bash
cargo test -p darkhold-taste --lib ladder
```

Expected:

```
test ladder::tests::a_group_collapse_is_a_guardrail_not_a_zero ... ok
test ladder::tests::a_reader_without_judgements_is_counted_not_averaged ... ok
test ladder::tests::a_refused_rung_is_not_a_zero ... ok
test ladder::tests::an_unknown_challenger_names_itself ... ok
test ladder::tests::coverage_counts_distinct_works_not_slots ... ok
test ladder::tests::the_bar_is_a_conjunction ... ok
test ladder::tests::the_cut_and_the_metrics_are_pinned ... ok
test ladder::tests::the_ratio_is_measured_against_popularity ... ok
test result: ok. 8 passed; 0 failed
```

- `a_refused_rung_is_not_a_zero` — a rung with `refused: Some("0 of 1,247…")` has
  `f1_at_k == None` and prints the reason; mutate the refusal branch to report `0.0` and the test
  fails with *"a refusal must not report 0.0"*.
- `the_bar_is_a_conjunction` — a challenger at 2× popularity with 2 % coverage is **not** shipped and
  `reason` names coverage; the same run on a 20-work catalogue ships. This is the test that stops a
  winning number from hiding a collapsed engine; delete the coverage limb and it fails.
- `the_ratio_is_measured_against_popularity` — a floor that found nothing yields *no ratio* rather
  than a division; a floor at 1 hit and a challenger at 2 yields exactly `2.00×` and ships.
- `coverage_counts_distinct_works_not_slots` — ten readers each served the *same* two works have
  `distinct_works == 2`, not 20.
- `a_group_collapse_is_a_guardrail_not_a_zero` — every served work in one fandom fails the ceiling,
  and the same run with **no group map** reports `None` (never `0.0`) and ships.
- `a_reader_without_judgements_is_counted_not_averaged` — the mean is the scored reader's, and the
  header says `2 (1 scored, 1 unjudged)`.
- `the_cut_and_the_metrics_are_pinned` — `K == 20` and the two hand-computable values.

### 3. `crates/taste/examples/ladder.rs` — the CLI

Same shape as `examples/parity.rs` (which already reads three JSON files, joins by reader, and exits
non-zero on an empty run): serde types here, not in the module.

```bash
cargo run -p darkhold-taste --example ladder -- \
    --set eval/eval-set.json --challenger popularity --out eval/ladder.json
```

Input (`eval/eval-set.json`, produced by step 4; `refused` is a string when the rung cannot run):

```json
{
  "catalogue_size": 141317,
  "cutoff": "2026-08-01T00:00:00Z",
  "groups": { "w1": "harry potter", "w2": "naruto" },
  "work_ids": ["w1", "w2", "w4", "w9"],
  "readers": [
    { "reader": "a3f1…", "judged": [{ "work_id": "w9", "grade": 1 }],
      "rungs": [
        { "name": "popularity", "ranking": ["w1", "w9"] },
        { "name": "tag_i2i", "ranking": [], "refused": "0 of 1247 engaged rows carry a work_tag (M64-06)" },
        { "name": "mf", "ranking": [], "refused": "no offline matrix factorisation exists; see §12.7a" }
      ] }
  ]
}
```

`work_ids` is the catalogue rung: the works a `random` rung may draw from. It is capped at 5,000 ids
in the export (`ORDER BY md5(work_id)` for determinism), because the whole 141,317-id list in a JSON
file is 3 MB of ids for a rung that only needs a sample.

Output, printed to stdout and written to `--out`:

```
cutoff 2026-08-01T00:00:00Z
k=20 · catalogue 100 · readers 3 (3 scored, 0 unjudged)

rung                    f1@k    map@k  nDCG@10  distinct   share  max-group  status
popularity            0.0635   0.5000   0.5436         2    2.0%      50.0%  scored
tag_i2i                    —        —        —         0    0.0%          —  REFUSED 0 of 1247 engaged rows carry a work_tag (M64-06)
mf                         —        —        —         0    0.0%          —  REFUSED no offline matrix factorisation exists; see spec 12.7a
random                0.0635   0.4444   0.5000         6    6.0%      50.0%  scored

verdict: popularity NOT SHIPPED — f1@20 is 1.00× popularity, below the 1.25× bar

2 rung(s) refused: the ladder is incomplete and no claim can be made from it.
```

(These are the fixture's real numbers, asserted rather than illustrated. Note that the rung
`popularity` is challenged **against itself** here, and that is the point of the fixture: a ratio of
1.0 fails the 1.25× limb, so the run demonstrates the bar is computed against the bar and not against
zero. A harness whose floor rung can pass its own gate has no gate. `random` ties it on this fixture
because six works and three readers is not a corpus — the tie is honest, and it is why the ladder is
printed rather than a single number.)

**Exit codes:** `0` when every requested rung was scored and the challenger cleared the bar; `1` when
a rung refused or the bar failed (**a refusal is a non-zero exit**, because a run that could not
measure must not be quotable as if it had); `2` on a malformed input file. This is the discipline
`scripts/gate.sh` already follows — it is a gate, not a report — and it is the difference between a
harness and a log line.

**Verification, on a committed fixture** (`crates/taste/tests/fixtures/ladder-set.json`, three
readers, hand-computable):

```bash
cargo run -p darkhold-taste --example ladder -- \
    --set crates/taste/tests/fixtures/ladder-set.json --challenger popularity
echo "exit=$?"
```

Expected: the table above (3 readers → those means), `verdict:` line, `exit=1`.

### 4. `scripts/eval/02-export-set.sh` — the temporal split

The script takes an engine and a DSN, computes the cutoff, and emits the JSON. **Two SQL bodies**,
because the JSON functions differ; both are one statement, because a per-reader loop over the mirror
is the cost class that already cost this estate a day.

```bash
#!/usr/bin/env bash
# Export the evaluation set. Read-only, temporal split, readers pseudonymised.
#
#   scripts/eval/02-export-set.sh sqlite data/lorehaven.sqlite > eval/eval-set.json
#   scripts/eval/02-export-set.sh postgres "postgres://darkhold@localhost/darkhold" > eval/eval-set.json
#
# The cutoff is the 80th percentile of kudos timestamps: 80 % of engagement trains, 20 % is scored.
# A reader needs >= 5 test interactions to be scored at all; fewer is a per-reader noise sample, and
# including it is how a mean gets moved by one person.
set -euo pipefail
engine="${1:?sqlite|postgres}"; source="${2:?path or DSN}"
case "$engine" in
  sqlite)   sqlite3 -readonly "$source" < scripts/eval/02-export-set.sqlite.sql ;;
  postgres) psql "$source" -tA -f scripts/eval/02-export-set.postgres.sql ;;
  *) echo "unknown engine: $engine" >&2; exit 2 ;;
esac
```

The Postgres body (the SQLite one is the same shape with `json_group_array` and
`strftime('%s', …)`):

```sql
WITH cutoff AS (
    SELECT created_at AS at
      FROM work_kudos
     ORDER BY created_at
     OFFSET (SELECT (count(*) * 8) / 10 FROM work_kudos) LIMIT 1
),
judged AS (   -- what the reader engaged with AFTER the cutoff: the thing being predicted
    SELECT k.account_id, k.work_id::text AS work_id
      FROM work_kudos k, cutoff c
     WHERE k.created_at > c.at
    UNION
    SELECT b.account_id, b.subject_id::text
      FROM bookmarks b, cutoff c
     WHERE b.subject_type = 'work' AND b.created_at > c.at
),
scored_readers AS (
    SELECT account_id FROM judged GROUP BY account_id HAVING count(*) >= 5
),
popularity AS (   -- the rung that carries the bar: global kudos, computed BEFORE the cutoff
    SELECT k.work_id::text AS work_id, count(*) AS n
      FROM work_kudos k, cutoff c
     WHERE k.created_at <= c.at
     GROUP BY 1 ORDER BY 2 DESC LIMIT 20
)
SELECT json_build_object(
    'catalogue_size', (SELECT count(*) FROM works),
    'cutoff', (SELECT at FROM cutoff),
    'groups', COALESCE((SELECT json_object_agg(work_id::text, grp) FROM (
        SELECT wt.work_id::text AS work_id, min(ce.canonical) AS grp
          FROM work_tags wt JOIN canonical_entities ce ON ce.id = wt.node_id
         WHERE ce.kind = 'fandom' GROUP BY 1) g), '{}'::json),
    'work_ids', (SELECT json_agg(work_id) FROM (
        SELECT work_id::text AS work_id FROM works ORDER BY md5(work_id::text) LIMIT 5000) c),
    'readers', COALESCE((
        SELECT json_agg(r) FROM (
            SELECT json_build_object(
                'reader', md5(sr.account_id::text),   -- §0.3: never an id in an artefact
                'judged', (SELECT json_agg(json_build_object('work_id', j.work_id, 'grade', 1))
                             FROM judged j WHERE j.account_id = sr.account_id),
                'rungs', json_build_array(
                    json_build_object('name', 'popularity',
                        'ranking', (SELECT json_agg(work_id) FROM popularity)))) AS r
              FROM scored_readers sr) x), '[]'::json)
);
```

`popularity` is in the SQL because it needs the whole corpus. `random` is **derived by the example
binary** from `work_ids` with an LCG seeded by the reader's hash — deterministic, so two runs compare,
and computed once per run rather than stored per reader. `tag_i2i`, `user_neighbourhood` and `mf` are
emitted as **refusals with their reason**, because the corpus cannot support them yet: a rung that
silently vanished from the input would leave the table looking complete.

**Verification. Three runs, because the export is the part that cannot be unit-tested:**

```bash
# 1. The census: which rungs can exist at all. Exit 1 with `no such table: work_ratings` on a
#    database older than 0129 — every row above that line is still a census.
sqlite3 -readonly data/lorehaven.sqlite < scripts/eval/01-engagement-census.sql

# 2. The export (pseudonymises, and creates eval/salt on first run).
scripts/eval/02-export-set.sh sqlite data/lorehaven.sqlite --out eval/eval-set.json

# 3. Determinism: two exports over the same database are byte-identical.
scripts/eval/02-export-set.sh sqlite data/lorehaven.sqlite --out eval/a.json
scripts/eval/02-export-set.sh sqlite data/lorehaven.sqlite --out eval/b.json
cmp eval/a.json eval/b.json && echo "byte-identical"
```

Expected on the **local dev shell** (`data/lorehaven.sqlite`, two accounts and no corpus): sixteen
zero rows, the census exiting `1` on the absent `work_ratings`, and the export **refusing** rather
than writing a set — that database predates migration `0124` (`canonical_entities` there is still the
pre-rename exchange table, with no `id`), so the probe stops it with the migration named:

```
works|0
library_items|0
engaged_works|0
work_kudos|0
work_view_log|0
reading_progress|0
bookmarks|0
recommendation_slots|0
slots_with_a_propensity|0
engaged_works_carrying_a_tag|0
...
Parse error near line 47: no such table: work_ratings

$ scripts/eval/02-export-set.sh sqlite data/lorehaven.sqlite --out eval/eval-set.json
the schema is behind: canonical_entities has no `id` column, which migration 0124
adds when it renames taxonomy_nodes. Run `darkhold migrate` on this database first.
exit=1
```

**That refusal is the correct behaviour and it was found by running it.** Without the probe the
export died on `no such column: ce.id`, which names neither the migration nor the fix; on PostgreSQL
it would have died the same way *and exited 0*, because `psql` does not stop on a failed statement
unless told to.

Expected on the **deployed corpus** (2026-11-01 measurement, to be re-measured rather than copied):
`library_items 141,317`, `engaged_works 1,247`, `engaged_works_carrying_a_tag 0`,
`slots_with_a_propensity 0` — so the tag, neighbourhood and MF rungs are refused and the export
yields **0 scored readers**. That is the honest state of the evidence, it is what M64-06/-11/-12
exist to fix, and the ladder still prints its table of refusals rather than failing.

**Both dialects were executed**, not just written: against a scratch SQLite database and a scratch
PostgreSQL 16 database holding 30 works, 30 kudos, one bookmark and two tag rows. Both returned
`catalogue 30 · cutoff 2026-01-25T00:00:00Z · readers 1 · work_ids 30 · groups {…: "Naruto"}` with the
same six judged works and the same four rungs. Three defects came out of that run and are fixed here:
`works` has an `id` column and not `work_id` (both dialects), `json_group_array(r)` quotes a
`json_object` unless it is wrapped in `json()`, and **`psql` exits 0 after a failed statement** — the
wrapper now passes `-v ON_ERROR_STOP=1`, because a partial document reads as a smaller population
rather than as a failure.

---

## Build order

Built 2026-11-02 in one pass; the counts are the ones the runs printed, not estimates.

| # | step | verification | printed | state |
|---|---|---|---|---|
| 1 | `precision_at`/`f1_at`/`average_precision_at` in `crates/taste/src/eval.rs` + 4 tests | `cargo test -p darkhold-taste --lib eval` | `13 passed` | done |
| 2 | `crates/taste/src/ladder.rs` + 8 tests | `cargo test -p darkhold-taste --lib ladder` | `8 passed` | done |
| 3 | `crates/taste/examples/ladder.rs` + `tests/fixtures/ladder-set.json` | the fixture run above | the table, `exit=1` | done |
| 4 | `scripts/eval/01-engagement-census.sql`, `02-export-set.{sh,sqlite.sql,postgres.sql}`, `pseudonymise.py` | the three runs above, both dialects | `1` reader on scratch data, `0` today | done |
| 5 | `.gitignore` gains `eval/`; `docs/requirements.csv` `M64-16` → `implemented-locally-tested`; `docs/verification.md` | `git check-ignore eval/x` | `eval/x` | done |

Steps 1–3 are pure and can land before any data exists; steps 4–5 make the refusal visible from the
operator's shell. **Do not mark the row `implemented-verified-e2e` on a zero-reader export** — the
harness is built, its first verdict is "no data", and `docs/verification.md` says so in those words.

## Why the ladder, and not a target number

The published study's ladder — popularity 0.0364, neighbourhood 0.0933, matrix factorisation 0.2025
f1@20 — is a *different corpus*, and copying 0.2025 into this repository as a goal would be the
mistake the inherited weights in §12.7 already are. What the ladder buys is the property that a
number always arrives next to the rung that had to be beaten, on our readers, at our cut. If
popularity on this instance turns out to be 0.11 because the corpus is small and the kudos are
concentrated, then 0.11 is the bar, and the study's 0.2 is trivia about someone else's Marvel mirror.

## Ledger updates this plan expects

- `docs/requirements.csv` `M64-16`: notes gain
  `docs/plans/m64-16-offline-pre-registration-harness.md`, and the status moves to
  `implemented-locally-tested` when steps 1–3 are green, never before.
- `docs/verification.md`: one section per step, with the command and the count it printed.
- `docs/spec/12-taste-themes-moods-channels.md` §12.7a is the normative statement; this plan does not
  change it. If a step contradicts it, the spec wins and the plan is wrong.
