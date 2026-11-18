//! `dh lib diff` / `dh lib push --dry-run`: compare a derived mirror to the site.
//!
//! The mirror stores a JSON payload per row, so a diff is a field-by-field
//! comparison of two `serde_json::Value` objects. This module never writes to
//! the site. `push_plan` is the preview a future, explicitly authorised writer
//! would consume; until that writer exists, a real push refuses.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::lib_sync::{read_changes, MirrorChange};

type MirrorTuple = (String, String, String, String);

/// One row in the derived SQLite mirror.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorRow {
    pub kind: String,
    pub id: String,
    pub updated_at: String,
    pub payload: String,
}

/// How one row differs between the mirror and the site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffState {
    Changed,
    LocalOnly,
    RemoteOnly,
}

/// One finding. `fields` is empty for rows present on only one side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRow {
    pub kind: String,
    pub id: String,
    pub state: DiffState,
    pub fields: Vec<String>,
}

/// The whole comparison. Rows are sorted by kind, then id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffReport {
    pub rows: Vec<DiffRow>,
}

fn state_word(state: DiffState) -> &'static str {
    match state {
        DiffState::Changed => "changed",
        DiffState::LocalOnly => "local-only",
        DiffState::RemoteOnly => "remote-only",
    }
}

/// Read all mirror rows. The mirror is always SQLite; the second match arm is
/// present so the dialect gate can see the decision rather than a bare pool.
pub async fn read_mirror(path: &Path) -> Result<Vec<MirrorRow>> {
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let config = darkhold_db::DatabaseConfig::new(url);
    let db = darkhold_db::Database::connect(&config).await?;
    let sql = "SELECT kind, id, updated_at, payload FROM mirror_rows";
    let rows = match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_as::<sqlx::Sqlite, MirrorTuple>(sql)
                .fetch_all(db.sqlite_pool().expect("mirror is SQLite"))
                .await?
        }
        darkhold_db::Backend::Postgres => panic!("the mirror is always SQLite"),
    };
    db.close().await;
    Ok(rows
        .into_iter()
        .map(|(kind, id, updated_at, payload)| MirrorRow {
            kind,
            id,
            updated_at,
            payload,
        })
        .collect())
}

fn changed_fields(local: &Value, remote: &Value) -> Vec<String> {
    match (local.as_object(), remote.as_object()) {
        (Some(local), Some(remote)) => {
            let mut fields: Vec<String> = local
                .keys()
                .filter(|key| local.get(*key) != remote.get(*key))
                .cloned()
                .collect();
            for key in remote.keys() {
                if !local.contains_key(key) && !fields.contains(key) {
                    fields.push(key.clone());
                }
            }
            fields.sort();
            fields
        }
        _ if local != remote => vec!["payload".to_owned()],
        _ => Vec::new(),
    }
}

fn key(kind: &str, id: &str) -> String {
    format!("{kind}\u{1f}{id}")
}

fn source_payloads(source: &[MirrorChange]) -> BTreeMap<String, &Value> {
    source
        .iter()
        .map(|change| (key(change.kind, &change.id), &change.payload))
        .collect()
}

/// Compare the mirror payloads to the configured site database.
pub async fn diff(
    db: &darkhold_db::Database,
    mirror_path: &Path,
    account: Option<&str>,
) -> Result<DiffReport> {
    let mirror = read_mirror(mirror_path)
        .await
        .with_context(|| format!("reading mirror {}", mirror_path.display()))?;
    let source = read_changes(db, None, account).await?;
    let remote = source_payloads(&source);
    let mut rows = Vec::new();

    for row in &mirror {
        let local: Value = serde_json::from_str(&row.payload)
            .with_context(|| format!("mirrored payload for {} {} is not JSON", row.kind, row.id))?;
        let k = key(&row.kind, &row.id);
        match remote.get(&k) {
            Some(remote_payload) => {
                let fields = changed_fields(&local, remote_payload);
                if !fields.is_empty() {
                    rows.push(DiffRow {
                        kind: row.kind.clone(),
                        id: row.id.clone(),
                        state: DiffState::Changed,
                        fields,
                    });
                }
            }
            None => rows.push(DiffRow {
                kind: row.kind.clone(),
                id: row.id.clone(),
                state: DiffState::LocalOnly,
                fields: Vec::new(),
            }),
        }
    }

    let mut local_keys: BTreeMap<String, ()> = BTreeMap::new();
    for row in &mirror {
        local_keys.insert(key(&row.kind, &row.id), ());
    }
    for change in &source {
        if !local_keys.contains_key(&key(change.kind, &change.id)) {
            rows.push(DiffRow {
                kind: change.kind.to_owned(),
                id: change.id.clone(),
                state: DiffState::RemoteOnly,
                fields: Vec::new(),
            });
        }
    }

    rows.sort_by(|a, b| (&a.kind, &a.id).cmp(&(&b.kind, &b.id)));
    Ok(DiffReport { rows })
}

/// The rows a future writer would update. Local-only and remote-only rows are
/// deliberately excluded: there is no safe create or delete story for them yet.
pub async fn push_plan(
    db: &darkhold_db::Database,
    mirror_path: &Path,
    account: Option<&str>,
) -> Result<Vec<DiffRow>> {
    let report = diff(db, mirror_path, account).await?;
    Ok(report
        .rows
        .into_iter()
        .filter(|row| row.state == DiffState::Changed)
        .collect())
}

/// A human-readable diff. Counts describe this comparison, not the catalogue.
pub fn render_report(report: &DiffReport) -> String {
    let mut out = format!(
        "{} changed, {} local-only, {} remote-only\n",
        report
            .rows
            .iter()
            .filter(|r| r.state == DiffState::Changed)
            .count(),
        report
            .rows
            .iter()
            .filter(|r| r.state == DiffState::LocalOnly)
            .count(),
        report
            .rows
            .iter()
            .filter(|r| r.state == DiffState::RemoteOnly)
            .count(),
    );
    for row in &report.rows {
        if row.fields.is_empty() {
            let _ = writeln!(out, "{} {} {}", state_word(row.state), row.kind, row.id);
        } else {
            let _ = writeln!(
                out,
                "{} {} {} fields {}",
                state_word(row.state),
                row.kind,
                row.id,
                row.fields.join(",")
            );
        }
    }
    out
}

/// A human-readable push preview. No SQL is emitted because no write is enabled.
pub fn render_plan(plan: &[DiffRow]) -> String {
    let mut out = format!("{} row(s) would be updated\n", plan.len());
    for row in plan {
        let _ = writeln!(
            out,
            "update {} {} fields {}",
            row.kind,
            row.id,
            row.fields.join(",")
        );
    }
    out
}
