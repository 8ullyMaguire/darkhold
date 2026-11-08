//! M69-21 — the Saved Filter's public life, on whichever engine `DARKHOLD_TEST_PG_URL` names.
//!
//! The rules that belong to the store rather than to a route:
//!
//! * a keybind is one digit per account, refused on collision rather than swapped;
//! * following twice is following once, and unfollowing really removes the row;
//! * a private filter cannot be followed;
//! * a blend may only name the reader's own filters, and cannot name one filter twice.

use darkhold_db::{library, saved_filter_life, Database};
use darkhold_domain::library::ViewScope;

const NOW: &str = "2026-11-08T00:00:00Z";

fn make_config(url: String) -> darkhold_db::DatabaseConfig {
    let mut config = darkhold_db::DatabaseConfig::new(url);
    config.max_connections = 2;
    config.acquire_timeout = std::time::Duration::from_secs(60);
    config
}

async fn connect() -> Database {
    let Ok(admin) = std::env::var("DARKHOLD_TEST_PG_URL") else {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "darkhold-sflife-{}-{}",
            uuid::Uuid::new_v4().simple(),
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let db = Database::connect(&make_config(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        )))
        .await
        .expect("connect sqlite");
        db.migrate().await.expect("migrate");
        return db;
    };
    let name = format!("lh_sflife_{}", uuid::Uuid::new_v4().simple());
    let admin_db = Database::connect(&make_config(admin.clone()))
        .await
        .expect("admin");
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(admin_db.postgres_pool().expect("postgres handle"))
        .await
        .expect("create database");
    let url = admin
        .rsplit_once('/')
        .map(|(prefix, _)| format!("{prefix}/{name}"))
        .expect("a database url has a path");
    let db = Database::connect(&make_config(url)).await.expect("connect");
    db.migrate().await.expect("migrate");
    db
}

async fn account(db: &Database, tag: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let sql = db.sql(
        "INSERT INTO accounts (id, email, created_at, updated_at) VALUES (?, ?, ?, ?)",
        "INSERT INTO accounts (id, email, created_at, updated_at) VALUES ($1::uuid, $2, $3, $4)",
    );
    let email = format!("{tag}-{id}@example.test");
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(&email)
                .bind(NOW)
                .bind(NOW)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("insert account");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(&email)
                .bind(NOW)
                .bind(NOW)
                .execute(db.postgres_pool().expect("postgres"))
                .await
                .expect("insert account");
        }
    }
    id
}

async fn view(db: &Database, account: &str, name: &str, scope: ViewScope) -> String {
    let view = library::create_catalogue_view(db, account, name, "sort=likes", scope, false)
        .await
        .expect("create view");
    view.id
}

#[tokio::test]
async fn one_digit_belongs_to_one_filter_and_the_collision_is_refused() {
    let db = connect().await;
    let db = &db;
    let owner = account(db, "keybind").await;
    let first = view(db, &owner, "Morning", ViewScope::Public).await;
    let second = view(db, &owner, "Evening", ViewScope::Public).await;

    assert!(
        saved_filter_life::set_keybind(db, &owner, &first, Some(3), NOW)
            .await
            .expect("bind"),
        "the first binding lands"
    );
    let binds = saved_filter_life::keybinds(db, &owner)
        .await
        .expect("keybinds");
    assert_eq!(
        binds,
        vec![(3, first.clone(), "Morning".to_owned())],
        "the digit is bound to the first filter, and the answer names it"
    );

    // Binding the same filter to the same digit again is a no-op, not a collision.
    saved_filter_life::set_keybind(db, &owner, &first, Some(3), NOW)
        .await
        .expect("re-binding the same filter is idempotent");

    // A different filter on the same digit is refused, and the refusal names the digit.
    let error = saved_filter_life::set_keybind(db, &owner, &second, Some(3), NOW)
        .await
        .expect_err("one digit, one filter");
    assert!(error.to_string().contains('3'), "{error}");

    // Digit 0 and 10 are refused: zero is beside backspace and ten is not a key.
    for digit in [0_i64, 10] {
        let error = saved_filter_life::set_keybind(db, &owner, &second, Some(digit), NOW)
            .await
            .expect_err("not a digit a reader can press");
        assert!(error.to_string().contains("1 to 9"), "{error}");
    }

    // Clearing frees the digit for the other filter.
    saved_filter_life::set_keybind(db, &owner, &first, None, NOW)
        .await
        .expect("clear");
    saved_filter_life::set_keybind(db, &owner, &second, Some(3), NOW)
        .await
        .expect("the digit is free again");
    let binds = saved_filter_life::keybinds(db, &owner)
        .await
        .expect("keybinds");
    assert_eq!(binds.len(), 1);
    assert_eq!(binds[0].0, 3);
    assert_eq!(binds[0].1, second);

    // A view that is not this account's is not bindable, and the answer is `false` rather than an error that names it.
    let stranger = account(db, "keybind-stranger").await;
    assert!(
        !saved_filter_life::set_keybind(db, &stranger, &second, Some(4), NOW)
            .await
            .expect("query"),
        "somebody else's view is not bindable, and its existence is not confirmed"
    );
}

#[tokio::test]
async fn following_is_one_row_and_a_private_filter_cannot_be_followed() {
    let db = connect().await;
    let db = &db;
    let owner = account(db, "follow-owner").await;
    let reader = account(db, "follow-reader").await;
    let public = view(db, &owner, "Shared", ViewScope::Public).await;
    let private = view(db, &owner, "Mine", ViewScope::Library).await;

    assert!(
        saved_filter_life::subscribe(db, &reader, &public, NOW)
            .await
            .expect("follow"),
        "the first follow inserts"
    );
    assert!(
        !saved_filter_life::subscribe(db, &reader, &public, NOW)
            .await
            .expect("follow again"),
        "following twice is following once"
    );
    assert_eq!(
        saved_filter_life::subscriber_count(db, &public)
            .await
            .expect("count"),
        1,
        "one reader, one row"
    );

    let followed = saved_filter_life::followed(db, &reader)
        .await
        .expect("followed");
    assert_eq!(followed.len(), 1);
    assert_eq!(followed[0].view_id, public);
    assert_eq!(followed[0].owner_account_id, owner);

    let error = saved_filter_life::subscribe(db, &reader, &private, NOW)
        .await
        .expect_err("a private filter is not followable");
    assert!(error.to_string().contains("private"), "{error}");

    assert!(
        saved_filter_life::unsubscribe(db, &reader, &public)
            .await
            .expect("unfollow"),
        "the row is removed"
    );
    assert!(
        !saved_filter_life::unsubscribe(db, &reader, &public)
            .await
            .expect("unfollow again"),
        "unfollowing twice is a no-op, not an error"
    );
    assert!(saved_filter_life::followed(db, &reader)
        .await
        .expect("followed")
        .is_empty());
}

#[tokio::test]
async fn a_blend_names_the_readers_own_filters_and_only_two_of_them() {
    let db = connect().await;
    let db = &db;
    let owner = account(db, "blend-owner").await;
    let other = account(db, "blend-other").await;
    let left = view(db, &owner, "Unfinished", ViewScope::Public).await;
    let right = view(db, &owner, "Short", ViewScope::Public).await;
    let theirs = view(db, &other, "Not yours", ViewScope::Public).await;

    let created = saved_filter_life::create_blend(
        db,
        &owner,
        saved_filter_life::NewBlend {
            name: "Queue and shorts",
            left_view_id: &left,
            right_view_id: &right,
            mode: "interleave",
            pinned: true,
        },
        NOW,
    )
    .await
    .expect("create blend");
    assert_eq!(created.mode, "interleave");
    assert!(created.pinned);

    let error = saved_filter_life::create_blend(
        db,
        &owner,
        saved_filter_life::NewBlend {
            name: "Same twice",
            left_view_id: &left,
            right_view_id: &left,
            mode: "interleave",
            pinned: false,
        },
        NOW,
    )
    .await
    .expect_err("a blend of a filter with itself is that filter");
    assert!(error.to_string().contains("two different"), "{error}");

    let error = saved_filter_life::create_blend(
        db,
        &owner,
        saved_filter_life::NewBlend {
            name: "Not mine",
            left_view_id: &left,
            right_view_id: &theirs,
            mode: "union",
            pinned: false,
        },
        NOW,
    )
    .await
    .expect_err("somebody else's filter is not a side of your blend");
    assert!(error.to_string().contains("your own"), "{error}");

    let error = saved_filter_life::create_blend(
        db,
        &owner,
        saved_filter_life::NewBlend {
            name: "Weighted",
            left_view_id: &left,
            right_view_id: &right,
            mode: "weighted",
            pinned: false,
        },
        NOW,
    )
    .await
    .expect_err("'weighted' is not a mode");
    assert!(error.to_string().contains("interleave"), "{error}");

    let list = saved_filter_life::blends(db, &owner).await.expect("blends");
    assert_eq!(
        list.len(),
        1,
        "the refused creations stored nothing: {list:?}"
    );
    assert_eq!(list[0].id, created.id);

    assert!(saved_filter_life::delete_blend(db, &owner, &created.id)
        .await
        .expect("delete"));
    assert!(saved_filter_life::find_blend(db, &owner, &created.id)
        .await
        .expect("find")
        .is_none());
    assert!(
        !saved_filter_life::delete_blend(db, &other, &created.id)
            .await
            .expect("delete"),
        "not yours is not deletable, and not its existence confirmed"
    );
}
