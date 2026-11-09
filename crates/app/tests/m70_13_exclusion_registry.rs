//! The exclusion registry a gate reads and nobody could write (M70-13 pass 60).
//!
//! `permission::insert_exclusion_entry` had no caller, and `permission::is_excluded` was read in exactly one place: Guard 2 of
//! `fork_work` ("this work is excluded from forking"). No route anywhere wrote to `exclusion_registry`, and no test seeded it
//! either — so the registry was empty on every instance, Guard 2 could never fire, and §33's exclusion was a comment with a SQL
//! query behind it. A gate nobody can arm is worse than no gate: it reads as protection in the code and the spec.
//!
//! `POST /admin/exclusions` (operator) arms it, and `GET /admin/exclusions?target_type=&target_id=` lets an operator confirm what
//! they armed. Both are operator-only through `require_admin_of(None)`, and a caller with no standing gets `404`.
//!
//! Three decisions:
//!
//! * **A second registration of the same target is `409`, not a second row.** A registry of decisions must answer "when was this
//!   excluded" — two rows for one target make that question ambiguous, and asking it is the point of having a registry.
//! * **The reason is required.** An entry nobody can explain is a decision nobody can review, and the table has a `reason`
//!   column that would otherwise be filled with an empty string forever.
//! * **`creator` is accepted and stored, and named as unread.** The enum has both targets; `ExclusionTarget::Creator` has no
//!   consumer today, and refusing to store what the vocabulary allows would make the door lie about the schema. The response
//!   says so, so an operator is not told a creator exclusion is in force when nothing consults it.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::Database;
use serde_json::json;
use test_support::{register, scratch_dir, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

/// A published work by `author`, with one chapter.
async fn published_work(author: &mut TestClient, title: &str) -> String {
    let (status, body) = author.post("/api/v1/works", json!({ "title": title })).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let work = body["id"].as_str().expect("work").to_owned();
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/chapters"),
            json!({ "title": "One" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let unit = body["id"].as_str().expect("unit").to_owned();
    let version = body["version"].as_i64().expect("version");
    let (status, body) = author
        .request(
            "PATCH",
            &format!("/api/v1/chapters/{unit}"),
            Some(json!({
                "expected_version": version,
                "document": { "type": "doc", "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": "Source text." }] },
                ]},
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, body) = author.get(format!("/api/v1/works/{work}")).await;
    let version = body["version"].as_i64().expect("work version");
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{work}/publish"),
            json!({ "expected_version": version, "idempotency_key": format!("excl-{work}") }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    work
}

#[tokio::test]
async fn the_registry_can_be_armed_and_the_fork_guard_fires_for_the_first_time() {
    let dir = scratch_dir("exclusion-registry");
    let tdb = TestDb::connect_with_dir("m7013-exclusion-registry", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "ex-author@example.test", "exauthor").await;
    let parent = published_work(&mut author, "Remixable").await;

    let mut reader = TestClient::new(build(&config, &db));
    register(&mut reader, "ex-reader@example.test", "exreader").await;

    // 1. Baseline: the fork route works, so a later refusal is the registry's doing and not one of the other three guards.
    let (status, body) = reader
        .post(&format!("/api/v1/works/{parent}/fork"), json!({}))
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "a fork of a remixable, unexcluded work succeeds: {body}"
    );

    // 2. An operator arms the registry.
    let mut operator = TestClient::new(build(&config, &db));
    let account = register(&mut operator, "ex-operator@example.test", "exoperator").await;
    let mut admin = Config::development_defaults();
    admin.administration.operator_account_id = Some(account.parse().expect("account id"));
    let mut operator = TestClient::new(build(&admin, &db));
    test_support::sign_in_as(&mut operator, &tdb, "ex-operator@example.test", "exoperator").await;

    let (status, body) = operator
        .post(
            "/api/v1/admin/exclusions",
            json!({ "target_type": "work", "target_id": parent, "reason": "author withdrew permission to remix" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["target_type"], json!("work"), "{body}");
    assert_eq!(body["target_id"], json!(parent), "{body}");

    // 3. The store now answers the question the guard asks.
    assert!(
        darkhold_db::permission::is_excluded(
            &db,
            darkhold_domain::permission::ExclusionTarget::Work,
            &parent
        )
        .await
        .expect("is_excluded"),
        "the registry answers what Guard 2 asks"
    );

    // 4. And the guard fires — this is the assertion that matters, because until this pass nothing in the codebase could make it
    //    true. Same caller, same work, same request as the baseline.
    let (status, body) = reader
        .post(&format!("/api/v1/works/{parent}/fork"), json!({}))
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the exclusion now refuses the fork: {body}"
    );
    assert!(
        body.to_string().contains("excluded"),
        "the refusal names the exclusion rather than a generic validation message: {body}"
    );

    // 5. A second registration is a conflict, not a second row: a registry cannot answer "when was this excluded" if it holds two
    //    answers.
    let (status, body) = operator
        .post(
            "/api/v1/admin/exclusions",
            json!({ "target_type": "work", "target_id": parent, "reason": "again" }),
        )
        .await;
    // `422` with a field, not `409`: the only `409` this error vocabulary carries is `REVISION_CONFLICT`, which would tell a
    // client a version raced — a different fact, and one this request never mentioned.
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.to_string().contains("already"),
        "the refusal says the target is already excluded: {body}"
    );

    // 6. An entry with no reason is refused: a decision nobody can explain is a decision nobody can review.
    //
    //    The target is a FRESH work, and that is the point of the fixture: reusing the already-excluded parent made this assertion
    //    pass for the wrong reason — the duplicate check answered 422 whether or not the reason was validated, and the mutant that
    //    deleted the reason check survived. An assertion that cannot tell two refusals apart is not testing either of them.
    let fresh = published_work(&mut author, "Remixable Two").await;
    let (status, body) = operator
        .post(
            "/api/v1/admin/exclusions",
            json!({ "target_type": "work", "target_id": fresh, "reason": "  " }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.to_string().contains("reason"),
        "the refusal names the reason field: {body}"
    );
    assert!(
        !darkhold_db::permission::is_excluded(
            &db,
            darkhold_domain::permission::ExclusionTarget::Work,
            &fresh
        )
        .await
        .expect("is_excluded"),
        "a refused registration leaves the registry alone"
    );

    // 7. The read door answers what was armed, and only for an operator.
    let (status, body) = operator
        .get(&format!("/api/v1/admin/exclusions?target_type=work&target_id={parent}"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["reason"], json!("author withdrew permission to remix"), "{body}");

    let (status, _) = author
        .get(&format!("/api/v1/admin/exclusions?target_type=work&target_id={parent}"))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a caller with no standing does not learn that this instance holds an exclusion list"
    );
    let (status, _) = author
        .post(
            "/api/v1/admin/exclusions",
            json!({ "target_type": "work", "target_id": parent, "reason": "mine now" }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 8. `creator` is stored, and the response says it is not consulted by anything yet — an operator must not be told an
    //    exclusion is in force when no gate reads it.
    let (status, body) = operator
        .post(
            "/api/v1/admin/exclusions",
            json!({ "target_type": "creator", "target_id": uuid::Uuid::new_v4().to_string(), "reason": "repeated plagiarism" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        body["enforced_by"],
        json!([]),
        "the answer names the gates that consult this target, and currently none do: {body}"
    );
}
