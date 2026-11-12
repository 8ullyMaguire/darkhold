//! `dh bench` — the twenty canonical queries, timed against one instance (M63-24).
//!
//! **What a benchmark can and cannot say.** These are twenty searches whose *shape* is stable — a single tag, a
//! numeric floor, a conjunction, a negation, a recency window — so a change to an index, a query planner or a
//! strategy shows up as a number rather than as a feeling. What it cannot say is whether an instance is "fast": there
//! is no threshold here, because the one that matters belongs to the machine and the catalogue size. The command
//! prints what it measured and leaves the verdict to whoever knows what the box was doing at the time.
//!
//! **Client-side milliseconds, deliberately.** The number includes the network when the target is remote, which is why
//! `--base-url` is printed in the output and in the JSON: a table without its subject is not a measurement. Where the
//! server exposes its own timing (the `explain=1` plan) this command does not use it — a bench that reports the
//! server's opinion of its own speed is measuring the wrong thing.

use std::time::Instant;

use clap::Args;
use serde_json::{json, Value};

use crate::rec::{bad_request, environment, Failure};
use crate::Cli;

/// The twenty shapes worth timing, and why each is here rather than in a list of arbitrary examples.
///
/// Every expression is one the site's own search box accepts, so a broken one is a bench that fails loudly instead of
/// quietly timing a 422. They are ordered cheapest-first: a warm-up pass runs the list once before anything is
/// recorded, so the first query is not paying for a cold connection it will never pay for again.
pub const CANONICAL: [&str; 20] = [
    // The unselective baseline: everything, at the default order. If this is slow, nothing below means anything.
    "*",
    // One tag, the most common shape a reader types.
    "tag:naruto",
    // A number floor, which is the shape an index either serves or does not.
    "kudos:>=100",
    "kudos:>=1000",
    "bookmarks:>=500",
    "hits:>=100000",
    // Two tags, conjunction: the shape that decides whether the tag index is composed or scanned.
    "tag:naruto AND kudos:>=100",
    "tag:angst AND tag:fluff",
    // Negation, which no index answers directly.
    "tag:au NOT tag:crossover",
    // A phrase, where the parser has to keep two tokens together. Quoted, because that is the form the grammar
    // documents for a tag with a space in it; unquoted it happens to parse as one token and would stop doing so the
    // day the tag rules change (both were tried against a live instance, 2026-11-12).
    "tag:\"time travel\"",
    "tag:fix-it",
    // Recency windows: the two directions of the same range, because one is a tail scan and the other is not.
    "updated:>=7d",
    "updated:<30d",
    // Size and shape, where the catalogue's own distribution decides the cost.
    "words:>=50000",
    "words:<5000",
    "chapters:>=100",
    // Engagement aggregates, the pair most likely to be denormalised in one place and computed in another.
    "comments:>=100",
    "complete:true",
    // A long conjunction: the shape whose cost grows with the number of terms.
    "tag:naruto AND tag:angst AND kudos:>=100",
    // The widest conjunction in the list, and the one a planning regression hits first.
    "tag:\"harry potter\" AND tag:angst AND tag:complete AND kudos:>=500",
];

#[derive(Args, Debug, Clone)]
pub struct BenchArgs {
    /// Rounds per query. Each round is one request per query, so the request count is rounds x 20.
    #[arg(long, default_value_t = 5, value_name = "N")]
    pub rounds: u32,

    /// Rows asked for per query. The server's own bound is 1..=100.
    #[arg(long, default_value_t = 20, value_name = "N")]
    pub limit: i64,
}

/// The timings for one query: every round's elapsed milliseconds, in the order they were taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub expression: &'static str,
    pub millis: Vec<u64>,
}

impl Sample {
    /// The nearest-rank percentile over the rounds taken.
    ///
    /// Nearest-rank rather than an interpolation: with five rounds an interpolated p95 is a number between two
    /// measurements that were never observed, and printing a measurement nobody made is how a bench becomes a
    /// decoration. The index is clamped so p95 of one round is that round.
    pub fn percentile(&self, p: u32) -> u64 {
        let mut sorted = self.millis.clone();
        sorted.sort_unstable();
        if sorted.is_empty() {
            return 0;
        }
        let rank = ((p as usize * sorted.len()).div_ceil(100)).max(1);
        sorted[rank.min(sorted.len()) - 1]
    }

    /// The fastest round. Named `fastest` rather than `min` so it is a fact about this sample and not an
    /// `Iterator` method the type does not have (`clippy::should_implement_trait` is right to ask).
    pub fn fastest(&self) -> u64 {
        self.millis.iter().copied().min().unwrap_or(0)
    }
}

/// The table, in the order the queries were run.
///
/// Fixed-width columns rather than a real table library: this is a terminal command with no dependency budget, and
/// the expressions are known at compile time, so the widest one is the column width.
pub fn table(base_url: &str, limit: i64, samples: &[Sample]) -> String {
    let width = samples
        .iter()
        .map(|s| s.expression.chars().count())
        .max()
        .unwrap_or(0)
        .max("query".len());
    let mut out = format!(
        "{:<width$}  {:>6}  {:>6}  {:>6}\n",
        "query", "min", "p50", "p95"
    );
    out.push_str(&format!(
        "{:<width$}  {:>6}  {:>6}  {:>6}\n",
        "-".repeat(width),
        "-".repeat(6),
        "-".repeat(6),
        "-".repeat(6)
    ));
    for sample in samples {
        out.push_str(&format!(
            "{:<width$}  {:>6}  {:>6}  {:>6}\n",
            sample.expression,
            sample.fastest(),
            sample.percentile(50),
            sample.percentile(95)
        ));
    }
    // The footer carries the subject and the load, because a number without them is not a measurement: the same
    // twenty queries against a warm laptop and a busy box are different facts.
    out.push_str(&format!(
        "\n{} queries x {} rounds = {} requests, limit {limit}, against {base_url}\n",
        samples.len(),
        samples.first().map(|s| s.millis.len()).unwrap_or(0),
        samples.iter().map(|s| s.millis.len()).sum::<usize>(),
    ));
    out
}

/// The same measurements as JSON, for a script that wants to track them over time rather than read them once.
pub fn document(base_url: &str, limit: i64, samples: &[Sample]) -> Value {
    json!({
        "base_url": base_url,
        "limit": limit,
        "queries": samples.iter().map(|s| json!({
            "expression": s.expression,
            "rounds": s.millis.len(),
            "min_ms": s.fastest(),
            "p50_ms": s.percentile(50),
            "p95_ms": s.percentile(95),
            "millis": s.millis,
        })).collect::<Vec<_>>(),
    })
}

/// Both refusals happen before the first request, so a mistyped bench costs no load on the instance it was aimed at.
pub fn validate(args: &BenchArgs) -> Result<(), Failure> {
    if !(1..=200).contains(&args.rounds) {
        return Err(bad_request(
            "rounds must be between 1 and 200: a bench of zero rounds measures nothing, and one of ten thousand \
             is a load test of the instance rather than a measurement of a query",
        ));
    }
    if !(1..=100).contains(&args.limit) {
        return Err(bad_request(
            "limit must be between 1 and 100, which is the server's own bound on rows per search",
        ));
    }
    Ok(())
}

pub async fn run(cli: &Cli, args: &BenchArgs) -> Result<(), Failure> {
    validate(args)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| environment(format!("http client: {e}")))?;
    let token = cli.token.clone();

    // One request, timed, through the same send path every other command uses so the numbers include the retries and
    // the redirects a reader would also pay for.
    let once = |expression: &'static str| {
        let client = client.clone();
        let token = token.clone();
        let base_url = cli.base_url.clone();
        async move {
            let url = crate::q::url(&base_url, expression, args.limit, false);
            let started = Instant::now();
            let (status, _) = crate::http::send(
                || {
                    let mut request = client.get(&url);
                    if let Some(token) = &token {
                        request = request.bearer_auth(token);
                    }
                    request
                },
                &base_url,
            )
            .await?;
            let elapsed = started.elapsed().as_millis() as u64;
            if !(200..300).contains(&status) {
                // A non-2xx here is the bench's own input being wrong, not the instance being slow, and reporting it as
                // a timing would hide a 422 behind a respectable-looking number.
                return Err(environment(format!(
                    "{expression} answered {status} — a canonical query the instance rejects is a bench that is \
                     measuring the parser, not the index"
                )));
            }
            Ok::<u64, Failure>(elapsed)
        }
    };

    // The warm-up: every query once, unrecorded. Without it the first measurement of the first query carries the
    // connection setup and the numbers below are not comparable with each other.
    for expression in CANONICAL {
        once(expression).await?;
    }

    let mut samples = Vec::with_capacity(CANONICAL.len());
    for expression in CANONICAL {
        let mut millis = Vec::with_capacity(args.rounds as usize);
        for _ in 0..args.rounds {
            millis.push(once(expression).await?);
        }
        samples.push(Sample { expression, millis });
    }

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&document(&cli.base_url, args.limit, &samples))
                .unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        print!("{}", table(&cli.base_url, args.limit, &samples));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(millis: &[u64]) -> Sample {
        Sample {
            expression: "tag:naruto",
            millis: millis.to_vec(),
        }
    }

    #[test]
    fn the_canonical_list_is_twenty_distinct_queries() {
        // A duplicated entry is a bench that reports the same measurement twice and calls it two shapes.
        assert_eq!(CANONICAL.len(), 20);
        let mut seen = std::collections::HashSet::new();
        for expression in CANONICAL {
            assert!(
                !expression.trim().is_empty(),
                "an empty query is a 422, not a measurement"
            );
            assert!(seen.insert(expression), "{expression} appears twice");
        }
    }

    #[test]
    fn percentiles_are_nearest_rank_over_the_rounds_actually_taken() {
        let one = sample(&[7]);
        assert_eq!(one.percentile(50), 7);
        assert_eq!(one.percentile(95), 7);

        // Five rounds of 10..=50: p50 is the third smallest (30), p95 is the largest (50). An interpolated p95 would
        // print a number that was never measured.
        let five = sample(&[50, 10, 40, 20, 30]);
        assert_eq!(five.fastest(), 10);
        assert_eq!(five.percentile(50), 30);
        assert_eq!(five.percentile(95), 50);

        // An empty sample is not a zero-latency query; it is no measurement at all, and callers must not see 0 as one.
        assert_eq!(sample(&[]).percentile(50), 0);
    }

    #[test]
    fn the_table_carries_its_subject_and_its_load() {
        let rendered = table("https://example.test", 20, &[sample(&[12, 14, 19])]);
        assert!(rendered.starts_with("query"), "{rendered}");
        assert!(rendered.contains("tag:naruto"), "{rendered}");
        // The footer is what stops a screenshot of this table from being an unfalsifiable claim about "the site".
        assert!(
            rendered.contains("1 queries x 3 rounds = 3 requests, limit 20"),
            "{rendered}"
        );
        assert!(rendered.contains("https://example.test"), "{rendered}");
    }

    #[test]
    fn a_bench_of_zero_rounds_and_a_limit_the_server_would_refuse_are_both_refused_here() {
        let zero = BenchArgs {
            rounds: 0,
            limit: 20,
        };
        assert!(validate(&zero).is_err());

        let wide = BenchArgs {
            rounds: 5,
            limit: 500,
        };
        let error = validate(&wide).expect_err("limit 500 is outside the server's bound");
        assert!(
            error.message.contains("between 1 and 100"),
            "{}",
            error.message
        );

        let ok = BenchArgs {
            rounds: 1,
            limit: 100,
        };
        assert!(validate(&ok).is_ok());
    }
}
