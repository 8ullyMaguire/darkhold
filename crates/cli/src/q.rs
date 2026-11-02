//! `dh q` — the query language at a terminal.
//!
//! One expression, one route: `GET /api/v1/search?q=…`, the same endpoint the web search box
//! posts to. That is the whole point of the verb. A power user's query is an object, and this
//! program's job is to send it, not to understand it: the grammar (`tag:`, `AND`/`OR`/`NOT`,
//! `kudos>=`, `updated>=-7d`, `words>`, `completion:`) is parsed on the server by
//! `darkhold-domain`, so the terminal, the site's search box, the saved filters and the feeds
//! cannot disagree about what an expression means. A second parser here would be a second
//! definition waiting to drift.
//!
//! What this module owns is the part the server cannot: the **shape** of the output. `--jsonl`
//! one hit per line, `--cols`/`--wide` for a table, and the same refusals `dh rec` makes
//! (`--json` with `--jsonl`, `--fields` without `--jsonl`), so a script that reads one verb reads
//! the other.
//!
//! ```text
//! dh q 'tag:naruto AND kudos:>=100'
//! dh q '(tag:romance OR tag:angst) AND updated>=-7d' --jsonl --fields title,author,words
//! dh q 'words:>10000 AND completion:complete' --limit 50 --wide
//! ```
//!
//! A query the reader got wrong comes back as the server's own `422` and exit code `2` — the
//! parse error names the token and where it went wrong, and repeating the command unchanged
//! cannot help. That is the same code a locally refused flag combination gets, because to a
//! script both mean "fix the command".

use anyhow::Result;
use clap::Args;
use serde_json::Value;

use crate::rec::{bad_request, environment, failure_for, Failure};
use crate::{output, Cli};

/// Options for `q`.
#[derive(Debug, Clone, Args)]
pub struct QArgs {
    /// The expression, in the server's query language.
    ///
    /// Quote it: `>` and `|` are the shell's, and an unquoted `words:>1000` is a redirect.
    ///
    /// ```text
    /// tag:naruto AND kudos:>=100
    /// (tag:romance OR tag:angst) AND updated>=-7d
    /// "a phrase" AND NOT tag:gore
    /// words:>10000 AND completion:complete
    /// ```
    #[arg(value_name = "EXPR", num_args = 1.., required = true)]
    pub expression: Vec<String>,

    /// How many hits to ask for (1–100).
    #[arg(long, default_value_t = 20, value_name = "N")]
    pub limit: i64,

    /// One JSON object per hit, for a pipeline: `dh q '…' --jsonl | jq -r .title`.
    #[arg(long)]
    pub jsonl: bool,

    /// With `--jsonl`: print only these fields, in this order.
    ///
    /// The vocabulary is the table's — `rank`, `score`, `words`, `title`, `author` — because a
    /// search hit carries no more than that. `--fields source` is refused with the reason rather
    /// than printed blank.
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
}

/// The request URL for one expression.
///
/// The expression is percent-encoded whole rather than split: it is free text to this program, and
/// the five characters an encoder can get wrong (`&`, `+`, `#`, `%`, a space) are exactly the ones
/// a query contains — `tag:a AND tag:b` has two spaces and an `&` in the naive spelling.
pub fn url(base_url: &str, expression: &str, limit: i64) -> String {
    format!(
        "{}/api/v1/search?q={}&limit={limit}",
        base_url.trim_end_matches('/'),
        crate::rec::encode(expression)
    )
}

/// Decide the output shape from the flags, refusing combinations rather than picking a winner.
fn shape(args: &QArgs, json: bool) -> Result<output::Shape, Failure> {
    use output::{parse_projection, Column, SEARCH_COLUMNS};
    let mut shape = output::Shape::default();

    if json && args.jsonl {
        return Err(bad_request(
            "--json prints the whole page; --jsonl prints one row per line. Pick one.",
        ));
    }
    if json && (args.fields.is_some() || args.cols.is_some()) {
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
    if args.fields.is_some() && !args.jsonl {
        return Err(bad_request(
            "--fields selects JSON fields; add --jsonl. The table's projection is --cols.",
        ));
    }

    shape.wide = args.wide;
    shape.jsonl = args.jsonl;
    if let Some(raw) = args.fields.as_deref() {
        shape.fields = Some(
            parse_projection(raw, &SEARCH_COLUMNS)
                .map_err(|e| bad_request(format!("--fields: {e}")))?,
        );
    }
    if let Some(raw) = args.cols.as_deref() {
        shape.cols = Some(
            parse_projection(raw, &SEARCH_COLUMNS)
                .map_err(|e| bad_request(format!("--cols: {e}")))?,
        );
        // `--cols score` alone on a page the index did not score prints nothing that identifies a
        // row, and an empty table reads as "nothing matched".
        let cols = shape.cols.as_deref().unwrap_or_default();
        if !cols.iter().any(|column| *column != Column::Score) {
            return Err(bad_request(
                "--cols needs at least one column beside `score`: a page with no scores would print nothing at all",
            ));
        }
    }
    Ok(shape)
}

/// Run `dh q`.
pub async fn run(cli: &Cli, args: &QArgs) -> Result<(), Failure> {
    let expression = args.expression.join(" ");
    if expression.trim().is_empty() {
        return Err(bad_request("the expression is empty"));
    }
    if !(1..=100).contains(&args.limit) {
        return Err(bad_request("limit must be between 1 and 100"));
    }
    // Before the request, so a contradiction costs no round trip and the message is about the
    // command rather than about the server's answer to a request it should never have got.
    let shape_decided = shape(args, cli.json)?;
    let request_url = url(&cli.base_url, &expression, args.limit);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| environment(format!("http client: {e}")))?;
    let token = cli.token.clone();
    let (status, bytes) = crate::http::send(
        || {
            let mut request = client.get(&request_url);
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
        // A `422` here is the parser speaking, not a fault: it names the token, and `failure_for`
        // turns it into exit code 2 with the server's sentence intact.
        return Err(failure_for(status, &body, &cli.base_url));
    }

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string())
        );
        return Ok(());
    }

    if shape_decided.jsonl {
        print!("{}", output::jsonl(&body, shape_decided.fields.as_deref()));
        return Ok(());
    }

    let cols = shape_decided
        .cols
        .clone()
        .unwrap_or_else(|| output::SEARCH_COLUMNS.to_vec());
    let printed = output::table(&body, &cols, shape_decided.wide);
    if printed.is_empty() {
        // stderr, not stdout: a caller piping the table into a file must not receive a sentence
        // where rows were expected.
        eprintln!("no works matched {expression:?}");
    } else {
        print!("{printed}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn args(argv: &[&str]) -> (Cli, QArgs) {
        let cli = Cli::try_parse_from(argv).expect("parses");
        let crate::Command::Q(q) = cli.command.clone() else {
            panic!("'q' is the subcommand this test builds")
        };
        (cli, *q)
    }

    #[test]
    fn q_takes_one_quoted_expression() {
        let (_, q) = args(&["dh", "q", "tag:naruto AND kudos:>=100"]);
        assert_eq!(q.expression, vec!["tag:naruto AND kudos:>=100".to_owned()]);
        assert_eq!(q.limit, 20);
    }

    #[test]
    fn q_joins_unquoted_positionals_with_a_space() {
        // `dh q tag:naruto AND words:'>1000'` — the shell splits on spaces, and rejoining them is
        // what lets a reader type the expression without one pair of quotes around all of it.
        let (_, q) = args(&["dh", "q", "tag:naruto", "AND", "words:>1000"]);
        assert_eq!(q.expression.join(" "), "tag:naruto AND words:>1000");
    }

    #[test]
    fn q_requires_an_expression() {
        let parsed = Cli::try_parse_from(["dh", "q"]);
        assert!(
            parsed.is_err(),
            "`dh q` with nothing to search is a usage error"
        );
    }

    #[test]
    fn the_url_encodes_the_operators_and_the_spaces() {
        let url = url("https://example.test", "tag:a AND kudos:>=100", 5);
        assert_eq!(
            url,
            "https://example.test/api/v1/search?q=tag%3Aa%20AND%20kudos%3A%3E%3D100&limit=5"
        );
        // One `&` and it is ours: the separator before `limit`. A raw `&` from the expression would
        // turn the rest of the query into a second parameter the server never sees as a query.
        assert_eq!(
            url.matches('&').count(),
            1,
            "the expression must not inject a parameter: {url}"
        );
        assert!(url.ends_with("&limit=5"), "{url}");
    }

    #[test]
    fn a_trailing_slash_on_the_base_url_does_not_double() {
        assert_eq!(
            url("https://example.test/", "tag:naruto", 2),
            "https://example.test/api/v1/search?q=tag%3Anaruto&limit=2"
        );
    }

    #[test]
    fn fields_without_jsonl_says_which_flag_is_missing() {
        let (cli, q) = args(&["dh", "q", "tag:naruto", "--fields", "title"]);
        let error = shape(&q, cli.json).unwrap_err();
        assert_eq!(error.code, 2);
        assert!(error.message.contains("--jsonl"), "{}", error.message);
    }

    #[test]
    fn a_column_the_search_page_does_not_carry_is_refused_by_name() {
        // `likes` exists on a catalogue row and not on a search hit; printing it blank would read
        // as "no likes" rather than "not on this page".
        let (cli, q) = args(&["dh", "q", "tag:naruto", "--cols", "title,likes"]);
        let error = shape(&q, cli.json).unwrap_err();
        assert!(error.message.contains("likes"), "{}", error.message);
        assert!(
            error.message.contains("rank") && error.message.contains("title"),
            "the refusal must print the vocabulary, got: {}",
            error.message
        );
    }

    #[test]
    fn json_and_jsonl_are_refused_together() {
        let (cli, q) = args(&["dh", "q", "tag:naruto", "--json", "--jsonl"]);
        assert_eq!(shape(&q, cli.json).unwrap_err().code, 2);
    }

    #[test]
    fn the_default_table_is_the_five_columns_a_search_hit_has() {
        let (cli, q) = args(&["dh", "q", "tag:naruto"]);
        let shape = shape(&q, cli.json).expect("the default shape is legal");
        assert!(!shape.jsonl);
        assert!(
            shape.cols.is_none(),
            "no projection means the default columns"
        );
    }
}
