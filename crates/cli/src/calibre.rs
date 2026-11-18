//! M63-30: read Calibre's `metadata.db` as a file handshake.
//!
//! The contract is the seed grammar that `darkhold seed read-list` already parses:
//! `<calibre-id> <rating-0-10>  <title>  <author>`. Calibre keeps its own database; darkhold keeps its own conversion
//! from 0–10 to 1–5 stars. No crate link, RPC link, HTTP route or site write is involved.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::Row;

use crate::rec::{bad_request, environment, Failure};

/// `dh lib --calibre <path> [--out <path>]`
#[derive(Debug, Clone, clap::Args)]
pub struct LibArgs {
    /// Path to Calibre's `metadata.db`. Opened read-only.
    #[arg(long, value_name = "PATH")]
    pub calibre: Option<PathBuf>,

    /// Write the same seed text to this local file. Nothing is sent to a site.
    #[arg(long, value_name = "PATH")]
    pub out: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CalibreRow {
    pub id: i64,
    pub rating: i64,
    pub title: String,
    pub author: String,
}

/// The server seeder splits on two or more spaces; emit exactly that grammar.
#[must_use]
pub fn seed_line(row: &CalibreRow) -> String {
    format!("{}  {}  {}  {}", row.id, row.rating, row.title, row.author)
}

#[must_use]
pub fn render(rows: &[CalibreRow]) -> String {
    rows.iter().map(seed_line).collect::<Vec<_>>().join("\n")
}

/// Read only rated books. Zero in Calibre means "never rated", and stays absent.
pub async fn read(path: &Path) -> Result<Vec<CalibreRow>, Failure> {
    if !path.is_file() {
        return Err(bad_request(
            "the Calibre database does not exist or is not a file",
        ));
    }
    let options = SqliteConnectOptions::new().filename(path).read_only(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|error| {
            bad_request(format!(
                "cannot open the Calibre database read-only: {error}"
            ))
        })?;

    #[derive(sqlx::FromRow)]
    struct Book {
        id: i64,
        title: String,
        rating: i64,
    }

    let books: Vec<Book> = sqlx::query_as::<_, Book>(
        "SELECT id, title, rating FROM books WHERE rating > 0 ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .map_err(|error| {
        bad_request(format!(
            "this database does not look like Calibre metadata: {error}"
        ))
    })?;

    let links: Vec<(i64, String)> =
        sqlx::query("SELECT b.book, a.name FROM books_authors_link b JOIN authors a ON a.id = b.author ORDER BY b.book, a.name")
            .fetch_all(&pool)
            .await
            .map_err(|error| bad_request(format!("cannot read Calibre authors: {error}")))?
            .into_iter()
            .map(|row| (row.get::<i64, _>("book"), row.get::<String, _>("name")))
            .collect();
    pool.close().await;

    let mut by_book: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    for (book, author) in links {
        by_book.entry(book).or_default().push(author);
    }
    Ok(books
        .into_iter()
        .map(|book| CalibreRow {
            id: book.id,
            rating: book.rating,
            title: book.title,
            author: by_book.remove(&book.id).unwrap_or_default().join(" & "),
        })
        .collect())
}

/// Dispatch. `--out` is an explicit local export; the default is a dry-run preview.
pub async fn run(cli: &crate::Cli, args: &LibArgs) -> Result<(), Failure> {
    let Some(path) = &args.calibre else {
        return Err(bad_request(
            "dh lib needs --calibre <path> to a Calibre metadata.db (read-only)",
        ));
    };
    let rows = read(path).await?;
    let text = render(&rows);
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&rows)
                .map_err(|error| environment(format!("json: {error}")))?
        );
    } else if text.is_empty() {
        eprintln!("no rated books in {path:?}");
    } else {
        println!("{text}");
    }
    if let Some(out) = &args.out {
        std::fs::write(out, format!("{text}\n"))
            .map_err(|error| environment(format!("cannot write {out:?}: {error}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seed_grammar_keeps_two_spaces_between_every_field() {
        let row = CalibreRow {
            id: 1,
            rating: 8,
            title: "A Study in Green".to_owned(),
            author: "A. Reader".to_owned(),
        };
        assert_eq!(seed_line(&row), "1  8  A Study in Green  A. Reader");
        assert_eq!(render(&[row]), "1  8  A Study in Green  A. Reader");
    }
}
