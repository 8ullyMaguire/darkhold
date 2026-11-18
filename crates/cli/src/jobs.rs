//! ADR 0048 row 409 A3: `dh jobs` — scheduled darkhold work on the shared job store.
//!
//! There is no HTTP client in this module. A run claims a row in the shared `polaris_jobs` table,
//! dispatches to a member-owned kind handler that talks to darkhold's store, and persists the
//! outcome. That is the A3 gate: work happens without a request opening a route.
//!
//! `polaris-jobs`' `PgStore` owns its own Tokio runtime, so its methods must never be called from
//! inside a runtime. `run`/`enqueue` are synchronous and are entered from a dedicated OS thread by
//! `main`; the member store's async functions are polled on a separate runtime that lives for the
//! whole invocation.

use std::time::Duration as StdDuration;

use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use darkhold_db::{finder, jobs, Database, DatabaseConfig};
use polaris_jobs::{Backoff, Job, JobStore, Lease, MaxAttempts, Outcome, PgStore};
use serde_json::json;
use time::{Duration, OffsetDateTime};

/// The member-owned kind registry. Both names have real callers in this tree:
/// `finder::sweep_expired` and `jobs::purge_terminal_jobs` are run today by the in-process worker
/// (`crates/app/src/worker.rs`), which loses its schedule on restart. No kind here invents work.
pub const KINDS: [&str; 2] = ["expire_bounties", "purge_terminal_jobs"];

/// Unknown kinds are refused before a row exists.
pub fn is_known(kind: &str) -> bool {
    KINDS.contains(&kind)
}

/// `dh jobs …`
#[derive(Debug, Clone, Args)]
pub struct JobsArgs {
    #[command(subcommand)]
    pub action: JobsCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum JobsCommand {
    /// Run ready jobs until the store is empty or `--limit` is reached.
    Run(RunArgs),
    /// Enqueue one job on the shared store. Refuses an unknown kind without writing a row.
    Enqueue(EnqueueArgs),
}

#[derive(Debug, Clone, Args)]
pub struct RunArgs {
    /// DSN of the shared `polaris_jobs` table (never a member's production DSN in a test).
    #[arg(long, env = "DARKHOLD_JOBS_DSN")]
    pub dsn: String,
    /// The member database the handlers work on.
    #[arg(long, env = "DARKHOLD_DB_URL")]
    pub db_url: String,
    /// Worker name recorded on every lease.
    #[arg(long, default_value = "dh-jobs")]
    pub worker: String,
    /// Only run this kind, when the timer declares one job per unit.
    #[arg(long)]
    pub kind: Option<String>,
    /// Stop after this many jobs; `0` means drain everything ready now.
    #[arg(long, default_value_t = 0)]
    pub limit: usize,
    /// Retention window for `purge_terminal_jobs`.
    #[arg(long, default_value_t = 30)]
    pub retention_days: i64,
    /// Explicit clock for deterministic tests; epoch milliseconds.
    #[arg(long)]
    pub now_ms: Option<i64>,
}

#[derive(Debug, Clone, Args)]
pub struct EnqueueArgs {
    #[arg(long, env = "DARKHOLD_JOBS_DSN")]
    pub dsn: String,
    /// A name from `KINDS`; anything else is a usage error and leaves no row.
    #[arg(long)]
    pub kind: String,
    /// Opaque JSON payload, passed to the handler.
    #[arg(long, default_value = "{}")]
    pub payload: String,
    #[arg(long, default_value_t = 5)]
    pub max_attempts: u32,
    /// Explicit epoch-millisecond run time for deterministic tests.
    #[arg(long)]
    pub now_ms: Option<i64>,
}

fn now_ms(explicit: Option<i64>) -> i64 {
    explicit
        .unwrap_or_else(|| (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64)
}

/// One handler invocation. The returned text is the count/effect the JSON line reports.
async fn handle(db: &Database, kind: &str, retention_days: i64) -> Result<String, String> {
    match kind {
        "expire_bounties" => {
            let expired = finder::sweep_expired(db).await.map_err(|e| e.to_string())?;
            Ok(format!("{expired}"))
        }
        "purge_terminal_jobs" => {
            let cutoff = OffsetDateTime::now_utc()
                - Duration::seconds(retention_days.saturating_mul(24 * 60 * 60));
            let purged = jobs::purge_terminal_jobs(db, cutoff)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("{purged}"))
        }
        other => Err(format!("unknown job kind: {other}")),
    }
}

fn report(job: &Job, outcome: &Outcome, effect: Option<&str>) {
    let (name, error) = match outcome {
        Outcome::Done => ("done", None),
        Outcome::Retry { after } => ("retry", Some(format!("after={:?}", after))),
        Outcome::Failed { error } => ("failed", Some(error.clone())),
    };
    println!(
        "{}",
        json!({
            "job_id": job.id,
            "kind": job.kind,
            "attempts": job.attempts,
            "outcome": name,
            "effect": effect,
            "detail": error
        })
    );
}

/// Run one invocation. Synchronous on purpose: see the module note.
pub fn run(args: RunArgs) -> Result<()> {
    if let Some(kind) = &args.kind {
        if !is_known(kind) {
            return Err(anyhow!("unknown job kind: {kind}"));
        }
    }
    let now = now_ms(args.now_ms);
    let backoff = Backoff::default();
    let mut store = PgStore::connect_shared(&args.dsn).context("connect shared job store")?;

    // A member store pool needs a live runtime for as long as the handlers use it.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build member-store runtime")?;
    let db = rt
        .block_on(Database::connect(&DatabaseConfig::new(&args.db_url)))
        .context("connect member store")?;

    let mut processed = 0usize;
    let mut failed = false;
    loop {
        if args.limit > 0 && processed >= args.limit {
            break;
        }
        let Some(claim) = store
            .claim(now, &args.worker, Lease::default())
            .context("claim job")?
        else {
            break;
        };
        let Some(job) = store.by_id(claim.job_id).context("read claimed job")? else {
            return Err(anyhow!("claimed job {} vanished", claim.job_id));
        };
        if let Some(only) = &args.kind {
            if job.kind != *only {
                // Put the lease back so a later invocation of this kind can take it.
                store.apply(
                    &claim,
                    &Outcome::Retry {
                        after: StdDuration::ZERO,
                    },
                    now,
                    &backoff,
                )?;
                continue;
            }
        }
        let kind = job.kind.clone();
        let result = rt.block_on(handle(&db, &kind, args.retention_days));
        let outcome = match result {
            Ok(effect) => {
                report(&job, &Outcome::Done, Some(&effect));
                Outcome::Done
            }
            Err(error) => {
                report(
                    &job,
                    &Outcome::Failed {
                        error: error.clone(),
                    },
                    None,
                );
                Outcome::Failed { error }
            }
        };
        failed |= matches!(outcome, Outcome::Failed { .. });
        store
            .apply(&claim, &outcome, now, &backoff)
            .context("apply outcome")?;
        processed += 1;
    }
    println!("{}", json!({"processed": processed, "failed": failed}));
    if failed {
        return Err(anyhow!("a job handler failed; see the JSON line above"));
    }
    Ok(())
}

/// Enqueue one row, with the kind gate ahead of the insert.
pub fn enqueue(args: EnqueueArgs) -> Result<()> {
    if !is_known(&args.kind) {
        return Err(anyhow!("unknown job kind: {}", args.kind));
    }
    let payload: serde_json::Value = serde_json::from_str(&args.payload)
        .with_context(|| format!("payload is not JSON: {}", args.payload))?;
    let store = PgStore::connect_shared(&args.dsn).context("connect shared job store")?;
    let id = store
        .insert(
            &args.kind,
            payload,
            now_ms(args.now_ms),
            MaxAttempts(args.max_attempts),
        )
        .context("insert job")?;
    println!(
        "{}",
        json!({"job_id": id, "kind": args.kind, "status": "pending"})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use polaris_jobs::{Error as JobsError, Status};

    static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Serialise the tests on one scratch table and leave it empty for each one.
    fn fresh_store() -> std::sync::MutexGuard<'static, ()> {
        let guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dsn = jobs_dsn();
        // `connect_shared` owns the migration; TRUNCATE alone would fail on a fresh DB.
        PgStore::connect_shared(&dsn).unwrap();
        let rt = cleanup_rt();
        rt.block_on(async {
            let pool = sqlx::PgPool::connect(&dsn).await.unwrap();
            sqlx::query("TRUNCATE polaris_jobs")
                .execute(&pool)
                .await
                .unwrap();
            pool.close().await;
        });
        guard
    }
    use test_support::TestDb;

    fn cleanup_rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn jobs_dsn() -> String {
        std::env::var("DARKHOLD_JOBS_TEST_DSN")
            .expect("DARKHOLD_JOBS_TEST_DSN must point at a scratch PostgreSQL database")
    }

    struct Fixture {
        db: Database,
        fixture: TestDb,
    }

    fn fixture() -> Fixture {
        let dir = std::path::Path::new("/home/alvaro/.cargo-target/darkhold/jobs-a3-tests");
        std::fs::create_dir_all(dir).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let fixture = rt.block_on(TestDb::connect_with_dir("jobs_a3", dir));
        let db = fixture.db().clone();
        Fixture { db, fixture }
    }

    const T0: i64 = 1_700_000_000_000;

    /// The A3 gate: an invocation with no HTTP base URL and no server claims and finishes a real
    /// store-backed job. A second store (a new process in production) sees the terminal row.
    #[test]
    fn a_job_runs_with_no_http_request_and_a_new_store_sees_the_result() {
        let dsn = jobs_dsn();
        let _store_guard = fresh_store();
        let f = fixture();
        let id = {
            let store = PgStore::connect_shared(&dsn).unwrap();
            store
                .insert(
                    "expire_bounties",
                    json!({"source": "a3"}),
                    T0,
                    MaxAttempts::default(),
                )
                .unwrap()
        };
        assert!(
            std::env::var("DARKHOLD_BASE_URL").is_err()
                || std::env::var("DARKHOLD_BASE_URL").unwrap().is_empty()
        );

        run(RunArgs {
            dsn: dsn.clone(),
            db_url: f.db.redacted_url().to_owned(),
            worker: "worker-a".into(),
            kind: Some("expire_bounties".into()),
            limit: 1,
            retention_days: 30,
            now_ms: Some(T0),
        })
        .unwrap();

        let store = PgStore::connect_shared(&dsn).unwrap();
        let job = store.by_id(id).unwrap().unwrap();
        assert_eq!(job.status, Status::Done);
        assert_eq!(job.attempts, 1);
        assert_eq!(store.count().unwrap(), 1);
        assert!(job.locked_until.is_none());
        cleanup_rt().block_on(f.fixture.cleanup());
    }

    /// The crash case: a worker claimed and died with a live lease. A later worker reclaims it
    /// only after expiry, and the attempt is recorded rather than reset.
    #[test]
    fn an_expired_lease_is_recovered_with_attempts_recorded() {
        let dsn = jobs_dsn();
        let _store_guard = fresh_store();
        let f = fixture();
        let (id, lease_until) = {
            let mut store = PgStore::connect_shared(&dsn).unwrap();
            let id = store
                .insert("purge_terminal_jobs", json!({}), T0, MaxAttempts::default())
                .unwrap();
            let claim = store
                .claim(T0, "crashed-worker", Lease::default())
                .unwrap()
                .unwrap();
            (id, claim.lease_until)
        };
        run(RunArgs {
            dsn: dsn.clone(),
            db_url: f.db.redacted_url().to_owned(),
            worker: "worker-b".into(),
            kind: Some("purge_terminal_jobs".into()),
            limit: 1,
            retention_days: 30,
            now_ms: Some(lease_until + 1),
        })
        .unwrap();
        let store = PgStore::connect_shared(&dsn).unwrap();
        let job = store.by_id(id).unwrap().unwrap();
        assert_eq!(job.status, Status::Done);
        assert_eq!(
            job.attempts, 2,
            "the crashed claim must remain in the record"
        );
        cleanup_rt().block_on(f.fixture.cleanup());
    }

    #[test]
    fn unknown_kind_is_refused_with_no_row() {
        let dsn = jobs_dsn();
        let _store_guard = fresh_store();
        let store = PgStore::connect_shared(&dsn).unwrap();
        let before = store.count().unwrap();
        let error = enqueue(EnqueueArgs {
            dsn: dsn.clone(),
            kind: "not_a_darkhold_kind".into(),
            payload: "{}".into(),
            max_attempts: 5,
            now_ms: Some(T0),
        })
        .unwrap_err();
        assert!(error.to_string().contains("unknown job kind"));
        let after = PgStore::connect_shared(&dsn).unwrap().count().unwrap();
        assert_eq!(after, before, "a refused kind must not leave a row");
    }

    /// The member-side gate that is mutation-proved: a retrying handler past `max_attempts` leaves
    /// a terminal row with `last_error`, and nothing is deleted.
    #[test]
    fn max_attempts_leaves_a_terminal_row_with_last_error() {
        let dsn = jobs_dsn();
        let _store_guard = fresh_store();
        let (id, before) = {
            let store = PgStore::connect_shared(&dsn).unwrap();
            let before = store.count().unwrap();
            let id = store
                .insert("expire_bounties", json!({}), T0, MaxAttempts(2))
                .unwrap();
            (id, before)
        };
        let backoff = Backoff::new(StdDuration::from_millis(1), StdDuration::from_millis(1), 0);
        for n in 0..2 {
            let mut store = PgStore::connect_shared(&dsn).unwrap();
            let claim = store
                .claim(T0 + n * 10, "worker-a", Lease::default())
                .unwrap()
                .unwrap();
            let job = store.by_id(id).unwrap().unwrap();
            assert!(is_known(&job.kind));
            store
                .apply(
                    &claim,
                    &Outcome::Retry {
                        after: StdDuration::ZERO,
                    },
                    T0 + n * 10,
                    &backoff,
                )
                .unwrap();
        }
        let store = PgStore::connect_shared(&dsn).unwrap();
        let job = store.by_id(id).unwrap().unwrap();
        assert_eq!(job.status, Status::Failed);
        assert_eq!(job.attempts, 2);
        assert!(job
            .last_error
            .as_deref()
            .unwrap_or("")
            .contains("max attempts"));
        assert_eq!(store.count().unwrap(), before + 1);
    }

    #[test]
    fn a_job_store_with_no_shared_table_is_an_error_not_an_empty_success() {
        // This asserts the constructor actually talks to the supplied DSN; a bogus DSN cannot be
        // mistaken for a drained queue.
        let result = PgStore::connect_shared("postgres://nobody:nobody@127.0.0.1:1/nope");
        assert!(matches!(result, Err(JobsError::Pg(_))));
    }
}
