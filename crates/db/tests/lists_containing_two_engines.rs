//! M64-11 — the public "lists containing this work" query on both engines.

use darkhold_db::user_list;
use darkhold_domain::user_list::{AddUserListItemRequest, CreateUserListRequest};

const OWNER: &str = "01955100-0000-4000-8000-000000000001";
const WORK: &str = "01955100-0000-4000-8000-000000000005";

async fn fixture(tag: &str) -> test_support::TestDb {
    let dir = std::env::temp_dir().join(format!(
        "darkhold-lists-containing-{tag}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let tdb = test_support::TestDb::connect_with_dir(tag, &dir).await;
    let sql = "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES \
               ('01955100-0000-4000-8000-000000000001','active','owner@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)";
    match tdb.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(sql)
                .execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("account");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(sql)
                .execute(tdb.db().postgres_pool().expect("postgres"))
                .await
                .expect("account");
        }
    }
    let pseud = "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) \
                 VALUES ('01955100-0000-4000-8000-000000000004','01955100-0000-4000-8000-000000000001','owner','owner','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)";
    match tdb.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(pseud)
                .execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("pseud");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(pseud)
                .execute(tdb.db().postgres_pool().expect("postgres"))
                .await
                .expect("pseud");
        }
    }
    let work = "INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, created_at, updated_at) VALUES \
                ('01955100-0000-4000-8000-000000000005','01955100-0000-4000-8000-000000000004','One','public','published','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')";
    match tdb.db().backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(work)
                .execute(tdb.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("work");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(work)
                .execute(tdb.db().postgres_pool().expect("postgres"))
                .await
                .expect("work");
        }
    }
    tdb
}

#[tokio::test]
async fn only_public_lists_are_returned_in_newest_first_order() {
    let tdb = fixture("public-only").await;
    let db = tdb.db();
    for (name, visibility) in [("Older public", "public"), ("Private bundle", "private")] {
        let list = user_list::create(
            db,
            OWNER,
            &CreateUserListRequest {
                name: name.to_owned(),
                description: String::new(),
                visibility: Some(visibility.to_owned()),
                moderation: None,
                criteria: None,
            },
        )
        .await
        .expect("create list");
        user_list::add_item(
            db,
            &list.id,
            OWNER,
            &AddUserListItemRequest {
                work_id: WORK.to_owned(),
                note: String::new(),
            },
        )
        .await
        .expect("add item");
    }

    let found = darkhold_db::user_list::lists_containing(db, WORK, 10)
        .await
        .expect("lists containing");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].name, "Older public");
    assert_eq!(found[0].visibility, "public");
}
