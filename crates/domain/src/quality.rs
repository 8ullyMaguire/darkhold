//! Quality metrics that do not depend on how large an archive is.
//!
//! Raw likes are the wrong ranking: a story on a large archive accumulates more
//! of them, and the catalogue proves it — the same story carries 33,107 likes on
//! `sb` (p98.2 of 884 engaged rows) and 4,179 on `qq` (p92.8 of 209). Eight times
//! the raw count, five percentile points apart. Every metric here is therefore
//! computed *within the row's own source* before anything is compared.
//!
//! Two measured facts shape the whole module: 1,247 of 141,317 catalogue rows
//! carry any endorsement count, and `views` is null on all of them. So "no likes"
//! means unmeasured, never "bad", and no rate with a view denominator exists yet.
//!
//! This module is pure: no database, no clock. The caller passes
//! `freshness_days`, and the store fills the candidates.
//!
//! # Why sample size is a separate signal and not a correction inside the rank
//!
//! Shrinking a percentile toward the midpoint by its sample size
//! (`0.5 + (pct−0.5)·n/(n+10)`) looks like the obvious guardrail and it is wrong
//! here: it makes the **achievable maximum depend on `n`**, so the best row of a
//! source with 209 engaged rows can never score above 0.977 while one with 884
//! can reach 0.994. Measured on the live catalogue 2026-11-01, that put 34 of the
//! top 40 `source_pct` rows in the single largest archive — the archive-size bias
//! this whole vocabulary exists to remove, reintroduced by its own guardrail.
//!
//! So the percentile is raw (a source's best row is 1.0 whatever the source's
//! size), and evidence is a signal of its own: [`Signals::confidence`], worth
//! `n/(n+10)`, which the composite weighs instead of the ranking distorting.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// How many unobserved rows `confidence` assumes. Chosen against the live
/// engaged populations (209, 154, 884): it discounts a source with a handful of
/// engaged rows without touching the large ones.
///
/// A percentile computed from four engaged rows is not worth the same as one
/// computed from eight hundred: the first says "best of four", which is what a
/// two-like story in a tiny archive looks like. Each percentile is therefore
/// pulled toward the midpoint by `n / (n + PRIOR_ROWS)`, so a small source can
/// still be right about its best row but cannot claim the confidence of a large
/// one. Ten is a deliberate underestimate of a typical engaged set (the live
/// sources hold 209, 154 and 884) — it corrects the small sources without
/// touching the large ones.
const PERCENTILE_PRIOR_ROWS: f64 = 10.0;

/// Days over which `recency` decays to `1/e`.
const RECENCY_HALF_LIFE_DAYS: f64 = 180.0;

/// A ranking rule over catalogue rows.
///
/// The wire name is the CLI argument, so `--sort source_pct` is exactly
/// [`Metric::SourcePct`]. Raw sorts (`likes`, `aggregate`, `views`, `words`,
/// `updated`) are kept because they are the honest comparison: they are what the
/// catalogue did before, and an operator should be able to see the difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// The composite: source-relative quality, blended with what we can measure.
    Quality,
    /// Percentile of likes among the source's engaged rows, shrunk toward the
    /// midpoint by sample size.
    SourcePct,
    /// Logistic of the z-score of `ln(1+likes)` within the source.
    SourceZ,
    /// Percentile of `likes per 1k words` within the source.
    EngagementDensity,
    /// `source_pct` plus a bonus for being carried by more than one archive.
    Consensus,
    /// Binary: does this story exist on two or more archives?
    Crosspost,
    /// Bucket score on word count — effort, with no endorsement input.
    WordsMaturity,
    /// `min(source_count, 5) / 5`.
    SourceCoverage,
    /// `source_pct` inside the caller's fandom cohort.
    CohortPct,
    /// `exp(-days / 180)` on `source_updated_at`.
    Recency,
    /// How much the source's engaged population is worth, on its own.
    Confidence,
    /// Triage for rows with no endorsement data: which to check first.
    Unmeasured,
    /// Raw imported likes, descending.
    Likes,
    /// `total_likes`, descending: every source plus darkhold's own kudos.
    Aggregate,
    /// Views, descending. Null on every row today.
    Views,
    /// Word count, descending.
    Words,
    /// Most recently changed catalogue row.
    Updated,
    /// Δlikes per week. Blocked: needs two snapshots.
    Velocity,
    /// High velocity percentile, low absolute percentile. Blocked with `Velocity`.
    HiddenGem,
    /// Kudos per 1k views. Blocked on the join, not on the data: per-archive
    /// views are recorded in `library_item_engagement` (M63-37), and the
    /// catalogue sort does not read that table yet.
    LikesPerView,
    /// Wilson lower bound on likes per view — the same join, and a bound rather
    /// than a point estimate so a 3-kudos-over-20-views row cannot outrank one
    /// with 400 kudos over 9,000 views.
    Wilson,
    /// `quality` multiplied by `recency`: the same composite, discounted by age.
    FreshnessWeightedQuality,
    /// `library_items.status` — complete, ongoing, hiatus, cancelled. A stored
    /// column, so the routes order it in SQL rather than scoring it here.
    Completion,
    /// Δkudos per hour. Blocked with [`Self::Velocity`]: a rate needs two
    /// snapshots, and one is all the catalogue has.
    KudosPerHour,
    /// How often an author ships what they start. Blocked: nothing records an
    /// author's update history yet.
    UpdaterReliability,
}

/// What a metric can do with today's data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Computed here, in memory, from the row's own source.
    Ranked,
    /// A plain SQL `ORDER BY` on a stored column.
    Raw,
    /// A metadata ordering for rows with no endorsement data.
    Triage,
    /// Refused, with the reason and the work item that unblocks it.
    Blocked(&'static str),
}

impl Metric {
    /// Every metric, in the order `--list-metrics` prints them.
    pub const ALL: &'static [Self] = &[
        Self::Quality,
        Self::SourcePct,
        Self::SourceZ,
        Self::EngagementDensity,
        Self::Consensus,
        Self::Crosspost,
        Self::WordsMaturity,
        Self::SourceCoverage,
        Self::CohortPct,
        Self::Recency,
        Self::Confidence,
        Self::Unmeasured,
        Self::FreshnessWeightedQuality,
        Self::Completion,
        Self::KudosPerHour,
        Self::UpdaterReliability,
        Self::Likes,
        Self::Aggregate,
        Self::Views,
        Self::Words,
        Self::Updated,
        Self::Velocity,
        Self::HiddenGem,
        Self::LikesPerView,
        Self::Wilson,
    ];

    /// Parse the `sort=` parameter and the `--sort` argument.
    ///
    /// Deliberately strict: an unknown word is `None` rather than a silent
    /// fallback to `likes`, because a caller who typed `--sort velocity` must
    /// not be handed a likes ranking and told it is velocity.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "quality" => Some(Self::Quality),
            "source_pct" | "source-percentile" | "source_percentile" | "pct" => {
                Some(Self::SourcePct)
            }
            "source_z" | "zscore" | "z_score" => Some(Self::SourceZ),
            "engagement_density" | "density" | "likes_per_1k_words" => {
                Some(Self::EngagementDensity)
            }
            "consensus" => Some(Self::Consensus),
            "crosspost" | "crosspost_survival" => Some(Self::Crosspost),
            "words_maturity" | "word_count_maturity" | "maturity" => Some(Self::WordsMaturity),
            "source_coverage" | "coverage" => Some(Self::SourceCoverage),
            "cohort_pct" | "tag_cohort_pct" | "cohort" => Some(Self::CohortPct),
            "recency" | "freshness" => Some(Self::Recency),
            "confidence" | "evidence" => Some(Self::Confidence),
            "unmeasured" | "triage" => Some(Self::Unmeasured),
            "likes" | "like" => Some(Self::Likes),
            "aggregate" | "total" => Some(Self::Aggregate),
            "views" => Some(Self::Views),
            "words" | "word_count" => Some(Self::Words),
            "updated" | "recent" => Some(Self::Updated),
            "velocity" | "momentum" => Some(Self::Velocity),
            "hidden_gem" | "hidden-gem" => Some(Self::HiddenGem),
            "likes_per_view" | "kudos_per_view" => Some(Self::LikesPerView),
            "wilson" | "wilson_lower_bound" => Some(Self::Wilson),
            "freshness_weighted_quality" | "fresh_quality" | "fq" => {
                Some(Self::FreshnessWeightedQuality)
            }
            "completion" | "status" => Some(Self::Completion),
            "kudos_per_hour" | "likes_per_hour" => Some(Self::KudosPerHour),
            "updater_reliability" | "reliability" => Some(Self::UpdaterReliability),
            // The archive's own word for the imported count (the row names it),
            // and the rate spelled the way a reader would type it.
            "kudos" => Some(Self::Likes),
            "kudos_per_1k_views" => Some(Self::LikesPerView),
            _ => None,
        }
    }

    /// The wire name, which is also the CLI argument.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quality => "quality",
            Self::SourcePct => "source_pct",
            Self::SourceZ => "source_z",
            Self::EngagementDensity => "engagement_density",
            Self::Consensus => "consensus",
            Self::Crosspost => "crosspost",
            Self::WordsMaturity => "words_maturity",
            Self::SourceCoverage => "source_coverage",
            Self::CohortPct => "cohort_pct",
            Self::Recency => "recency",
            Self::Confidence => "confidence",
            Self::Unmeasured => "unmeasured",
            Self::Likes => "likes",
            Self::Aggregate => "aggregate",
            Self::Views => "views",
            Self::Words => "words",
            Self::Updated => "updated",
            Self::Velocity => "velocity",
            Self::HiddenGem => "hidden_gem",
            Self::LikesPerView => "likes_per_view",
            Self::Wilson => "wilson",
            Self::FreshnessWeightedQuality => "freshness_weighted_quality",
            Self::Completion => "completion",
            Self::KudosPerHour => "kudos_per_hour",
            Self::UpdaterReliability => "updater_reliability",
        }
    }

    /// Whether the metric can be computed from what the catalogue holds.
    #[must_use]
    pub fn support(self) -> Support {
        match self {
            Self::Likes | Self::Aggregate | Self::Views | Self::Words | Self::Updated => {
                Support::Raw
            }
            // A `CHECK`-constrained stored column: the routes order it in SQL.
            Self::Completion => Support::Raw,
            Self::Unmeasured => Support::Triage,
            Self::Velocity | Self::HiddenGem => Support::Blocked(
                "needs two snapshots of the same row; the monthly recheck (M63-32) creates the second",
            ),
            Self::KudosPerHour => Support::Blocked(
                "Δkudos needs two snapshots of the same row; the monthly recheck (M63-32) creates the second",
            ),
            Self::UpdaterReliability => Support::Blocked(
                "needs the author's update history, which nothing records yet; the recheck (M63-32) starts it",
            ),
            // Ranked, with a data-time refusal: the metric is computable (M63-37 records per-archive views), so what can stop
            // it is a population where no row carries a view count — decided by the store against the loaded rows rather than
            // guessed here, where no data is in scope.
            Self::LikesPerView | Self::Wilson => Support::Ranked,
            Self::Quality
            | Self::SourcePct
            | Self::SourceZ
            | Self::EngagementDensity
            | Self::Consensus
            | Self::Crosspost
            | Self::WordsMaturity
            | Self::SourceCoverage
            | Self::CohortPct
            | Self::Recency
            | Self::Confidence
            | Self::FreshnessWeightedQuality => Support::Ranked,
        }
    }

    /// Whether this metric cannot answer without a view count.
    ///
    /// The store asks this of the loaded rows: a metric that needs views, asked for a population where none carries one, must refuse
    /// by name rather than rank the catalogue by an all-zero signal.
    pub fn needs_views(self) -> bool {
        matches!(self, Self::LikesPerView | Self::Wilson)
    }

    /// The name a refusal should use: the word the caller typed, plus the
    /// canonical name when the two differ.
    ///
    /// `--sort kudos_per_1k_views` answered as `sort: "likes_per_view" is not
    /// available yet` tells the caller about a word they did not type; the alias
    /// and the canonical name both belong in the sentence.
    #[must_use]
    pub fn refusal_name(self, typed: &str) -> String {
        let typed = typed.trim();
        if typed.eq_ignore_ascii_case(self.as_str()) {
            typed.to_owned()
        } else {
            format!("{typed} (= {})", self.as_str())
        }
    }

    /// One line for `--list-metrics`.
    #[must_use]
    pub fn help(self) -> &'static str {
        match self {
            Self::Quality => {
                "0.45 source_pct + 0.30 consensus + 0.15 density + 0.10 crosspost, renormalised"
            }
            Self::SourcePct => "percentile of likes among the source's engaged rows (raw)",
            Self::SourceZ => "logistic z-score of ln(1+likes) within the source",
            Self::EngagementDensity => "percentile of likes per 1k words within the source",
            Self::Consensus => "source_pct plus a bonus for other archives carrying it",
            Self::Crosspost => "1.0 when two or more archives carry the story",
            Self::WordsMaturity => "<10k 0.2, 10-50k 0.45, 50-200k 0.8, >=200k 1.0",
            Self::SourceCoverage => "min(source_count, 5) / 5",
            Self::CohortPct => "source_pct inside the --fandom cohort",
            Self::Recency => "exp(-days/180) on the source's last update",
            Self::Confidence => "n/(n+10): how much the source's engaged population is worth",
            Self::Unmeasured => "0.4 coverage + 0.3 maturity + 0.3 cross-archive presence",
            Self::Likes => "raw imported likes (what the catalogue did before)",
            Self::Aggregate => "total likes across every source plus darkhold kudos",
            Self::Views => "stored views (null on every row)",
            Self::Words => "word count",
            Self::Updated => "most recently changed catalogue row",
            Self::Velocity => "likes gained per week",
            Self::HiddenGem => "high velocity, low absolute count",
            Self::LikesPerView => "likes per 1k views",
            Self::Wilson => "Wilson lower bound on likes per view",
            Self::FreshnessWeightedQuality => "quality × exp(-days/180); an undated row scores 0",
            Self::Completion => "complete, then ongoing, hiatus, unknown, cancelled",
            Self::KudosPerHour => "kudos gained per hour",
            Self::UpdaterReliability => "how often an author finishes what they start",
        }
    }
}

/// One row, reduced to the fields a metric needs.
///
/// `freshness_days` arrives pre-computed so this module never reads a clock;
/// `None` means the row has no source date (2,009 of `sb`'s rows do not).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    /// Library item id.
    pub id: String,
    /// The archive it came from.
    pub source_key: String,
    /// Imported endorsement count.
    pub likes: i64,
    /// The source's word count, when it publishes one.
    pub word_count: Option<i64>,
    /// How many archives carry this story.
    pub source_count: i64,
    /// Likes summed across every copy, including this one.
    pub total_likes: i64,
    /// Days since the author last touched it.
    pub freshness_days: Option<f64>,
    /// The view/hit count **that same archive** published (M63-37), or `None`
    /// when it publishes none. Absent is not zero: a board that reports likes and
    /// no views has no rate, and treating that as `0` would rank it below every
    /// story whose archive happens to publish hits.
    pub views: Option<i64>,
}

/// The individual signals behind one row's score.
///
/// Every field is optional because every one of them can be absent for a real
/// reason: no word count, no source date, no endorsement. A missing signal is
/// dropped from the composite and the remaining weights are renormalised, rather
/// than being read as a zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Signals {
    /// [`Metric::SourcePct`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_pct: Option<f64>,
    /// [`Metric::SourceZ`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_z: Option<f64>,
    /// [`Metric::EngagementDensity`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub density: Option<f64>,
    /// [`Metric::Consensus`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consensus: Option<f64>,
    /// [`Metric::Crosspost`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crosspost: Option<f64>,
    /// [`Metric::WordsMaturity`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words_maturity: Option<f64>,
    /// [`Metric::SourceCoverage`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_coverage: Option<f64>,
    /// [`Metric::Recency`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recency: Option<f64>,
    /// How much the source's engaged population is worth: `n/(n+10)`.
    ///
    /// Separate from [`Signals::source_pct`] on purpose: the percentile says how
    /// good the row is *for its archive*, this says how much that claim is worth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    /// [`Metric::Unmeasured`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triage: Option<f64>,
    /// [`Metric::LikesPerView`]: endorsements per view, from one archive's own pair.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate: Option<f64>,
    /// [`Metric::Wilson`]: the lower bound of that rate's 95% interval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wilson: Option<f64>,
}

/// One ranked row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    /// Library item id — the join key back to the stored row.
    pub id: String,
    /// The metric's score, 0..=1 for every ranked metric.
    pub score: f64,
    /// False for a row the metric could not measure; it is ordered by triage.
    pub measured: bool,
    /// The components of the score.
    pub signals: Signals,
}

/// The page, plus the numbers that stop it reading as a verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ranking {
    /// The metric these scores belong to.
    pub metric: Metric,
    /// Engaged rows the metric could score, across the whole filtered set — not
    /// just this page. This is the gate: `ranked N of total`.
    pub ranked: usize,
    /// Rows the filters matched.
    pub total: usize,
    /// The page.
    pub hits: Vec<Hit>,
}

/// Per-source distributions, built once per ranking.
#[derive(Debug, Default)]
struct SourceStats {
    /// Raw likes for engaged rows, sorted ascending — the percentile's input.
    raw: Vec<f64>,
    /// `ln(1+likes)` for engaged rows.
    log_likes: Vec<f64>,
    /// `likes per 1k words` for engaged rows that publish a word count, sorted.
    density: Vec<f64>,
    /// Sum and sum of squares of `log_likes`, for the z-score.
    log_sum: f64,
    log_sq_sum: f64,
}

impl SourceStats {
    fn finish(&mut self) {
        self.raw
            .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        self.density
            .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
}

/// Where `value` sits in `sorted`: 0 at the bottom, 1 at the top.
///
/// The divisor is `len - 1`, not `len`, and that is the whole point: a mid-rank
/// percentile over `n` rows tops out at `(n − 0.5)/n`, so the best of a four-row
/// archive would score 0.875 while the best of a four-hundred-row one scores
/// 0.9988 — an n-dependent ceiling, which is the same archive-size bias in a
/// milder dress. Anchoring both ends makes "the best row of this archive" mean
/// the same thing everywhere, and leaves the size question to
/// [`Signals::confidence`], where it belongs.
///
/// Ties share a position: `below + 0.5·(equal − 1)` over `len − 1`. A single row
/// has no order to express and scores 0.5.
fn percentile_of(value: f64, sorted: &[f64]) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    if sorted.len() == 1 {
        return Some(0.5);
    }
    let below = sorted.iter().filter(|&&v| v < value).count() as f64;
    let equal = sorted.iter().filter(|&&v| v == value).count() as f64;
    let position = below + 0.5 * (equal - 1.0);
    Some((position / (sorted.len() as f64 - 1.0)).clamp(0.0, 1.0))
}

/// How much a percentile from `n` rows is worth: `n / (n + k)`.
///
/// Deliberately *outside* the percentile. A percentile is a statement about how
/// many rows are below a value, and it is as trustworthy as there are rows; that
/// is a separate fact from the value itself, and folding it in makes the top of a
/// small source unreachable instead of merely less certain.
fn confidence(n: usize) -> f64 {
    let n = n as f64;
    n / (n + PERCENTILE_PRIOR_ROWS)
}

/// Bucket score on length. `None` when the source publishes no word count.
fn words_maturity(word_count: Option<i64>) -> Option<f64> {
    let words = word_count?;
    Some(if words < 10_000 {
        0.2
    } else if words < 50_000 {
        0.45
    } else if words < 200_000 {
        0.8
    } else {
        1.0
    })
}

fn logistic(z: f64) -> f64 {
    1.0 / (1.0 + (-z).exp())
}

/// Rank `candidates` by `metric`, returning the page plus the gate.
///
/// Every metric is accepted, including the raw columns: the routes answer those
/// with a SQL `ORDER BY`, but the store's ranker is also reachable directly and a
/// silent zero-score ordering would be a trap.
///
/// Measured rows always precede unmeasured ones, whatever the metric: comparing
/// a percentile to a row with no endorsement data is the mistake this whole
/// module exists to avoid, so the page is two blocks and [`Hit::measured`] says
/// which block a row is in.
#[must_use]
pub fn rank(metric: Metric, candidates: &[Candidate], limit: usize, offset: usize) -> Ranking {
    rank_with(metric, candidates, limit, offset, &QualityWeights::default())
}

/// [`rank`] with an operator's weights (M63-38). The same population sorted two ways is two judgements, and the caller that owns the
/// catalogue is the one entitled to make the second.
pub fn rank_with(
    metric: Metric,
    candidates: &[Candidate],
    limit: usize,
    offset: usize,
    weights: &QualityWeights,
) -> Ranking {
    let total = candidates.len();
    let engaged: Vec<&Candidate> = candidates.iter().filter(|c| c.likes > 0).collect();
    let ranked = engaged.len();

    let mut stats: HashMap<&str, SourceStats> = HashMap::new();
    for candidate in &engaged {
        let entry = stats.entry(candidate.source_key.as_str()).or_default();
        let log_likes = (1.0 + candidate.likes as f64).ln();
        entry.raw.push(candidate.likes as f64);
        entry.log_likes.push(log_likes);
        entry.log_sum += log_likes;
        entry.log_sq_sum += log_likes * log_likes;
    }
    for s in stats.values_mut() {
        s.finish();
    }
    for candidate in &engaged {
        if let Some(words) = candidate.word_count.filter(|w| *w > 0) {
            let density = (candidate.likes as f64) * 1_000.0 / (words as f64);
            if let Some(s) = stats.get_mut(candidate.source_key.as_str()) {
                s.density.push(density);
            }
        }
    }
    for s in stats.values_mut() {
        s.density
            .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }

    let signals_of = |candidate: &Candidate| -> Signals {
        let s = stats.get(candidate.source_key.as_str());
        let coverage = (candidate.source_count.clamp(0, 5) as f64) / 5.0;
        let maturity = words_maturity(candidate.word_count);
        let crosspost = if candidate.source_count >= 2 {
            1.0
        } else {
            0.0
        };
        let recency = candidate
            .freshness_days
            .map(|days| (-days / RECENCY_HALF_LIFE_DAYS).exp());

        let mut signals = Signals {
            crosspost: Some(crosspost),
            source_coverage: Some(coverage),
            words_maturity: maturity,
            recency,
            ..Signals::default()
        };

        // A rate exists when this archive published both halves and the
        // denominator is real. `Wilson` is the point estimate's lower bound, so a
        // 3-kudos-over-20-views row cannot outrank 400 over 9,000 on a fluke.
        if let (Some(views), likes) = (candidate.views.filter(|v| *v > 0), candidate.likes.max(0)) {
            if views > 0 && likes >= 0 {
                signals.rate = Some(likes as f64 / views as f64);
                signals.wilson = wilson_lower_bound(likes as u64, views as u64);
            }
        }

        if candidate.likes > 0 {
            if let Some(s) = s {
                signals.source_pct = percentile_of(candidate.likes as f64, &s.raw);
                signals.confidence = Some(confidence(s.raw.len()));
                let n = s.log_likes.len() as f64;
                if n >= 2.0 {
                    let mean = s.log_sum / n;
                    let variance = (s.log_sq_sum / n) - mean * mean;
                    let sd = variance.max(0.0).sqrt();
                    // A source where every engaged row has the same count has no
                    // spread to standardise against; the midpoint is the only
                    // honest answer, not a division by zero.
                    signals.source_z = Some(if sd > 0.0 {
                        let log_likes = (1.0 + candidate.likes as f64).ln();
                        logistic((log_likes - mean) / sd)
                    } else {
                        0.5
                    });
                } else {
                    signals.source_z = Some(0.5);
                }
                if let Some(words) = candidate.word_count.filter(|w| *w > 0) {
                    let density = (candidate.likes as f64) * 1_000.0 / (words as f64);
                    signals.density = percentile_of(density, &s.density);
                }
            }
            let cross_bonus = 0.10 * ((candidate.source_count.clamp(1, 3) - 1) as f64)
                + if candidate.total_likes > candidate.likes {
                    0.05
                } else {
                    0.0
                };
            signals.consensus = signals
                .source_pct
                .map(|p| (p + cross_bonus).clamp(0.0, 1.0));
        } else {
            // No endorsement data: nothing about quality is known, so only the
            // metadata signals are filled and the triage score decides the order.
            signals.triage = Some(
                0.4 * coverage
                    + 0.3 * maturity.unwrap_or(0.0)
                    + 0.3 * (candidate.source_count.clamp(0, 3) as f64) / 3.0,
            );
        }
        signals
    };

    let mut hits: Vec<Hit> = Vec::with_capacity(engaged.len());
    for candidate in &engaged {
        let signals = signals_of(candidate);
        // The raw sorts are here so the store can rank by any name it is given;
        // the routes still use SQL for them, which is cheaper. `ln_1p` keeps the
        // score in a sane range without changing the order.
        let score = match metric {
            Metric::Likes => (candidate.likes as f64).ln_1p(),
            Metric::Aggregate => (candidate.total_likes as f64).ln_1p(),
            Metric::Words => candidate.word_count.map_or(0.0, |w| (w as f64).ln_1p()),
            Metric::Views => 0.0,
            // Fresher means fewer days, and the sort is descending.
            Metric::Updated => candidate.freshness_days.map_or(f64::MIN, |days| -days),
            _ => score_of(metric, &signals, weights),
        };
        hits.push(Hit {
            id: candidate.id.clone(),
            score,
            measured: true,
            signals,
        });
    }
    // Unmeasured rows: a page never mixes the two blocks, and within the block
    // the order is metadata, which is what `unmeasured` is for.
    let mut unmeasured: Vec<Hit> = candidates
        .iter()
        .filter(|c| c.likes <= 0)
        .map(|candidate| {
            let signals = signals_of(candidate);
            Hit {
                id: candidate.id.clone(),
                score: signals.triage.unwrap_or(0.0),
                measured: false,
                signals,
            }
        })
        .collect();

    let tie_break = |a: &Hit, b: &Hit| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    };
    hits.sort_by(tie_break);
    unmeasured.sort_by(tie_break);

    let ordered: Vec<Hit> = if metric == Metric::Unmeasured {
        // Triage is the point of this metric, so it is the whole page — but the
        // gate still reports how many rows carry an endorsement, because that is
        // the number that says whether any of this is evidence of quality.
        unmeasured.into_iter().chain(hits).collect()
    } else {
        hits.iter().cloned().chain(unmeasured).collect()
    };

    let hits = ordered.into_iter().skip(offset).take(limit).collect();
    Ranking {
        metric,
        ranked,
        total,
        hits,
    }
}


/// Wilson score interval lower bound for `successes` out of `trials`, at 95%.
///
/// Why a bound and not the plain ratio: 1 kudos over 10 hits is a 10% rate and 400 over 9,000 is 4.4%, and a reader told the first is
/// "twice as well received" has been told a story about one person clicking. The 95% lower bound puts them at 1.8% and 4.0% — the
/// ordering reverses, because the interval around ten views is enormous.
///
/// **Where it is not enough, measured:** 3/20 bounds to 5.2%, still above 400/9,000's 4.0%. A 95% bound does not rescue every thin ratio,
/// only ones whose absolute count is small enough for the interval to swallow the point estimate. That is pinned by a test rather than
/// assumed, because a bound that quietly fails to reorder is a metric a caller will trust for the wrong reason.
///
/// `None` when `trials` is zero: a rate over no views is undefined, not zero. Successes are clamped to trials, because a 1,001st kudos
/// on 1,000 views is a source bug and a bound computed from it would be a fabricated number rather than a refused one.
pub fn wilson_lower_bound(successes: u64, trials: u64) -> Option<f64> {
    if trials == 0 {
        return None;
    }
    const Z: f64 = 1.959_963_984_540_054;
    let n = trials as f64;
    let successes = successes.min(trials);
    let p = successes as f64 / n;
    let z2 = Z * Z;
    let denominator = 1.0 + z2 / n;
    let centre = p + z2 / (2.0 * n);
    let margin = Z * ((p * (1.0 - p) / n) + z2 / (4.0 * n * n)).sqrt();
    Some(((centre - margin) / denominator).clamp(0.0, 1.0))
}


/// The weights of the `quality` composite, as data instead of literals (M63-38).
///
/// `quality` is a weighted mean over five signals, renormalised over whichever of them a row actually has. Which five weights are right
/// is an operator's judgement about their own catalogue — an archive where every source publishes word counts can afford to trust
/// density; one where most rows have no dates must not lean on recency. Compiled-in numbers make that judgement invisible and
/// unarguable, so they live here and are configured per profile.
///
/// Every field is a non-negative weight. The mean is renormalised, so the values need not sum to one: `(0.4, 0.25, 0.15, 0.1, 0.1)` and
/// `(4, 2.5, 1.5, 1, 1)` are the same ranking, and requiring a sum would be a validation error that rejects a legal choice.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct QualityWeights {
    /// [`Metric::SourcePct`].
    #[serde(default = "default_source_pct")]
    pub source_pct: f64,
    /// [`Metric::Consensus`].
    #[serde(default = "default_consensus")]
    pub consensus: f64,
    /// [`Metric::EngagementDensity`].
    #[serde(default = "default_density")]
    pub density: f64,
    /// [`Metric::Crosspost`].
    #[serde(default = "default_crosspost")]
    pub crosspost: f64,
    /// [`Metric::Confidence`] — evidence, not quality: how much the source's sample is worth.
    #[serde(default = "default_confidence")]
    pub confidence: f64,
}

// Per-field serde defaults, so a profile may state one weight and inherit the other four. A struct-level `default` would make a section
// that mentions only `density` silently zero the rest, which is the opposite of what an operator writing one line means.
fn default_source_pct() -> f64 {
    0.40
}
fn default_consensus() -> f64 {
    0.25
}
fn default_density() -> f64 {
    0.15
}
fn default_crosspost() -> f64 {
    0.10
}
fn default_confidence() -> f64 {
    0.10
}

impl Default for QualityWeights {
    /// The numbers this module shipped with. Named so a profile that only wants to change one weight can lean on the rest.
    fn default() -> Self {
        Self {
            source_pct: 0.40,
            consensus: 0.25,
            density: 0.15,
            crosspost: 0.10,
            confidence: 0.10,
        }
    }
}

impl QualityWeights {
    /// Refuse weights that cannot produce a ranking.
    ///
    /// Three things are refused, each for a different failure: a negative weight inverts a signal the operator meant to favour; `NaN` or
    /// infinity makes every comparison false, so the sort silently falls back to id order; and all-zero weights make `quality` the same
    /// number for every row, which is not a ranking but looks like one.
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("source_pct", self.source_pct),
            ("consensus", self.consensus),
            ("density", self.density),
            ("crosspost", self.crosspost),
            ("confidence", self.confidence),
        ] {
            if !value.is_finite() {
                return Err(format!("quality weight `{name}` is {value}, which is not a number"));
            }
            if value < 0.0 {
                return Err(format!(
                    "quality weight `{name}` is {value}: a negative weight inverts the signal"
                ));
            }
        }
        if self.total() <= 0.0 {
            return Err("every quality weight is zero, which makes `quality` identical for every row".to_owned());
        }
        Ok(())
    }

    /// The sum of the weights. Not required to be one — [`QualityWeights`] explains why — but the caller can report it in `dh doctor`.
    pub fn total(&self) -> f64 {
        self.source_pct + self.consensus + self.density + self.crosspost + self.confidence
    }

    /// How much of the composite this signal carries, as a share of the total, for a surface that wants to explain itself.
    pub fn share(&self, signal: &str) -> Option<f64> {
        let weight = match signal {
            "source_pct" => self.source_pct,
            "consensus" => self.consensus,
            "density" => self.density,
            "crosspost" => self.crosspost,
            "confidence" => self.confidence,
            _ => return None,
        };
        let total = self.total();
        if total > 0.0 {
            Some(weight / total)
        } else {
            None
        }
    }
}

/// The composite: source-relative quality, renormalised over what the row has.
fn score_of(metric: Metric, signals: &Signals, weights: &QualityWeights) -> f64 {
    match metric {
        Metric::Quality => {
            let parts: [(Option<f64>, f64); 5] = [
                (signals.source_pct, weights.source_pct),
                (signals.consensus, weights.consensus),
                (signals.density, weights.density),
                (signals.crosspost, weights.crosspost),
                // Evidence, not quality: a row that leads a four-row archive is
                // not worth the same as one that leads eight hundred.
                (signals.confidence, weights.confidence),
            ];
            let mut weighted = 0.0;
            let mut weight = 0.0;
            for (value, w) in parts {
                if let Some(value) = value {
                    weighted += value * w;
                    weight += w;
                }
            }
            if weight > 0.0 {
                weighted / weight
            } else {
                0.0
            }
        }
        Metric::SourcePct | Metric::CohortPct => signals.source_pct.unwrap_or(0.0),
        Metric::SourceZ => signals.source_z.unwrap_or(0.0),
        Metric::EngagementDensity => signals.density.unwrap_or(0.0),
        Metric::Consensus => signals.consensus.unwrap_or(0.0),
        Metric::Crosspost => signals.crosspost.unwrap_or(0.0),
        Metric::WordsMaturity => signals.words_maturity.unwrap_or(0.0),
        Metric::SourceCoverage => signals.source_coverage.unwrap_or(0.0),
        Metric::Recency => signals.recency.unwrap_or(0.0),
        Metric::Confidence => signals.confidence.unwrap_or(0.0),
        Metric::Unmeasured => signals.triage.unwrap_or(0.0),
        // Freshness is what the metric is for, so a missing date scores zero
        // rather than being renormalised away — the same choice `Recency` makes.
        Metric::FreshnessWeightedQuality => {
            score_of(Metric::Quality, signals, weights) * signals.recency.unwrap_or(0.0)
        }
        Metric::LikesPerView => signals.rate.unwrap_or(0.0),
        Metric::Wilson => signals.wilson.unwrap_or(0.0),
        Metric::Likes
        | Metric::Aggregate
        | Metric::Views
        | Metric::Words
        | Metric::Updated
        | Metric::Completion
        | Metric::Velocity
        | Metric::HiddenGem
        | Metric::KudosPerHour
        | Metric::UpdaterReliability => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, source: &str, likes: i64, words: Option<i64>) -> Candidate {
        Candidate {
            id: id.to_owned(),
            source_key: source.to_owned(),
            likes,
            word_count: words,
            source_count: 1,
            total_likes: likes,
            freshness_days: Some(30.0),
            views: None,
        }
    }

    /// A row whose archive published both halves of the rate.
    fn viewed(id: &str, source: &str, likes: i64, views: i64) -> Candidate {
        Candidate {
            views: Some(views),
            ..candidate(id, source, likes, Some(20_000))
        }
    }


    /// M63-38: two operators, one catalogue, two defensible orderings. The weights are data, so this is provable without a config file.
    #[test]
    fn different_weights_rank_the_same_population_differently() {
        // `wide` leads its archive but has a modest density; `dense` is mid-pack but short and heavily liked per 1k words. A
        // source-percentile profile ranks `wide` first; a density profile ranks `dense` first. Neither is wrong — which is the point.
        let mut population = vec![
            // Most liked in its archive, and the worst likes-per-word: a long story that is widely read.
            candidate("wide", "sb", 1_000, Some(500_000)),
            // Mid-pack on likes, and by far the most likes per 1k words: short and heavily liked.
            candidate("dense", "sb", 900, Some(2_000)),
        ];
        for i in 0..8 {
            population.push(candidate(&format!("f{i}"), "sb", 100 + i * 50, Some(50_000)));
        }
        // Two single-signal profiles, because the property is that the mixture is data: one operator trusts position in the archive,
        // the other trusts engagement per word, and the same population answers both questions differently.
        let percentile = QualityWeights {
            source_pct: 1.0,
            consensus: 0.0,
            density: 0.0,
            crosspost: 0.0,
            confidence: 0.0,
        };
        let density = QualityWeights {
            source_pct: 0.0,
            consensus: 0.0,
            density: 1.0,
            crosspost: 0.0,
            confidence: 0.0,
        };
        assert!(percentile.validate().is_ok());
        assert!(density.validate().is_ok());

        let a = rank_with(Metric::Quality, &population, 2, 0, &percentile);
        let b = rank_with(Metric::Quality, &population, 2, 0, &density);
        assert_eq!(a.hits[0].id, "wide", "percentile weights trust position");
        assert_eq!(b.hits[0].id, "dense", "density weights trust likes per 1k words");
        // The signals are the same rows observed the same way; only the mixture changed, so `dense` carries the better density in both
        // rankings even where it loses the page.
        let density_of = |ranking: &Ranking, id: &str| {
            ranking
                .hits
                .iter()
                .find(|hit| hit.id == id)
                .and_then(|hit| hit.signals.density)
        };
        assert!(
            density_of(&a, "dense") > density_of(&a, "wide"),
            "density is a property of the row, not of the weights"
        );

        // Weights that cannot rank are refused rather than silently producing id order.
        assert!(QualityWeights { source_pct: -0.1, ..QualityWeights::default() }
            .validate()
            .is_err());
        assert!(QualityWeights { source_pct: f64::NAN, ..QualityWeights::default() }
            .validate()
            .is_err());
        assert!(QualityWeights {
            source_pct: 0.0,
            consensus: 0.0,
            density: 0.0,
            crosspost: 0.0,
            confidence: 0.0,
        }
        .validate()
        .is_err());
        // Scaling every weight leaves the ranking identical: the mean is renormalised, not summed.
        let scaled = QualityWeights {
            source_pct: 4.0,
            consensus: 0.0,
            density: 0.0,
            crosspost: 0.0,
            confidence: 0.0,
        };
        let c = rank_with(Metric::Quality, &population, 2, 0, &scaled);
        assert_eq!(c.hits[0].id, a.hits[0].id);
        assert_eq!(c.hits[1].id, a.hits[1].id);
    }

    /// The bound, not the point estimate: 1 kudos over 10 hits is a 10% rate and the weakest evidence there is, so the 95% bound puts it
    /// *below* 400 over 9,000 (4.4%).
    #[test]
    fn the_wilson_bound_ranks_evidence_over_a_lucky_ratio() {
        let lucky = wilson_lower_bound(1, 10).expect("a bound");
        let solid = wilson_lower_bound(400, 9_000).expect("a bound");
        assert!(lucky < 0.02, "1/10 bounds to 1.8%, was {lucky}");
        assert!(solid > 0.04, "400/9000 bounds above 4%, was {solid}");
        assert!(lucky < solid, "the thin arm ranks below the evidenced one");
        // And the case the bound does NOT rescue, pinned so nobody assumes it does: 3/20 is 5.2%, still above 4.0%. A 95% bound saves
        // ratios whose absolute count is tiny, not every ratio above the incumbent.
        let thin_but_not_tiny = wilson_lower_bound(3, 20).expect("a bound");
        assert!(
            thin_but_not_tiny > solid,
            "3/20 ({thin_but_not_tiny}) outranks 400/9000 ({solid}) on the bound — documented, not wished away"
        );
        // The ratio says the opposite, which is the whole reason for the bound.
        fn ratio(successes: i64, trials: i64) -> f64 {
            successes as f64 / trials as f64
        }
        assert!(
            ratio(1, 10) > ratio(400, 9_000),
            "the point estimate prefers the lucky row; the bound reverses it"
        );
        // More views at the same rate moves the bound up: the interval narrows toward the estimate.
        assert!(wilson_lower_bound(40, 900).unwrap() > wilson_lower_bound(4, 90).unwrap());
        // No views is undefined, not zero — a rate over nothing is not a bad rate.
        assert_eq!(wilson_lower_bound(0, 0), None);
        // Successes above trials are a source bug clamped to certainty, not a fabricated above-100% bound.
        assert_eq!(wilson_lower_bound(1_001, 1_000), wilson_lower_bound(1_000, 1_000));
    }

    /// A rate is only computed from a row whose archive published both halves; the metric then scores it, and a row with no view count
    /// scores zero and lands in the triage block rather than being ranked as if it had no readers.
    #[test]
    fn a_rate_needs_both_halves_from_the_same_archive() {
        let mut population = vec![
            viewed("a", "ao3", 50, 1_000),
            viewed("b", "ao3", 5, 1_000),
            candidate("c", "sb", 30, Some(20_000)),
        ];
        for i in 0..6 {
            population.push(candidate(&format!("s{i}"), "sb", 10 + i * 5, Some(30_000)));
        }
        let ranking = rank(Metric::LikesPerView, &population, 10, 0);
        let first = ranking.hits.first().expect("a hit");
        assert_eq!(first.id, "a", "5% beats 0.5% and beats a row with no views");
        let c = ranking.hits.iter().find(|hit| hit.id == "c").expect("c ranked");
        assert_eq!(c.signals.rate, None, "no views published, no rate invented");
        // Wilson orders the same pair the same way, with a tighter gap.
        let wilson = rank(Metric::Wilson, &population, 10, 0);
        assert_eq!(wilson.hits.first().unwrap().id, "a");
    }

    /// The case that motivates the module, with the live population sizes: the
    /// same story on `sb` (33,107 likes, 884 engaged rows) and on `qq` (4,179,
    /// 209). Raw likes are 8x apart; source-relative scores must be close, because
    /// with these sample sizes the guardrail barely applies — which is correct.
    #[test]
    fn the_same_story_scores_close_in_two_archives_of_different_size() {
        let mut candidates = Vec::new();
        // 884 engaged rows on sb, the story third from the top.
        for i in 0..881 {
            candidates.push(candidate(
                &format!("sb{i}"),
                "sb",
                10 + i as i64 * 20,
                Some(50_000),
            ));
        }
        candidates.push(candidate("sb-second", "sb", 40_000, Some(200_000)));
        candidates.push(candidate("sb-first", "sb", 45_000, Some(200_000)));
        candidates.push(candidate("sb-marvel", "sb", 33_107, Some(200_000)));
        // 209 engaged rows on qq, the story at the top.
        for i in 0..208 {
            candidates.push(candidate(
                &format!("qq{i}"),
                "qq",
                5 + i as i64 * 10,
                Some(50_000),
            ));
        }
        candidates.push(candidate("qq-marvel", "qq", 4_179, Some(200_000)));

        let ranked = rank(Metric::SourcePct, &candidates, 2_000, 0);
        let sb = ranked
            .hits
            .iter()
            .find(|h| h.id == "sb-marvel")
            .expect("sb copy ranked");
        let qq = ranked
            .hits
            .iter()
            .find(|h| h.id == "qq-marvel")
            .expect("qq copy ranked");
        let gap = (sb.score - qq.score).abs();
        assert!(
            gap < 0.10,
            "percentile gap {gap} should be small; raw likes are 8x apart"
        );
        assert!(
            sb.score > 0.85 && qq.score > 0.85,
            "both are top-of-archive stories"
        );
    }

    /// A single like in a large archive is a weak story, not a strong one — the
    /// failure mode of percentile-over-all-rows, which would score it p99.
    #[test]
    fn one_like_in_a_big_archive_scores_near_the_floor() {
        let mut candidates = vec![candidate("weak", "sb", 1, Some(5_000))];
        for i in 0..499 {
            candidates.push(candidate(
                &format!("s{i}"),
                "sb",
                100 + i as i64,
                Some(5_000),
            ));
        }
        let ranked = rank(Metric::SourcePct, &candidates, 1_000, 0);
        let weak = ranked.hits.iter().find(|h| h.id == "weak").expect("ranked");
        assert!(weak.score < 0.05, "score was {}", weak.score);
        assert!(!ranked
            .hits
            .iter()
            .take_while(|h| h.measured)
            .any(|h| h.id == "weak" && h.score > 0.5));
    }

    /// The reason the guardrail is *not* inside the percentile: shrinking it by
    /// sample size caps the achievable score at `0.5 + 0.5·n/(n+10)`, so a source
    /// with 4 engaged rows could never reach 1.0. On the live catalogue that put
    /// 34 of the top 40 rows in the largest archive.
    #[test]
    fn the_best_row_of_any_source_reaches_one_whatever_the_source_size() {
        let mut candidates = vec![candidate("tiny-top", "qq", 2, Some(20_000))];
        for i in 0..3 {
            candidates.push(candidate(&format!("tiny-{i}"), "qq", 1, Some(20_000)));
        }
        for i in 0..400 {
            candidates.push(candidate(
                &format!("big{i}"),
                "sb",
                10 + i as i64,
                Some(20_000),
            ));
        }
        candidates.push(candidate("big-top", "sb", 4_000, Some(20_000)));

        let ranked = rank(Metric::SourcePct, &candidates, 500, 0);
        let score = |id: &str| {
            ranked
                .hits
                .iter()
                .find(|h| h.id == id)
                .unwrap_or_else(|| panic!("{id} ranked"))
                .score
        };
        assert!(
            (score("tiny-top") - 1.0).abs() < 1e-9,
            "best of its own archive"
        );
        assert!(
            (score("big-top") - 1.0).abs() < 1e-9,
            "best of its own archive"
        );
    }

    /// Evidence is its own signal, and it is what discounts a small population —
    /// in the composite, not by capping the rank.
    #[test]
    fn confidence_is_what_discounts_a_small_engaged_population() {
        let mut candidates = vec![candidate("tiny-top", "qq", 2, Some(20_000))];
        for i in 0..3 {
            candidates.push(candidate(&format!("tiny-{i}"), "qq", 1, Some(20_000)));
        }
        for i in 0..400 {
            candidates.push(candidate(
                &format!("big{i}"),
                "sb",
                10 + i as i64,
                Some(20_000),
            ));
        }
        candidates.push(candidate("big-top", "sb", 4_000, Some(20_000)));

        let ranked = rank(Metric::Confidence, &candidates, 500, 0);
        let tiny = ranked
            .hits
            .iter()
            .find(|h| h.id == "tiny-top")
            .expect("ranked");
        let big = ranked
            .hits
            .iter()
            .find(|h| h.id == "big-top")
            .expect("ranked");
        assert!(
            tiny.score < 0.3,
            "four rows are worth little: {}",
            tiny.score
        );
        assert!(
            big.score > 0.9,
            "four hundred are worth a lot: {}",
            big.score
        );

        // And the composite, which weighs it, prefers the better-evidenced row
        // even though both lead their own archive.
        let quality = rank(Metric::Quality, &candidates, 500, 0);
        let q = |id: &str| {
            quality
                .hits
                .iter()
                .find(|h| h.id == id)
                .expect("ranked")
                .score
        };
        assert!(
            q("big-top") > q("tiny-top"),
            "quality must prefer evidence: {} vs {}",
            q("big-top"),
            q("tiny-top")
        );
    }

    /// Unmeasured rows never mingle with measured ones, whatever the metric.
    #[test]
    fn unmeasured_rows_follow_measured_ones_and_say_so() {
        let candidates = vec![
            candidate("no-data", "sb", 0, Some(60_000)),
            candidate("has-data", "sb", 900, Some(60_000)),
        ];
        let ranked = rank(Metric::Quality, &candidates, 10, 0);
        assert_eq!(ranked.ranked, 1);
        assert_eq!(ranked.total, 2);
        assert_eq!(ranked.hits[0].id, "has-data");
        assert!(ranked.hits[0].measured);
        assert_eq!(ranked.hits[1].id, "no-data");
        assert!(!ranked.hits[1].measured);
    }

    /// The triage metric ranks the unmeasured block on metadata, and the gate
    /// still reports zero measured rows — it is not evidence of quality.
    #[test]
    fn triage_ranks_metadata_and_claims_no_quality() {
        let mut zero = candidate("lonely", "sb", 0, None);
        zero.source_count = 1;
        let mut cross = candidate("everywhere", "sb", 0, Some(220_000));
        cross.source_count = 4;
        let ranked = rank(Metric::Unmeasured, &[zero, cross], 5, 0);
        assert_eq!(ranked.ranked, 0);
        assert_eq!(ranked.hits[0].id, "everywhere");
        assert!(ranked.hits.iter().all(|h| !h.measured));
        assert!(ranked.hits[0].signals.triage.unwrap() > ranked.hits[1].signals.triage.unwrap());
    }

    #[test]
    fn a_composite_drops_a_missing_signal_and_renormalises_rather_than_zeroing_it() {
        let with_words = candidate("with", "sb", 500, Some(120_000));
        let without = candidate("without", "sb", 500, None);
        let ranked = rank(Metric::Quality, &[with_words, without], 5, 0);
        for hit in &ranked.hits {
            assert!(
                hit.score > 0.0,
                "a missing word count must not zero the composite"
            );
        }
    }

    #[test]
    fn every_metric_round_trips_through_parse() {
        for metric in Metric::ALL {
            assert_eq!(Metric::parse(metric.as_str()), Some(*metric), "{metric:?}");
        }
    }

    /// The idea sweep wrote the names a reader of AO3 would type, and this row's
    /// requirement quotes them verbatim. `kudos` is the imported count under the
    /// archive's own word for it; `completion` is the stored status; and a typo is
    /// still refused rather than quietly reordered.
    #[test]
    fn the_kudos_and_completion_spellings_reach_their_metric() {
        assert_eq!(Metric::parse("kudos"), Some(Metric::Likes));
        assert_eq!(
            Metric::parse("kudos_per_1k_views"),
            Some(Metric::LikesPerView)
        );
        assert_eq!(Metric::parse("completion"), Some(Metric::Completion));
        assert_eq!(Metric::parse("kudos_per_hour"), Some(Metric::KudosPerHour));
        assert_eq!(
            Metric::parse("updater_reliability"),
            Some(Metric::UpdaterReliability)
        );
        assert_eq!(
            Metric::parse("freshness_weighted_quality"),
            Some(Metric::FreshnessWeightedQuality)
        );
        assert_eq!(
            Metric::parse("kudos2"),
            None,
            "a typo is refused, not folded onto a neighbour"
        );
        // A refusal quotes the word the caller typed; the canonical name follows
        // in parentheses only when it is a different word.
        assert_eq!(
            Metric::LikesPerView.refusal_name("kudos_per_1k_views"),
            "kudos_per_1k_views (= likes_per_view)"
        );
        assert_eq!(
            Metric::KudosPerHour.refusal_name("kudos_per_hour"),
            "kudos_per_hour"
        );
    }

    /// Freshness is the whole point of the metric, so a row with no source date
    /// scores zero rather than being renormalised back up to its quality — the
    /// same decision `recency` already made, asserted here so the two cannot drift
    /// apart.
    #[test]
    fn a_freshness_weighted_quality_prefers_the_fresher_of_two_equal_rows() {
        let mut fresh = candidate("fresh", "sb", 500, Some(60_000));
        fresh.freshness_days = Some(0.0);
        let mut stale = candidate("stale", "sb", 500, Some(60_000));
        stale.freshness_days = Some(180.0);
        let mut undated = candidate("undated", "sb", 500, Some(60_000));
        undated.freshness_days = None;

        let ranked = rank(
            Metric::FreshnessWeightedQuality,
            &[fresh, stale, undated],
            5,
            0,
        );
        let score = |id: &str| {
            ranked
                .hits
                .iter()
                .find(|h| h.id == id)
                .unwrap_or_else(|| panic!("{id} ranked"))
                .score
        };
        // 180 days is one half-life by construction, so the ratio is exactly 1/e.
        assert!(
            (score("fresh") - score("stale") * std::f64::consts::E).abs() < 1e-9,
            "one half-life apart: fresh {} vs stale {}",
            score("fresh"),
            score("stale")
        );
        assert_eq!(score("undated"), 0.0, "no date, no freshness to weight by");
        assert!(score("fresh") > 0.0, "a measured row still has a quality");
    }

    #[test]
    fn an_unknown_metric_is_refused_rather_than_silently_reordered() {
        // `kudos` is a name now; its typo is the one that must still be refused.
        assert_eq!(Metric::parse("kudos2"), None);
        assert_eq!(Metric::parse("likes desc; drop table"), None);
    }

    #[test]
    fn a_blocked_metric_names_the_work_that_unblocks_it() {
        for metric in [
            Metric::Velocity,
            Metric::HiddenGem,
            Metric::KudosPerHour,
            Metric::UpdaterReliability,
        ] {
            match metric.support() {
                Support::Blocked(reason) => assert!(reason.contains("M63-32"), "{reason}"),
                other => panic!("{metric:?} should be blocked, was {other:?}"),
            }
        }
        // `likes_per_view` and `wilson` are no longer blocked by the data model: M63-37 records per-archive views, so the metric is
        // computable. What can still stop them is a *population* where no row carries a view count, and that is decided against the
        // loaded rows (the store's `views_rows`), not guessed here where no rows are in scope.
        for metric in [Metric::LikesPerView, Metric::Wilson] {
            assert!(metric.needs_views(), "{metric:?} ranks on a view count");
            assert!(
                matches!(metric.support(), Support::Ranked),
                "{metric:?} is computable: {:?}",
                metric.support()
            );
        }
        assert!(!Metric::Likes.needs_views(), "likes need no denominator");
    }

    #[test]
    fn paging_is_stable_across_pages_because_ties_break_on_the_id() {
        let candidates: Vec<Candidate> = (0..10)
            .map(|i| candidate(&format!("id{i:02}"), "sb", 100, Some(50_000)))
            .collect();
        let first = rank(Metric::SourcePct, &candidates, 5, 0);
        let second = rank(Metric::SourcePct, &candidates, 5, 5);
        assert_eq!(first.hits.len(), 5);
        assert_eq!(second.hits.len(), 5);
        let ids: Vec<&str> = first
            .hits
            .iter()
            .chain(second.hits.iter())
            .map(|h| h.id.as_str())
            .collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(
            ids, sorted,
            "a second page must continue the first, not repeat it"
        );
    }

    #[test]
    fn recency_decays_and_absent_dates_are_absent_rather_than_old() {
        let mut fresh = candidate("fresh", "sb", 10, Some(20_000));
        fresh.freshness_days = Some(0.0);
        let mut undated = candidate("undated", "sb", 10, Some(20_000));
        undated.freshness_days = None;
        let ranked = rank(Metric::Recency, &[fresh, undated], 5, 0);
        let fresh = ranked.hits.iter().find(|h| h.id == "fresh").unwrap();
        assert!((fresh.score - 1.0).abs() < 1e-9);
        // Both score > 0 because ties are broken on the id; the undated row is 0.
        assert!(ranked
            .hits
            .iter()
            .all(|h| h.id != "undated" || h.score == 0.0));
    }
}
