//! The under-mirrored queue, the proposal record, and the cap the bounty was never paid against (M70-13).
//!
//! Four functions in `media_resilience.rs` had no caller, and they were four different failures:
//!
//! * `sum_curator_rewards_today` — the daily cap. The config declared `daily_credits_cap`, `caps::within_daily_cap` was a
//!   tested predicate, and the sum was a query: **none of the three ran in the award path**, which hard-coded `15` and paid
//!   on every call. The same mirror added a hundred times paid a hundred times. The link is still kept when the cap is
//!   reached — the correction is what the feature is for — but the credits are paid to the cap and the response says so.
//! * `find_references_below_threshold` — the queue the bounties are paid against. A curator could only find an
//!   under-mirrored reference by stumbling into it.
//! * `find_match_proposal` — what happened to a proposal, which is the only record of a merge that was *refused*: the list
//!   route shows what is pending, and a rejected merge leaves no other trace.
//! * `find_work_media_references` — not wired but **deleted**: a narrower twin of `list_work_media_references`, which the
//!   `GET /works/{id}/media` route already calls and which answers the same rows with each link's health and best mirror.
//!   Two functions answering one question is how the two answers drift.
//!
//! The refusals follow this file's existing rule (trust level, `403`) rather than the estate-hidden `404` used by
//! `admin_scope` — the neighbouring media doors assert the same answer, and one file answering two ways for one caller is
//! worse than either answer. What matters here is that a proposal id is a capability: it is refused to a reader and
//! "not found" for a malformed one, never a 500 from PostgreSQL's uuid cast.

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::{governance, media_resilience, Database};
use darkhold_domain::media_resilience::{LinkProvider, LinkStatus, MediaKind};
use test_support::{id, register, scratch_dir, sign_in_as, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

#[tokio::test]
async fn the_cap_is_paid_against_the_queue_is_real_and_a_refused_merge_is_still_readable() {
    let dir = scratch_dir("media-doors");
    let tdb = TestDb::connect_with_dir("m7013-media-doors", &dir).await;
    let db: Database = tdb.db().clone();

    // A cap low enough to reach in three mirrors: the bounty is 15 and the cap 20, so the second contribution pays 5 and the
    // third pays nothing. Testing a 500-credit cap would take thirty-four requests and prove the same three branches.
    let base = Config::development_defaults();
    let mut bootstrap = TestClient::new(build(&base, &db));
    let operator_account = register(&mut bootstrap, "md-operator@example.test", "mdoperator").await;
    let mut config = base.clone();
    config.administration.operator_account_id = Some(operator_account.parse().expect("account id"));
    config.media_resilience.daily_credits_cap = 20;
    config.media_resilience.mirror_add_credits = 15;
    let mut operator = TestClient::new(build(&config, &db));
    sign_in_as(
        &mut operator,
        &tdb,
        "md-operator@example.test",
        "mdoperator",
    )
    .await;
    // The mirror route's own gate is trust level 5 or an active curator role; the configured operator is neither until trust
    // is granted, so grant it here rather than duplicating the route's rule in the test.
    governance::set_trust(&db, &operator_account, 5, "{}")
        .await
        .expect("grant operator trust");

    let mut reader = TestClient::new(build(&config, &db));
    let reader_account = register(&mut reader, "md-reader@example.test", "mdreader").await;
    assert_ne!(reader_account, operator_account);

    // Two references: one under-mirrored (two healthy links), one properly mirrored (three). The threshold decides which is
    // in the queue, so the fixture has to contain both or the filter proves nothing.
    //
    // The ids are UUIDs because `media_references.id` is a UUID column on PostgreSQL. `test_support::id` derives one from a
    // label, so the test reads by name and the database still gets a type it accepts.
    let at_risk = id("md-ref-at-risk");
    let fine = id("md-ref-fine");
    for (reference, hash) in [(&at_risk, "hash-at-risk"), (&fine, "hash-fine")] {
        media_resilience::insert_media_reference(&db, reference, hash, MediaKind::Image)
            .await
            .expect("insert reference");
    }
    for (index, reference, healthy) in [("1", &at_risk, 2), ("2", &fine, 3)] {
        for n in 0..healthy {
            let link = id(&format!("md-link-{index}-{n}"));
            media_resilience::insert_availability_link(
                &db,
                &link,
                reference,
                &format!("https://example.test/{index}/{n}.png"),
                LinkProvider::Other,
                None,
                50,
            )
            .await
            .expect("insert link");
            // A link added through the route starts `pending_verification`, and the queue counts only `healthy`: the fixture
            // writes the status the counter reads, or the test would measure the wrong column.
            media_resilience::update_link_status(&db, &link, LinkStatus::Healthy, 0)
                .await
                .expect("mark the link healthy");
        }
    }

    // 1. The queue, at the spec's own threshold: fewer than three healthy links. The under-mirrored reference is in it and
    //    the properly mirrored one is not.
    let (status, queue) = operator.get("/api/v1/admin/media/references/at-risk").await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(queue["threshold"], serde_json::json!(3), "{queue}");
    let at_risk_rows: Vec<String> = queue["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|row| row["id"].as_str().expect("an id").to_owned())
        .collect();
    assert_eq!(at_risk_rows, vec![at_risk.clone()], "{queue}");

    // 2. The threshold is a parameter, not a constant: at one, two healthy links is enough and the queue is empty.
    let (status, queue) = operator
        .get("/api/v1/admin/media/references/at-risk?threshold=1")
        .await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(queue["threshold"], serde_json::json!(1), "{queue}");
    assert_eq!(
        queue["items"].as_array().expect("items").len(),
        0,
        "{queue}"
    );

    // 3. A reader is refused. This file's admin doors answer `403` rather than the estate-hidden `404` — the documented
    //    decision here is that a signed-in reader who is not yet a curator can act on "not yet", and the media suites assert
    //    the same answer on the neighbouring doors. Routing one door of the file through a different authority would make
    //    the surface answer two ways for the same caller.
    let (status, _) = reader.get("/api/v1/admin/media/references/at-risk").await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the queue is a curator's surface"
    );

    // 4. Three mirrors, and the cap decides what each one pays: 15, then 5, then nothing. Every contribution is kept.
    let mut awarded = Vec::new();
    let mut capped = Vec::new();
    for n in 0..3 {
        let (status, body) = operator
            .post(
                &format!("/api/v1/media/references/{at_risk}/mirrors"),
                serde_json::json!({ "url": format!("https://mirror.example.test/{n}.png") }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        awarded.push(body["credits_awarded"].as_i64().expect("credits_awarded"));
        capped.push(body["credits_capped"].as_bool().expect("credits_capped"));
    }
    assert_eq!(
        awarded,
        vec![15, 5, 0],
        "the cap is the third contribution's whole answer"
    );
    assert_eq!(
        capped,
        vec![false, true, true],
        "and it is reported, not silent"
    );

    // 5. The ledger agrees with the responses: the sum never passed the cap.
    let earned = media_resilience::sum_curator_rewards_today(&db, &operator_account)
        .await
        .expect("the day's credits");
    assert_eq!(earned, 20, "paid to the cap and no further");

    // 6. A proposal, as the matcher records one, then resolved with a refusal — the record the detail route exists for.
    media_resilience::record_match_proposal(
        &db,
        &at_risk,
        &fine,
        "hash-at-risk",
        Some("0000000000000000"),
        2,
    )
    .await
    .expect("record the proposal");
    let pending = media_resilience::list_pending_match_proposals(&db, 10)
        .await
        .expect("the pending list");
    let proposal_id = pending
        .iter()
        .find(|row| row.proposal.candidate_reference_id == at_risk)
        .map(|row| row.proposal.id.clone())
        .expect("the proposal is pending");
    media_resilience::resolve_match_proposal(
        &db,
        &proposal_id,
        darkhold_db::media_resilience::ProposalDecision::Reject,
        &operator_account,
        Some("different crops of the same photo; not a duplicate"),
    )
    .await
    .expect("resolve the proposal");

    let (status, view) = operator
        .get(format!("/api/v1/media/match-proposals/{proposal_id}"))
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["status"], serde_json::json!("rejected"), "{view}");
    assert_eq!(
        view["resolution_note"],
        serde_json::json!("different crops of the same photo; not a duplicate"),
        "the reason survives the decision: {view}"
    );
    assert_eq!(view["hamming_distance"], serde_json::json!(2), "{view}");

    // 7. A proposal id is a capability, and the two ways of not having it answer the same: a reader is refused by the door,
    //    and a malformed id is not found rather than a 500 from PostgreSQL's uuid cast.
    let (status, _) = reader
        .get(format!("/api/v1/media/match-proposals/{proposal_id}"))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = operator
        .get("/api/v1/media/match-proposals/not-a-uuid")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 8. The surviving twin still answers the route it serves: `/works/{id}/media` is `list_work_media_references`, and the
    //    deletion of `find_work_media_references` must not have taken the live view with it.
    // A work id is a UUID too, and the route casts it: a hand-written `md-work` is a 500 on PostgreSQL, not an empty list.
    let work = id("md-work");
    let (status, view) = reader.get(&format!("/api/v1/works/{work}/media")).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["items"], serde_json::json!([]), "{view}");
}
