//! M63-32 — backfill source-backed tags from metadata the importer already stored.
//!
//! The importer kept `library_items.provenance_json` even when it had no typed
//! category column. Pawchive rows carry `fandom`, `characters`, `relationships`
//! and `tags`; archive rows carry `tags`; AO3 rows carry `genre`. Reading those
//! fields is a bounded database pass, not a re-fetch of 30,000 source pages.
//!
//! Only `Claimant::Source` is written here. A model proposal would be invisible
//! to readers until confirmed and could not fix coverage.

use serde_json::Value;

use crate::tag_assertions::{assert_tag, Claimant};
use crate::{Database, Result};

#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Library items examined.
    pub scanned: usize,
    /// Works that received at least one tag.
    pub works_tagged: usize,
    /// Tag assertions attempted (idempotent re-assertions count here).
    pub assertions: usize,
    /// Nodes that did not exist before this pass.
    pub nodes_created: usize,
}

/// Backfill visible source tags from stored provenance JSON.
///
/// `source` limits the pass to one `library_items.source_key`; `limit` caps the
/// number of rows examined. `dry_run` parses and counts but writes nothing.
pub async fn backfill_from_provenance(
    db: &Database,
    source: Option<&str>,
    limit: usize,
    dry_run: bool,
) -> Result<Report> {
    let mut report = Report::default();
    if limit == 0 {
        return Ok(report);
    }
    let (sqlite_sql, postgres_sql) = match source {
        Some(_) => (
            "SELECT CAST(work_id AS TEXT) AS work_id, provenance_json
               FROM library_items
              WHERE work_id IS NOT NULL AND provenance_json IS NOT NULL
                AND provenance_json <> '{}' AND source_key = ?
              ORDER BY id LIMIT ?",
            "SELECT CAST(work_id AS TEXT) AS work_id, provenance_json
               FROM library_items
              WHERE work_id IS NOT NULL AND provenance_json IS NOT NULL
                AND provenance_json <> '{}' AND source_key = $1
              ORDER BY id LIMIT $2",
        ),
        None => (
            "SELECT CAST(work_id AS TEXT) AS work_id, provenance_json
               FROM library_items
              WHERE work_id IS NOT NULL AND provenance_json IS NOT NULL
                AND provenance_json <> '{}'
              ORDER BY id LIMIT ?",
            "SELECT CAST(work_id AS TEXT) AS work_id, provenance_json
               FROM library_items
              WHERE work_id IS NOT NULL AND provenance_json IS NOT NULL
                AND provenance_json <> '{}'
              ORDER BY id LIMIT $1",
        ),
    };
    let rows = match db.backend() {
        crate::Backend::Sqlite => {
            let mut query = sqlx::query_as::<_, (String, String)>(sqlite_sql);
            if let Some(source) = source {
                query = query.bind(source);
            }
            query = query.bind(limit as i64);
            query.fetch_all(db.sqlite_pool().expect("sqlite")).await?
        }
        crate::Backend::Postgres => {
            let mut query = sqlx::query_as::<_, (String, String)>(postgres_sql);
            if let Some(source) = source {
                query = query.bind(source);
            }
            query = query.bind(limit as i64);
            query
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };

    for (work_id, provenance) in rows {
        report.scanned += 1;
        let Ok(value) = serde_json::from_str::<Value>(&provenance) else {
            continue;
        };
        let claims = claims_from(&value);
        if claims.is_empty() {
            continue;
        }
        let (written, created) = assert_claims(db, &work_id, &claims, dry_run).await?;
        report.assertions += written;
        report.nodes_created += created;
        if written > 0 {
            report.works_tagged += 1;
        }
    }
    Ok(report)
}

/// Extract `(kind, canonical)` claims from one provenance object.
fn claims_from(value: &Value) -> Vec<(String, String)> {
    let mut seen = std::collections::HashSet::new();
    let mut claims = Vec::new();
    for (field, kind) in [
        ("fandom", "fandom"),
        ("characters", "character"),
        ("relationships", "relationship"),
        ("tags", "tag"),
        ("genre", "tag"),
    ] {
        let mut push = |raw: &str| {
            let canonical = raw.trim();
            if canonical.is_empty() || canonical.len() > 200 {
                return;
            }
            let key = (kind.to_owned(), canonical.to_ascii_lowercase());
            if seen.insert(key) {
                claims.push((kind.to_owned(), canonical.to_owned()));
            }
        };
        match value.get(field) {
            Some(Value::String(raw)) => push(raw),
            Some(Value::Array(items)) => {
                for item in items {
                    if let Some(raw) = item.as_str() {
                        push(raw);
                    }
                }
            }
            _ => {}
        }
    }
    claims
}


/// Write, or count in a dry run, the source assertions for one work.
pub async fn assert_claims(
    db: &Database,
    work_id: &str,
    claims: &[(String, String)],
    dry_run: bool,
) -> Result<(usize, usize)> {
    if dry_run {
        return Ok((claims.len(), 0));
    }
    let mut written = 0;
    let mut created = 0;
    for (kind, canonical) in claims {
        let existed = crate::taxonomy::node_id_by_norm(
            db,
            kind,
            &crate::taxonomy::canonical_form(canonical),
        )
        .await?
        .is_some();
        let node = crate::taxonomy::ensure_node_from_signal(db, kind, canonical).await?;
        if !existed {
            created += 1;
        }
        assert_tag(db, work_id, &node.id, Claimant::Source, false, 1.0).await?;
        written += 1;
    }
    Ok((written, created))
}
