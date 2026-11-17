//! `dh lib tag --from-csv`: a small, auditable CSV loader.
//!
//! The loader validates every row before writing anything. A malformed row does
//! not block a good one, but it is reported with its line number and the command
//! exits non-zero. Only rows actually written go into the rollback file, so an
//! inverse can never delete a tag this command did not create.

use std::fmt::Write as _;

use darkhold_db::taxonomy::{self, TagAssertion, TagSource};

/// One candidate row from the CSV.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagRow {
    pub line: usize,
    pub work_id: String,
    pub tag: String,
    /// Set only after a successful write; part of the rollback's identity.
    pub added_at: Option<String>,
}

/// One rejected row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagError {
    pub line: usize,
    pub reason: String,
}

/// The whole validation result.
#[derive(Debug, Clone, Default)]
pub struct TagCsv {
    pub rows: Vec<TagRow>,
    pub errors: Vec<TagError>,
}

/// Parse `work_id,tag` CSV. Quoted fields and embedded commas are rejected by
/// design: an ambiguous row must become a report, never a silent retagging.
pub fn parse_tag_csv(input: &str) -> TagCsv {
    let mut result = TagCsv::default();
    let mut lines = input.lines();
    let Some(header) = lines.next() else {
        result.errors.push(TagError {
            line: 1,
            reason: "missing header: expected `work_id,tag`".to_owned(),
        });
        return result;
    };
    if header.trim_end_matches('\r').trim() != "work_id,tag" {
        result.errors.push(TagError {
            line: 1,
            reason: "unknown header: expected `work_id,tag`".to_owned(),
        });
        return result;
    }
    for (offset, raw) in lines.enumerate() {
        let line_number = offset + 2;
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        if line.contains('"') {
            result.errors.push(TagError {
                line: line_number,
                reason: "quoted fields are not supported; remove the quotes and commas".to_owned(),
            });
            continue;
        }
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        if fields.len() != 2 {
            result.errors.push(TagError {
                line: line_number,
                reason: "expected exactly two columns: work_id,tag".to_owned(),
            });
            continue;
        }
        let work = fields[0];
        let tag = fields[1];
        if uuid::Uuid::parse_str(work).is_err() {
            result.errors.push(TagError {
                line: line_number,
                reason: "work_id is not a UUID".to_owned(),
            });
            continue;
        }
        if tag.is_empty() || tag.contains(',') {
            result.errors.push(TagError {
                line: line_number,
                reason: "tag must be a non-empty name without a comma".to_owned(),
            });
            continue;
        }
        result.rows.push(TagRow {
            line: line_number,
            work_id: work.to_owned(),
            tag: tag.to_owned(),
            added_at: None,
        });
    }
    result
}

/// A human-readable, per-row report. Counts are for the parsed batch, not the
/// whole database, so a rerun cannot be mistaken for a full catalogue audit.
pub fn render_report(parsed: &TagCsv) -> String {
    let mut out = format!(
        "{} applied, {} failed\n",
        parsed.rows.len(),
        parsed.errors.len()
    );
    for error in &parsed.errors {
        let _ = writeln!(out, "line {}: {}", error.line, error.reason);
    }
    out
}

/// SQL that undoes exactly one applied row. Single quotes are doubled so a tag
/// name cannot become a second statement.
pub fn rollback_sql(row: &TagRow) -> String {
    let quote = |value: &str| value.replace('\'', "''");
    let mut sql = format!(
        "DELETE FROM work_tags WHERE work_id='{}' AND node_id='{}'",
        quote(&row.work_id),
        quote(&row.tag)
    );
    if let Some(added_at) = &row.added_at {
        let _ = write!(sql, " AND added_at='{}'", quote(added_at));
    }
    sql.push_str(";\n");
    sql
}

/// Apply every valid row through the product's write path. Returns the rows
/// actually written, each carrying the timestamp its rollback needs.
pub async fn apply(db: &darkhold_db::Database, parsed: &TagCsv) -> anyhow::Result<Vec<TagRow>> {
    let mut applied = Vec::with_capacity(parsed.rows.len());
    let assertion = TagAssertion {
        source: TagSource::Curator,
        ..TagAssertion::default()
    };
    for row in &parsed.rows {
        let written = taxonomy::tag_work(db, &row.work_id, &row.tag, assertion).await?;
        applied.push(TagRow {
            line: row.line,
            work_id: row.work_id.clone(),
            tag: row.tag.clone(),
            added_at: Some(written.added_at),
        });
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_with_no_added_at_still_has_a_safe_rollback() {
        let row = TagRow {
            line: 2,
            work_id: "11111111-1111-4111-8111-111111111111".to_owned(),
            tag: "dark's-mc".to_owned(),
            added_at: None,
        };
        assert!(rollback_sql(&row).contains("dark''s-mc"));
    }
}
