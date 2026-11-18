//! M63-11: `dh lib diff` and `dh lib push --dry-run`.

use std::path::PathBuf;

use clap::Parser;
use darkhold_app::cli::{Cli, Command, LibCommand};
use darkhold_app::lib_diff::{diff, push_plan, render_plan, render_report, DiffState};
use darkhold_app::lib_sync::{read_changes, write_mirror, MirrorChange};
use darkhold_db::Database;
use serde_json::json;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("darkhold-lib-diff-{tag}-{}", std::process::id()));
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

async fn scalar(db: &Database, sql: &str) -> String {
    match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar(sql)
            .fetch_one(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("scalar"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar(sql)
            .fetch_one(db.postgres_pool().expect("postgres"))
            .await
            .expect("scalar"),
    }
}

#[tokio::test]
async fn diff_classifies_changes_on_both_engines() {
    let dir = scratch("diff");
    let tdb = test_support::TestDb::connect_with_dir("lib-diff", &dir).await;
    let source = tdb.db();
    let account = "11111111-1111-4111-8111-111111111111";
    let pseud = "22222222-2222-4222-8222-222222222222";
    let work = "33333333-3333-4333-8333-333333333333";
    let item = "44444444-4444-4444-8444-444444444444";
    let late_work = "55555555-5555-4555-8555-555555555555";
    let local_only = "66666666-6666-4666-8666-666666666666";

    exec(source, &format!("INSERT INTO accounts (id, email, created_at, updated_at) VALUES ('{account}', 'diff@x.test', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")).await;
    exec(source, &format!("INSERT INTO pseuds (id, account_id, handle, display_name, created_at, updated_at) VALUES ('{pseud}', '{account}', 'diff', 'Diff', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")).await;
    exec(source, &format!("INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES ('{work}', '{pseud}', 'Site title', '2026-01-01T00:00:00Z', '2026-02-01T00:00:00Z')")).await;
    exec(source, &format!("INSERT INTO library_items (id, account_id, work_id, source_key, source_work_key, title, source_url, created_at, updated_at) VALUES ('{item}', '{account}', '{work}', 'site:{item}', 'story-1', 'Mirrored item', 'https://example.test/1', '2026-01-01T00:00:00Z', '2026-02-01T00:00:00Z')")).await;

    let mirror_path = dir.join("mirror.sqlite");
    let snapshot = read_changes(source, None, None).await.expect("snapshot");
    assert_eq!(snapshot.len(), 2);
    write_mirror(&mirror_path, &snapshot, "2026-02-01T00:00:00Z")
        .await
        .expect("mirror write");

    // A local edit, a local-only row, and a later site row must be three
    // different findings; a push plan must not silently mix them.
    let mut local = snapshot.clone();
    for change in &mut local {
        if change.kind == "work" {
            change.payload["title"] = json!("Local title");
        }
    }
    local.push(MirrorChange {
        kind: "work",
        id: local_only.to_owned(),
        updated_at: "2026-02-01T00:00:00Z".to_owned(),
        payload: json!({"id": local_only, "title": "Only in the mirror", "summary": "", "rating": null, "visibility": "public", "lifecycle": "draft", "completion": "unknown", "owner_pseud_id": null, "deleted_at": null}),
    });
    write_mirror(&mirror_path, &local, "2026-02-01T00:00:00Z")
        .await
        .expect("edited mirror write");
    exec(source, &format!("INSERT INTO works (id, owner_pseud_id, title, created_at, updated_at) VALUES ('{late_work}', '{pseud}', 'Late site work', '2026-01-01T00:00:00Z', '2026-03-01T00:00:00Z')")).await;

    let report = diff(source, &mirror_path, None).await.expect("diff");
    let changed: Vec<_> = report
        .rows
        .iter()
        .filter(|r| r.state == DiffState::Changed)
        .collect();
    assert_eq!(changed.len(), 1, "{}", render_report(&report));
    assert_eq!(changed[0].id, work);
    assert_eq!(changed[0].fields, vec!["title"]);
    assert_eq!(
        report
            .rows
            .iter()
            .filter(|r| r.state == DiffState::LocalOnly)
            .count(),
        1
    );
    assert_eq!(
        report
            .rows
            .iter()
            .filter(|r| r.state == DiffState::RemoteOnly)
            .count(),
        1
    );

    let plan = push_plan(source, &mirror_path, None)
        .await
        .expect("push plan");
    assert_eq!(plan.len(), 1, "{}", render_plan(&plan));
    assert_eq!(plan[0].id, work);
    assert_eq!(plan[0].fields, vec!["title"]);
    // A dry run is a report; the site row must still carry the site's title.
    assert_eq!(
        scalar(
            source,
            &format!("SELECT title FROM works WHERE id = '{work}'")
        )
        .await,
        "Site title"
    );
}

#[test]
fn the_cli_exposes_lib_diff_and_dry_run_push() {
    let diff_cli =
        Cli::try_parse_from(["darkhold", "lib", "diff", "--mirror", "/tmp/mirror.sqlite"])
            .expect("diff parses");
    match diff_cli.command {
        Command::Lib(lib) => match lib.command {
            LibCommand::Diff(args) => {
                assert_eq!(args.mirror.to_string_lossy(), "/tmp/mirror.sqlite")
            }
            other => panic!("wrong lib command: {other:?}"),
        },
        other => panic!("wrong command: {other:?}"),
    }

    let push_cli = Cli::try_parse_from([
        "darkhold",
        "lib",
        "push",
        "--mirror",
        "/tmp/mirror.sqlite",
        "--dry-run",
    ])
    .expect("push parses");
    match push_cli.command {
        Command::Lib(lib) => match lib.command {
            LibCommand::Push(args) => {
                assert_eq!(args.mirror.to_string_lossy(), "/tmp/mirror.sqlite");
                assert!(args.dry_run);
            }
            other => panic!("wrong lib command: {other:?}"),
        },
        other => panic!("wrong command: {other:?}"),
    }
}
