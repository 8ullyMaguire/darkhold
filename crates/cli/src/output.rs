//! Output shape is a contract (`M63-05`).
//!
//! A script that reads this program breaks in one of two ways: quietly (a column
//! moved, and the field it wanted is now the title) or loudly (a number became a
//! string, and `jq` refused). Both are the same defect — the output was never a
//! contract, only whatever the printer happened to produce that week.
//!
//! So there are exactly three shapes, and each is chosen explicitly:
//!
//! | shape | for | stability promise |
//! |---|---|---|
//! | `--table` (default) | a person at a terminal | the layout may change between versions |
//! | `--jsonl` | a pipeline | one object per row; **keys never disappear**; order is fixed |
//! | `--link md\|url` | pasting, or opening | one link per row, nothing else |
//!
//! Two rules make the JSONL shape worth trusting:
//!
//! * **A field is always present.** A row with no word count prints `"words":null`
//!   rather than omitting the key, because `jq -r '.words'` on a missing key is
//!   `null` too — but `has("words")` is not, and neither is a column count.
//! * **Order is written here, not inherited.** `serde_json` sorts object keys
//!   alphabetically (no `preserve_order` in this workspace), which would put
//!   `author` before `title` and make a diff of two runs read like a different
//!   record. The line is built from this file's column order instead.
//!
//! `--cols` and `--fields` are the same idea on the two shapes: an explicit
//! projection. The default table is byte-for-byte what this program printed
//! before the flags existed — there is a test that holds the two renderers to
//! that, so a projection change cannot quietly move a column under a script.

use serde_json::Value;

use crate::rec::{self, Row};

/// One nameable thing in a row.
///
/// The table's vocabulary and the JSON vocabulary overlap but are not the same:
/// `measured` and `shared` are facts a pipeline wants and a table cannot show as
/// a column, so `--cols` refuses them and `--fields` accepts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Rank,
    Score,
    Likes,
    Words,
    Status,
    Updated,
    Title,
    Author,
    Source,
    /// `false` when the metric could not measure the row. JSON only.
    Measured,
    /// The tokens that carried a seed search's similarity. JSON only.
    Shared,
}

/// The table's columns, in the order this program has always printed them.
pub const TABLE_COLUMNS: [Column; 9] = [
    Column::Rank,
    Column::Score,
    Column::Likes,
    Column::Words,
    Column::Status,
    Column::Updated,
    Column::Title,
    Column::Author,
    Column::Source,
];

/// The JSONL columns, in the order this file promises.
///
/// `rank` first and `title` second because that is what a person reads in `jq`
/// output; `measured` and `shared` last because they are the rare keys.
pub const JSON_COLUMNS: [Column; 11] = [
    Column::Rank,
    Column::Title,
    Column::Author,
    Column::Source,
    Column::Likes,
    Column::Words,
    Column::Status,
    Column::Updated,
    Column::Score,
    Column::Measured,
    Column::Shared,
];

impl Column {
    /// The spelling a user types and a script reads.
    pub fn name(self) -> &'static str {
        match self {
            Column::Rank => "rank",
            Column::Score => "score",
            Column::Likes => "likes",
            Column::Words => "words",
            Column::Status => "status",
            Column::Updated => "updated",
            Column::Title => "title",
            Column::Author => "author",
            Column::Source => "source",
            Column::Measured => "measured",
            Column::Shared => "shared",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        Some(match raw.trim() {
            "rank" => Column::Rank,
            "score" => Column::Score,
            "likes" => Column::Likes,
            "words" => Column::Words,
            "status" => Column::Status,
            "updated" => Column::Updated,
            "title" => Column::Title,
            "author" => Column::Author,
            "source" => Column::Source,
            "measured" => Column::Measured,
            "shared" => Column::Shared,
            _ => return None,
        })
    }
}

/// A refusal a caller can act on: the vocabulary is in the message.
fn vocabulary(allowed: &[Column]) -> String {
    allowed
        .iter()
        .map(|column| column.name())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Parse a comma-separated projection.
///
/// Refuses an empty projection and a repeat: `--cols title,title` would print the
/// title twice, which is never what anyone meant, and a silent duplicate hides a
/// typo in the column that *was* meant.
pub fn parse_projection(raw: &str, allowed: &[Column]) -> Result<Vec<Column>, String> {
    let mut out: Vec<Column> = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(format!(
                "empty column in `{raw}`; the vocabulary is: {}",
                vocabulary(allowed)
            ));
        }
        let Some(column) = Column::parse(part) else {
            return Err(format!(
                "unknown column `{part}`; the vocabulary is: {}",
                vocabulary(allowed)
            ));
        };
        if !allowed.contains(&column) {
            return Err(format!(
                "`{}` is not a table column; the vocabulary is: {}",
                column.name(),
                vocabulary(allowed)
            ));
        }
        if out.contains(&column) {
            return Err(format!("`{}` appears twice", column.name()));
        }
        out.push(column);
    }
    Ok(out)
}

/// Which kind of link `--link` prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// `- [title](url)` — a markdown list, for pasting into a note.
    Md,
    /// Bare URLs, one per line, for `xargs`.
    Url,
}

impl LinkKind {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim() {
            "md" => Ok(LinkKind::Md),
            "url" => Ok(LinkKind::Url),
            // `epub` is in the row's wording and there is nothing to link to yet:
            // this instance has no download route, so a link would be a 404 with
            // a file extension. Refusing with the reason is better than shipping
            // a link that is wrong, and better than pretending the flag works.
            "epub" => Err("`--link epub` needs a download route this instance does not have yet \
                           (tracked as M63-05b); `md` and `url` work today"
                .to_owned()),
            other => Err(format!(
                "unknown link kind `{other}`; the vocabulary is: md, url"
            )),
        }
    }
}

/// Everything the printer needs, decided once from the flags.
#[derive(Debug, Clone, Default)]
pub struct Shape {
    pub jsonl: bool,
    pub fields: Option<Vec<Column>>,
    pub cols: Option<Vec<Column>>,
    pub wide: bool,
    pub link: Option<LinkKind>,
}

/// Where a row points, for `--link`.
///
/// The source URL is the work's own page on the site it came from, which is what
/// a person pasting a link into a note actually wants. A work that is *also* on
/// this instance gets this instance's reader page, because a local copy can be
/// read without leaving.
fn row_link(item: &Value, base_url: &str) -> Option<String> {
    let text = |key: &str| item.get(key).and_then(Value::as_str).filter(|v| !v.is_empty());
    if let Some(work_key) = text("work_key") {
        if item.get("on_darkhold").and_then(Value::as_bool) == Some(true) {
            return Some(format!("{}/works/{work_key}", base_url.trim_end_matches('/')));
        }
    }
    text("source_url").map(str::to_owned)
}

/// The links, one line per row.
pub fn links(page: &Value, kind: LinkKind, base_url: &str) -> String {
    let mut out = String::new();
    let items = page
        .get("items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for item in items {
        let Some(url) = row_link(item, base_url) else {
            continue;
        };
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("(untitled)");
        match kind {
            LinkKind::Url => out.push_str(&format!("{url}\n")),
            // A `]` in a title ends the markdown text early; escaping it is the
            // difference between a link and a broken paragraph.
            LinkKind::Md => out.push_str(&format!(
                "- [{}]({url})\n",
                title
                    .replace('\\', "\\\\")
                    .replace('[', "\\[")
                    .replace(']', "\\]")
            )),
        }
    }
    out
}

/// One JSON value for a column, written into `out` in this file's order.
fn json_value(column: Column, row: &Row, item: &Value, index: usize, out: &mut String) {
    out.push('"');
    out.push_str(column.name());
    out.push_str("\":");
    match column {
        Column::Rank => out.push_str(&(index + 1).to_string()),
        Column::Score => match row.score {
            Some(score) => out.push_str(&format_score(score)),
            None => out.push_str("null"),
        },
        Column::Likes => out.push_str(&row.likes.to_string()),
        Column::Words => match row.words {
            Some(words) => out.push_str(&words.to_string()),
            None => out.push_str("null"),
        },
        Column::Measured => out.push_str(if row.measured { "true" } else { "false" }),
        Column::Shared => {
            let tokens = row
                .shared
                .iter()
                .map(|token| serde_json::to_string(token).unwrap_or_else(|_| "\"\"".to_owned()))
                .collect::<Vec<_>>()
                .join(",");
            out.push('[');
            out.push_str(&tokens);
            out.push(']');
        }
        Column::Status | Column::Updated | Column::Title | Column::Author | Column::Source => {
            let value = match column {
                Column::Status => &row.status,
                Column::Updated => &row.updated,
                Column::Title => &row.title,
                Column::Author => &row.author,
                Column::Source => &row.source,
                _ => unreachable!("the arm above covers the string columns"),
            };
            out.push_str(
                &serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned()),
            );
        }
    }
    // `item` is read for one thing only: a row whose title the server omitted
    // still has a title here, taken from the raw page. Kept for symmetry with
    // the table, which prints what the server said and nothing more.
    let _ = item;
}

/// A score prints with three decimals, which is the precision the metrics
/// compute at. More would be a claim the number does not support.
fn format_score(score: f64) -> String {
    format!("{score:.3}")
}

/// The JSONL shape: one object per row, keys in [`JSON_COLUMNS`] order or in the
/// order `--fields` asked for.
pub fn jsonl(page: &Value, fields: Option<&[Column]>) -> String {
    let rows = rec::rows(page);
    let items = page
        .get("items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let columns: &[Column] = fields.unwrap_or(&JSON_COLUMNS);
    let mut out = String::new();
    for (index, row) in rows.iter().enumerate() {
        let item = items.get(index).unwrap_or(&Value::Null);
        out.push('{');
        for (position, column) in columns.iter().enumerate() {
            if position > 0 {
                out.push(',');
            }
            json_value(*column, row, item, index, &mut out);
        }
        out.push_str("}\n");
    }
    out
}

/// Pad or truncate to `width`, counting characters rather than bytes.
fn fit(value: &str, width: usize, align: Align, wide: bool, truncate: bool) -> String {
    if wide || width == 0 {
        return value.to_owned();
    }
    let len = value.chars().count();
    if truncate && len >= width {
        return format!(
            "{}…",
            value
                .chars()
                .take(width.saturating_sub(1))
                .collect::<String>()
        );
    }
    match align {
        Align::Left => format!("{value}{}", " ".repeat(width - len)),
        Align::Right => format!("{}{value}", " ".repeat(width - len)),
    }
}

#[derive(Debug, Clone, Copy)]
enum Align {
    Left,
    Right,
}

/// How one column is laid out: its width, its alignment, and the decoration its
/// neighbours would otherwise have to know about.
struct Spec {
    width: usize,
    align: Align,
    prefix: &'static str,
    suffix: &'static str,
    /// Numbers are padded but never truncated: `602.2k` is six characters and
    /// cutting it to `602.2…` is a wrong number rather than a short one.
    numeric: bool,
}

/// The space between two columns.
///
/// It belongs to the boundary, not to either column: the title has no separator
/// of its own because the author's ` by ` carries the space, and a projection
/// that drops the author must not glue the title to the next column.
fn boundary(left: Column, right: Column) -> &'static str {
    match (left, right) {
        // The rank marker borrows one of these two spaces.
        (Column::Rank, Column::Score) => " ",
        // The historical layout has one single-space boundary.
        (Column::Status, Column::Updated) => " ",
        (Column::Updated, Column::Title) => "  ",
        // ` by ` arrives with its own leading space.
        (Column::Title, Column::Author) => "",
        (Column::Author, Column::Source) => " ",
        _ => "  ",
    }
}

fn spec_for(column: Column) -> Spec {
    match column {
        // The rank marker (`~`, unmeasured) is printed inside the rank cell, so
        // the score column can stay a plain right-aligned number.
        Column::Rank => Spec { width: 3, align: Align::Right, prefix: "", suffix: "", numeric: false },
        Column::Score => Spec { width: 6, align: Align::Right, prefix: "", suffix: "", numeric: true },
        Column::Likes => Spec { width: 6, align: Align::Right, prefix: "", suffix: " likes", numeric: true },
        Column::Words => Spec { width: 9, align: Align::Right, prefix: "", suffix: " words", numeric: true },
        Column::Status => Spec { width: 8, align: Align::Left, prefix: "", suffix: "", numeric: false },
        Column::Updated => Spec { width: 0, align: Align::Left, prefix: "", suffix: "", numeric: false },
        Column::Title => Spec { width: 44, align: Align::Left, prefix: "", suffix: "", numeric: false },
        Column::Author => Spec { width: 18, align: Align::Left, prefix: " by ", suffix: "", numeric: false },
        Column::Source => Spec { width: 0, align: Align::Left, prefix: "[", suffix: "]", numeric: false },
        // Not table columns; refused by `parse_projection` before reaching here.
        Column::Measured | Column::Shared => {
            Spec { width: 0, align: Align::Left, prefix: "", suffix: "", numeric: false }
        }
    }
}

/// The table, with a projection and a width policy.
///
/// The default projection produces exactly what [`rec::table`] prints; the test
/// `the_default_projection_is_the_layout_scripts_already_parse` holds the two to
/// the same bytes.
pub fn table(page: &Value, cols: &[Column], wide: bool) -> String {
    let rows = rec::rows(page);
    let requested_score = cols.contains(&Column::Score);
    // The score column appears only on a page that has scores: a column of `—`
    // would suggest every row was measured, which is the one thing a metric page
    // must not imply. `--cols score` on such a page says so instead of printing
    // an empty column.
    let scored = requested_score && rows.iter().any(|row| row.score.is_some());
    let mut out = String::new();
    for (index, row) in rows.iter().enumerate() {
        let marker = if row.measured { ' ' } else { '~' };
        let mut line = String::new();
        let mut previous: Option<Column> = None;
        for column in cols.iter().copied() {
            if column == Column::Score && !scored {
                continue;
            }
            let spec = spec_for(column);
            let value = match column {
                // Unscored rows have no marker: the historical layout prints
                // the rank alone and two spaces, and a table that shifted one
                // column right broke every `awk` in the wild.
                Column::Rank => match scored {
                    true => format!("{:>3}{}", index + 1, marker),
                    false => format!("{:>3}", index + 1),
                },
                Column::Score => row
                    .score
                    .map(format_score)
                    .unwrap_or_else(|| "—".to_owned()),
                Column::Likes => rec::compact(row.likes),
                Column::Words => row.words.map(rec::compact).unwrap_or_else(|| "—".to_owned()),
                Column::Status => row.status.clone(),
                Column::Updated => row.updated.clone(),
                Column::Title => row.title.clone(),
                Column::Author => row.author.clone(),
                Column::Source => row.source.clone(),
                Column::Measured | Column::Shared => String::new(),
            };
            // The rank cell is already padded by its own format; everything else
            // goes through `fit`.
            let cell = match column {
                Column::Rank => value,
                Column::Updated | Column::Source => value,
                // Numbers widen; text truncates.
                _ => fit(&value, spec.width, spec.align, wide, !spec.numeric),
            };
            // The space belongs to the boundary, so it is written *before* the
            // cell it separates — a boundary after the last column would leave a
            // trailing space on every row and shift every cell by one.
            if let Some(left) = previous {
                line.push_str(boundary(left, column));
            }
            line.push_str(spec.prefix);
            line.push_str(&cell);
            line.push_str(spec.suffix);
            previous = Some(column);
        }
        // A projection can end on a column that printed nothing (no date, no
        // source), and trailing spaces on a line are the difference between a
        // diff a person can read and one they cannot.
        while line.ends_with(' ') {
            line.pop();
        }
        line.push('\n');
        // A seed page's shared tokens belong under its row. Dropped when the
        // title column is not shown, because there is nothing to attach them to.
        if !row.shared.is_empty() && cols.contains(&Column::Title) {
            line.push_str(&format!("     shared: {}\n", row.shared.join(", ")));
        }
        out.push_str(&line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn page() -> Value {
        json!({
            "items": [
                {
                    "title": "The Long Night",
                    "author_text": "sable",
                    "source_key": "sb",
                    "likes": 21400,
                    "word_count": 1200,
                    "status": "complete",
                    "updated_at": "2026-10-01T00:00:00Z",
                    "source_url": "https://example.test/works/1"
                },
                {
                    "title": "A & B",
                    "author_text": "quill",
                    "source_key": "ao3",
                    "likes": 7,
                    "status": "wip",
                    "source_updated_at": "2026-09-30T00:00:00Z"
                }
            ]
        })
    }

    fn scored_page() -> Value {
        json!({
            "items": [
                {"title": "T", "author_text": "a", "source_key": "sb", "likes": 1, "score": 0.8123, "measured": true,
                 "updated_at": "2026-10-01T00:00:00Z", "shared": ["watercolor", "landscape"]},
                {"title": "U", "author_text": "b", "source_key": "sb", "likes": 2, "score": 0.5, "measured": false,
                 "updated_at": "2026-10-02T00:00:00Z"}
            ]
        })
    }


    #[test]
    fn the_default_projection_is_the_layout_scripts_already_parse() {
        // Two renderers is one too many, so this test says they agree.
        assert_eq!(table(&page(), &TABLE_COLUMNS, false), rec::table(&page()));
        assert_eq!(
            table(&scored_page(), &TABLE_COLUMNS, false),
            rec::table(&scored_page())
        );
    }

    #[test]
    fn jsonl_has_every_key_on_every_row_in_the_promised_order() {
        let out = jsonl(&page(), None);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        // A row with no word count still carries the key: a pipeline that counts
        // keys must not see two different shapes.
        assert!(lines[1].contains("\"words\":null"), "{}", lines[1]);
        assert!(lines[1].contains("\"author\":\"quill\""), "{}", lines[1]);
        for line in &lines {
            let keys: Vec<&str> = line
                .trim_start_matches('{')
                .trim_end_matches('}')
                .split(',')
                .filter_map(|pair| pair.split(':').next())
                .collect();
            assert_eq!(
                keys,
                vec![
                    "\"rank\"", "\"title\"", "\"author\"", "\"source\"", "\"likes\"", "\"words\"",
                    "\"status\"", "\"updated\"", "\"score\"", "\"measured\"", "\"shared\""
                ],
                "{line}"
            );
        }
    }

    #[test]
    fn jsonl_escapes_a_title_that_would_break_the_line() {
        let out = jsonl(&page(), None);
        assert!(out.contains(r#""title":"A & B""#), "{out}");
        let broken = json!({"items": [{"title": "quote \" and \n newline"}]});
        let line = jsonl(&broken, None);
        assert_eq!(line.lines().count(), 1, "a newline in a title must not split the record");
        assert!(line.contains(r#"\n"#), "{line}");
        // And it is still parseable JSON, which is the whole contract.
        serde_json::from_str::<Value>(line.trim()).expect("a JSONL line is JSON");
    }

    #[test]
    fn fields_project_and_order() {
        let out = jsonl(&page(), Some(&[Column::Title, Column::Likes]));
        let line = out.lines().next().expect("a row");
        assert_eq!(line, r#"{"title":"The Long Night","likes":21400}"#);
    }

    #[test]
    fn an_unknown_column_names_the_vocabulary() {
        let err = parse_projection("titel", &TABLE_COLUMNS).expect_err("refused");
        assert!(err.contains("unknown column `titel`"), "{err}");
        assert!(err.contains("title"), "the vocabulary is in the message: {err}");
    }

    #[test]
    fn a_json_only_field_is_not_a_table_column() {
        let err = parse_projection("measured", &TABLE_COLUMNS).expect_err("refused");
        assert!(err.contains("not a table column"), "{err}");
        // But it is a perfectly good field.
        assert!(parse_projection("measured", &JSON_COLUMNS).is_ok());
    }

    #[test]
    fn a_duplicate_column_is_refused_rather_than_printed_twice() {
        let err = parse_projection("title,title", &TABLE_COLUMNS).expect_err("refused");
        assert!(err.contains("appears twice"), "{err}");
    }

    #[test]
    fn cols_reorders_without_changing_the_default() {
        let out = table(&page(), &[Column::Likes, Column::Title], false);
        let first = out.lines().next().expect("a row");
        assert!(first.trim_start().starts_with("21.4k likes"), "{first}");
        assert!(first.contains("The Long Night"), "{first}");
    }

    #[test]
    fn wide_stops_truncating() {
        let long = json!({"items": [{"title": "x".repeat(80), "author_text": "a", "source_key": "sb",
                                     "likes": 1, "status": "wip", "updated_at": "2026-10-01T00:00:00Z"}]});
        let narrow = table(&long, &TABLE_COLUMNS, false);
        assert!(narrow.contains('…'), "the default truncates: {narrow}");
        let wide = table(&long, &TABLE_COLUMNS, true);
        assert!(!wide.contains('…'), "{wide}");
        assert!(wide.contains(&"x".repeat(80)), "{wide}");
    }

    #[test]
    fn links_prefer_the_source_page_and_use_the_local_reader_page_when_there_is_one() {
        let page = json!({"items": [
            {"title": "A [bracket] title", "source_url": "https://example.test/works/1"},
            {"title": "Local", "work_key": "w-1", "on_darkhold": true, "source_url": "https://elsewhere.test/2"}
        ]});
        let md = links(&page, LinkKind::Md, "https://darkhold.test");
        assert!(md.contains("- [A \\[bracket\\] title](https://example.test/works/1)"), "{md}");
        assert!(md.contains("(https://darkhold.test/works/w-1)"), "a local copy is readable here: {md}");
        let url = links(&page, LinkKind::Url, "https://darkhold.test");
        assert_eq!(
            url,
            "https://example.test/works/1\nhttps://darkhold.test/works/w-1\n"
        );
    }

    #[test]
    fn epub_is_refused_with_the_reason_rather_than_a_broken_link() {
        let err = LinkKind::parse("epub").expect_err("refused");
        assert!(err.contains("M63-05b"), "{err}");
    }
}
