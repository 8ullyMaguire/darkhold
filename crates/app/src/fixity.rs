//! Fixity: read the stored bytes back and say whether they are still what `content_blobs` claims.
//!
//! The distinction that shapes this module is between a *run* and a *finding*. A run is bounded and disposable —
//! it looked at some bytes, for some reason, and stopped. A finding is a fact about a file, and it is only
//! meaningful next to the run that observed it, which is why `fixity_drift` carries `run_id` in its key and why
//! this module never deletes an old row.
//!
//! Nothing here writes to a blob. A fixity pass that repaired what it disliked would be a job that destroys an
//! archive the first time its own hash is wrong. Repair is an operator decision made with the report in hand,
//! and the report is this module's whole output.

use anyhow::Result;
use darkhold_db::storage::BlobStore;
use darkhold_db::{Backend, Database};
use sha2::{Digest, Sha256};
use std::time::Instant;

/// What one pass concluded.
///
/// Every count is here because a summary that reports only "drifted 0" hides the difference between a clean
/// store and a pass that read nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The run row's id.
    pub run_id: String,
    /// Blobs whose bytes were read and matched.
    pub clean: i64,
    /// Blobs whose bytes were read and did not match.
    pub drifted: i64,
    /// Blob rows whose file was not there at all.
    pub missing: i64,
    /// Files that could not be opened or read.
    pub unreadable: i64,
    /// Total bytes claimed by the blobs this pass looked at.
    pub bytes_hashed: i64,
    /// Whether the pass reached the end of the store.
    pub complete: bool,
    /// How long it took, in milliseconds, for the log line.
    pub elapsed_ms: i64,
}

impl Summary {
    /// One line for the job log, with the counts named and the budget verdict spelled out.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "run {}: {} clean, {} drifted, {} missing, {} unreadable, {} bytes hashed, {} in {}ms",
            self.run_id,
            self.clean,
            self.drifted,
            self.missing,
            self.unreadable,
            self.bytes_hashed,
            if self.complete {
                "complete"
            } else {
                "budget exhausted"
            },
            self.elapsed_ms
        )
    }
}

/// One blob row as the scan needs it: the claim, not the content.
struct Claim {
    checksum: String,
    storage_key: String,
    byte_size: i64,
}

/// What reading one file concluded.
enum Verdict {
    Clean,
    Drift {
        kind: &'static str,
        actual_bytes: Option<i64>,
        actual_checksum: Option<String>,
    },
}

/// Scan the store, writing one run row and one row per finding.
///
/// `budget_bytes` bounds the pass by bytes claimed rather than by blob count: a store of small thumbnails and a
/// store of two-gigabyte PDFs need the same wall-clock promise, and only a byte budget provides it. A pass that
/// stops early records `complete = false`, so "we did not finish" is never mistaken for "everything is fine".
///
/// # Errors
/// Only when the run's own rows cannot be written. A blob that cannot be read is a *finding*, not an error: a
/// pass that returned `Err` on the first unreadable file would report nothing about the other ninety-nine
/// thousand.
pub async fn scan(db: &Database, store: &BlobStore, budget_bytes: i64) -> Result<Summary> {
    let started = Instant::now();
    let run_id = uuid::Uuid::new_v4().to_string();
    insert_run(db, &run_id, &now_rfc3339(), budget_bytes).await?;

    let claims = load_claims(db).await?;
    let mut summary = Summary {
        run_id: run_id.clone(),
        clean: 0,
        drifted: 0,
        missing: 0,
        unreadable: 0,
        bytes_hashed: 0,
        complete: true,
        elapsed_ms: 0,
    };

    for claim in claims {
        if summary.bytes_hashed >= budget_bytes {
            summary.complete = false;
            break;
        }
        match read_one(store, &claim).await {
            Verdict::Clean => summary.clean += 1,
            Verdict::Drift {
                kind,
                actual_bytes,
                actual_checksum,
            } => {
                match kind {
                    "missing" => summary.missing += 1,
                    "unreadable" => summary.unreadable += 1,
                    _ => summary.drifted += 1,
                }
                insert_drift(
                    db,
                    &run_id,
                    &claim,
                    kind,
                    actual_bytes,
                    actual_checksum.as_deref(),
                    &now_rfc3339(),
                )
                .await?;
            }
        }
        summary.bytes_hashed += claim.byte_size;
    }

    summary.elapsed_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    finish_run(db, &summary).await?;
    tracing::info!(target: "darkhold::fixity", "{}", summary.describe());
    Ok(summary)
}

/// Read one file and compare it to its claim.
///
/// Size is compared before hashing, and the ordering matters: it is the difference between a bounded pass and
/// one that reads a four-gigabyte truncated file to learn what `stat` already said. A file that is the right
/// size but the wrong bytes is exactly what a size check cannot see, which is why the hash exists at all.
async fn read_one(store: &BlobStore, claim: &Claim) -> Verdict {
    let path = store.path_for(&claim.checksum);
    let metadata = match tokio::fs::metadata(&path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Verdict::Drift {
                kind: "missing",
                actual_bytes: None,
                actual_checksum: None,
            }
        }
        Err(_) => {
            return Verdict::Drift {
                kind: "unreadable",
                actual_bytes: None,
                actual_checksum: None,
            }
        }
    };
    let actual_bytes = i64::try_from(metadata.len()).unwrap_or(i64::MAX);
    if actual_bytes != claim.byte_size {
        return Verdict::Drift {
            kind: "size",
            actual_bytes: Some(actual_bytes),
            actual_checksum: None,
        };
    }
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Verdict::Drift {
                kind: "unreadable",
                actual_bytes: Some(actual_bytes),
                actual_checksum: None,
            }
        }
    };
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual == claim.checksum {
        Verdict::Clean
    } else {
        Verdict::Drift {
            kind: "hash",
            actual_bytes: Some(actual_bytes),
            actual_checksum: Some(actual),
        }
    }
}

/// One drift row, as the report shows it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DriftRow {
    /// The checksum the row claimed, which is also the file's name on disk.
    pub checksum: String,
    /// Path under the storage root.
    pub storage_key: String,
    /// `missing`, `size`, `hash`, or `unreadable`.
    pub kind: String,
    /// What `content_blobs` said.
    pub expected_bytes: i64,
    /// What the file actually is, when that could be read.
    pub actual_bytes: Option<i64>,
    /// The hash actually computed, when the bytes were read.
    pub actual_checksum: Option<String>,
    /// When the finding was recorded.
    pub noted_at: String,
}

/// The latest run and its findings, for the operator's screen.
///
/// Defaults to empty rather than to an error: an instance that has never run a scan has no drift, and saying
/// "never scanned" is the caller's job, not a 500's.
#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct Report {
    /// The run this report describes, if any run has happened.
    pub run_id: Option<String>,
    /// When it started.
    pub started_at: Option<String>,
    /// When it finished; `None` means it is running or it crashed mid-pass.
    pub finished_at: Option<String>,
    /// The byte budget it was given.
    pub budget_bytes: i64,
    /// Bytes it looked at.
    pub bytes_hashed: i64,
    /// Blobs it examined.
    pub checked: i64,
    /// Blobs that matched.
    pub clean: i64,
    /// Blobs that did not match.
    pub drifted: i64,
    /// Blob rows with no file.
    pub missing: i64,
    /// Whether it reached the end of the store.
    pub complete: bool,
    /// Why it failed, if it did.
    pub error: Option<String>,
    /// The findings of *this* run only.
    pub drift: Vec<DriftRow>,
}

/// Read the most recent run and its findings.
///
/// The newest run decides what "now" means: an old run's findings are history, and showing them as current
/// would make a repaired store look broken forever.
/// One row of `fixity_runs`, in column order.
///
/// A named type because clippy is right: an eleven-tuple in a `let` is unreadable at the call site and
/// the compiler cannot tell you which field moved when one does.
type FixityRunRow = (
    String,
    String,
    Option<String>,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    Option<String>,
);

///
/// # Errors
/// As the underlying queries.
pub async fn latest_report(db: &Database) -> Result<Report> {
    let sql = db.sql(
        "SELECT id, started_at, finished_at, budget_bytes, bytes_hashed, checked, clean, drifted, missing, \
                complete, error
           FROM fixity_runs ORDER BY seq DESC LIMIT 1",
        "SELECT id, started_at, finished_at, budget_bytes, bytes_hashed, checked, clean, drifted, missing, \
                complete, error
           FROM fixity_runs ORDER BY seq DESC LIMIT 1",
    );
    let row: Option<FixityRunRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .fetch_optional(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .fetch_optional(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    let Some((
        id,
        started_at,
        finished_at,
        budget_bytes,
        bytes_hashed,
        checked,
        clean,
        drifted,
        missing,
        complete,
        error,
    )) = row
    else {
        return Ok(Report::default());
    };

    let drift_sql = db.sql(
        "SELECT checksum, storage_key, kind, expected_bytes, actual_bytes, actual_checksum, noted_at \
           FROM fixity_drift WHERE run_id = ? ORDER BY kind, checksum",
        "SELECT checksum, storage_key, kind, expected_bytes, actual_bytes, actual_checksum, noted_at \
           FROM fixity_drift WHERE run_id = $1 ORDER BY kind, checksum",
    );
    type DriftTuple = (
        String,
        String,
        String,
        i64,
        Option<i64>,
        Option<String>,
        String,
    );
    let drift: Vec<DriftTuple> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, DriftTuple>(&drift_sql)
                .bind(&id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, DriftTuple>(&drift_sql)
                .bind(&id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };

    Ok(Report {
        run_id: Some(id),
        started_at: Some(started_at),
        finished_at,
        budget_bytes,
        bytes_hashed,
        checked,
        clean,
        drifted,
        missing,
        complete: complete != 0,
        error,
        drift: drift
            .into_iter()
            .map(
                |(
                    checksum,
                    storage_key,
                    kind,
                    expected_bytes,
                    actual_bytes,
                    actual_checksum,
                    noted_at,
                )| {
                    DriftRow {
                        checksum,
                        storage_key,
                        kind,
                        expected_bytes,
                        actual_bytes,
                        actual_checksum,
                        noted_at,
                    }
                },
            )
            .collect(),
    })
}

// ---- rows -----------------------------------------------------------------
//
// Each of these is a two-arm match for the same reason `BlobStore::put` is: SQLite and PostgreSQL disagree
// about placeholder syntax, and `db.sql(a, b)` exists so that disagreement is written down once.

/// Insert the run row before any reading, so a pass that dies leaves evidence that it started.
///
/// `seq` is computed inside the insert (`COALESCE(MAX(seq),0)+1` over the same table) rather than in a separate
/// statement, so two runs starting at once cannot pick the same number: the statement sees one snapshot and the
/// unique index refuses a duplicate. A separate `SELECT MAX` followed by an `INSERT` would be a race that only
/// shows up when somebody runs two scans at once — which is exactly when an operator is worried.
async fn insert_run(db: &Database, id: &str, started_at: &str, budget_bytes: i64) -> Result<()> {
    let sql = db.sql(
        "INSERT INTO fixity_runs (id, seq, started_at, budget_bytes) \
         SELECT ?, COALESCE(MAX(seq), 0) + 1, ?, ? FROM fixity_runs",
        "INSERT INTO fixity_runs (id, seq, started_at, budget_bytes) \
         SELECT $1, COALESCE(MAX(seq), 0) + 1, $2, $3 FROM fixity_runs",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(id)
                .bind(started_at)
                .bind(budget_bytes)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(id)
                .bind(started_at)
                .bind(budget_bytes)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(())
}

/// Write the counts the pass ended with.
async fn finish_run(db: &Database, summary: &Summary) -> Result<()> {
    let checked = summary.clean + summary.drifted + summary.missing + summary.unreadable;
    let sql = db.sql(
        "UPDATE fixity_runs SET finished_at = ?, bytes_hashed = ?, checked = ?, clean = ?, drifted = ?, \
                missing = ?, complete = ? WHERE id = ?",
        "UPDATE fixity_runs SET finished_at = $1, bytes_hashed = $2, checked = $3, clean = $4, drifted = $5, \
                missing = $6, complete = $7 WHERE id = $8",
    );
    let finished_at = now_rfc3339();
    let complete = i64::from(summary.complete);
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&finished_at)
                .bind(summary.bytes_hashed)
                .bind(checked)
                .bind(summary.clean)
                .bind(summary.drifted)
                .bind(summary.missing)
                .bind(complete)
                .bind(&summary.run_id)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&finished_at)
                .bind(summary.bytes_hashed)
                .bind(checked)
                .bind(summary.clean)
                .bind(summary.drifted)
                .bind(summary.missing)
                .bind(complete)
                .bind(&summary.run_id)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(())
}

/// Record one finding against the run that made it.
#[allow(clippy::too_many_arguments)]
async fn insert_drift(
    db: &Database,
    run_id: &str,
    claim: &Claim,
    kind: &str,
    actual_bytes: Option<i64>,
    actual_checksum: Option<&str>,
    noted_at: &str,
) -> Result<()> {
    let sql = db.sql(
        "INSERT INTO fixity_drift (run_id, checksum, storage_key, kind, expected_bytes, actual_bytes, \
                actual_checksum, noted_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        "INSERT INTO fixity_drift (run_id, checksum, storage_key, kind, expected_bytes, actual_bytes, \
                actual_checksum, noted_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(run_id)
                .bind(&claim.checksum)
                .bind(&claim.storage_key)
                .bind(kind)
                .bind(claim.byte_size)
                .bind(actual_bytes)
                .bind(actual_checksum)
                .bind(noted_at)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(run_id)
                .bind(&claim.checksum)
                .bind(&claim.storage_key)
                .bind(kind)
                .bind(claim.byte_size)
                .bind(actual_bytes)
                .bind(actual_checksum)
                .bind(noted_at)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(())
}

/// Every blob the store claims to hold, in checksum order.
///
/// Ordered, because a pass that stops on a budget must stop at the same place twice: an unordered scan of a
/// large store can starve the same tail forever.
async fn load_claims(db: &Database) -> Result<Vec<Claim>> {
    let sql = db.sql(
        "SELECT checksum, storage_key, byte_size FROM content_blobs ORDER BY checksum",
        "SELECT checksum, storage_key, byte_size FROM content_blobs ORDER BY checksum",
    );
    let rows: Vec<(String, String, i64)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(checksum, storage_key, byte_size)| Claim {
            checksum,
            storage_key,
            byte_size,
        })
        .collect())
}

/// RFC 3339 with nanoseconds.
///
/// Seconds are fine for a human reading a log and not fine for ordering two events that happened in the same
/// second. `seq` is what the run ordering actually relies on; the extra precision is for `noted_at`, where two
/// findings in one pass otherwise carry identical stamps.
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}
