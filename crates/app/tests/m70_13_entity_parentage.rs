//! The entity tree had no door, so the closure was always empty (M70-13).
//!
//! `entity_closure::rebuild` builds the closure from `canonical_entities.parent_id`, and `descendants_of` — "the `+children`
//! expansion's answer" (spec §11.4) — reads the closure and had no caller. The chain is only as real as its first link, and the
//! first link was missing: **nothing in the product ever wrote `parent_id`**. Measured on the deployed database: 0 of 58,042
//! entities have a parent, `entity_closure` holds 0 rows, and `entity_edges` holds 0 rows. So the tree did not exist, the nightly
//! rebuild faithfully materialised nothing, and `+children` could never expand anything — not because expansion was unwritten,
//! but because there was never a way to say "this tag is a child of that one".
//!
//! This is the door for that sentence, and the test walks the whole chain rather than the door alone: set the parent, observe
//! that the closure is still empty (it is rebuilt by the maintenance pass, ~30s), run the rebuild the worker runs, and only then
//! require the descendant. A door whose effect is only claimed is how the closure got into this state.
//!
//! Three decisions, recorded beside the code:
//!
//! * **`parent_id`, not `entity_edges(rel = 'parent')`.** Both vocabularies exist (0124 renamed the graph tables, and 0104's edge
//!   table still accepts `rel = 'parent'`), but the closure walks the column. Writing the edge would be a second place to say the
//!   same thing that nothing reads — the exact defect this file is about.
//! * **A cycle is refused at the door.** The rebuild's visited set terminates whatever the data contains (that is why the walk is
//!   in Rust), but a cycle in a *tree* is a curator mistake, and the place to say so is where it is made, not in a walk that
//!   quietly tolerates it.
//! * **Below the floor is not a node.** §6.4/R-23: an entity below the floor is an assertion, and an assertion is not in the tree,
//!   so a parent edge to or from one would describe a tree that does not exist.

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

async fn a_node(author: &mut TestClient, canonical: &str) -> String {
    let (status, body) = author
        .post(
            "/api/v1/taxonomy",
            json!({ "kind": "tag", "canonical": canonical }),
        )
        .await;
    assert!(status.is_success(), "{body}");
    body["node"]["id"].as_str().expect("node id").to_owned()
}

async fn account_of(client: &mut TestClient) -> String {
    let (status, me) = client.get("/api/v1/auth/me").await;
    assert_eq!(status, StatusCode::OK, "{me}");
    me["account"]["id"].as_str().expect("account id").to_owned()
}

/// Grant the curator role the way `media_resilience::is_active_curator` reads it.
async fn make_curator(db: &Database, account: &str) {
    // Each arm is a block: the two executors return different result types, and a statement with a
    // value is a statement whose match arms do not unify.
    match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO curator_roles (account_id, opted_in_by) VALUES (?, 'test')
                 ON CONFLICT(account_id) DO NOTHING",
            )
            .bind(account)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await
            .expect("grant curator (sqlite)");
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query(
                "INSERT INTO curator_roles (account_id, opted_in_by) VALUES ($1, 'test')
                 ON CONFLICT (account_id) DO NOTHING",
            )
            .bind(account)
            .execute(db.postgres_pool().expect("postgres"))
            .await
            .expect("grant curator (postgres)");
        }
    }
}

async fn parent_of(db: &Database, node: &str) -> Option<String> {
    // The column is nullable, so the scalar has to be `Option<String>`: `fetch_one` into `String` would reject the NULL that
    // means "no parent" — a decode error standing in for the answer.
    let value: Option<String> = match db.backend() {
        darkhold_db::Backend::Sqlite => sqlx::query_scalar::<_, Option<String>>(
            "SELECT parent_id FROM canonical_entities WHERE id = ?",
        )
        .bind(node)
        .fetch_one(db.sqlite_pool().expect("sqlite"))
        .await
        .expect("parent (sqlite)"),
        darkhold_db::Backend::Postgres => sqlx::query_scalar::<_, Option<String>>(
            "SELECT parent_id FROM canonical_entities WHERE id = $1",
        )
        .bind(node)
        .fetch_one(db.postgres_pool().expect("postgres"))
        .await
        .expect("parent (postgres)"),
    };
    value
}

#[tokio::test]
async fn a_curator_can_set_a_parent_and_the_closure_follows() {
    let dir = scratch_dir("entity-parentage");
    let tdb = TestDb::connect_with_dir("m7013-entity-parentage", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let mut curator = TestClient::new(build(&config, &db));
    register(&mut curator, "ep-curator@example.test", "epcurator").await;
    let curator_account = account_of(&mut curator).await;
    make_curator(&db, &curator_account).await;

    let mut reader = TestClient::new(build(&config, &db));
    register(&mut reader, "ep-reader@example.test", "epreader").await;

    let parent = a_node(&mut curator, "angst").await;
    let child = a_node(&mut curator, "angst-adjacent").await;
    let door = format!("/api/v1/taxonomy/{child}/parent");

    // 1. A reader cannot restructure the vocabulary. The refusal is 403 and not 404: the door is published, and the caller is
    //    told the act exists but is not theirs — hiding a curator act would leave a curator unable to discover it.
    let (status, body) = reader
        .request("PUT", &door, Some(json!({ "parent_id": parent })))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(parent_of(&db, &child).await, None, "{body}");

    // 2. A curator sets it, and the column is the place it lands — the same column `rebuild` walks.
    let (status, body) = curator
        .request("PUT", &door, Some(json!({ "parent_id": parent })))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        parent_of(&db, &child).await.as_deref(),
        Some(parent.as_str())
    );

    // 3. The closure is the maintenance pass's, not the door's, and the test says so rather than pretending otherwise: before the
    //    rebuild the descendant is not there yet.
    assert!(
        darkhold_db::entity_closure::descendants_of(&db, &parent)
            .await
            .expect("descendants")
            .is_empty(),
        "the closure is rebuilt by the maintenance pass; the door does not write it"
    );

    // 4. Run the pass the worker runs. Now the expansion exists, which is what §11.4 promises and what nothing could produce.
    let report = darkhold_db::entity_closure::rebuild(&db)
        .await
        .expect("rebuild");
    assert!(report.rows >= 1, "{report:?}");
    let descendants = darkhold_db::entity_closure::descendants_of(&db, &parent)
        .await
        .expect("descendants");
    assert_eq!(
        descendants,
        vec![(child.clone(), 1)],
        "the child did not reach the closure"
    );

    // 5. A cycle is refused where it is made. `rebuild`'s visited set would terminate anyway — that is why the walk is in Rust —
    //    but a cycle is a mistake, and a mistake should be answered, not tolerated.
    let (status, body) = curator
        .request(
            "PUT",
            &format!("/api/v1/taxonomy/{parent}/parent"),
            Some(json!({ "parent_id": child })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.to_string().contains("parent_id"), "{body}");
    assert_eq!(
        parent_of(&db, &parent).await,
        None,
        "a refused cycle must not half-apply"
    );

    // 6. Self-parenthood is the degenerate cycle, and it is refused by name too.
    let (status, body) = curator
        .request(
            "PUT",
            &format!("/api/v1/taxonomy/{child}/parent"),
            Some(json!({ "parent_id": child })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // 7. Detaching is a first-class act: a curator who attached by mistake must be able to say so. `null` is that sentence, and
    //    it is not the same request as "parent_id: \"\"" — an empty id is a malformed id, not an absence.
    let (status, body) = curator
        .request("PUT", &door, Some(json!({ "parent_id": null })))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parent_of(&db, &child).await, None);

    // 8. An unknown node is 404 — there is no node whose parent could be set.
    let (status, body) = curator
        .request(
            "PUT",
            "/api/v1/taxonomy/00000000-0000-4000-8000-000000000000/parent",
            Some(json!({ "parent_id": parent })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// §11.4's expansion, end to end: a parent tag's browse surface can include its children's works, and says so only when asked.
#[tokio::test]
async fn a_parent_tag_can_include_its_childrens_works() {
    let dir = scratch_dir("entity-expansion");
    let tdb = TestDb::connect_with_dir("m7013-entity-expansion", &dir).await;
    let db: Database = tdb.db().clone();
    let config = Config::development_defaults();

    let mut author = TestClient::new(build(&config, &db));
    register(&mut author, "ex-author@example.test", "exauthor").await;

    let parent = a_node(&mut author, "fluff").await;
    let child = a_node(&mut author, "fluff-adjacent").await;

    // Give the author the curator role: the parent edge is a curator act.
    let author_account = account_of(&mut author).await;
    make_curator(&db, &author_account).await;
    let (status, body) = author
        .request(
            "PUT",
            &format!("/api/v1/taxonomy/{child}/parent"),
            Some(json!({ "parent_id": parent })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // The closure is the maintenance pass's work; run the pass rather than waiting ~30s.
    darkhold_db::entity_closure::rebuild(&db)
        .await
        .expect("rebuild");

    // Two published works: one tagged with the parent, one only with the child. An `async fn` rather than a closure so the
    // helper's future has one concrete type.
    async fn publish_tagged(author: &mut TestClient, tag: &str, title: &str) -> String {
        let (status, work) = author
            .post("/api/v1/works", json!({ "title": title }))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{work}");
        let work_id = work["id"].as_str().expect("work id").to_owned();
        let (status, body) = author
            .post(
                &format!("/api/v1/works/{work_id}/tags"),
                json!({ "node_id": tag }),
            )
            .await;
        assert!(status.is_success(), "tag: {body}");
        // A work needs one unit before it can be published (§ publication rule), and the unit is also what makes the browse row
        // real rather than a draft that the listing filters out.
        let (status, unit) = author
            .post(
                &format!("/api/v1/works/{work_id}/chapters"),
                json!({ "title": "One" }),
            )
            .await;
        assert!(status.is_success(), "unit: {unit}");
        let unit_id = unit["id"].as_str().expect("unit id").to_owned();
        // An empty work has nothing to read, so a unit needs text before the work can be published.
        let (status, body) = author
            .request(
                "PATCH",
                &format!("/api/v1/chapters/{unit_id}"),
                Some(json!({
                    "expected_version": unit["version"].as_i64().unwrap_or(1),
                    "document": {
                        "type": "doc",
                        "content": [{
                            "type": "paragraph",
                            "content": [{ "type": "text", "text": "Some words a reader can open." }]
                        }]
                    }
                })),
            )
            .await;
        assert!(status.is_success(), "write unit: {body}");
        // Publication is versioned (an author publishing from a stale tab is refused by name), so the request carries the version
        // the create answered with rather than a guess.
        let (status, current) = author.get(&format!("/api/v1/works/{work_id}")).await;
        assert_eq!(status, StatusCode::OK, "{current}");
        let version = current["version"].as_i64().unwrap_or(1);
        let (status, body) = author
            .post(
                &format!("/api/v1/works/{work_id}/publish"),
                json!({ "expected_version": version }),
            )
            .await;
        assert!(status.is_success(), "publish: {body}");
        work_id
    }
    let parent_work = publish_tagged(&mut author, &parent, "Parent only").await;
    let child_work = publish_tagged(&mut author, &child, "Child only").await;
    // A work carrying BOTH names is one work. This is the row that proves the membership test is an `IN` and not a join: a join
    // would return it twice in the expanded listing, once per matching tag row.
    let both_work = publish_tagged(&mut author, &parent, "Both").await;
    let (status, body) = author
        .post(
            &format!("/api/v1/works/{both_work}/tags"),
            json!({ "node_id": child }),
        )
        .await;
    assert!(status.is_success(), "second tag: {body}");

    // 1. Without the query, the surface is exactly what it always was: this tag's own works.
    let (status, listed) = author.get("/api/v1/tags/fluff").await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let mut ids: Vec<&str> = listed["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|item| item["work_id"].as_str())
        .collect();
    ids.sort_unstable();
    let mut expected_unexpanded = vec![parent_work.as_str(), both_work.as_str()];
    expected_unexpanded.sort_unstable();
    assert_eq!(ids, expected_unexpanded, "{listed}");

    // 2. With `children=true`, the child's work is included — the expansion §11.4 promises and nothing could produce before the
    //    tree had a write.
    let (status, expanded) = author.get("/api/v1/tags/fluff?children=true").await;
    assert_eq!(status, StatusCode::OK, "{expanded}");
    let mut expanded_ids: Vec<&str> = expanded["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|item| item["work_id"].as_str())
        .collect();
    expanded_ids.sort_unstable();
    assert_eq!(
        expanded_ids.len(),
        3,
        "a work carrying both names must appear once, not once per matching tag: {expanded}"
    );
    let mut expected = vec![
        parent_work.as_str(),
        child_work.as_str(),
        both_work.as_str(),
    ];
    expected.sort_unstable();
    assert_eq!(expanded_ids, expected, "{expanded}");

    // 3. A name the vocabulary does not hold answers an empty page either way — the expansion must not turn a 200 into a 404.
    let (status, unknown) = author.get("/api/v1/tags/not-a-tag?children=true").await;
    assert_eq!(status, StatusCode::OK, "{unknown}");
    assert_eq!(
        unknown["items"].as_array().expect("items").len(),
        0,
        "{unknown}"
    );
}
