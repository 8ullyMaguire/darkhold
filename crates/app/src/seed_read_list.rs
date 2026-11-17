//! `darkhold seed read-list` — seed private ratings from the operator's
//! Calibre export, through the same write function the rating route calls.
//!
//! Three rules keep this command honest:
//!
//! 1. **Dry-run is the default.** Parsing and matching read the database, but
//!    nothing is written unless `--apply` is passed.
//! 2. **No work is invented.** Matches come from an existing materialised
//!    library item (external id + source, else normalised title + author); a
//!    title with more than one distinct work id is reported, never guessed.
//! 3. **Every row carries provenance.** `reading::upsert_rating` receives
//!    `Some("seed.read_list")`, so a seeded row is distinguishable from a
//!    human one with one query and no side table.
//!
//! The exit codes are local to the `darkhold` binary and are the ones named in
//! the row 407 brief: 0 clean, 2 unreadable file, 3 unmatched rows, 4 apply
//! without an account/pseud, 5 a refused write. (`docs/cli-exit-codes.md`
//! documents the *other* binary, `dh`; its table must not be renumbered.)

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::process::ExitCode;

use anyhow::Result;
use darkhold_db::{imports, reading, Database};
use darkhold_domain::{AccountId, PseudId, WorkId};

use crate::cli::ReadListArgs;
use crate::config::Config;

/// The value stamped into `rating.source` on every row this command writes.
pub const PROVENANCE: &str = "seed.read_list";
/// The source of the ids in a Calibre export. No catalogue row uses it today,
/// so the external-id strategy honestly reports zero hits on the live data.
const LIST_SOURCE: &str = "calibre";

pub const EXIT_OK: u8 = 0;
pub const EXIT_UNREADABLE_FILE: u8 = 2;
pub const EXIT_UNMATCHED: u8 = 3;
pub const EXIT_MISSING_IDENTITY: u8 = 4;
pub const EXIT_WRITE_REFUSED: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strategy {
    ExternalIdSource,
    TitleAuthor,
    Unmatched,
}

impl Strategy {
    fn label(self) -> &'static str {
        match self {
            Self::ExternalIdSource => "external_id_source",
            Self::TitleAuthor => "title_author",
            Self::Unmatched => "unmatched",
        }
    }
}

#[derive(Debug, Clone)]
struct ParsedRow {
    line: usize,
    external_id: String,
    calibre_rating: i64,
    title: String,
    author: String,
    stars: i64,
    recovered: bool,
    strategy: Strategy,
    work_id: Option<WorkId>,
}

#[derive(Debug, Default)]
struct ParseOutcome {
    lines: usize,
    rows: Vec<ParsedRow>,
    unmatched_lines: Vec<(usize, String)>,
    skipped_unrated: usize,
    recovered: usize,
    before: BTreeMap<i64, usize>,
    after: BTreeMap<i64, usize>,
}

/// Calibre stores stars × 2 on a 0–10 field. Zero means "never rated" and is
/// dropped; every other value lands in 1..=5, the range the route enforces.
fn stars_from_calibre(value: i64) -> Option<i64> {
    if value <= 0 {
        return None;
    }
    Some(((f64::from(value as i32) / 2.0).round() as i64).clamp(1, 5))
}

/// Case-fold, drop punctuation and squash runs of whitespace. Deliberately
/// conservative: it never claims that two different ids name the same work,
/// and it never edits the stored title.
fn normalize(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    out.trim().to_owned()
}

/// Parse the export, joining a wrapped line to the row it continues before
/// retrying the roadmap's rule. A continuation is consumed only when the join
/// actually matches, so an orphan continuation stays in the unmatched report.
fn parse(text: &str) -> ParseOutcome {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = ParseOutcome {
        lines: lines.len(),
        ..ParseOutcome::default()
    };
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let mut candidate = line.to_owned();
        let mut recovered = false;
        if row_match(&candidate).is_none() && i + 1 < lines.len() {
            let next = lines[i + 1];
            let indent = next.len() - next.trim_start().len();
            let leading_id = next
                .trim_start()
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit());
            if indent >= 11 && !leading_id {
                // Keep the continuation's indentation: the roadmap's rule needs
                // two spaces between title and author to recognise the row.
                candidate = format!("{}{}", line.trim_end(), next);
                recovered = row_match(&candidate).is_some();
                if recovered {
                    i += 1;
                }
            }
        }
        if let Some((id, rating, title, author)) = row_match(&candidate) {
            let calibre = rating.parse::<i64>().unwrap_or(0);
            let Some(stars) = stars_from_calibre(calibre) else {
                out.skipped_unrated += 1;
                i += 1;
                continue;
            };
            if recovered {
                out.recovered += 1;
            }
            *out.before.entry(calibre).or_default() += 1;
            *out.after.entry(stars).or_default() += 1;
            out.rows.push(ParsedRow {
                line: i + 1, // 1-based line of the row's first physical line
                external_id: id.to_owned(),
                calibre_rating: calibre,
                title: title.to_owned(),
                author: author.to_owned(),
                stars,
                recovered,
                strategy: Strategy::Unmatched,
                work_id: None,
            });
        } else {
            out.unmatched_lines.push((i + 1, line.trim().to_owned()));
        }
        i += 1;
    }
    out
}

/// The roadmap's rule, without pulling in a regex crate:
/// `^(\d+)\s+(\d{1,2})\s+(\S.*?)\s{2,}(\S.*)$`.
///
/// Returns (id, rating, title, author). A separator is two or more spaces;
/// a single space inside a title is not a separator.
fn row_match(line: &str) -> Option<(&str, &str, &str, &str)> {
    let s = line.trim_start();
    let (id, _) = split_number(s)?;
    let remainder = &s[id.len()..];
    if !remainder.starts_with(char::is_whitespace) {
        return None; // no whitespace after the id
    }
    let rest = remainder.trim_start();
    let (rating, tail) = split_number(rest)?;
    if rating.is_empty() || rating.len() > 2 {
        return None;
    }
    let after = tail.trim_start();
    if after.len() == tail.len() || after.is_empty() {
        return None;
    }
    let separator = after.find("  ")?;
    let title = after[..separator].trim();
    let author = after[separator..].trim();
    if title.is_empty() || author.is_empty() {
        return None;
    }
    Some((id, rating, title, author))
}

fn split_number(value: &str) -> Option<(&str, &str)> {
    let end = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    if end == 0 {
        return None;
    }
    Some((&value[..end], &value[end..]))
}

#[derive(Default)]
struct Catalogue {
    by_external: HashMap<(String, String), BTreeSet<String>>,
    by_title_author: HashMap<(String, String), BTreeSet<String>>,
}

impl Catalogue {
    fn new(items: Vec<imports::MaterialisedLibraryItem>) -> Self {
        let mut catalog = Self::default();
        for item in items {
            catalog
                .by_external
                .entry((item.source_key, item.source_work_key))
                .or_default()
                .insert(item.work_id.clone());
            catalog
                .by_title_author
                .entry((normalize(&item.title), normalize(&item.author_text)))
                .or_default()
                .insert(item.work_id);
        }
        catalog
    }

    fn resolve(&self, row: &ParsedRow) -> (Strategy, Option<WorkId>) {
        if let Some(ids) = self
            .by_external
            .get(&(LIST_SOURCE.to_owned(), row.external_id.clone()))
        {
            // The table's primary key is (scheme, value): one source id names
            // one work. First hit wins, and it is never a fuzzy match.
            if let Some(id) = ids.iter().next() {
                if let Ok(work) = id.parse::<WorkId>() {
                    return (Strategy::ExternalIdSource, Some(work));
                }
            }
        }
        let key = (normalize(&row.title), normalize(&row.author));
        if let Some(ids) = self.by_title_author.get(&key) {
            if ids.len() == 1 {
                if let Some(id) = ids.iter().next() {
                    if let Ok(work) = id.parse::<WorkId>() {
                        return (Strategy::TitleAuthor, Some(work));
                    }
                }
            }
        }
        (Strategy::Unmatched, None)
    }
}

#[derive(Debug, Default)]
struct ApplyStats {
    created: usize,
    refreshed: usize,
    skipped_existing: usize,
}

/// Render the whole report. The matched table is printed in full: a report
/// that hides rows is not a report an operator can audit.
fn render(
    args: &ReadListArgs,
    outcome: &ParseOutcome,
    strategies: &BTreeMap<&'static str, usize>,
    applied: Option<&ApplyStats>,
) {
    println!(
        "seed read-list — {}",
        if args.apply {
            "apply"
        } else {
            "dry-run (no writes)"
        }
    );
    println!("file                 {}", args.file.display());
    println!("lines                {}", outcome.lines);
    println!("parsed               {}", outcome.rows.len());
    println!("recovered            {}", outcome.recovered);
    println!("unmatched lines      {}", outcome.unmatched_lines.len());
    println!("skipped (0 / None)   {}", outcome.skipped_unrated);
    println!(
        "distinct ids         {}",
        outcome
            .rows
            .iter()
            .map(|row| row.external_id.as_str())
            .collect::<BTreeSet<_>>()
            .len()
    );
    for (name, count) in strategies {
        println!("strategy {name:<22} {count}");
    }
    println!("histogram before     {}", histogram(&outcome.before));
    println!("histogram after      {}", histogram(&outcome.after));
    if let Some(stats) = applied {
        println!("created              {}", stats.created);
        println!("refreshed            {}", stats.refreshed);
        println!("skipped existing     {}", stats.skipped_existing);
    } else {
        println!(
            "would write          {}",
            outcome.rows.iter().filter(|r| r.work_id.is_some()).count()
        );
    }
    println!("\nmatched table");
    println!(
        "{:<6} {:<9} {:<6} {:<6} {:<20} {:<36} title — author",
        "line", "calibre", "stars", "wrap", "strategy", "work_id"
    );
    for row in outcome.rows.iter().filter(|r| r.work_id.is_some()) {
        println!(
            "{:<6} {:<9} {:<6} {:<6} {:<20} {:<36} {} — {}",
            row.line,
            row.calibre_rating,
            row.stars,
            if row.recovered { "yes" } else { "-" },
            row.strategy.label(),
            row.work_id
                .map_or_else(|| "-".to_owned(), |id| id.to_string()),
            row.title,
            row.author
        );
    }
    println!("\nunmatched report");
    for (line, text) in &outcome.unmatched_lines {
        println!("  line {line:>3}: {text}");
    }
    let unmatched_rows = outcome.rows.iter().filter(|r| r.work_id.is_none()).count();
    println!("\nunmatched rows       {unmatched_rows}");
}

fn histogram(counts: &BTreeMap<i64, usize>) -> String {
    counts
        .iter()
        .rev()
        .map(|(value, count)| format!("{value}:{count}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parse, report, and — only with `--apply` — write seeded ratings.
pub async fn run(config: &Config, args: &ReadListArgs) -> Result<ExitCode> {
    let text = match std::fs::read_to_string(&args.file) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("error: cannot read {}: {error}", args.file.display());
            return Ok(ExitCode::from(EXIT_UNREADABLE_FILE));
        }
    };

    let db = crate::connect(config).await?;
    let result = seed(&db, args, &text).await;
    db.close().await;
    result
}

async fn seed(db: &Database, args: &ReadListArgs, text: &str) -> Result<ExitCode> {
    let mut outcome = parse(text);
    let catalog = Catalogue::new(imports::materialised_library_items(db).await?);
    for row in &mut outcome.rows {
        let (strategy, work) = catalog.resolve(row);
        row.strategy = strategy;
        row.work_id = work;
    }
    let mut strategies = BTreeMap::new();
    for row in &outcome.rows {
        *strategies.entry(row.strategy.label()).or_default() += 1;
    }

    let applied = if args.apply {
        let (Some(account_raw), Some(pseud_raw)) = (&args.account, &args.pseud) else {
            eprintln!("error: --apply requires --account and --pseud");
            return Ok(ExitCode::from(EXIT_MISSING_IDENTITY));
        };
        let account = match parse_id::<AccountId>(account_raw, "--account") {
            Ok(id) => id,
            Err(code) => return Ok(ExitCode::from(code)),
        };
        let pseud = match parse_id::<PseudId>(pseud_raw, "--pseud") {
            Ok(id) => id,
            Err(code) => return Ok(ExitCode::from(code)),
        };
        let stats = apply(db, &outcome, account, pseud).await?;
        match stats {
            Ok(stats) => Some(stats),
            Err(()) => return Ok(ExitCode::from(EXIT_WRITE_REFUSED)),
        }
    } else {
        None
    };

    render(args, &outcome, &strategies, applied.as_ref());
    let unmatched_rows = outcome.rows.iter().filter(|r| r.work_id.is_none()).count();
    if args.apply && unmatched_rows > 0 {
        eprintln!("{unmatched_rows} rows unmatched; no rating was written for them");
        return Ok(ExitCode::from(EXIT_UNMATCHED));
    }
    Ok(ExitCode::SUCCESS)
}

fn parse_id<T: std::str::FromStr>(raw: &str, flag: &str) -> std::result::Result<T, u8> {
    raw.parse::<T>().map_err(|_| {
        eprintln!("error: {flag} must be a UUID, got {raw:?}");
        EXIT_MISSING_IDENTITY
    })
}

/// Write only matched rows. A human rating (NULL source) is never rewritten,
/// and a seeded row with the same stars needs no write. A seeded row whose
/// stars changed in the export is refreshed through the same upsert, so the
/// CLI is idempotent without baking a stale value in forever.
async fn apply(
    db: &Database,
    outcome: &ParseOutcome,
    account: AccountId,
    pseud: PseudId,
) -> Result<std::result::Result<ApplyStats, ()>> {
    let mut stats = ApplyStats::default();
    for row in outcome.rows.iter().filter(|r| r.work_id.is_some()) {
        let work = row.work_id.expect("filtered");
        let existing = reading::rating_for(db, pseud, work).await?;
        let source = reading::rating_source_for(db, pseud, work).await?;
        if let Some(rating) = &existing {
            if source.as_deref() != Some(PROVENANCE) || rating.stars == row.stars {
                stats.skipped_existing += 1;
                continue;
            }
        }
        let result =
            reading::upsert_rating(db, account, pseud, work, row.stars, false, Some(PROVENANCE))
                .await;
        match result {
            Ok(_) => {
                if existing.is_some() {
                    stats.refreshed += 1;
                } else {
                    stats.created += 1;
                }
            }
            Err(error) => {
                eprintln!(
                    "error: write refused for work {} (line {}): {error:#}",
                    work, row.line
                );
                return Ok(Err(()));
            }
        }
    }
    Ok(Ok(stats))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibre_zero_is_skipped_and_every_other_value_maps_into_one_to_five() {
        assert_eq!(stars_from_calibre(0), None);
        assert_eq!(stars_from_calibre(1), Some(1));
        assert_eq!(stars_from_calibre(5), Some(3));
        assert_eq!(stars_from_calibre(9), Some(5));
        assert_eq!(stars_from_calibre(10), Some(5));
    }

    #[test]
    fn a_wrapped_title_is_recovered_and_an_unrated_row_is_not() {
        let text = "header\n2398  7    Shut up and Sith: or how I learned to keep calm and UNLIMITED POWAH! - A SWTOR Karp\n           Gamer Story\n228   None The Lust Reaper                                                                Golork\n";
        let out = parse(text);
        assert_eq!(out.recovered, 1, "one wrapped row recovered");
        assert_eq!(out.skipped_unrated, 1, "a None rating is not a rating");
        assert_eq!(out.rows.len(), 1);
        assert_eq!(out.rows[0].author, "Gamer Story");
        assert_eq!(out.rows[0].stars, 4);
        assert!(out.unmatched_lines.iter().any(|(_, line)| line == "header"));
    }
}
