//! `dh rec` — the anchor verb.
//!
//! Two server routes, chosen by whether an engine was asked for:
//!
//! | invocation | route | why |
//! |---|---|---|
//! | `dh rec --fandom naruto --updated 7d` | `/api/v1/catalogue` | public, metric order, no token |
//! | `dh rec … --engine rrf` | `/api/v1/rec/run` | the engines read your history, so they need a session |
//!
//! The split is deliberate rather than convenient: the catalogue is a public
//! surface and a user who only wants "top Naruto fics by likes" should not need a
//! token to ask. The moment an engine is named, the request is about *this*
//! account's recommendations, and the server refuses it without one.

use anyhow::Result;
use clap::Args;
use serde_json::Value;

use crate::Cli;

/// Options for `rec`.
#[derive(Debug, Clone, Args)]
pub struct RecArgs {
    /// Restrict to one fandom, spelled as the catalogue spells it.
    #[arg(long, value_name = "NAME")]
    pub fandom: Option<String>,

    /// Window on when the author last touched it: `7d`, `>30d`, `@2026-10-27`,
    /// `2026-01-01..2026-10-15`.
    #[arg(long, value_name = "WINDOW")]
    pub updated: Option<String>,

    /// Window on when the crawler last rechecked it.
    #[arg(long, value_name = "WINDOW")]
    pub refreshed: Option<String>,

    /// Window on when it entered this catalogue.
    #[arg(long, value_name = "WINDOW")]
    pub added: Option<String>,

    /// Restrict to one source: `sb`, `sv`, `qq`, `ao3`, `royalroad`, …
    #[arg(long, value_name = "KEY")]
    pub source: Option<String>,

    /// Only rows with at least this many likes.
    #[arg(long, value_name = "N")]
    pub min_likes: Option<i64>,

    /// One JSON object per row, for a pipeline: keys never disappear, order is
    /// fixed, nothing wraps. Conflicting shapes are refused rather than merged.
    #[arg(long)]
    pub jsonl: bool,

    /// With `--jsonl`: print only these fields, in this order.
    /// `dh rec --jsonl --fields title,likes | jq -r '.<title>'`.
    #[arg(long, value_name = "FIELDS")]
    pub fields: Option<String>,

    /// The table. This is the default; naming it is how a script says so.
    #[arg(long)]
    pub table: bool,

    /// With the table: print only these columns, in this order.
    #[arg(long, value_name = "COLS")]
    pub cols: Option<String>,

    /// Do not truncate long titles and authors.
    #[arg(long)]
    pub wide: bool,

    /// One link per row instead of a table: `md` (a markdown list) or `url`
    /// (bare URLs). `epub` is refused until this instance has a download route.
    #[arg(long, value_name = "KIND")]
    pub link: Option<String>,

    /// Substring over title and author.
    #[arg(long, value_name = "TEXT")]
    pub q: Option<String>,

    /// The metric that orders the page. `--list-metrics` prints every name with
    /// whether it can be computed today.
    #[arg(long, value_name = "METRIC")]
    pub sort: Option<String>,

    /// Print the metric vocabulary and exit. No server is contacted.
    #[arg(long)]
    pub list_metrics: bool,

    /// Order the candidates with an engine: `catalogue`, `legacy`, `rrf`, or a
    /// registered strategy name. Needs `--token`.
    #[arg(long, value_name = "ENGINE")]
    pub engine: Option<String>,

    /// Print what each engine contributed. Implies `--engine rrf` when no engine
    /// was named, because there is nothing to explain without one.
    #[arg(long)]
    pub explain: bool,

    /// How many rows to print.
    #[arg(long, value_name = "N", default_value_t = 20)]
    pub limit: i64,

    /// Similar to this work: its id, its source URL, or its title.
    #[arg(long, value_name = "WORK")]
    pub seed: Option<String>,

    /// Restrict to one author's pseud: exact, case-insensitive.
    #[arg(long, value_name = "PSEUD")]
    pub author: Option<String>,

    /// Drop what you are already reading. Reads your history, so it needs a token.
    #[arg(long)]
    pub exclude_read: bool,

    /// Store this query as a saved view on the server. Needs a token.
    ///
    /// What is stored is the query string this call would have sent, verbatim,
    /// so running it later asks the catalogue exactly what it asks now.
    #[arg(long, value_name = "NAME")]
    pub save: Option<String>,

    /// Run a saved view by name, replaying the parameters it stored.
    #[arg(long, value_name = "NAME")]
    pub saved: Option<String>,

    /// With `--saved`: print only the rows that were not on this query's last run.
    #[arg(long)]
    pub digest_diff: bool,

    /// List saved views and exit. Needs a token.
    #[arg(long)]
    pub queries: bool,
}

/// What a failed run must tell the shell.
///
/// The exit codes are the contract a script depends on: `2` means the request
/// was wrong (bad fandom, unknown metric, missing token) and retrying it
/// unchanged cannot help; `1` means something environmental went wrong and it
/// might.
#[derive(Debug)]
pub struct Failure {
    pub message: String,
    pub code: u8,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

pub(crate) fn bad_request(message: impl Into<String>) -> Failure {
    Failure {
        message: message.into(),
        code: 2,
    }
}

/// Refuse a `--sort` word that is not in the vocabulary, before any request.
fn validate_sort(raw: &str) -> Result<(), Failure> {
    match darkhold_domain::quality::Metric::parse(raw) {
        Some(_) => Ok(()),
        None => Err(bad_request(format!(
            "--sort {raw}: unknown metric. `dh rec --list-metrics` prints the vocabulary"
        ))),
    }
}

/// A local refusal that means the same thing an HTTP `401` would: supply a session.
pub(crate) fn auth_required(message: impl Into<String>) -> Failure {
    Failure {
        message: message.into(),
        code: crate::exit::AUTH,
    }
}

pub(crate) fn environment(message: impl Into<String>) -> Failure {
    Failure {
        message: message.into(),
        code: 1,
    }
}

/// Which route this invocation talks to.
///
/// `explain` implies an engine: there is nothing to explain about a plain metric
/// ordering, and answering "no strategies ran" to a `--explain` would be a worse
/// experience than running the default engine and showing it.
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    Catalogue,
    Engine(String),
}

impl Target {
    pub fn resolve(args: &RecArgs, configured: &str) -> Self {
        // A seed is an order of its own, and it is answered by the catalogue
        // route: `--seed X --explain` explains the neighbours (the shared
        // tokens), not an engine's contribution.
        if args.seed.is_some() {
            return Self::Catalogue;
        }
        match (args.engine.as_deref(), args.explain) {
            (Some(engine), _) => Self::Engine(engine.to_owned()),
            (None, true) => Self::Engine(configured.to_owned()),
            (None, false) => Self::Catalogue,
        }
    }
}

/// Percent-encode a query value.
///
/// Written out rather than taking a dependency: the five values this program
/// sends are a name, a window, a metric, a source key and free text, and an
/// encoder that mishandles `&` in a title search is the only thing that could go
/// wrong with it.
pub(crate) fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Build the request URL: the route, then every parameter that was given.
pub fn url(base_url: &str, target: &Target, args: &RecArgs) -> String {
    let path = path_for(target);
    let mut params: Vec<(String, String)> = Vec::new();
    let mut push = |key: &str, value: Option<&str>| {
        if let Some(value) = value.filter(|v| !v.is_empty()) {
            params.push((key.to_owned(), value.to_owned()));
        }
    };
    push("fandom", args.fandom.as_deref());
    push("updated", args.updated.as_deref());
    push("refreshed", args.refreshed.as_deref());
    push("added", args.added.as_deref());
    push("source", args.source.as_deref());
    push("q", args.q.as_deref());
    push("sort", args.sort.as_deref());
    push("seed", args.seed.as_deref());
    push("author", args.author.as_deref());
    if args.exclude_read {
        params.push(("exclude_read".to_owned(), "1".to_owned()));
    }
    if let Some(min_likes) = args.min_likes {
        params.push(("min_likes".to_owned(), min_likes.to_string()));
    }
    params.push(("limit".to_owned(), args.limit.to_string()));
    if let Target::Engine(engine) = target {
        params.push(("engine".to_owned(), engine.clone()));
        if args.explain {
            params.push(("explain".to_owned(), "1".to_owned()));
        }
    }
    let query = params
        .iter()
        .map(|(key, value)| format!("{key}={}", encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{}{path}?{query}", base_url.trim_end_matches('/'))
}

/// A catalogue row, as this program prints it.
///
/// Only the keys listed here are read, and that is the point: the server can add
/// a field to its page without moving a column under a script's nose.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub title: String,
    pub author: String,
    pub source: String,
    pub likes: i64,
    pub words: Option<i64>,
    pub status: String,
    pub updated: String,
    /// A quality metric's score, when the page carries one.
    pub score: Option<f64>,
    /// False when the metric could not measure the row (`measured: false`).
    pub measured: bool,
    /// The tokens that carried a seed search's similarity, when the page is a
    /// seed page. Empty for every other kind of page.
    pub shared: Vec<String>,
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn number(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

/// Read the rows out of either route's page shape.
///
/// Both routes put the catalogue row's fields at the top level of each item —
/// `/rec/run` flattens the row and adds `rec_score`/`rec_rank` beside it — so one
/// reader serves both, and a change that un-flattened one of them would fail the
/// render tests rather than silently printing blanks.
pub fn rows(page: &Value) -> Vec<Row> {
    page.get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| Row {
                    title: text(item, "title"),
                    author: {
                        // `/rec/run` and `/catalogue` call it `author_text`; a search hit calls it
                        // `author_handle`. One reader for every page shape (its doc says so), so
                        // the fallback lives here rather than in a second reader in `q.rs` that
                        // would drift the day one of the two fields is renamed.
                        let named = text(item, "author_text");
                        match named.is_empty() {
                            true => text(item, "author_handle"),
                            false => named,
                        }
                    },
                    source: text(item, "source_key"),
                    likes: number(item, "likes").unwrap_or(0),
                    words: number(item, "word_count"),
                    status: text(item, "status"),
                    updated: {
                        // The stored shape is `YYYY-MM-DDTHH:MM:SSZ`; the date is
                        // the part a reader scans. A metric page carries the
                        // source date as `updated_at`, because the catalogue row
                        // itself does not hold it. A row with no date prints as
                        // empty rather than as "1970".
                        let raw = match text(item, "source_updated_at") {
                            empty if empty.is_empty() => text(item, "updated_at"),
                            found => found,
                        };
                        raw.split('T').next().unwrap_or("").to_owned()
                    },
                    score: item.get("score").and_then(Value::as_f64),
                    measured: item
                        .get("measured")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    shared: item
                        .get("shared")
                        .and_then(Value::as_array)
                        .map(|tokens| {
                            tokens
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `21.0k`, `1.2M`, or the plain number.
pub fn compact(value: i64) -> String {
    match value {
        v if v >= 1_000_000 => format!("{:.1}M", v as f64 / 1_000_000.0),
        v if v >= 1_000 => format!("{:.1}k", v as f64 / 1_000.0),
        v => v.to_string(),
    }
}

/// Pad to `width` columns, counting characters rather than bytes.
fn pad(value: &str, width: usize) -> String {
    let len = value.chars().count();
    match len >= width {
        // Truncation is explicit: a title that silently overflows pushes every
        // column right and makes the table unreadable in a narrow terminal.
        true => format!(
            "{}…",
            value
                .chars()
                .take(width.saturating_sub(1))
                .collect::<String>()
        ),
        false => format!("{value}{}", " ".repeat(width - len)),
    }
}

/// The table.
///
/// The score column appears only on a metric-ranked page: a column of `—` would
/// suggest every row was measured, which is the one thing a metric page must not
/// imply. Unmeasured rows are marked with `~` for the same reason.
pub fn table(page: &Value) -> String {
    let rows = rows(page);
    let scored = rows.iter().any(|row| row.score.is_some());
    let mut out = String::new();
    for (index, row) in rows.iter().enumerate() {
        let marker = if row.measured { ' ' } else { '~' };
        if scored {
            out.push_str(&format!(
                "{:>3}{} {:>6}  {:>6} likes  {:>9} words  {} {}  {} by {} [{}]\n",
                index + 1,
                marker,
                row.score
                    .map(|score| format!("{score:.3}"))
                    .unwrap_or_else(|| "—".to_owned()),
                compact(row.likes),
                row.words.map(compact).unwrap_or_else(|| "—".to_owned()),
                pad(&row.status, 8),
                row.updated,
                pad(&row.title, 44),
                pad(&row.author, 18),
                row.source,
            ));
            if !row.shared.is_empty() {
                out.push_str(&format!("     shared: {}\n", row.shared.join(", ")));
            }
        } else {
            out.push_str(&format!(
                "{:>3}  {:>6} likes  {:>9} words  {} {}  {} by {} [{}]\n",
                index + 1,
                compact(row.likes),
                row.words.map(compact).unwrap_or_else(|| "—".to_owned()),
                pad(&row.status, 8),
                row.updated,
                pad(&row.title, 44),
                pad(&row.author, 18),
                row.source,
            ));
        }
    }
    out
}

/// The gate line for a metric page: how many rows the metric could actually
/// score, and how many matched.
///
/// Without it a percentile page reads as a verdict on the whole catalogue, when
/// on the live catalogue it is a verdict on 0.9 % of it.
pub fn gate(page: &Value) -> Option<String> {
    let ranked = page.get("ranked").and_then(Value::as_i64)?;
    let total = page.get("total").and_then(Value::as_i64)?;
    let metric = page
        .get("metric")
        .and_then(Value::as_str)
        .unwrap_or("metric");
    // A seed page has three numbers, not two, and the third is the one that stops
    // it reading as a claim about the catalogue: `scanned` is what the ranker saw.
    if metric == "similarity" {
        let scanned = page
            .get("scanned")
            .and_then(Value::as_i64)
            .unwrap_or(ranked);
        let seed = page
            .get("seed")
            .and_then(|seed| seed.get("title"))
            .and_then(Value::as_str)
            .unwrap_or("?");
        return Some(format!(
            "similar to \"{seed}\": {ranked} neighbours among the {scanned} rows scanned, \
             of {total} matching the filters"
        ));
    }
    let share = match total {
        0 => 0.0,
        total => ranked as f64 * 100.0 / total as f64,
    };
    let mut line = format!(
        "{metric}: ranked {ranked} of {total} matching rows ({share:.1}% carry an endorsement count)"
    );
    if ranked < total {
        line.push_str("; the rest are ordered by metadata and marked ~");
    }
    Some(line)
}

/// The metric vocabulary, as the CLI prints it.
pub fn metrics_list() -> String {
    use darkhold_domain::quality::{Metric, Support};
    // The name column is measured, not fixed at twenty: `freshness_weighted_quality`
    // is twenty-six characters, and a list that prints `freshness_weighted_…` cannot
    // be copied into `--sort`, which is the only thing this list is for.
    let width = Metric::ALL
        .iter()
        .map(|metric| {
            metric.as_str().chars().count()
                + usize::from(matches!(metric.support(), Support::Blocked(_)))
        })
        .max()
        .unwrap_or(20)
        .max(20)
        + 2;

    let mut out = String::from("QUALITY METRICS — dh rec --sort <name>\n\n");
    for metric in Metric::ALL {
        let (status, detail) = match metric.support() {
            Support::Ranked => ("ready", metric.help().to_owned()),
            Support::Triage => ("triage", metric.help().to_owned()),
            Support::Raw => ("raw", metric.help().to_owned()),
            Support::Blocked(reason) => ("blocked", format!("{} — {reason}", metric.help())),
        };
        let name = match metric.support() {
            Support::Blocked(_) => format!("{}*", metric.as_str()),
            _ => metric.as_str().to_owned(),
        };
        out.push_str(&format!(
            "  {} {} {}\n",
            pad(&name, width),
            pad(status, 8),
            detail
        ));
    }
    out.push_str("\nraw = a stored column ordered in SQL. triage = metadata order for rows with no endorsement data.\n");
    out.push_str("* = refused with the reason and the work item that unblocks it.\n");
    out.push_str(concat!(
        "aliases: kudos = likes, kudos_per_1k_views = likes_per_view, ",
        "status = completion, fresh_quality = freshness_weighted_quality\n"
    ));
    out
}

/// The coverage line, when the page carries one.
///
/// Printed whenever a fandom was filtered on, because the honest answer to
/// "Naruto fics" is "265 tagged ones": 94.6 % of the live catalogue has no fandom
/// metadata, and a reader who is not told reads a short page as a bad ranking.
pub fn coverage(page: &Value) -> Option<String> {
    let tagged = page.get("fandom_tagged_rows").and_then(Value::as_i64)?;
    let total = page.get("catalogue_rows").and_then(Value::as_i64)?;
    let fandom = page.get("fandom").and_then(Value::as_str)?;
    let share = match total {
        0 => 0.0,
        total => tagged as f64 * 100.0 / total as f64,
    };
    let rankable = page
        .get("fandom_tagged_with_likes")
        .and_then(Value::as_i64)
        .unwrap_or(tagged);
    let ranking = match rankable {
        0 => " — and none of them publishes a like count, so this order is ties only".to_owned(),
        n if n < tagged => format!(", {n} of them with a like count"),
        _ => String::new(),
    };
    Some(format!(
        "{fandom}: {tagged} of {total} catalogue rows carry a fandom tag ({share:.1}%){ranking}"
    ))
}

/// The explanation block, when strategies were reported.
pub fn explanation(page: &Value) -> Option<String> {
    let strategies = page.get("strategies").and_then(Value::as_array)?;
    if strategies.is_empty() {
        return None;
    }
    let engine = page.get("engine").and_then(Value::as_str).unwrap_or("?");
    let candidates = page.get("candidates").and_then(Value::as_i64).unwrap_or(0);
    let mut out = format!("engine {engine} over {candidates} candidates\n\n");
    out.push_str(&format!(
        "{}  {:>8}  {:>7}  head\n",
        pad("strategy", 18),
        "produced",
        "matched"
    ));
    for strategy in strategies {
        let name = strategy.get("name").and_then(Value::as_str).unwrap_or("?");
        let produced = strategy
            .get("produced")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let matched = strategy.get("matched").and_then(Value::as_u64).unwrap_or(0);
        let head = strategy
            .get("top")
            .and_then(Value::as_array)
            .map(|top| {
                top.iter()
                    .filter_map(|pair| pair.get("score").and_then(Value::as_f64))
                    .map(|score| format!("{score:.4}"))
                    .collect::<Vec<_>>()
                    .join("  ")
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "{}  {:>8}  {:>7}  {head}\n",
            pad(name, 18),
            produced,
            matched
        ));
    }
    Some(out)
}

/// Turn the server's refusal into something a person can act on.
///
/// Delegates to [`crate::http::refusal`] (`M63-19`): the sentence is the same for every verb, so
/// `dh rec` cannot tell a script one story about a `403` and `dh supply` another.
pub fn failure_for(status: u16, body: &Value, base_url: &str) -> Failure {
    crate::http::refusal(status, body, base_url)
}

/// Decide the output shape from the flags, refusing combinations rather than
/// picking a winner.
///
/// A program that silently prefers one of two contradicting flags is a program
/// whose output depends on argument order — and the failure it produces is a
/// script that reads a table where it asked for JSON.
fn shape(args: &RecArgs, json: bool) -> Result<crate::output::Shape, Failure> {
    use crate::output::{parse_projection, Column, JSON_COLUMNS, TABLE_COLUMNS};
    let mut shape = crate::output::Shape::default();

    if json && args.jsonl {
        return Err(bad_request(
            "--json prints the whole page; --jsonl prints one row per line. Pick one.",
        ));
    }
    if json && (args.fields.is_some() || args.link.is_some() || args.cols.is_some()) {
        return Err(bad_request(
            "--json prints the server's page unchanged, so a projection on it means nothing",
        ));
    }
    if json {
        return Ok(shape);
    }
    if args.jsonl && args.table {
        return Err(bad_request(
            "--jsonl and --table print different shapes; pick one",
        ));
    }
    if args.jsonl && args.link.is_some() {
        return Err(bad_request(
            "--jsonl and --link print different shapes; pick one",
        ));
    }
    if args.link.is_some() && args.cols.is_some() {
        return Err(bad_request(
            "--link prints links; --cols chooses table columns. Pick one.",
        ));
    }
    if args.fields.is_some() && !args.jsonl {
        return Err(bad_request(
            "--fields selects JSON fields; add --jsonl. The table's projection is --cols.",
        ));
    }

    shape.wide = args.wide;
    shape.jsonl = args.jsonl;
    if let Some(raw) = args.fields.as_deref() {
        shape.fields = Some(
            parse_projection(raw, &JSON_COLUMNS)
                .map_err(|e| bad_request(format!("--fields: {e}")))?,
        );
    }
    if let Some(raw) = args.cols.as_deref() {
        shape.cols = Some(
            parse_projection(raw, &TABLE_COLUMNS)
                .map_err(|e| bad_request(format!("--cols: {e}")))?,
        );
        // `--cols score` on a page with no scores prints an empty table, and an
        // empty table reads as "nothing matched".
        let cols = shape.cols.as_deref().unwrap_or_default();
        if !cols.iter().any(|column| *column != Column::Score) {
            return Err(bad_request(
                "--cols needs at least one column beside `score`: a page with no scores would print nothing at all",
            ));
        }
    }
    if let Some(raw) = args.link.as_deref() {
        shape.link = Some(crate::output::LinkKind::parse(raw).map_err(bad_request)?);
    }
    Ok(shape)
}

/// Run `dh rec`.
pub async fn run(cli: &Cli, args: &RecArgs) -> Result<(), Failure> {
    if args.list_metrics {
        print!("{}", metrics_list());
        return Ok(());
    }
    if args.queries {
        if args.save.is_some() || args.saved.is_some() || args.digest_diff {
            return Err(bad_request(
                "--queries lists saved views; it does not also save, run or diff one",
            ));
        }
        return list_queries(cli).await;
    }
    // Answered before anything else, because each pair names two different verbs
    // and silently picking one would make the other a lie.
    if args.save.is_some() && args.saved.is_some() {
        return Err(bad_request(
            "--save stores a query and --saved runs one; pick one",
        ));
    }
    if args.digest_diff && args.saved.is_none() {
        return Err(bad_request(
            "--digest-diff needs --saved <name>: there is no saved query to diff against",
        ));
    }
    // A stored catalogue query and a personal ranking are different questions; a
    // `--digest-diff` across the two would compare an engine's answer against a
    // catalogue page's and call the difference "new".
    if args.saved.is_some() && (args.engine.is_some() || args.explain) {
        return Err(bad_request(
            "--saved replays a stored catalogue query; --engine ranks against your own history, so \
             the diff would compare two different answers",
        ));
    }
    if args.save.is_some() && args.digest_diff {
        return Err(bad_request(
            "--save stores a query without running it, so there is no page to diff",
        ));
    }
    // Refuse an unknown metric here, where the message can point at the
    // vocabulary, rather than letting the server answer `unknown sort` after a
    // round trip.
    if let Some(raw) = args.sort.as_deref() {
        validate_sort(raw)?;
    }
    if !(1..=100).contains(&args.limit) {
        return Err(bad_request("limit must be between 1 and 100"));
    }
    // These come before the engine check on purpose: `--seed X --engine rrf` is a
    // contradiction whatever the token situation is, and answering it with
    // "set DARKHOLD_TOKEN" sends the user to fix the wrong thing.
    if args.seed.is_some() && args.sort.is_some() {
        return Err(bad_request(
            "--seed already decides the order; --sort would be silently ignored",
        ));
    }
    if args.seed.is_some() && args.engine.is_some() {
        return Err(bad_request(
            "--seed orders by similarity and --engine orders by your history; pick one",
        ));
    }
    // An engine request without a token is refused here rather than by the
    // server, so the message can say what to do instead of quoting a 401 — and with the code the
    // server would have used (`M63-16`): a missing session is **4**, not `2`. The first version
    // returned the usage code, so a wrapper was told its command was wrong when the fix was to
    // supply a token. Refusing locally must not change what the number means.
    let engine = args.engine.clone();
    if engine.is_some() && cli.token.is_none() {
        return Err(auth_required(format!(
            "--engine {} reads your own reading history, so it needs a session: \
             set DARKHOLD_TOKEN",
            engine.unwrap_or_default()
        )));
    }
    if args.exclude_read && cli.token.is_none() {
        return Err(auth_required(
            "--exclude-read needs your reading history, so it needs a session: set DARKHOLD_TOKEN",
        ));
    }
    // `explain` without an engine means "the configured engine", and only the
    // server knows which that is; ask for rrf, which is what pluggable mode
    // serves, and let the server's refusal say so if it is disabled.
    let target = Target::resolve(args, "rrf");
    if let Some(name) = args.save.as_deref() {
        return save_query(cli, &target, args, name).await;
    }
    let saved = match args.saved.as_deref() {
        Some(name) => Some(lookup_view(cli, name).await?),
        None => None,
    };
    let url = match &saved {
        Some(view) => format!(
            "{}{}?{}",
            cli.base_url.trim_end_matches('/'),
            path_for(&target),
            view.params.as_deref().unwrap_or_default()
        ),
        None => url(&cli.base_url, &target, args),
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| environment(format!("http client: {e}")))?;
    // One request path for every verb (`crate::http`), so a rate limit is waited out the same way
    // here as in `dh supply`, and a `429` that outlasts the attempts is code 5 everywhere.
    let token = cli.token.clone();
    let (status, bytes) = crate::http::send(
        || {
            let mut request = client.get(&url);
            if let Some(token) = &token {
                request = request.bearer_auth(token);
            }
            request
        },
        &cli.base_url,
    )
    .await?;
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(failure_for(status, &body, &cli.base_url));
    }

    // The diff is asked of the server, which owns the query's memory: the client
    // only has to say which ids it just saw. A second machine running the same
    // name therefore gets the same answer (M63-06).
    let mut digest_note = None;
    let body = match (&saved, args.digest_diff) {
        (Some(view), true) => {
            let (filtered, note) = digest(cli, view, body).await?;
            digest_note = Some(note);
            filtered
        }
        _ => body,
    };

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string())
        );
        return Ok(());
    }

    let shape = shape(args, cli.json)?;
    match shape.link {
        Some(kind) => {
            // Links replace the table *and* the summary lines: a caller piping
            // this wants URLs, and a trailing "ranked 12 of 900" would become a
            // broken link on the next line.
            print!("{}", crate::output::links(&body, kind, &cli.base_url));
            return Ok(());
        }
        None if shape.jsonl => {
            print!("{}", crate::output::jsonl(&body, shape.fields.as_deref()));
            return Ok(());
        }
        None => {}
    }
    let cols = shape
        .cols
        .clone()
        .unwrap_or_else(|| crate::output::TABLE_COLUMNS.to_vec());
    let printed = match shape.cols.is_some() || shape.wide {
        true => crate::output::table(&body, &cols, shape.wide),
        // The default path keeps the historical renderer: a script that parses
        // the table today must keep parsing it after this feature landed.
        false => table(&body),
    };
    if printed.is_empty() {
        eprintln!("no rows matched");
    } else {
        print!("{printed}");
    }
    if let Some(line) = &digest_note {
        println!("\n{line}");
    }
    if let Some(line) = gate(&body) {
        println!("\n{line}");
    }
    if let Some(line) = coverage(&body) {
        println!("\n{line}");
    }
    if let Some(block) = explanation(&body) {
        println!("\n{block}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Saved queries (M63-06)
// ---------------------------------------------------------------------------

/// Where a target's parameters are sent — the one place that mapping lives.
fn path_for(target: &Target) -> &'static str {
    match target {
        Target::Catalogue => "/api/v1/catalogue",
        Target::Engine(_) => "/api/v1/rec/run",
    }
}

/// A saved view, as `GET /saved-views` reports it.
struct StoredView {
    id: String,
    name: String,
    kind: String,
    /// A catalogue view's canonical query string; `None` for a library view or a
    /// document this build cannot read.
    params: Option<String>,
    updated_at: String,
}

#[must_use]
fn stored_view(item: &Value) -> StoredView {
    let field = |key: &str| {
        item.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    StoredView {
        id: field("id"),
        name: field("name"),
        // A build that stored a document before `kind` existed means a library
        // view; guessing otherwise would offer a library filter as a query.
        kind: item
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("library")
            .to_owned(),
        params: item
            .get("params")
            .and_then(Value::as_str)
            .map(str::to_owned),
        updated_at: field("updated_at"),
    }
}

/// The session cookie, and the CSRF token carried beside it.
///
/// `/saved-views` is a **session** surface: `RequireSession` reads the cookie the
/// browser sends and never a bearer token (measured — a valid `DARKHOLD_TOKEN`
/// gets a 401 there). `dh module` learned this first and this follows it, down to
/// refusing with the variable that would work rather than the one that would not.
fn session_cookie(cli: &Cli, why: &str) -> Result<(String, Option<String>), Failure> {
    let raw = cli.cookie.clone().ok_or_else(|| {
        auth_required(format!(
            "{why}: set DARKHOLD_COOKIE (the session cookie — saving a view is a session \
             operation, so DARKHOLD_TOKEN will not open it)"
        ))
    })?;
    let csrf = raw
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| name.trim() == "darkhold_csrf")
        .map(|(_, value)| value.trim().to_owned());
    Ok((raw, csrf))
}

/// Attach the session, and the CSRF header a session request needs on a write.
fn with_session(
    request: reqwest::RequestBuilder,
    cookie: &str,
    csrf: Option<&str>,
) -> reqwest::RequestBuilder {
    let request = request.header(reqwest::header::COOKIE, cookie);
    match csrf {
        Some(token) => request.header("x-csrf-token", token),
        None => request,
    }
}

async fn stored_views(cli: &Cli) -> Result<Vec<StoredView>, Failure> {
    let (cookie, csrf) = session_cookie(
        cli,
        "saved views belong to an account, so this needs a session",
    )?;
    let url = format!("{}/api/v1/saved-views", cli.base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| environment(format!("http client: {e}")))?;
    let (status, bytes) = crate::http::send(
        || with_session(client.get(&url), &cookie, csrf.as_deref()),
        &cli.base_url,
    )
    .await?;
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(failure_for(status, &body, &cli.base_url));
    }
    Ok(body
        .get("items")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(stored_view).collect())
        .unwrap_or_default())
}

async fn list_queries(cli: &Cli) -> Result<(), Failure> {
    let views = stored_views(cli).await?;
    if views.is_empty() {
        println!("no saved views");
        return Ok(());
    }
    let width = views
        .iter()
        .map(|view| view.name.chars().count())
        .max()
        .unwrap_or(4)
        .max(4);
    for view in &views {
        println!(
            "{:<width$}  {:<9}  {}",
            view.name,
            view.kind,
            view.updated_at,
            width = width
        );
    }
    Ok(())
}

/// The view named `name`, refused unless it is one `rec` can run.
async fn lookup_view(cli: &Cli, name: &str) -> Result<StoredView, Failure> {
    let views = stored_views(cli).await?;
    let view = views
        .into_iter()
        .find(|view| view.name == name)
        .ok_or_else(|| {
            bad_request(format!(
                "no saved view named \"{name}\" — `dh rec --queries` lists them"
            ))
        })?;
    if view.kind != "catalogue" {
        return Err(bad_request(format!(
            "\"{name}\" is a {} view: it filters your library items, so `dh library` opens it and \
             `dh rec` cannot run it",
            view.kind
        )));
    }
    if view.params.as_deref().is_none_or(str::is_empty) {
        return Err(bad_request(format!(
            "\"{name}\" has nothing stored this build can run — re-save it with \
             `dh rec --save {name}`"
        )));
    }
    Ok(view)
}

/// Store the query this call would have sent, and print where it lives.
async fn save_query(cli: &Cli, target: &Target, args: &RecArgs, name: &str) -> Result<(), Failure> {
    if matches!(target, Target::Engine(_)) {
        return Err(bad_request(
            "--save stores a catalogue query; an engine run is ranked against your own history, so \
             it is not a query a second reader could run",
        ));
    }
    let url_built = url(&cli.base_url, target, args);
    let params = url_built
        .split_once('?')
        .map_or(String::new(), |(_, query)| query.to_owned());
    // Answered before the credential is looked at: this is a contradiction in the
    // command, and telling the caller to set a cookie would send them to fix the
    // wrong thing (the same ordering the engine and `--seed` checks use).
    // The writer refuses it too, but it cannot name the flag the caller typed.
    if params
        .split('&')
        .any(|pair| pair.split('=').next() == Some("exclude_read"))
    {
        return Err(bad_request(
            "--exclude-read depends on your own reading history, so the view could not be shared; \
             drop it to save the query",
        ));
    }
    let (cookie, csrf) = session_cookie(cli, "--save writes a view to your account")?;
    let payload = serde_json::json!({
        "name": name,
        "kind": "catalogue",
        "params": params,
        "scope": "public",
    });
    let url = format!("{}/api/v1/saved-views", cli.base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| environment(format!("http client: {e}")))?;
    let (status, bytes) = crate::http::send(
        || with_session(client.post(&url).json(&payload), &cookie, csrf.as_deref()),
        &cli.base_url,
    )
    .await?;
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(failure_for(status, &body, &cli.base_url));
    }
    let id = body.get("id").and_then(Value::as_str).unwrap_or_default();
    println!("saved \"{name}\" ({id})");
    println!(
        "{}{}?{}",
        cli.base_url.trim_end_matches('/'),
        path_for(target),
        params
    );
    Ok(())
}

/// Ask the server what is new, then keep only those rows.
async fn digest(cli: &Cli, view: &StoredView, body: Value) -> Result<(Value, String), Failure> {
    let items: Vec<Value> = body
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let ids: Vec<String> = items
        .iter()
        .filter_map(|row| row.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();
    let page = ids.len();

    let (cookie, csrf) = session_cookie(cli, "--digest-diff records a run against a saved view")?;
    let url = format!(
        "{}/api/v1/saved-views/{}/runs",
        cli.base_url.trim_end_matches('/'),
        view.id
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| environment(format!("http client: {e}")))?;
    let payload = serde_json::json!({ "ids": ids });
    let (status, bytes) = crate::http::send(
        || with_session(client.post(&url).json(&payload), &cookie, csrf.as_deref()),
        &cli.base_url,
    )
    .await?;
    let run: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(failure_for(status, &run, &cli.base_url));
    }

    let new_ids: Vec<String> = run
        .get("new_ids")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let previous = run.get("previous").and_then(Value::as_i64).unwrap_or(0);
    let kept: Vec<Value> = items
        .into_iter()
        .filter(|row| {
            row.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| new_ids.iter().any(|new| new == id))
        })
        .collect();
    let new_count = kept.len();
    let note = match previous {
        0 => format!("first run — nothing to diff against ({page} on the page)"),
        seen => format!("{new_count} new of {page} on the page (the last run recorded {seen})"),
    };
    let mut out = body;
    out["items"] = Value::Array(kept);
    out["digest"] = serde_json::json!({
        "new": new_count,
        "page": page,
        "previous": previous,
        "ran_at": run.get("ran_at").cloned().unwrap_or(Value::Null),
    });
    Ok((out, note))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn args(argv: &[&str]) -> (Cli, RecArgs) {
        let cli = Cli::try_parse_from(argv).expect("parses");
        let crate::Command::Rec(rec) = cli.command.clone() else {
            panic!("'rec' is the subcommand this test builds")
        };
        (cli, *rec)
    }

    #[test]
    fn rec_parses_the_anchor_ask() {
        let (cli, rec) = args(&[
            "dh",
            "rec",
            "--fandom",
            "naruto",
            "--updated",
            "7d",
            "--engine",
            "rrf",
            "--sort",
            "wilson",
            "--explain",
        ]);
        assert_eq!(rec.fandom.as_deref(), Some("naruto"));
        assert_eq!(rec.updated.as_deref(), Some("7d"));
        assert_eq!(rec.engine.as_deref(), Some("rrf"));
        // Carried verbatim, not validated here: the metric vocabulary is the
        // server's, and a client-side alias table would be a second definition
        // of it. `wilson` is M63-03's, and until then the server answers
        // `unknown sort: wilson` — which is the correct refusal, in one place.
        assert_eq!(rec.sort.as_deref(), Some("wilson"));
        assert!(rec.explain);
        assert_eq!(cli.base_url, "http://127.0.0.1:8081");
        assert_eq!(rec.limit, 20);
    }

    // --- output shape (M63-05) -------------------------------------------------

    #[test]
    fn jsonl_and_the_pretty_page_are_refused_together() {
        let (cli, rec) = args(&["dh", "--json", "rec", "--jsonl"]);
        assert!(cli.json, "the global flag is what this test is about");
        let err = shape(&rec, cli.json).expect_err("refused");
        assert_eq!(
            err.code, 2,
            "a wrong request is code 2, not an environment fault"
        );
        assert!(err.message.contains("--json"), "{}", err.message);
        // And a clean ask is accepted.
        assert!(shape(&rec, false).expect("accepted").jsonl);
    }

    #[test]
    fn fields_without_jsonl_says_which_flag_is_missing() {
        let (cli, rec) = args(&["dh", "rec", "--fields", "title,likes"]);
        let err = shape(&rec, cli.json).expect_err("refused");
        assert!(err.message.contains("--jsonl"), "{}", err.message);
        let (_, ok) = args(&["dh", "rec", "--jsonl", "--fields", "title,likes"]);
        let shape = shape(&ok, false).expect("accepted");
        assert_eq!(shape.fields.expect("fields").len(), 2);
    }

    #[test]
    fn a_cols_projection_of_only_score_is_refused() {
        // On a page with no scores this prints nothing at all, which reads as
        // "nothing matched" rather than "you asked for an empty table".
        let (cli, rec) = args(&["dh", "rec", "--cols", "score"]);
        let err = shape(&rec, cli.json).expect_err("refused");
        assert!(err.message.contains("beside `score`"), "{}", err.message);
    }

    #[test]
    fn link_epub_is_refused_with_the_missing_route_named() {
        let (cli, rec) = args(&["dh", "rec", "--link", "epub"]);
        let err = shape(&rec, cli.json).expect_err("refused");
        assert!(err.message.contains("M63-05b"), "{}", err.message);
        let (_, ok) = args(&["dh", "rec", "--link", "md"]);
        assert_eq!(
            shape(&ok, false).expect("accepted").link,
            Some(crate::output::LinkKind::Md)
        );
    }

    #[test]
    fn explain_without_an_engine_picks_one_rather_than_explaining_nothing() {
        let (_, rec) = args(&["dh", "rec", "--fandom", "naruto", "--explain"]);
        assert_eq!(
            Target::resolve(&rec, "rrf"),
            Target::Engine("rrf".to_owned())
        );
        let (_, plain) = args(&["dh", "rec", "--fandom", "naruto"]);
        assert_eq!(Target::resolve(&plain, "rrf"), Target::Catalogue);
    }

    #[test]
    fn the_url_carries_only_what_was_asked_for() {
        let (cli, rec) = args(&["dh", "rec", "--fandom", "Harry Potter", "--updated", ">30d"]);
        let request_url = url(&cli.base_url, &Target::Catalogue, &rec);
        assert_eq!(
            request_url,
            "http://127.0.0.1:8081/api/v1/catalogue?fandom=Harry%20Potter&updated=%3E30d&limit=20",
            "a space and a `>` have to be encoded or the window grammar breaks"
        );

        let (cli, rec) = args(&[
            "dh",
            "rec",
            "--engine",
            "rrf",
            "--fandom",
            "naruto",
            "--explain",
            "--limit",
            "5",
        ]);
        let request_url = url(&cli.base_url, &Target::Engine("rrf".to_owned()), &rec);
        assert!(request_url.starts_with("http://127.0.0.1:8081/api/v1/rec/run?"));
        assert!(request_url.contains("engine=rrf"));
        assert!(request_url.contains("explain=1"));
        assert!(request_url.contains("limit=5"));
    }

    fn page(items: Vec<Value>) -> Value {
        serde_json::json!({ "items": items, "sort": "likes", "limit": 20, "offset": 0 })
    }

    fn item(title: &str, likes: i64) -> Value {
        serde_json::json!({
            "id": "id", "source_key": "sb", "source_work_key": "w", "title": title,
            "author_text": "Esama", "summary": "", "word_count": 21044, "status": "complete",
            "likes": likes, "views": null, "source_url": "https://x.invalid",
            "source_updated_at": "2026-10-27T14:02:11Z",
        })
    }

    #[test]
    fn the_table_prints_the_columns_a_reader_scans() {
        let rendered = table(&page(vec![item("Naruto: A Quiet Sort", 812)]));
        assert!(rendered.contains("812 likes"), "{rendered}");
        assert!(rendered.contains("21.0k words"), "{rendered}");
        assert!(rendered.contains("2026-10-27"), "the date, not the instant");
        assert!(rendered.contains("Naruto: A Quiet Sort"), "{rendered}");
        assert!(rendered.contains("by Esama"), "{rendered}");
        assert!(rendered.contains("[sb]"), "{rendered}");
    }

    #[test]
    fn a_field_the_cli_does_not_know_about_does_not_shift_the_table() {
        // The server is free to add a column; a script parsing this table must
        // not see the row move underneath it.
        let mut with_extra = item("Same Row", 10);
        with_extra["brand_new_field"] = serde_json::json!({"nested": [1, 2, 3]});
        let before = table(&page(vec![item("Same Row", 10)]));
        let after = table(&page(vec![with_extra]));
        assert_eq!(before, after);
    }

    #[test]
    fn a_long_title_is_truncated_rather_than_pushing_every_column_right() {
        let long = "A".repeat(80);
        let rendered = table(&page(vec![item(&long, 1)]));
        assert!(rendered.contains('…'), "{rendered}");
        // 44 columns for the title, plus the ellipsis replacing the last one.
        assert!(!rendered.contains(&"A".repeat(45)), "{rendered}");
    }

    #[test]
    fn coverage_is_printed_only_when_the_page_carries_it() {
        let plain = page(vec![item("x", 1)]);
        assert_eq!(coverage(&plain), None);

        let with = serde_json::json!({
            "items": [], "fandom": "Naruto", "fandom_tagged_rows": 7626, "catalogue_rows": 141317
        });
        let line = coverage(&with).expect("coverage");
        assert!(line.contains("7626 of 141317"), "{line}");
        assert!(line.contains("5.4%"), "{line}");
    }

    #[test]
    fn the_coverage_line_says_when_the_tagged_rows_cannot_be_ranked() {
        // The live catalogue's shape: 7,626 tagged rows, every one from a source
        // that publishes no endorsements. A page of zeros without this sentence
        // reads as a broken ranking.
        let with = serde_json::json!({
            "items": [], "fandom": "Naruto", "fandom_tagged_rows": 7626,
            "fandom_tagged_with_likes": 0, "catalogue_rows": 141317
        });
        let line = coverage(&with).expect("coverage");
        assert!(
            line.contains("none of them publishes a like count"),
            "{line}"
        );

        // When every tagged row can be ranked, the sentence is not printed.
        let rankable = serde_json::json!({
            "items": [], "fandom": "Naruto", "fandom_tagged_rows": 100,
            "fandom_tagged_with_likes": 100, "catalogue_rows": 1000
        });
        let line = coverage(&rankable).expect("coverage");
        assert!(!line.contains("like count"), "{line}");
    }

    #[test]
    fn a_strategy_that_produced_nothing_is_still_printed() {
        let with = serde_json::json!({
            "engine": "rrf", "candidates": 265,
            "strategies": [
                {"name": "cooccurrence", "produced": 0, "matched": 0, "top": []},
                {"name": "time_decay", "produced": 265, "matched": 44,
                 "top": [{"id": "a", "score": 0.0164}]}
            ]
        });
        let block = explanation(&with).expect("explanation");
        assert!(
            block.contains("cooccurrence"),
            "the silent strategy is the finding"
        );
        assert!(block.contains("0.0164"), "{block}");
    }

    fn metric_page() -> Value {
        serde_json::json!({
            "items": [
                {"id": "a", "source_key": "sb", "source_work_key": "a", "title": "Measured",
                 "author_text": "Esama", "summary": "", "word_count": 40000, "status": "ongoing",
                 "likes": 33107, "views": null, "source_url": "https://x.invalid",
                 "updated_at": "2026-10-27T00:00:00+00:00", "score": 0.75, "measured": true},
                {"id": "b", "source_key": "sb", "source_work_key": "b", "title": "Unmeasured",
                 "author_text": "Nobody", "summary": "", "word_count": 60000, "status": "ongoing",
                 "likes": 0, "views": null, "source_url": "https://x.invalid",
                 "score": 0.33, "measured": false}
            ],
            "sort": "source_pct", "metric": "source_pct", "ranked": 1247, "total": 141317
        })
    }

    #[test]
    fn a_metric_page_prints_a_score_column_and_an_unmeasured_marker() {
        let rendered = table(&metric_page());
        assert!(rendered.contains("0.750"), "{rendered}");
        assert!(
            rendered.contains("2026-10-27"),
            "the source date fills the column: {rendered}"
        );
        // The unmeasured row is marked, so a script cannot mistake triage for rank.
        assert!(rendered.contains('~'), "{rendered}");
    }

    #[test]
    fn the_gate_says_ranked_of_total_and_the_share() {
        let line = gate(&metric_page()).expect("gate");
        assert!(line.contains("1247 of 141317"), "{line}");
        assert!(line.contains("0.9%"), "{line}");
        assert!(line.contains("marked ~"), "{line}");

        // A page where everything was measured needs no caveat.
        let complete = serde_json::json!({
            "items": [], "metric": "source_pct", "ranked": 10, "total": 10
        });
        let line = gate(&complete).expect("gate");
        assert!(!line.contains("marked ~"), "{line}");
    }

    #[test]
    fn the_gate_is_absent_on_a_plain_column_sort() {
        assert_eq!(gate(&page(vec![item("x", 1)])), None);
    }

    #[test]
    fn the_metric_list_names_every_metric_its_status_and_what_unblocks_the_rest() {
        let text = metrics_list();
        for name in [
            "quality",
            "source_pct",
            "engagement_density",
            "consensus",
            "crosspost",
            "words_maturity",
            "source_coverage",
            "cohort_pct",
            "recency",
            "unmeasured",
            "freshness_weighted_quality",
            "completion",
            "kudos_per_hour",
            "updater_reliability",
        ] {
            assert!(text.contains(name), "{name} missing from:\n{text}");
        }
        assert!(
            text.contains("velocity*"),
            "blocked metrics are marked: {text}"
        );
        assert!(
            text.contains("M63-32"),
            "and say what unblocks them: {text}"
        );
        assert!(
            text.contains("aliases: kudos = likes"),
            "the words that are not canonical names are listed too: {text}"
        );
        // The list exists so a name can be copied into `--sort`; a truncated name
        // is a name the reader has to guess the tail of.
        assert!(
            text.contains("freshness_weighted_quality"),
            "the longest name is printed whole: {text}"
        );
        assert!(
            text.contains("updater_reliability*"),
            "and so is the longest blocked one, with its marker: {text}"
        );
        assert!(
            text.contains("raw"),
            "the stored columns are still there: {text}"
        );
    }

    #[test]
    fn a_sort_word_outside_the_vocabulary_is_refused_locally() {
        assert!(validate_sort("source_pct").is_ok());
        assert!(validate_sort("wilson").is_ok());
        // M63-03: the row's own words are names now, blocked or not.
        for word in [
            "kudos",
            "kudos_per_1k_views",
            "kudos_per_hour",
            "completion",
            "updater_reliability",
            "freshness_weighted_quality",
        ] {
            assert!(validate_sort(word).is_ok(), "{word} must be a name");
        }
        let failure = validate_sort("wilson2").expect_err("unknown");
        assert_eq!(failure.code, 2);
        assert!(failure.message.contains("--list-metrics"));
    }

    #[test]
    fn a_401_says_what_to_do_instead_of_quoting_the_status() {
        let body = serde_json::json!({"error": {"message": "session required"}});
        let failure = failure_for(401, &body, "http://127.0.0.1:8081");
        assert_eq!(
            failure.code,
            crate::exit::AUTH,
            "M63-16: a missing session is code 4, not a usage error — a script can act on 4"
        );
        assert!(failure.message.contains("DARKHOLD_TOKEN"));
    }

    #[test]
    fn a_500_is_retryable_and_a_422_is_not() {
        let body = serde_json::json!({"error": {"message": "fandom: no fandom called \"narut\""}});
        assert_eq!(failure_for(500, &body, "u").code, crate::exit::ERROR);
        let failure = failure_for(422, &body, "u");
        assert_eq!(failure.code, crate::exit::USAGE);
        assert!(failure.message.contains("no fandom called"));
    }

    // -----------------------------------------------------------------------
    // Saved queries (M63-06). Each of these is refused before a request is made,
    // which is the point: the message names the flag, not a status code.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn digest_diff_without_a_saved_query_is_refused_by_name() {
        let (cli, args) = args(&["dh", "rec", "--digest-diff"]);
        let failure = run(&cli, &args).await.expect_err("refused");
        assert_eq!(failure.code, 2);
        assert!(failure.message.contains("--saved"), "{}", failure.message);
        assert!(
            failure.message.contains("--digest-diff"),
            "{}",
            failure.message
        );
    }

    #[tokio::test]
    async fn saving_and_running_are_two_verbs_and_are_not_merged() {
        let (cli, args) = args(&["dh", "rec", "--save", "weekly", "--saved", "weekly"]);
        let failure = run(&cli, &args).await.expect_err("refused");
        assert_eq!(failure.code, 2);
        assert!(failure.message.contains("--save"), "{}", failure.message);
        assert!(failure.message.contains("--saved"), "{}", failure.message);
    }

    #[tokio::test]
    async fn a_saved_query_and_a_personal_ranking_are_not_mixed() {
        let (mut cli, args) = args(&["dh", "rec", "--saved", "weekly", "--engine", "rrf"]);
        cli.token = Some("t".to_owned());
        cli.cookie = Some("darkhold_session=abc".to_owned());
        let failure = run(&cli, &args).await.expect_err("refused");
        assert_eq!(failure.code, 2, "{}", failure.message);
        assert!(failure.message.contains("--saved"), "{}", failure.message);
        assert!(failure.message.contains("--engine"), "{}", failure.message);
    }

    #[tokio::test]
    async fn listing_refuses_to_also_save_or_diff() {
        let (cli, args) = args(&["dh", "rec", "--queries", "--saved", "weekly"]);
        let failure = run(&cli, &args).await.expect_err("refused");
        assert_eq!(failure.code, 2);
        assert!(failure.message.contains("--queries"), "{}", failure.message);
    }

    #[tokio::test]
    async fn saving_without_a_session_is_an_auth_failure_not_a_usage_one() {
        let (cli, args) = args(&["dh", "rec", "--fandom", "naruto", "--save", "weekly"]);
        let failure = run(&cli, &args).await.expect_err("refused");
        assert_eq!(
            failure.code,
            crate::exit::AUTH,
            "a view is written to an account, so this is 4: {}",
            failure.message
        );
        assert!(
            failure.message.contains("DARKHOLD_COOKIE"),
            "the saved-view surface is a session surface, so the refusal must name the session \
             variable: {}",
            failure.message
        );
        assert!(
            failure.message.contains("DARKHOLD_TOKEN will not open it"),
            "a caller who tries the token must be told why it cannot work: {}",
            failure.message
        );
    }

    #[tokio::test]
    async fn listing_without_a_session_is_an_auth_failure() {
        let (cli, args) = args(&["dh", "rec", "--queries"]);
        let failure = run(&cli, &args).await.expect_err("refused");
        assert_eq!(failure.code, crate::exit::AUTH, "{}", failure.message);
    }

    #[tokio::test]
    async fn a_history_dependent_query_cannot_be_saved_as_a_shared_view() {
        let (mut cli, args) = args(&[
            "dh",
            "rec",
            "--fandom",
            "naruto",
            "--exclude-read",
            "--save",
            "weekly",
        ]);
        // `--exclude-read` needs a session of its own before this point, so both
        // credentials are present: the refusal being tested is the contradiction,
        // not the missing credential.
        cli.token = Some("t".to_owned());
        cli.cookie = Some("darkhold_session=abc".to_owned());
        let failure = run(&cli, &args).await.expect_err("refused");
        assert_eq!(failure.code, 2, "{}", failure.message);
        assert!(
            failure.message.contains("--exclude-read"),
            "the refusal must name the flag the caller typed: {}",
            failure.message
        );
    }

    #[test]
    fn the_csrf_token_is_read_out_of_the_cookie_header() {
        let (mut cli, _args) = args(&["dh", "rec", "--queries"]);
        cli.cookie = Some("darkhold_session=s3cret; darkhold_csrf=c51f; other=1".to_owned());
        let (_, csrf) = session_cookie(&cli, "test").expect("a cookie was set");
        assert_eq!(csrf.as_deref(), Some("c51f"));

        // A session with no CSRF cookie is not an error here — a GET needs none,
        // and the write paths let the server say so.
        cli.cookie = Some("darkhold_session=s3cret".to_owned());
        let (_, csrf) = session_cookie(&cli, "test").expect("a cookie was set");
        assert_eq!(csrf, None);
    }

    #[test]
    fn a_view_is_read_from_the_wire_with_its_kind_intact() {
        let view = stored_view(&serde_json::json!({
            "id": "abc",
            "name": "weekly",
            "kind": "catalogue",
            "params": "fandom=naruto&limit=2",
            "updated_at": "2026-11-04T10:00:00Z",
        }));
        assert_eq!(view.kind, "catalogue");
        assert_eq!(view.params.as_deref(), Some("fandom=naruto&limit=2"));

        // A document written before `kind` existed is a library view; offering it
        // as a runnable query would hand the catalogue a library filter.
        let old = stored_view(&serde_json::json!({ "id": "def", "name": "shelf" }));
        assert_eq!(old.kind, "library");
        assert_eq!(old.params, None);
    }
}
