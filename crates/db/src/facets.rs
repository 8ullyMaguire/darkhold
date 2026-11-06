//! Facets over the catalogue (M64-10).
//!
//! Length, status, rating and cadence are answered from one scan of the filtered rows rather than four queries, because the
//! four answers must describe **the same set**: a length histogram over a different filter than the rating histogram is a
//! page that argues with itself.
//!
//! The rule that matters more than the buckets: a facet whose column is null on the filtered rows **says so** instead of
//! returning an empty page. An empty histogram is indistinguishable from "nothing here", and on this catalogue it is usually
//! "we never measured it" — `word_count` is unpopulated for most imported works today.

use crate::{Backend, Database, Result};
use serde::Serialize;
use std::collections::BTreeMap;

/// What a facet is allowed to be about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Facet {
    Length,
    Status,
    Rating,
    Cadence,
}

impl Facet {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "length" => Some(Self::Length),
            "status" => Some(Self::Status),
            "rating" => Some(Self::Rating),
            "cadence" => Some(Self::Cadence),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Length => "length",
            Self::Status => "status",
            Self::Rating => "rating",
            Self::Cadence => "cadence",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Bucket {
    /// Machine value, stable across releases — the thing a filter takes.
    pub key: String,
    /// What a reader sees. Never a bare key.
    pub label: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FacetAnswer {
    pub facet: &'static str,
    pub buckets: Vec<Bucket>,
    /// Rows in the filtered set whose value for this facet is known.
    pub known: i64,
    /// Rows whose value is missing. Not "other" — a bucket for null would invite a filter that cannot work.
    pub unknown: i64,
    /// The sentence a page shows instead of an empty histogram. `None` when every row was measured.
    pub when_unknown: Option<String>,
}

/// The filters a facet scan accepts. Deliberately the same two the browse surfaces expose today; a facet is only honest about
/// the set it can name.
#[derive(Debug, Clone, Default)]
pub struct FacetFilter {
    /// Tag or fandom name, as `/tags/{tag}` and `/works/by-fandom/{fandom}` take it. Matched on the canonical entity's
    /// canonical spelling, so a page and its facets resolve the same way.
    pub tag: Option<String>,
    /// Which kind of entity `tag` names — `tag` when absent, because a tag is the common case and a fandom is the same join
    /// with one word different.
    pub tag_kind: Option<String>,
    /// Substring of the title, case-insensitive on both backends through `lower()`.
    pub title_contains: Option<String>,
    /// The same word-count window the search applies (M47-05), so a facet can describe a narrowed catalogue.
    pub min_words: Option<i64>,
    pub max_words: Option<i64>,
    /// The same rating ceiling the search applies. A facet computed without it would count works the search refuses to
    /// return, which is a histogram of a different shelf.
    pub max_rating: Option<String>,
}

/// One row of the scan, before bucketing.
struct Scanned {
    rating: Option<String>,
    lifecycle: Option<String>,
    words: Option<i64>,
    days: Option<i64>,
}

/// Every facet for one filtered set, computed from a single scan.
pub async fn all_facets(
    db: &Database,
    filter: &FacetFilter,
) -> Result<Vec<FacetAnswer>> {
    let rows = scan(db, filter).await?;
    Ok(vec![
        length_facet(&rows),
        status_facet(&rows),
        rating_facet(&rows),
        cadence_facet(&rows),
    ])
}

async fn scan(db: &Database, filter: &FacetFilter) -> Result<Vec<Scanned>> {
    // Tag filters go through `work_tags`/`canonical_entities` rather than a name match on the work: the tag page resolves the
    // same way, and two spellings of "the slow burn tag" is how a facet ends up describing a different set than the page that
    // links it.
    let mut sql = String::from(
        // `CAST(... AS TEXT)` on the dates because one backend stores `timestamptz` and the other text, and this needs only the
        // date prefix. Words live on the revision a unit currently points at — there is no `word_count` on `units`.
        "SELECT w.rating, w.lifecycle, CAST(w.published_at AS TEXT), CAST(w.updated_at AS TEXT), \
                COALESCE(SUM(r.word_count), 0) AS words, COUNT(r.word_count) AS measured \
         FROM works w \
         LEFT JOIN units u ON u.work_id = w.id AND u.deleted_at IS NULL \
         LEFT JOIN revisions r ON r.id = u.current_revision_id \
         WHERE w.visibility = 'public' AND w.deleted_at IS NULL",
    );
    // Parameter positions are assigned in the order they are bound, not by which filter is present: binding the title to `?2`
    // while the tag placeholder `?1` went unused leaves `?2` unbound, and SQLite answers that with no rows rather than an
    // error — an empty catalogue that looks like a measurement.
    let mut next = 1usize;
    if filter.tag.is_some() {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM work_tags wt JOIN canonical_entities n ON n.id = wt.node_id \
              WHERE wt.work_id = w.id AND n.kind = ",
        );
        sql.push_str(&format!("?{next}"));
        sql.push_str(" AND lower(n.canonical) = lower(");
        sql.push_str(&format!("?{}", next + 1));
        sql.push_str("))");
        next += 2;
    }
    if filter.title_contains.is_some() {
        sql.push_str(&format!(" AND lower(w.title) LIKE ?{next}"));
        next += 1;
    }
    if filter.max_rating.is_some() {
        // Rating order, not string order: `mature` outranks `general` alphabetically by accident, and a ceiling built from an
        // accidental order lets explicit works through a `mature` ceiling.
        sql.push_str(&format!(
            " AND CASE lower(w.rating) WHEN 'general' THEN 0 WHEN 'teen' THEN 1 WHEN 'mature' THEN 2 WHEN 'explicit' THEN 3 ELSE 4 END <= ?{next}"
        ));
        next += 1;
    }
    // The word window is on the per-work sum, so it goes in the HAVING clause rather than the WHERE: filtering in WHERE would
    // test individual revisions against a window meant for the whole work.
    let mut having: Vec<String> = Vec::new();
    if filter.min_words.is_some() {
        having.push(format!("COALESCE(SUM(r.word_count), 0) >= ?{next}"));
        next += 1;
    }
    if filter.max_words.is_some() {
        having.push(format!("COALESCE(SUM(r.word_count), 0) <= ?{next}"));
    }
    sql.push_str(" GROUP BY w.id, w.rating, w.lifecycle, w.published_at, w.updated_at");
    if !having.is_empty() {
        sql.push_str(" HAVING ");
        sql.push_str(&having.join(" AND "));
    }

    type Row = (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        i64,
    );

    let raw: Vec<Row> = match db.backend() {
        Backend::Sqlite => {
            let mut query = sqlx::query_as::<_, Row>(&sql);
            if let Some(tag) = filter.tag.as_deref() {
                query = query.bind(
                    filter
                        .tag_kind
                        .clone()
                        .unwrap_or_else(|| "tag".to_owned()),
                );
                query = query.bind(tag);
            }
            if let Some(title) = filter.title_contains.as_deref() {
                query = query.bind(format!("%{}%", title.to_lowercase()));
            }
            // Bind order follows SQL text order — tag, title, rating in WHERE, then the word window in HAVING — because a
            // positional bind that disagrees with its placeholder is the same silent-empty-set bug as before.
            if let Some(ceiling) = filter.max_rating.as_deref() {
                query = query.bind(rating_rank(ceiling));
            }
            if let Some(min) = filter.min_words {
                query = query.bind(min);
            }
            if let Some(max) = filter.max_words {
                query = query.bind(max);
            }
            query
                .fetch_all(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?
        }
        Backend::Postgres => {
            let mut query = sqlx::query_as::<_, Row>(&sql);
            if let Some(tag) = filter.tag.as_deref() {
                query = query.bind(
                    filter
                        .tag_kind
                        .clone()
                        .unwrap_or_else(|| "tag".to_owned()),
                );
                query = query.bind(tag);
            }
            if let Some(title) = filter.title_contains.as_deref() {
                query = query.bind(format!("%{}%", title.to_lowercase()));
            }
            // Bind order follows SQL text order — tag, title, rating in WHERE, then the word window in HAVING — because a
            // positional bind that disagrees with its placeholder is the same silent-empty-set bug as before.
            if let Some(ceiling) = filter.max_rating.as_deref() {
                query = query.bind(rating_rank(ceiling));
            }
            if let Some(min) = filter.min_words {
                query = query.bind(min);
            }
            if let Some(max) = filter.max_words {
                query = query.bind(max);
            }
            query
                .fetch_all(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?
        }
    };

    Ok(raw
        .into_iter()
        .map(|(rating, lifecycle, published_at, updated_at, words, measured)| {
            // `word_count` is unknown for a work whose units carry none. Storing `0` here would be a lie the length facet
            // cannot tell from a 0-word work, which is why the measured count travels with the sum.
            let words = (measured > 0).then_some(words);
            let days = published_at
                .as_deref()
                .and_then(days_since_epoch)
                .zip(updated_at.as_deref().and_then(days_since_epoch))
                .map(|(published, updated)| (updated - published).max(0));
            Scanned {
                rating: rating.filter(|value| !value.trim().is_empty()),
                lifecycle: lifecycle.filter(|value| !value.trim().is_empty()),
                words,
                days,
            }
        })
        .collect())
}

fn tally(buckets: BTreeMap<String, (String, i64)>) -> Vec<Bucket> {
    let mut buckets: Vec<Bucket> = buckets
        .into_iter()
        .map(|(key, (label, count))| Bucket {
            key,
            label,
            count,
        })
        .collect();
    // Largest first, then by label: a facet with a random order is a facet nobody can scan twice.
    buckets.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
    buckets
}

fn answer(
    facet: Facet,
    buckets: Vec<Bucket>,
    known: i64,
    total: i64,
) -> FacetAnswer {
    let unknown = total - known;
    let when_unknown = if unknown > 0 && buckets.is_empty() {
        Some(format!(
            "No work in this view has a known {} yet, so there is nothing to filter on — this is a gap in the catalogue, not an empty shelf.",
            facet.name()
        ))
    } else if unknown > 0 {
        Some(format!(
            "{} of {total} works in this view have no known {}.",
            unknown,
            facet.name()
        ))
    } else {
        None
    };
    FacetAnswer {
        facet: facet.name(),
        buckets,
        known,
        unknown,
        when_unknown,
    }
}

fn length_facet(rows: &[Scanned]) -> FacetAnswer {
    const BANDS: [(&str, &str, i64, i64); 4] = [
        ("under_1k", "Under 1,000 words", 0, 999),
        ("1k_5k", "1,000 – 5,000 words", 1_000, 4_999),
        ("5k_20k", "5,000 – 20,000 words", 5_000, 19_999),
        ("over_20k", "Over 20,000 words", 20_000, i64::MAX),
    ];
    let mut buckets: BTreeMap<String, (String, i64)> = BTreeMap::new();
    let mut known = 0;
    for row in rows {
        if let Some(words) = row.words {
            known += 1;
            let (key, label) = BANDS
                .iter()
                .find(|(_, _, low, high)| words >= *low && words <= *high)
                .map(|(key, label, _, _)| (*key, *label))
                .unwrap_or(("over_20k", "Over 20,000 words"));
            buckets
                .entry(key.to_owned())
                .and_modify(|entry| entry.1 += 1)
                .or_insert((label.to_owned(), 1));
        }
    }
    answer(Facet::Length, tally(buckets), known, rows.len() as i64)
}

fn status_facet(rows: &[Scanned]) -> FacetAnswer {
    let mut buckets: BTreeMap<String, (String, i64)> = BTreeMap::new();
    let mut known = 0;
    for row in rows {
        if let Some(lifecycle) = row.lifecycle.as_deref() {
            known += 1;
            let label = match lifecycle {
                "published" => "Published",
                "ongoing" => "Ongoing",
                "complete" => "Complete",
                "abandoned" => "Abandoned",
                "draft" => "Draft",
                other => other,
            };
            buckets
                .entry(lifecycle.to_owned())
                .and_modify(|entry| entry.1 += 1)
                .or_insert((label.to_owned(), 1));
        }
    }
    answer(Facet::Status, tally(buckets), known, rows.len() as i64)
}

fn rating_facet(rows: &[Scanned]) -> FacetAnswer {
    let mut buckets: BTreeMap<String, (String, i64)> = BTreeMap::new();
    let mut known = 0;
    for row in rows {
        if let Some(rating) = row.rating.as_deref() {
            known += 1;
            let label = match rating {
                "general" => "General",
                "teen" => "Teen",
                "mature" => "Mature",
                "explicit" => "Explicit",
                other => other,
            };
            buckets
                .entry(rating.to_owned())
                .and_modify(|entry| entry.1 += 1)
                .or_insert((label.to_owned(), 1));
        }
    }
    answer(Facet::Rating, tally(buckets), known, rows.len() as i64)
}

/// How fast a work is being written: words per week across the span between publication and the last update.
fn cadence_facet(rows: &[Scanned]) -> FacetAnswer {
    let mut buckets: BTreeMap<String, (String, i64)> = BTreeMap::new();
    let mut known = 0;
    for row in rows {
        // Both inputs are required: a work with no measurable words or no publication date has no cadence, and a cadence
        // invented from half the inputs would rank a work by how recently someone touched it.
        let (Some(words), Some(days)) = (row.words, row.days) else {
            continue;
        };
        known += 1;
        let weeks = (days as f64 / 7.0).max(1.0);
        let per_week = words as f64 / weeks;
        let (key, label) = if per_week < 500.0 {
            ("under_500", "Under 500 words a week")
        } else if per_week < 2_000.0 {
            ("500_2k", "500 – 2,000 words a week")
        } else if per_week < 10_000.0 {
            ("2k_10k", "2,000 – 10,000 words a week")
        } else {
            ("over_10k", "Over 10,000 words a week")
        };
        buckets
            .entry(key.to_owned())
            .and_modify(|entry| entry.1 += 1)
            .or_insert((label.to_owned(), 1));
    }
    answer(Facet::Cadence, tally(buckets), known, rows.len() as i64)
}

/// The rank a rating ceiling compares against, matching the `CASE` in the statement.
fn rating_rank(rating: &str) -> i64 {
    match rating.to_ascii_lowercase().as_str() {
        "general" => 0,
        "teen" => 1,
        "mature" => 2,
        "explicit" => 3,
        _ => 4,
    }
}

/// Days since the Unix epoch for a `YYYY-MM-DD` prefix, UTC.
///
/// The timestamps in this schema are stored as text and as `timestamptz` depending on the backend, so the prefix is all this
/// needs — and a prefix parser avoids pulling a date crate into `darkhold-db` for one subtraction.
fn days_since_epoch(timestamp: &str) -> Option<i64> {
    let bytes = timestamp.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    let year: i64 = timestamp.get(0..4)?.parse().ok()?;
    let month: i64 = timestamp.get(5..7)?.parse().ok()?;
    let day: i64 = timestamp.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Civil-to-days (Howard Hinnant), the inverse of the formatting in the experiment routes.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}
