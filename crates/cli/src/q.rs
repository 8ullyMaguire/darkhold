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

use std::path::{Path, PathBuf};

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
    #[arg(value_name = "EXPR", num_args = 0..)]
    pub expression: Vec<String>,

    /// Read the expression from a file instead of the command line.
    ///
    /// A bare name is looked for at `.darkhold/queries/<name>.q`, so a query library is a file in
    /// the repository — versioned, diffable and reviewable — rather than a command remembered from
    /// a shell history. Anything containing a `/` or ending in `.q` is used as a path.
    ///
    /// `#` starts a comment at the start of a line or after whitespace; blank lines are skipped. A
    /// syntax error is reported against the file, by line and column, not against the joined text.
    #[arg(long, value_name = "FILE")]
    pub from_file: Option<String>,

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

    /// Print the query plan and the indexes the engine would use, instead of hits.
    ///
    /// The plan is the engine's own — `EXPLAIN QUERY PLAN` on SQLite, `EXPLAIN` on PostgreSQL —
    /// run against the statement this search would execute, with this search's binds. That is
    /// the question "why is this query slow" answers itself from; a plan for a different
    /// statement would be a diagnosis of something nobody ran.
    #[arg(long)]
    pub explain: bool,
}

/// The request URL for one expression.
///
/// The expression is percent-encoded whole rather than split: it is free text to this program, and
/// the five characters an encoder can get wrong (`&`, `+`, `#`, `%`, a space) are exactly the ones
/// a query contains — `tag:a AND tag:b` has two spaces and an `&` in the naive spelling.
///
/// `explain` appends the flag `M63-08` added. It is a parameter rather than a second function so
/// there is one place the request is spelled, and the flag is the only thing that varies.
pub fn url(base_url: &str, expression: &str, limit: i64, explain: bool) -> String {
    let mut url = format!(
        "{}/api/v1/search?q={}&limit={limit}",
        base_url.trim_end_matches('/'),
        crate::rec::encode(expression)
    );
    if explain {
        url.push_str("&explain=1");
    }
    url
}

// ---------------------------------------------------------------------------
// A query read from a file (M63-09)
// ---------------------------------------------------------------------------

/// One line of a query file, and where its text begins in the joined expression.
#[derive(Debug, Clone, PartialEq, Eq)]
struct QueryLine {
    /// 1-based, so a message can say `weekly.q:3`.
    number: usize,
    text: String,
    /// Char offset in the joined expression where `text` starts.
    start: usize,
}

/// A query read from a file, with enough information to point back at it.
///
/// The server parses one string, so the file's lines are joined before they are sent; without the
/// map this carries, a `422` offset would land in text the reader never typed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct QueryFile {
    path: PathBuf,
    /// Every line's text, joined with one space — this is what the server parses.
    expression: String,
    lines: Vec<QueryLine>,
}

impl QueryFile {
    /// The `(line, column, text)` a joined-expression char offset came from, both 1-based.
    ///
    /// An offset in the space *between* two lines belongs to the earlier line, which is where a
    /// reader looks when a parser complains at a join.
    fn locate(&self, offset: usize) -> Option<(usize, usize, &str)> {
        let line = self.lines.iter().rev().find(|line| offset >= line.start)?;
        let column = offset - line.start;
        Some((line.number, column + 1, line.text.as_str()))
    }
}

/// Cut a `#` comment: only at the start of the line or after whitespace.
///
/// A `#` inside a value is not a comment — a tag can be spelled `#angst` — and there is no escape
/// rule to invent, because the server's grammar has quotes and no escapes.
///
/// The byte slice is safe because `#` is ASCII: the slice point is always the offset of an ASCII
/// byte, which is a char boundary whatever precedes it.
fn drop_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    for index in 0..bytes.len() {
        if bytes[index] == b'#' && (index == 0 || bytes[index - 1].is_ascii_whitespace()) {
            return &line[..index];
        }
    }
    line
}

/// Read `path` as a query: strip comments, join the lines, keep the map back.
fn read_query_file(path: &Path) -> Result<QueryFile, Failure> {
    let raw = std::fs::read_to_string(path)
        .map_err(|error| environment(format!("{}: {error}", path.display())))?;
    let mut expression = String::new();
    let mut lines = Vec::new();
    for (index, line) in raw.lines().enumerate() {
        // Trimmed at both ends: indentation is for the reader of the file, and a canonical
        // single-space join makes the expression comparable with the one-liner it replaces.
        let text = drop_comment(line).trim();
        if text.is_empty() {
            continue;
        }
        if !expression.is_empty() {
            expression.push(' ');
        }
        lines.push(QueryLine {
            number: index + 1,
            text: text.to_owned(),
            // Recorded after the joining space, so it indexes the string the server sees.
            start: expression.chars().count(),
        });
        expression.push_str(text);
    }
    if expression.trim().is_empty() {
        return Err(bad_request(format!(
            "{}: no expression — {} line(s), every one blank or a comment",
            path.display(),
            raw.lines().count()
        )));
    }
    Ok(QueryFile {
        path: path.to_path_buf(),
        expression,
        lines,
    })
}

/// The query library a bare `--from-file` name is looked up in.
const QUERY_LIBRARY: &str = ".darkhold/queries";

/// Where a `--from-file` name points.
///
/// The order matters, and the first version had it wrong: a `.q` suffix was read as "a literal
/// path", so `dh q --from-file weekly.q` — the spelling this row itself uses — answered
/// `weekly.q: No such file or directory` while the file sat in the library one directory down.
/// A name is a NAME: the library is searched first (adding `.q` when absent), then the literal
/// path beside the caller, and only then is the refusal built.
fn resolve_query_path(name: &str) -> Result<PathBuf, Failure> {
    resolve_query_path_in(Path::new(QUERY_LIBRARY), Path::new("."), name)
}

/// [`resolve_query_path`] with the library and the working directory passed in, so the rules can be
/// tested without creating `.darkhold/` inside the crate.
fn resolve_query_path_in(library: &Path, cwd: &Path, name: &str) -> Result<PathBuf, Failure> {
    // A slash is unambiguous: the caller meant a path.
    if name.contains('/') {
        return Ok(cwd.join(name));
    }
    let with_suffix = match name.ends_with(".q") {
        true => name.to_owned(),
        false => format!("{name}.q"),
    };
    let candidate = library.join(&with_suffix);
    if candidate.is_file() {
        return Ok(candidate);
    }
    let literal = cwd.join(name);
    if literal.is_file() {
        return Ok(literal);
    }
    // The refusal says what exists: "no such file" after a typo in a directory the caller cannot
    // see is the least useful sentence in the tool.
    let mut available: Vec<String> = std::fs::read_dir(library)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let path = entry.path();
                    path.extension().is_some_and(|ext| ext == "q").then(|| {
                        path.file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    available.sort();
    Err(bad_request(format!(
        "{}: no such query, and not {} — {} holds {}",
        candidate.display(),
        literal.display(),
        library.display(),
        match available.is_empty() {
            true => "no .q files (create the directory and put one there)".to_owned(),
            false => available.join(", "),
        }
    )))
}

/// The file's own line with a caret under the column the parser stopped at.
fn caret_in_file(file: &QueryFile, offset: usize) -> String {
    match file.locate(offset) {
        Some((line, column, text)) => format!(
            "{}:{line}:{column}\n{text}\n{}^",
            file.path.display(),
            " ".repeat(column - 1)
        ),
        None => format!("{}: end of expression", file.path.display()),
    }
}

/// The reader's expression with a caret under the character the parser stopped at.
///
/// The server counts **chars**, so this does too — a byte offset lands in the wrong column the
/// first time somebody queries a tag with an accent in it. Tabs are copied rather than widened to
/// spaces, because a terminal that receives a tab under a tab lines them up.
fn caret(expression: &str, offset: usize) -> String {
    let mut column = String::new();
    for ch in expression.chars().take(offset) {
        column.push(if ch == '\t' { '\t' } else { ' ' });
    }
    format!("{expression}\n{column}^")
}

/// The plan the server returned, as a terminal reads it: which engine answered, the statement it
/// was asked about, the access path, and the indexes the plan names.
fn render_plan(body: &Value) -> Result<String, Failure> {
    let explain = &body["explain"];
    if explain.is_null() {
        return Err(environment(
            "the server answered without an `explain` block; it is older than this command",
        ));
    }
    let mut out = String::new();
    out.push_str(&format!(
        "engine: {}\n\nstatement:\n",
        explain["engine"].as_str().unwrap_or("unknown")
    ));
    for line in wrap_sql(explain["sql"].as_str().unwrap_or(""), 96) {
        out.push_str(&format!("  {line}\n"));
    }
    out.push_str("\nplan:\n");
    let lines = explain["plan"].as_array();
    match lines {
        Some(lines) if !lines.is_empty() => {
            for line in lines {
                out.push_str(&format!("  {}\n", line.as_str().unwrap_or("")));
            }
        }
        _ => out.push_str("  (the engine returned no plan rows)\n"),
    }
    out.push_str("\nindexes: ");
    let indexes: Vec<&str> = explain["indexes"]
        .as_array()
        .map(|names| names.iter().filter_map(|n| n.as_str()).collect())
        .unwrap_or_default();
    if indexes.is_empty() {
        out.push_str("none — this statement is a scan\n");
    } else {
        out.push_str(&format!("{}\n", indexes.join(", ")));
    }
    Ok(out)
}

/// Break a statement at spaces so a terminal gets lines instead of one 900-character row.
fn wrap_sql(sql: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in sql.split_whitespace() {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Decide the output shape from the flags, refusing combinations rather than picking a winner.
fn shape(args: &QArgs, json: bool) -> Result<output::Shape, Failure> {
    use output::{parse_projection, Column, SEARCH_COLUMNS};
    let mut shape = output::Shape::default();

    if args.explain {
        // A diagnosis prints neither rows nor columns: there is nothing to project, and a
        // `--jsonl` stream of plan lines would be a shape no script asked for.
        let conflict = if json {
            None
        } else if args.jsonl {
            Some("--jsonl")
        } else if args.cols.is_some() {
            Some("--cols")
        } else if args.fields.is_some() {
            Some("--fields")
        } else if args.wide {
            Some("--wide")
        } else {
            None
        };
        if let Some(flag) = conflict {
            return Err(bad_request(format!(
                "--explain prints a plan, not rows: {flag} has nothing to select. Drop it, or drop --explain."
            )));
        }
        // `--json` stays meaningful: it prints the server's `explain` object unchanged.
        return Ok(shape);
    }
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
    // Two sources for one string: preferring either silently would make the other a lie, so the
    // combination is refused before anything is read.
    let query = match (args.from_file.as_deref(), args.expression.is_empty()) {
        (Some(_), false) => return Err(bad_request(
            "--from-file reads the expression from a file; passing one as an argument too would \
                 make one of them silently ignored",
        )),
        (Some(name), true) => Some(read_query_file(&resolve_query_path(name)?)?),
        (None, _) => None,
    };
    let expression = match &query {
        Some(file) => file.expression.clone(),
        None => args.expression.join(" "),
    };
    if expression.trim().is_empty() {
        return Err(bad_request("the expression is empty"));
    }
    if !(1..=100).contains(&args.limit) {
        return Err(bad_request("limit must be between 1 and 100"));
    }
    // Before the request, so a contradiction costs no round trip and the message is about the
    // command rather than about the server's answer to a request it should never have got.
    let shape_decided = shape(args, cli.json)?;
    let request_url = url(&cli.base_url, &expression, args.limit, args.explain);

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
        // turns it into exit code 2 with the server's sentence intact. `M63-08`: when the body
        // carries the offset, draw the mistake under the reader's own text first — a caret does
        // in one line what "offset 6" asks the reader to do by counting.
        if status == 422 {
            match (&query, body["error"]["query"]["offset"].as_u64()) {
                // From a file, the caret belongs to the file's own line and column.
                (Some(file), Some(offset)) => eprintln!("{}", caret_in_file(file, offset as usize)),
                (None, Some(offset)) => {
                    if let Some(at_error) = body["error"]["query"]["expression"].as_str() {
                        eprintln!("{}", caret(at_error, offset as usize));
                    }
                }
                (_, None) => {}
            }
        }
        return Err(failure_for(status, &body, &cli.base_url));
    }

    if args.explain && !cli.json {
        println!("{}", render_plan(&body)?);
        return Ok(());
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
        // Clap can no longer refuse this: `--from-file` makes the positional optional. The
        // refusal moved to `run`, where it can say the expression is empty and still exit `2`,
        // which is the code the contract promises for "fix the command".
        let (cli, args) = args(&["dh", "q"]);
        let failure = futures_lite_block_on(super::run(&cli, &args)).expect_err("nothing to send");
        assert_eq!(failure.code, 2);
        assert!(failure.message.contains("empty"), "{}", failure.message);
    }

    /// A tiny executor so a contract test can call the async `run` without a full runtime.
    fn futures_lite_block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(future)
    }

    #[test]
    fn the_url_encodes_the_operators_and_the_spaces() {
        let url = url("https://example.test", "tag:a AND kudos:>=100", 5, false);
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
            url("https://example.test/", "tag:naruto", 2, false),
            "https://example.test/api/v1/search?q=tag%3Anaruto&limit=2"
        );
    }

    #[test]
    fn explain_appends_its_flag_and_nothing_else() {
        let url = url("https://example.test", "tag:naruto", 3, true);
        assert_eq!(
            url,
            "https://example.test/api/v1/search?q=tag%3Anaruto&limit=3&explain=1"
        );
    }

    #[test]
    fn explain_with_a_projection_is_refused_and_names_the_flag() {
        let (cli, q) = args(&["dh", "q", "tag:naruto", "--explain", "--jsonl"]);
        let error = shape(&q, cli.json).unwrap_err();
        assert_eq!(error.code, 2);
        assert!(error.message.contains("--jsonl"), "{}", error.message);
        assert!(error.message.contains("--explain"), "{}", error.message);
    }

    #[test]
    fn explain_with_json_is_allowed_because_it_prints_the_server_object() {
        let (cli, q) = args(&["dh", "q", "tag:naruto", "--explain", "--json"]);
        assert!(shape(&q, cli.json).is_ok());
    }

    // -----------------------------------------------------------------------
    // M63-09: a query read from a file
    // -----------------------------------------------------------------------

    fn scratch(name: &str, body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dh-q-file-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("scratch");
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write");
        path
    }

    #[test]
    fn a_query_file_joins_its_lines_and_strips_comments() {
        let path = scratch(
            "weekly.q",
            "# weekly scan\ntag:naruto AND updated>=-7d\n  AND kudos:>=100   # at least a hundred\n",
        );
        let file = read_query_file(&path).expect("reads");
        assert_eq!(
            file.expression,
            "tag:naruto AND updated>=-7d AND kudos:>=100"
        );
        assert_eq!(file.lines.len(), 2, "two lines carry text");
        assert_eq!(file.lines[1].number, 3, "the third line of the file");
    }

    #[test]
    fn a_hash_inside_a_value_is_not_a_comment() {
        assert_eq!(
            drop_comment("tag:#angst AND tag:fluff"),
            "tag:#angst AND tag:fluff"
        );
        assert_eq!(drop_comment("tag:x  # why"), "tag:x  ");
        assert_eq!(drop_comment("# whole line"), "");
    }

    #[test]
    fn an_offset_points_at_the_line_and_column_it_came_from() {
        let path = scratch("two.q", "tag:naruto AND updated>=-7d\nAND kudos:>=100\n");
        let file = read_query_file(&path).expect("reads");
        let offset = file
            .expression
            .find("AND kudos")
            .expect("in the joined text");
        let (line, column, text) = file.locate(offset).expect("inside the expression");
        assert_eq!((line, column), (2, 1), "the second line's first column");
        assert_eq!(text, "AND kudos:>=100");
    }

    #[test]
    fn the_caret_points_into_the_file_not_the_joined_text() {
        let path = scratch("caret.q", "# a comment line\ntag:naruto AND kudos:>=100\n");
        let file = read_query_file(&path).expect("reads");
        let offset = file.expression.find("kudos").expect("in the joined text");
        let drawn = caret_in_file(&file, offset);
        let mut lines = drawn.lines();
        assert!(
            lines.next().unwrap_or_default().contains("caret.q:2:"),
            "the location names the file and the line: {drawn}"
        );
        assert_eq!(
            lines.next(),
            Some("tag:naruto AND kudos:>=100"),
            "the reader's own line is reprinted: {drawn}"
        );
        assert!(
            lines.next().unwrap_or_default().trim_end().ends_with('^'),
            "and the caret is under the column: {drawn}"
        );
    }

    #[test]
    fn a_file_with_no_expression_is_refused_by_name() {
        let path = scratch("empty.q", "# only a comment\n\n   \n");
        let failure = read_query_file(&path).expect_err("nothing to send");
        assert_eq!(failure.code, 2);
        assert!(failure.message.contains("empty.q"), "{}", failure.message);
        assert!(failure.message.contains("comment"), "{}", failure.message);
    }

    #[test]
    fn a_name_is_found_in_the_library_and_a_slash_is_a_path() {
        let root = std::env::temp_dir().join(format!("dh-q-lib-{}", std::process::id()));
        let library = root.join("queries");
        std::fs::create_dir_all(&library).expect("library");
        std::fs::write(library.join("weekly.q"), "tag:naruto").expect("write");
        std::fs::write(root.join("local.q"), "tag:local").expect("write");

        // The spelling the row uses: a bare name, found in the library.
        assert_eq!(
            resolve_query_path_in(&library, &root, "weekly").expect("library"),
            library.join("weekly.q")
        );
        assert_eq!(
            resolve_query_path_in(&library, &root, "weekly.q").expect("library"),
            library.join("weekly.q"),
            "the suffix is not a reason to look beside the caller first"
        );
        // A file beside the caller still works when the library has nothing by that name.
        assert_eq!(
            resolve_query_path_in(&library, &root, "local.q").expect("local"),
            root.join("local.q")
        );
        // A slash is a path, taken as given.
        assert_eq!(
            resolve_query_path_in(&library, &root, "sub/weekly").expect("path"),
            root.join("sub/weekly")
        );

        let failure = resolve_query_path_in(&library, &root, "nope").expect_err("missing");
        assert_eq!(failure.code, 2);
        assert!(
            failure.message.contains("weekly"),
            "the refusal lists what the library holds: {}",
            failure.message
        );
    }

    #[tokio::test]
    async fn from_file_and_an_argument_are_refused_together() {
        let (cli, args) = args(&["dh", "q", "tag:naruto", "--from-file", "weekly.q"]);
        let failure = super::run(&cli, &args).await.expect_err("refused");
        assert_eq!(failure.code, 2);
        assert!(
            failure.message.contains("--from-file"),
            "{}",
            failure.message
        );
        assert!(
            failure.message.contains("silently ignored"),
            "{}",
            failure.message
        );
    }

    #[test]
    fn the_caret_sits_under_the_character_the_parser_stopped_at() {
        assert_eq!(caret("kudos>=100", 5), "kudos>=100\n     ^");
    }

    #[test]
    fn the_caret_counts_characters_not_bytes() {
        // `é` is two bytes and one column. A byte offset would put the caret one place too far
        // right, on the character after the mistake, for every query with an accent in a tag.
        let expression = "tag:café AND kudos>=1";
        assert_eq!(
            caret(expression, 13),
            format!("{expression}\n             ^")
        );
        assert_ne!(
            caret(expression, 13),
            caret(expression, 13 + 1),
            "the caret moves one column per character"
        );
    }

    #[test]
    fn the_plan_renders_the_engine_the_statement_the_path_and_the_indexes() {
        let body = serde_json::json!({
            "explain": {
                "engine": "sqlite",
                "sql": "SELECT works.id FROM works",
                "plan": ["SCAN works", "SEARCH units USING INDEX idx_units_work (work_id=?)"],
                "indexes": ["idx_units_work"],
            }
        });
        let rendered = render_plan(&body).expect("renders");
        assert!(rendered.contains("engine: sqlite"), "{rendered}");
        assert!(rendered.contains("  SCAN works"), "{rendered}");
        assert!(
            rendered.contains("idx_units_work (work_id=?)"),
            "{rendered}"
        );
        assert!(rendered.contains("indexes: idx_units_work"), "{rendered}");
    }

    #[test]
    fn a_scan_says_it_named_no_index_rather_than_printing_an_empty_list() {
        let body = serde_json::json!({
            "explain": { "engine": "sqlite", "sql": "SELECT 1", "plan": ["SCAN works"], "indexes": [] }
        });
        let rendered = render_plan(&body).expect("renders");
        assert!(rendered.contains("indexes: none"), "{rendered}");
    }

    #[test]
    fn a_body_without_an_explain_block_is_an_environment_failure_not_a_blank_plan() {
        let error = render_plan(&serde_json::json!({ "items": [] })).unwrap_err();
        assert_eq!(error.code, 1, "an old server is not a usage error");
        assert!(error.message.contains("explain"), "{}", error.message);
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
