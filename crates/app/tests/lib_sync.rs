//! M63-10: an incremental local mirror, with a watermark and upserts.

use std::path::PathBuf;

use clap::Parser;
use darkhold_app::cli::{Cli, Command, LibCommand};
use darkhold_app::lib_sync::{read_changes, write_mirror};
use darkhold_db::{Database, DatabaseConfig};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("darkhold-lib-sync-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

async fn exec(db: &Database, sql: &str) {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(sql)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("statement");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(sql)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("statement");
        }
    }
}

async fn mirror_count(db: &Database, kind: &str, id: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM mirror_rows WHERE kind = ? AND id = ?")
        .bind(kind)
        .bind(id)
        .fetch_one(db.sqlite_pool().expect("mirror sqlite"))
        .await
        .expect("mirror count")
}

#[tokio::test]
async fn a_since_watermark_moves_the_mirror_forward_on_both_engines() {
    let dir = scratch("sync");
    let tdb = test_support::TestDb::connect_with_dir("lib-sync", &dir).await;
    let source = tdb.db();
    let account = "11111111-1111-4111-8111-111111111111";
    let pseud = "22222222-2222-4222-8222-222222222222";
    let work = "33333333-3333-4333-8333-333333333333";
    let item = "44444444-4444-4444-8444-444444444444";

    exec(source, &format!("INSERT INTO accounts (id, email, created_at, updated_at) VALUES ('{account}', 'sync@x.test', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")).await;
    exec(source, &format!("INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) VALUES ('{pseud}', '{account}', 'sync', 'Sync', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")).await;
    exec(source, &format!("INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES ('{work}', '{pseud}', 'Mirrored work', '2026-01-01T00:00:00Z', '2026-02-01T00:00:00Z')")).await;
    exec(source, &format!("INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, source_url, created_at, updated_at) VALUES ('{item}', '{account}', '{work}', 'site:{item}', 'story-1', 'Mirrored item', 'https://example.test/1', '2026-01-01T00:00:00Z', '2026-02-01T00:00:00Z')")).await;

    let mirror_path = dir.join("mirror.sqlite");
    let first = read_changes(source, Some("2026-01-15T00:00:00Z"), None)
        .await
        .expect("first read");
    assert_eq!(first.len(), 2, "one work and one library item changed");
    let report = write_mirror(&mirror_path, &first, "2026-02-01T00:00:00Z")
        .await
        .expect("first write");
    assert_eq!(report.library_rows, 1);
    assert_eq!(report.work_rows, 1);

    let mirror_config = DatabaseConfig::new(format!("sqlite://{}?mode=rwc", mirror_path.display()));
    let mirror = Database::connect(&mirror_config)
        .await
        .expect("mirror opens");
    assert_eq!(mirror_count(&mirror, "work", work).await, 1);
    assert_eq!(mirror_count(&mirror, "library_item", item).await, 1);
    let watermark: String =
        sqlx::query_scalar("SELECT value FROM mirror_meta WHERE key = 'last_sync'")
            .fetch_one(mirror.sqlite_pool().expect("mirror sqlite"))
            .await
            .expect("watermark");
    assert_eq!(watermark, "2026-02-01T00:00:00Z");
    mirror.close().await;

    // A later sync with a later `since` must not rewrite the old rows.
    exec(source, &format!("UPDATE works SET title = 'Renamed', updated_at = '2026-03-01T00:00:00Z' WHERE id = '{work}'")).await;
    let second = read_changes(source, Some("2026-02-15T00:00:00Z"), None)
        .await
        .expect("second read");
    assert_eq!(second.len(), 1, "only the work moved after the watermark");
    assert_eq!(second[0].id, work);
    let _ = write_mirror(&mirror_path, &second, "2026-03-01T00:00:00Z")
        .await
        .expect("second write");
}

#[test]
fn the_cli_exposes_lib_sync() {
    let cli = Cli::try_parse_from([
        "darkhold",
        "lib",
        "sync",
        "--to",
        "/tmp/mirror.sqlite",
        "--since",
        "2026-02-01T00:00:00Z",
    ])
    .expect("parses");
    match cli.command {
        Command::Lib(lib) => match lib.command {
            LibCommand::Sync(args) => {
                assert_eq!(args.to.to_string_lossy(), "/tmp/mirror.sqlite");
                assert_eq!(args.since.as_deref(), Some("2026-02-01T00:00:00Z"));
            }
            other => panic!("wrong lib command: {other:?}"),
        },
        other => panic!("wrong command: {other:?}"),
    }
}
