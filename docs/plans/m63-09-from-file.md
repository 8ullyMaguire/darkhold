# M63-09 — `dh q --from-file`: the query library is a file, not browser history

Status: implementing. Row: `M63-09` (power-user). Idea: `docs/ideas/2026-11-01-idea-sweep-power-users.md`
P9 — "a user's query library is versioned, reviewable and shareable, in the same shape as the
instance's own config".

## What exists, measured first

`dh q` takes its expression as positional words joined with a space (`QArgs.expression: Vec<String>`,
`num_args = 1.., required = true`; `run` does `args.expression.join(" ")`). A caret renderer already
exists from `M63-08`: on a `422` carrying `error.query.expression` and `error.query.offset` it draws `^`
under the character the server's parser stopped at, counting **chars** not bytes (`é` is two bytes and
one column). That renderer is the part worth extending rather than duplicating: a file that answers
"offset 41" is worse than the shell's own syntax error.

Nothing in the tree reads `.darkhold/` today — `grep -rn "\.darkhold" crates/ docs/` finds only the
write-probes in `health.rs`/`doctor.rs` and the idea text itself — so this row creates the convention.

## Decisions, with the rejected alternative named

1. **The file is read, comments stripped, lines joined with one space — and the offset map is kept.**
   A `.q` file worth versioning is worth commenting:
   ```
   # weekly scan: naruto, updated this week, at least 100 kudos
   tag:naruto AND updated>=-7d
   AND kudos:>=100
   ```
   The server only ever sees one line, so a `422` offset indexes the *joined* text; without a map the
   caret points at a character in a string the user never typed. `read_query_file` therefore returns
   the expression **and** one span per source line. Rejected: sending the file's raw text (newlines are
   whitespace to the parser but make the offset unreportable); rejected: drawing the caret over the
   joined text (the reader then cannot find the line in the file).
2. **A bare name means `.darkhold/queries/<name>.q`; anything with a `/` or a `.q` suffix is a path.**
   The row names the directory; a literal path is the obvious general case and costs one `match`. A
   missing file lists the `.q` files that do exist, because "no such file" after a typo in a directory
   you cannot see is the least useful refusal in the tool.
3. **`#` starts a comment only at the start of a line or after whitespace**, so a tag spelled `#angst`
   survives. There is no escape rule to inherit — the server's grammar has quotes but no escapes — and
   inventing one here would make the file a second language.
4. **`--from-file` and a positional expression are refused together.** They are two sources for one
   string; silently preferring either makes the other a lie. Also refused: an empty file (naming it),
   and a file whose every line is blank or a comment (naming the line count).
5. **Not in scope: writing a query into the directory.** The shell writes files; a `--write-file` verb
   would need overwrite semantics, a directory creator and a naming rule — three decisions belonging to
   a row that asks for them. Recorded so the next session knows it was considered and left.

## Steps

### Step 1 — the reader and the map, tests first

`crates/cli/src/q.rs`:

```rust
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
        lines.push(QueryLine {
            number: index + 1,
            text: text.to_owned(),
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
```

Note the ordering in the loop: `start` is recorded **after** the joining space is pushed, so the map
is an index into the string the server sees. Getting that backwards shifts every line by one char
after the first, which is exactly what `an_offset_points_at_the_line_and_column_it_came_from` pins.

Tests:

```rust
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
```

### Step 2 — the flags

```rust
    /// Read the expression from a file. A bare name is looked for at
    /// `.darkhold/queries/<name>.q`; anything with a `/` or ending in `.q` is a path.
    ///
    /// `#` starts a comment at the start of a line or after whitespace. Blank lines are skipped.
    #[arg(long, value_name = "FILE")]
    pub from_file: Option<String>,
```

and `expression` loses `required = true`, becoming `#[arg(value_name = "EXPR", num_args = 0..)]`.

### Step 3 — `run` reads whichever source was named

```rust
    let query = match (args.from_file.as_deref(), args.expression.is_empty()) {
        (Some(_), false) => {
            return Err(bad_request(
                "--from-file reads the expression from a file; passing one as an argument too would \
                 make one of them silently ignored",
            ))
        }
        (Some(name), true) => Some(read_query_file(&resolve_query_path(name)?)?),
        (None, _) => None,
    };
    let expression = match &query {
        Some(file) => file.expression.clone(),
        None => args.expression.join(" "),
    };
```

and the `422` arm points at the file when there is one:

```rust
        if status == 422 {
            match (&query, body["error"]["query"]["offset"].as_u64()) {
                (Some(file), Some(offset)) => eprintln!("{}", caret_in_file(file, offset as usize)),
                (None, Some(offset)) => {
                    if let Some(at_error) = body["error"]["query"]["expression"].as_str() {
                        eprintln!("{}", caret(at_error, offset as usize));
                    }
                }
                (_, None) => {}
            }
        }
```

`resolve_query_path` and `caret_in_file`:

```rust
/// Where a `--from-file` name points: the literal path, or the query library.
fn resolve_query_path(name: &str) -> Result<PathBuf, Failure> {
    let literal = Path::new(name);
    if name.contains('/') || literal.extension().is_some_and(|ext| ext == "q") {
        return Ok(literal.to_path_buf());
    }
    let dir = Path::new(".darkhold/queries");
    let candidate = dir.join(format!("{name}.q"));
    if candidate.is_file() {
        return Ok(candidate);
    }
    let mut available: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let path = entry.path();
                    path.extension()
                        .is_some_and(|ext| ext == "q")
                        .then(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned())
                })
                .collect()
        })
        .unwrap_or_default();
    available.sort();
    Err(bad_request(format!(
        "{}: no such query — {} holds {}",
        candidate.display(),
        dir.display(),
        match available.is_empty() {
            true => "no .q files (create the directory and put one there)".to_owned(),
            false => available.join(", "),
        }
    )))
}

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
```

### Step 3b — two deviations the live probe forced, recorded rather than smoothed over

1. **Resolution order.** Decision 2 as first written treated a `.q` suffix as "a literal path", so
   `dh q --from-file weekly.q` answered `weekly.q: No such file or directory` while the file sat in
   the library one directory down — and that spelling is the one the row itself uses. The rule is now:
   a slash is a path; otherwise the library is searched first (adding `.q` when absent), then the
   literal path beside the caller, then the refusal, which lists what the library holds.
   `resolve_query_path_in(library, cwd, name)` takes both directories so the rules are tested without
   creating `.darkhold/` inside the crate.
2. **`dh q` with no expression is refused by `run`, not by clap.** `required = true` had to go for
   `--from-file` to exist, so `q_requires_an_expression` now asserts the runtime refusal (exit `2`,
   naming the empty expression) instead of a parse error. The contract is unchanged — a bare `dh q`
   is still refused with the code that says "fix the command".

### Step 4 — verification

```bash
cargo test -p darkhold-cli --bin dh
cargo test -p darkhold-cli
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/gate.sh ledger && bash scripts/gate.sh repo
```

Mutation proof, run and restored: cutting `drop_comment` at the first `#` anywhere made
`a_hash_inside_a_value_is_not_a_comment` FAILED (23 passed, 1 failed); making `locate` answer the first
stored line regardless of `start` made `an_offset_points_at_the_line_and_column_it_came_from` FAILED
(23 passed, 1 failed). Both back to 82 passed after restore.

Note for the next reader: the caret test alone cannot catch the `locate` mutation, because first-stored
line and second line carry the same text in it — the map test with three distinct lines is the one that
guards the mapping.

Live probe (scratch sqlite on :8099, a couple of seeded works):
```
.darkhold/queries/weekly.q   two lines plus a comment
dh q --from-file weekly.q                        → same rows as the joined one-liner
dh q --from-file weekly.q --jsonl --fields title → works
dh q --from-file weekly.q 'tag:x'                → exit 2, "silently ignored"
dh q --from-file nope                            → exit 2, names the directory and its files
dh q --from-file empty.q                         → exit 2, names the file
dh q --from-file broken.q                        → 422 plus `broken.q:1:<col>` and a caret
```

### Step 5 — the record

Ledger row via `scripts/ledger.py`; `backlog.py` and `ideas_catalogue.py`; README's `dh q` paragraph
gains the flag and the comment rule; STANDING-PROMPT's state block re-derived.
