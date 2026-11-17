//! Row 407 proof: the real `darkhold` binary, twice, against a scratch SQLite
//! database. The second pass changes one rating in the export and asserts one
//! row per (pseud, work), the new value, and the provenance stamp.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use darkhold_db::{identity, Database, DatabaseConfig};
use darkhold_domain::policy::AgeState;
use darkhold_domain::{AccountId, PseudId};

struct Fixture {
    dir: PathBuf,
    db: Database,
    url: String,
    config: PathBuf,
    account: AccountId,
    pseud: PseudId,
    work_a: String,
}

fn stamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos()
}

async fn fixture() -> Fixture {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-row407-{}-{}",
        std::process::id(),
        stamp()
    ));
    fs::create_dir_all(&dir).expect("scratch dir");
    let url = format!(
        "sqlite://{}?mode=rwc",
        dir.join("darkhold.sqlite").display()
    );
    let config = dir.join("darkhold.toml");
    fs::write(&config, format!("[database]\nurl = \"{url}\"\n")).expect("config");

    let migrated = Command::new(env!("CARGO_BIN_EXE_darkhold"))
        .args(["--config", config.to_str().unwrap(), "migrate"])
        .output()
        .expect("migrate binary");
    assert!(
        migrated.status.success(),
        "migrate failed: {}",
        String::from_utf8_lossy(&migrated.stderr)
    );

    let db = Database::connect(&DatabaseConfig::new(url.clone()))
        .await
        .expect("connect");
    let account = identity::create_account(
        &db,
        "row407@example.invalid",
        AgeState::DeclaredAdult,
        identity::AccountStatus::Active,
    )
    .await
    .expect("account");
    let pseud = identity::create_pseud(&db, account, "row407", "Row 407")
        .await
        .expect("pseud");

    let work_a = uuid::Uuid::new_v4().to_string();
    let work_b = uuid::Uuid::new_v4().to_string();
    let now = "2026-11-17T00:00:00Z";
    for (work, title) in [(&work_a, "Alpha Work"), (&work_b, "Beta Work")] {
        sqlx::query(
            "INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(work)
        .bind(pseud.to_string())
        .bind(title)
        .bind(now)
        .bind(now)
        .execute(db.sqlite_pool().expect("sqlite"))
        .await
        .expect("work");
        sqlx::query(
            "INSERT INTO library_items
               (id, account_id, work_id, source_key, source_work_key, title, author_text,
                source_url, created_at, updated_at)
             VALUES (?, ?, ?, 'pawchive', ?, ?, ?, '', ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(account.to_string())
        .bind(work)
        .bind(format!("key-{title}"))
        .bind(title)
        .bind("Test Author")
        .bind(now)
        .bind(now)
        .execute(db.sqlite_pool().expect("sqlite"))
        .await
        .expect("library item");
    }

    Fixture {
        dir,
        db,
        url,
        config,
        account,
        pseud,
        work_a,
    }
}

fn write_export(f: &Fixture, alpha_calibre: i64) -> PathBuf {
    let file = f.dir.join("read.txt");
    let text = format!(
        "calibredb list --search '#last_read:True'\nid    *rating title                                                                          authors\n1     {alpha_calibre}    Alpha Work                                                        Test Author\n2     4     Beta Work                                                         Test Author\n"
    );
    fs::write(&file, text).expect("export");
    file
}

fn run_cli(f: &Fixture, file: &Path, apply: bool) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_darkhold"));
    cmd.args([
        "--config",
        f.config.to_str().unwrap(),
        "--database-url",
        &f.url,
        "seed",
        "read-list",
        "--file",
        file.to_str().unwrap(),
    ]);
    if apply {
        cmd.args([
            "--apply",
            "--account",
            &f.account.to_string(),
            "--pseud",
            &f.pseud.to_string(),
        ]);
    }
    cmd.output().expect("seed binary")
}

fn metric(stdout: &str, prefix: &str) -> i64 {
    let line = stdout
        .lines()
        .find(|line| line.starts_with(prefix))
        .unwrap_or_else(|| panic!("no {prefix:?} line in:\n{stdout}"));
    line[prefix.len()..]
        .split_whitespace()
        .last()
        .expect("metric value")
        .parse()
        .unwrap_or_else(|_| panic!("not a number: {line:?}"))
}

async fn ratings(f: &Fixture) -> Vec<(String, i64, Option<String>)> {
    sqlx::query_as("SELECT work_id, stars, source FROM rating ORDER BY work_id")
        .fetch_all(f.db.sqlite_pool().expect("sqlite"))
        .await
        .expect("rating rows")
}

#[tokio::test]
async fn reading_seed_cli_rates_twice_one_row_and_a_changed_value() {
    let f = fixture().await;
    let file = write_export(&f, 8); // 8 / 2 = 4 stars

    let dry = run_cli(&f, &file, false);
    assert!(
        dry.status.success(),
        "dry-run: {}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let stdout = String::from_utf8_lossy(&dry.stdout);
    assert!(stdout.contains("dry-run (no writes)"));
    assert_eq!(metric(&stdout, "strategy title_author"), 2);
    assert!(ratings(&f).await.is_empty(), "dry-run must write nothing");

    let first = run_cli(&f, &file, true);
    assert!(
        first.status.success(),
        "apply: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert_eq!(metric(&stdout, "created"), 2, "{stdout}");
    let rows = ratings(&f).await;
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|(_, _, source)| source.as_deref() == Some(darkhold_app::seed_read_list::PROVENANCE)));
    let alpha = rows
        .iter()
        .find(|(work, _, _)| work == &f.work_a)
        .expect("alpha rating");
    assert_eq!(alpha.1, 4);

    // Change only the export: the seeded row must be refreshed, not duplicated.
    let file = write_export(&f, 6); // 6 / 2 = 3 stars
    let second = run_cli(&f, &file, true);
    assert!(
        second.status.success(),
        "re-apply: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert_eq!(metric(&stdout, "refreshed"), 1, "{stdout}");
    assert_eq!(metric(&stdout, "skipped existing"), 1, "{stdout}");
    let rows = ratings(&f).await;
    assert_eq!(rows.len(), 2, "still one row per (pseud, work)");
    let alpha = rows
        .iter()
        .find(|(work, _, _)| work == &f.work_a)
        .expect("alpha rating");
    assert_eq!(alpha.1, 3, "the value changed through the CLI");
    assert_eq!(
        alpha.2.as_deref(),
        Some(darkhold_app::seed_read_list::PROVENANCE)
    );

    fs::remove_dir_all(&f.dir).ok();
}
