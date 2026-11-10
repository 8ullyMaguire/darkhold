//! Standing can be given without psql (M70-13).
//!
//! Three store functions had no caller, and between them they are how standing is given on this instance: `set_trust` (the
//! ladder), `apply_spam_demotion` (§14.4's one automatic demotion) and `grant_operator_role` (the estate claim). All three
//! were reachable only through SQL, which means the only person who could promote a moderator, demote a spammer, or answer a
//! "the spam reports were upheld and nothing happened" ticket was whoever had the database password.
//!
//! The test drives the doors the way an operator would, and checks the effect where it is *read* — `account_facts`, which is
//! what every other door consults — rather than only in the response the write returned.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::{json, Value};
use test_support::{register, scratch_dir, sign_in_as, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

/// Uphold `count` spam reports against an account: the evidence §14.4's demotion is made of.
///
/// Reports and their review task both, because the count joins them — a report with no upheld task is somebody's complaint,
/// not a finding, and the rule is careful about the difference.
async fn uphold_spam_reports(db: &Database, account: &str, count: i64) {
    let now = darkhold_db::identity::now_rfc3339();
    for i in 0..count {
        let report = format!("rpt-{account}-{i}");
        let (a, b, c, d) = (
            report.clone(),
            account.to_string(),
            now.clone(),
            now.clone(),
        );
        let sql = db.sql(
            "INSERT INTO reports (id, subject_type, subject_id, reporter, reason, created_at, state, resolution)
             VALUES (?, 'account', ?, 'moderator', 'spam', ?, 'resolved', 'upheld')",
            "INSERT INTO reports (id, subject_type, subject_id, reporter, reason, created_at, state, resolution)
             VALUES ($1, 'account', $2, 'moderator', 'spam', $3, 'resolved', 'upheld')",
        );
        match db.backend() {
            darkhold_db::Backend::Sqlite => {
                sqlx::query(&sql)
                    .bind(&a)
                    .bind(&b)
                    .bind(&c)
                    .execute(db.sqlite_pool().expect("sqlite"))
                    .await
                    .expect("report");
            }
            darkhold_db::Backend::Postgres => {
                sqlx::query(&sql)
                    .bind(&a)
                    .bind(&b)
                    .bind(&c)
                    .execute(db.postgres_pool().expect("postgres"))
                    .await
                    .expect("report");
            }
        }
        let task = format!("task-{account}-{i}");
        let (t, r, rev, at, outcome) = (
            task,
            report,
            "moderator".to_string(),
            d,
            "uphold".to_string(),
        );
        let sql = db.sql(
            "INSERT INTO review_tasks (id, reviewer, report_id, assigned_at, decided_at, outcome)
             VALUES (?, ?, ?, ?, ?, ?)",
            "INSERT INTO review_tasks (id, reviewer, report_id, assigned_at, decided_at, outcome)
             VALUES ($1, $2, $3, $4, $5, $6)",
        );
        match db.backend() {
            darkhold_db::Backend::Sqlite => {
                sqlx::query(&sql)
                    .bind(&t)
                    .bind(&rev)
                    .bind(&r)
                    .bind(&at)
                    .bind(&at)
                    .bind(&outcome)
                    .execute(db.sqlite_pool().expect("sqlite"))
                    .await
                    .expect("task");
            }
            darkhold_db::Backend::Postgres => {
                sqlx::query(&sql)
                    .bind(&t)
                    .bind(&rev)
                    .bind(&r)
                    .bind(&at)
                    .bind(&at)
                    .bind(&outcome)
                    .execute(db.postgres_pool().expect("postgres"))
                    .await
                    .expect("task");
            }
        }
    }
}

async fn trust_of(client: &mut TestClient, account: &str) -> i64 {
    let (status, facts) = client
        .get(format!("/api/v1/admin/accounts/{account}/facts"))
        .await;
    assert_eq!(status, StatusCode::OK, "{facts}");
    facts["trust_level"].as_i64().expect("trust level")
}

#[tokio::test]
async fn standing_is_given_by_a_door_and_read_where_it_is_enforced() {
    let dir = scratch_dir("trust-admin");
    let tdb = TestDb::connect_with_dir("m7013-trust-admin", &dir).await;
    let db: Database = tdb.db().clone();

    let base = Config::development_defaults();
    let mut bootstrap = TestClient::new(build(&base, &db));
    let operator_account = register(&mut bootstrap, "tr-operator@example.test", "troperator").await;
    let mut config = base.clone();
    config.administration.operator_account_id = Some(operator_account.parse().expect("account id"));
    let mut operator = TestClient::new(build(&config, &db));
    sign_in_as(
        &mut operator,
        &tdb,
        "tr-operator@example.test",
        "troperator",
    )
    .await;

    let mut author = TestClient::new(build(&config, &db));
    let subject = register(&mut author, "tr-subject@example.test", "trsubject").await;
    let mut member = TestClient::new(build(&config, &db));
    register(&mut member, "tr-member@example.test", "trmember").await;

    // 1. A level the ladder does not name is a typo, refused by field — not stored as a level whose name renders `null`.
    let (status, body) = operator
        .post(
            &format!("/api/v1/admin/accounts/{subject}/trust"),
            json!({ "level": 999, "reason": "promote" }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["field_errors"]["level"].is_string(),
        true,
        "{body}"
    );

    // 2. A trust change with no reason is refused by name: it is a decision in the one log that cannot be edited.
    let (status, body) = operator
        .post(
            &format!("/api/v1/admin/accounts/{subject}/trust"),
            json!({ "level": 4 }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["field_errors"]["reason"].is_string(),
        true,
        "{body}"
    );

    // 3. With both, the level moves — and the door reports the name from the ladder, so an operator sees "trusted" rather
    //    than an integer they have to look up.
    let before = trust_of(&mut operator, &subject).await;
    let (status, body) = operator
        .post(
            &format!("/api/v1/admin/accounts/{subject}/trust"),
            json!({ "level": 5, "reason": "three years of accepted proposals" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["level"], json!(5), "{body}");
    assert_eq!(body["previous_level"], json!(before), "{body}");
    let name = body["level_name"].as_str().expect("ladder name").to_owned();
    assert!(!name.is_empty(), "{body}");
    assert_eq!(
        trust_of(&mut operator, &subject).await,
        5,
        "the write landed"
    );

    // 4. Spam demotion with no upheld reports changes nothing and says *which* nothing: not enough evidence is a state an
    //    operator can act on, unlike a bare "no change".
    let (status, body) = operator
        .post(
            &format!("/api/v1/admin/accounts/{subject}/spam-demotion"),
            json!({ "reason": "routine check" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["outcome"],
        json!("not_enough_confirmed_reports"),
        "{body}"
    );
    assert_eq!(body["confirmed_reports"], json!(0), "{body}");
    assert_eq!(body["level_after"], json!(5), "{body}");
    assert_eq!(trust_of(&mut operator, &subject).await, 5, "nothing moved");

    // 5. Three upheld reports in ninety days drop one level — the spec's rule, applied by the door that now exists.
    uphold_spam_reports(&db, &subject, 3).await;
    let (status, body) = operator
        .post(
            &format!("/api/v1/admin/accounts/{subject}/spam-demotion"),
            json!({ "reason": "three upheld spam reports" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["outcome"], json!("demoted"), "{body}");
    assert_eq!(body["confirmed_reports"], json!(3), "{body}");
    assert_eq!(body["level_before"], json!(5), "{body}");
    assert_eq!(body["level_after"], json!(4), "{body}");
    assert_eq!(
        trust_of(&mut operator, &subject).await,
        4,
        "the demotion landed"
    );

    // 6. The floor is the specification's: at level 2 the same evidence answers `at_floor` rather than demoting to a level
    //    where the account's contents become foldable away.
    let (status, _) = operator
        .post(
            &format!("/api/v1/admin/accounts/{subject}/trust"),
            json!({ "level": 2, "reason": "floor check" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = operator
        .post(
            &format!("/api/v1/admin/accounts/{subject}/spam-demotion"),
            json!({ "reason": "floor check" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["outcome"], json!("at_floor"), "{body}");
    assert_eq!(body["level_after"], json!(2), "{body}");

    // 7. A role no reader of the claim recognises is refused by field, because it would look like standing and grant
    //    nothing.
    let (status, body) = operator
        .post(
            &format!("/api/v1/admin/accounts/{subject}/operator-role"),
            json!({ "role": "role:king", "reason": "why not" }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["field_errors"]["role"].is_string(),
        true,
        "{body}"
    );

    // 8. A recognised one lands in the table the estate reads.
    let (status, body) = operator
        .post(
            &format!("/api/v1/admin/accounts/{subject}/operator-role"),
            json!({ "role": "role:moderator:fandom-a", "reason": "elected by the community" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["role"], json!("role:moderator:fandom-a"), "{body}");
    let rows: Vec<(String,)> = match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_as("SELECT role FROM operator_role WHERE account = ?")
                .bind(&subject)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await
                .expect("roles")
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query_as("SELECT role FROM operator_role WHERE account = $1")
                .bind(&subject)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await
                .expect("roles")
        }
    };
    assert_eq!(rows.len(), 1, "the grant is stored: {rows:?}");
    assert_eq!(rows[0].0, "role:moderator:fandom-a");

    // 9. Every one of these is an operator's act: a member asking for any of them is answered with the absent page, not a
    //    403 that confirms the door exists.
    for (path, body) in [
        (
            format!("/api/v1/admin/accounts/{subject}/trust"),
            json!({ "level": 5, "reason": "self-promotion" }),
        ),
        (
            format!("/api/v1/admin/accounts/{subject}/spam-demotion"),
            json!({ "reason": "grudge" }),
        ),
        (
            format!("/api/v1/admin/accounts/{subject}/operator-role"),
            json!({ "role": "role:operator", "reason": "self-promotion" }),
        ),
    ] {
        let (status, reply) = member.post(&path, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {reply}");
        assert_eq!(reply["error"]["code"], json!("NOT_FOUND"), "{reply}");
    }
    let _ = Value::Null;
}
