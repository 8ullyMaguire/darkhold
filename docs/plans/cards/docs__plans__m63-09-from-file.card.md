# docs/plans/m63-09-from-file.md — plan card

Source: `docs/plans/m63-09-from-file.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Status: implementing. Row: `M63-09` (power-user). Idea: `docs/ideas/2026-11-01-idea-sweep-power-users.md`
Nothing in the tree reads `.darkhold/` today — `grep -rn "\.darkhold" crates/ docs/` finds only the
write-probes in `health.rs`/`doctor.rs` and the idea text itself — so this row creates the convention.
`crates/cli/src/q.rs`:
bash scripts/gate.sh ledger && bash scripts/gate.sh repo
Ledger row via `scripts/ledger.py`; `backlog.py` and `ideas_catalogue.py`; README's `dh q` paragraph

## Copy-pasteable code / commands

   # weekly scan: naruto, updated this week, at least 100 kudos
   tag:naruto AND updated>=-7d
   AND kudos:>=100
   

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
    /// Offsets that fall in the spaces *between* lines belong to the earlier line (its end),
    /// which is where a reader looks when a parser complains at a join.
    fn locate(&self, offset: usize) -> Option<(usize, usize, &str)> {
        let line = self.lines.iter().rev().find(|line| offset >= line.start)?;
        let column = offset - line.start;
        Some((line.number, column + 1, line.text.as_str()))
    }
}

/// Cut a `#` comment: only at the start of the line or after whitespace.
///
/// Byte indices are safe because both `#` and the whitespace classes are ASCII, and a multi-byte
/// character earlier in the line only ever shifts later indices *past* themselves — the slice point
/// is always the offset of an ASCII byte, which is a char boundary.
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
        let text = drop_comment(line).trim_end();
        if text.trim().is_empty() {
            continue;
        }
        if !expression.is_empty() {
            expression.push(' ');
        }

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
        let path = scratch("weekly.q", "# weekly scan\ntag:naruto AND updated>=-7d\n  AND kudos:>=100   # at least a hundred\n");
        let file = read_query_file(&path).expect("reads");
        assert_eq!(file.expression, "tag:naruto AND updated>=-7d AND kudos:>=100");
        assert_eq!(file.lines.len(), 2, "two lines carry text");
        assert_eq!(file.lines[1].number, 3, "the third line of the file");
    }

    #[test]
    fn a_hash_inside_a_value_is_not_a_comment() {
        assert_eq!(drop_comment("tag:#angst AND tag:fluff"), "tag:#angst AND tag:fluff");
        assert_eq!(drop_comment("tag:x  # why"), "tag:x  ");
        assert_eq!(drop_comment("# whole line"), "");
    }

    #[test]
    fn an_offset_points_at_the_line_and_column_it_came_from() {
        let path = scratch("two.q", "tag:naruto AND updated>=-7d\nAND kudos:>=100\n");
        let file = read_query_file(&path).expect("reads");
        let offset = file.expression.find("AND kudos").expect("present");
        let (line, column, text) = file.locate(offset).expect("inside the expression");
        assert_eq!((line, column), (2, 1));
        assert_eq!(text, "AND kudos:>=100");
    }

    #[test]
    fn a_file_with_no_expression_is_refused_by_name() { /* names the path and "comment" */ }

## Verification command and expected output

    #[test]
    #[test]
    #[test]
    #[test]
   `--from-file` to exist, so `q_requires_an_expression` now asserts the runtime refusal (exit `2`,
cargo test -p darkhold-cli --bin dh
cargo test -p darkhold-cli
bash scripts/gate.sh ledger && bash scripts/gate.sh repo
Note for the next reader: the caret test alone cannot catch the `locate` mutation, because first-stored
line and second line carry the same text in it — the map test with three distinct lines is the one that
dh q --from-file weekly.q 'tag:x'                → exit 2, "silently ignored"
dh q --from-file nope                            → exit 2, names the directory and its files
dh q --from-file empty.q                         → exit 2, names the file

## RED/GREEN evidence

`a_hash_inside_a_value_is_not_a_comment` FAILED (23 passed, 1 failed); making `locate` answer the first
stored line regardless of `start` made `an_offset_points_at_the_line_and_column_it_came_from` FAILED
(23 passed, 1 failed). Both back to 82 passed after restore.
