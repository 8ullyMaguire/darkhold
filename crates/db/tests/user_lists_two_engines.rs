//! Row 413 — the imported user-list service on both engines.
//!
//! `test_support::TestDb` picks SQLite or PostgreSQL from `DARKHOLD_TEST_PG_URL`;
//! neither engine is pinned here. The assertions cover create, item, collaborator,
//! collaborator read, non-collaborator refusal, moderator rules, and cascade.
//! Every one goes through `darkhold_db::user_list` — no route shortcuts.

use darkhold_db::user_list::{self, UserListError};
use darkhold_domain::user_list::{
    AddUserListCollaboratorRequest, AddUserListItemRequest, CreateUserListRequest,
    UpdateUserListRequest,
};
use serde_json::json;
use sqlx::Row;

const OWNER: &str = "01955100-0000-4000-8000-000000000001";
const COLLAB: &str = "01955100-0000-4000-8000-000000000002";
const STRANGER: &str = "01955100-0000-4000-8000-000000000003";
const WORK: &str = "01955100-0000-4000-8000-000000000005";
const WORK2: &str = "01955100-0000-4000-8000-000000000006";

async fn exec(db: &darkhold_db::Database, sql: &str) {
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(sql)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await
                .expect("fixture on SQLite");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(sql)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await
                .expect("fixture on PostgreSQL");
        }
    }
}

async fn fixture(tag: &str) -> test_support::TestDb {
    let dir =
        std::env::temp_dir().join(format!("darkhold-user-lists-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let db = test_support::TestDb::connect_with_dir(tag, &dir).await;
    let sql = "INSERT INTO accounts (id, status, email, age_state, created_at, updated_at, version) VALUES \
               ('01955100-0000-4000-8000-000000000001','active','owner@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1), \
               ('01955100-0000-4000-8000-000000000002','active','collab@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1), \
               ('01955100-0000-4000-8000-000000000003','active','stranger@test.dev','adult','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)";
    exec(db.db(), sql).await;
    let pseud = "INSERT INTO pseuds (id, account_id, handle, display_name, discoverability, created_at, updated_at, version) \
                 VALUES ('01955100-0000-4000-8000-000000000004','01955100-0000-4000-8000-000000000001','owner','owner','listed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)";
    exec(db.db(), pseud).await;
    let words = "INSERT INTO works (id, owner_pseud_id, title, visibility, lifecycle, created_at, updated_at) VALUES \
                 ('01955100-0000-4000-8000-000000000005','01955100-0000-4000-8000-000000000004','One','public','published','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z'), \
                 ('01955100-0000-4000-8000-000000000006','01955100-0000-4000-8000-000000000004','Two','public','published','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')";
    exec(db.db(), words).await;
    db
}

async fn child_counts(db: &darkhold_db::Database, list_id: &str) -> (i64, i64) {
    let sql = db.sql(
        "SELECT (SELECT count(*) FROM user_list_members WHERE list_id = ?), \
                (SELECT count(*) FROM user_list_collaborators WHERE list_id = ?)",
        "SELECT (SELECT count(*) FROM user_list_members WHERE list_id = $1::uuid), \
                (SELECT count(*) FROM user_list_collaborators WHERE list_id = $1::uuid)",
    );
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            let row = sqlx::query(&sql)
                .bind(list_id)
                .bind(list_id)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await
                .expect("count children");
            (row.get::<i64, _>(0), row.get::<i64, _>(1))
        }
        darkhold_db::Backend::Postgres => {
            let row = sqlx::query(&sql)
                .bind(list_id)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await
                .expect("count children");
            (row.get::<i64, _>(0), row.get::<i64, _>(1))
        }
    }
}

#[tokio::test]
async fn user_lists_service_gates_and_cascade_on_both_engines() {
    let db = fixture("service-gates").await;
    let db = db.db();

    // 1. Create: default visibility is private, and the owner can read the row.
    let list = user_list::create(
        db,
        OWNER,
        &CreateUserListRequest {
            name: "Autumn TBR".to_owned(),
            description: String::new(),
            visibility: None,
            moderation: Some("chosen_mods".to_owned()),
            criteria: Some(json!({"tag": "autumn"})),
        },
    )
    .await
    .expect("create list");
    assert_eq!(list.owner_id, OWNER);
    assert_eq!(list.visibility, "private");
    let id = list.id.clone();
    assert!(user_list::get_visible(db, &id, Some(OWNER))
        .await
        .expect("owner read")
        .is_some());

    // 2. Add an item. The paired assertion matters: an empty list would make
    // the later "collaborator sees the item" check vacuous.
    let item = user_list::add_item(
        db,
        &id,
        OWNER,
        &AddUserListItemRequest {
            work_id: WORK.to_owned(),
            note: "first".to_owned(),
        },
    )
    .await
    .expect("add item");
    assert_eq!(item.work_id, WORK);
    let owner_view = user_list::get_visible(db, &id, Some(OWNER))
        .await
        .expect("owner view")
        .expect("list exists");
    assert_eq!(owner_view.item_count, 1, "the item was actually stored");
    assert!(owner_view.is_owner);

    // 3. Add a collaborator, then prove the collaborator can read the private
    // list and its item; a stranger cannot, even though the list has content.
    let collaborator = user_list::add_collaborator(
        db,
        &id,
        OWNER,
        &AddUserListCollaboratorRequest {
            user_id: COLLAB.to_owned(),
            role: "moderator".to_owned(),
        },
    )
    .await
    .expect("add collaborator");
    assert_eq!(collaborator.role, "moderator");
    assert_eq!(
        user_list::items(db, &id, Some(COLLAB), 100)
            .await
            .expect("collab items")
            .len(),
        1
    );
    assert_eq!(
        user_list::collaborators(db, &id, Some(COLLAB), 100)
            .await
            .expect("collab collaborators")
            .len(),
        1
    );
    assert!(user_list::get_visible(db, &id, Some(STRANGER))
        .await
        .expect("stranger read")
        .is_none());
    assert!(matches!(
        user_list::items(db, &id, Some(STRANGER), 100).await,
        Err(UserListError::NotFound)
    ));
    assert!(matches!(
        user_list::collaborators(db, &id, Some(STRANGER), 100).await,
        Err(UserListError::NotFound)
    ));
    assert!(
        matches!(
            user_list::add_item(
                db,
                &id,
                STRANGER,
                &AddUserListItemRequest {
                    work_id: WORK2.to_owned(),
                    note: String::new()
                }
            )
            .await,
            Err(UserListError::Forbidden)
        ),
        "a non-collaborator cannot mutate"
    );

    // 4. chosen_mods: a moderator collaborator mutates, the owner still can,
    // and a curator cannot. The permission is the service's rule.
    let moderator_item = user_list::add_item(
        db,
        &id,
        COLLAB,
        &AddUserListItemRequest {
            work_id: WORK2.to_owned(),
            note: "added by moderator".to_owned(),
        },
    )
    .await
    .expect("moderator adds item");
    assert_eq!(moderator_item.added_by, COLLAB);
    assert_eq!(child_counts(db, &id).await.0, 2);
    let curator = user_list::add_collaborator(
        db,
        &id,
        OWNER,
        &AddUserListCollaboratorRequest {
            user_id: STRANGER.to_owned(),
            role: "curator".to_owned(),
        },
    )
    .await
    .expect("add curator");
    assert_eq!(curator.role, "curator");
    assert!(matches!(
        user_list::remove_item(db, &id, STRANGER, WORK2).await,
        Err(UserListError::Forbidden)
    ));

    // 5. Update through the service's mutation rule, then read the new state.
    let renamed = user_list::update(
        db,
        &id,
        OWNER,
        &UpdateUserListRequest {
            name: Some("Winter TBR".to_owned()),
            description: None,
            visibility: Some("public".to_owned()),
            moderation: None,
        },
    )
    .await
    .expect("update list");
    assert_eq!(renamed.name, "Winter TBR");
    assert!(user_list::get_visible(db, &id, Some(STRANGER))
        .await
        .expect("public read")
        .is_some());

    // 6. Cascade: both child tables held rows before the delete and hold none
    // after it. Counting *before* keeps this from passing on an empty list.
    let (items_before, collaborators_before) = child_counts(db, &id).await;
    assert_eq!((items_before, collaborators_before), (2, 2));
    user_list::delete(db, &id, OWNER)
        .await
        .expect("delete list");
    assert_eq!(child_counts(db, &id).await, (0, 0));
    assert!(user_list::get_visible(db, &id, Some(OWNER))
        .await
        .expect("after delete")
        .is_none());
}

#[tokio::test]
async fn public_quorum_is_storable_but_refuses_even_the_owner() {
    let db = fixture("quorum").await;
    let db = db.db();
    let list = user_list::create(
        db,
        OWNER,
        &CreateUserListRequest {
            name: "Quorum".to_owned(),
            description: String::new(),
            visibility: None,
            moderation: Some("public_quorum".to_owned()),
            criteria: None,
        },
    )
    .await
    .expect("create quorum list");
    assert!(matches!(
        user_list::add_item(
            db,
            &list.id,
            OWNER,
            &AddUserListItemRequest {
                work_id: WORK.to_owned(),
                note: String::new()
            }
        )
        .await,
        Err(UserListError::Forbidden)
    ));
}
