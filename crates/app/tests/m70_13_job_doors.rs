//! The queue can say what it is doing, and it keeps the job it took (M70-13).
//!
//! Three store functions in `jobs.rs` had no caller at all, and one of them was load-bearing:
//!
//! * `heartbeat` — the claim is a *lease* (`options.lease`, 120s by default) and the maintenance sweep returns expired
//!   leases to the queue. Nothing renewed it, so a job that outlived its lease was handed to a **second worker while the
//!   first was still writing** — the same import running twice over the same rows. The proof below is a probe that runs
//!   longer than a deliberately tiny lease, with a second worker asking for work while it runs: with renewal the second
//!   worker finds nothing, without it the second worker takes the job away.
//! * `counts_by_state` — the operator's dashboard. Every state is present, including the empty ones, because a summary built
//!   from `GROUP BY state` cannot distinguish "no failures today" from "nobody asked".
//! * `attempts_for` — the per-job history. A job that failed three times and succeeded on the fourth showed only its last
//!   error; now the owner can see all four attempts.
//!
//! Refusals are part of the contract: the summary is `404` to anyone without an estate claim (not `403` — confirming the
//! surface exists would map this instance's operator configuration), and a job id is not a capability, so the attempts of
//! somebody else's job are `404` rather than empty.

use std::time::Duration;

use axum::http::StatusCode;
use darkhold_app::config::Config;
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_app::worker::{Worker, WorkerOptions};
use darkhold_db::{jobs, Database};
use darkhold_domain::jobs::{JobKind, JobState, RetryPolicy, ALL_STATES};
use test_support::{register, scratch_dir, sign_in_as, TestClient, TestDb};

fn build(config: &Config, db: &Database) -> axum::Router {
    server::build_router(AppState::new(config.clone(), db.clone()))
}

#[tokio::test]
async fn the_lease_is_renewed_the_summary_counts_every_state_and_the_attempts_are_the_owners() {
    // The two lists must agree in both directions, or the dashboard silently drops a state: every state the parser accepts
    // must appear in `ALL_STATES`, and every entry there must parse back from its own wire spelling.
    assert_eq!(ALL_STATES.len(), 6, "a state was added without an entry in ALL_STATES");
    for state in ALL_STATES {
        assert_eq!(
            JobState::parse(state.as_str()),
            Some(*state),
            "{state:?} does not round-trip through its wire spelling"
        );
    }

    let dir = scratch_dir("job-doors");
    let tdb = TestDb::connect_with_dir("m7013-job-doors", &dir).await;
    let db: Database = tdb.db().clone();

    // The operator is configured after registering, so the router is rebuilt and the session signed in again — the shape
    // every other admin door's test uses (the cookie names an account the new config has to know about).
    let base = Config::development_defaults();
    let mut bootstrap = TestClient::new(build(&base, &db));
    let operator_account = register(&mut bootstrap, "jd-operator@example.test", "jdoperator").await;
    let mut config = base.clone();
    config.administration.operator_account_id = Some(operator_account.parse().expect("account id"));
    let mut operator = TestClient::new(build(&config, &db));
    sign_in_as(&mut operator, &tdb, "jd-operator@example.test", "jdoperator").await;

    let mut reader = TestClient::new(build(&config, &db));
    let reader_account = register(&mut reader, "jd-reader@example.test", "jdreader").await;
    let mut stranger = TestClient::new(build(&config, &db));
    register(&mut stranger, "jd-stranger@example.test", "jdstranger").await;

    // A probe that outlives its lease: eight steps of 600ms is nearly five seconds of work against a two-second lease. The
    // payload is the diagnostic `POST /jobs` accepts in development, so this is the job a person can actually start.
    //
    // Two seconds, not two hundred milliseconds: lease stamps are written at second resolution (`format_rfc3339` truncates),
    // so a sub-second lease is already expired the moment it is written and no renewal interval can save it. That is a limit
    // of the stamp, not of the renewal — the default lease is 120s precisely because these are seconds.
    let job = jobs::enqueue(
        &db,
        JobKind::Maintenance,
        r#"{"task":"probe","steps":8,"delay_ms":600}"#,
        None,
        Some(reader_account.parse().expect("account id")),
        0,
        &RetryPolicy::default(),
    )
    .await
    .expect("enqueue the probe");

    // 1. The summary before anything runs: every state present, one job queued, and the count is where it is.
    let (status, summary) = operator.get("/api/v1/admin/jobs/summary").await;
    assert_eq!(status, StatusCode::OK, "{summary}");
    assert_eq!(
        summary["states"].as_array().expect("states").len(),
        ALL_STATES.len(),
        "every state is listed, empty or not: {summary}"
    );
    assert_eq!(summary["total"], serde_json::json!(1), "{summary}");
    let queued = summary["states"]
        .as_array()
        .expect("states")
        .iter()
        .find(|row| row["state"] == serde_json::json!("queued"))
        .expect("a queued row");
    assert_eq!(queued["count"], serde_json::json!(1), "{summary}");
    let failed = summary["states"]
        .as_array()
        .expect("states")
        .iter()
        .find(|row| row["state"] == serde_json::json!("failed"))
        .expect("a failed row, present at zero");
    assert_eq!(failed["count"], serde_json::json!(0), "{summary}");

    // 2. A reader without an estate claim gets the absent-page answer, not a confirmation that the surface exists.
    let (status, _) = reader.get("/api/v1/admin/jobs/summary").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the summary is not a reader's surface");

    // 3. Run the probe on a worker with a lease far shorter than the work, and ask for the queue from a second worker while
    //    it runs. The lease is the only thing keeping the job out of the second worker's hands, and only a renewal keeps it
    //    alive past 150ms.
    let mut options = WorkerOptions::named("jd-worker");
    options.lease = Duration::from_secs(2);
    options.fairness = false;
    let worker = Worker::new(options);
    let running = {
        let state = AppState::new(config.clone(), db.clone());
        tokio::spawn(async move { worker.run_once(&state).await })
    };

    // Past one whole lease of the job's life, and still inside its work: with renewal the lease now expires around
    // second four, without renewal it expired at second two.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // The sweep is the half that makes an expired lease dangerous: a *queued* job is claimable, a `leased` one is not, and
    // `requeue_expired_leases` is what moves the second to the first. So the second worker runs the maintenance pass
    // production runs and then asks for work — one call alone cannot steal a lease, which is why an earlier version of this
    // test passed with renewal disabled.
    let mut thief_options = WorkerOptions::named("jd-other-worker");
    thief_options.lease = Duration::from_secs(2);
    let thief_worker = Worker::new(thief_options);
    let thief_state = AppState::new(config.clone(), db.clone());
    thief_worker
        .maintenance_pass(&thief_state)
        .await
        .expect("the maintenance sweep");
    let thief = jobs::claim_next(
        &db,
        "jd-other-worker",
        Duration::from_secs(2),
        time::OffsetDateTime::now_utc(),
        None,
        1,
        false,
    )
    .await
    .expect("a second worker asks for work");
    // What must not happen is that the *probe* is claimed. The sweep also enqueues the syndication pass, so "the queue is
    // empty" would be the wrong assertion: the second worker may legitimately find the job the sweep just created.
    let stolen = thief
        .as_ref()
        .is_some_and(|claimed| claimed.id == job.to_string());
    assert!(
        !stolen,
        "the lease must still be held: a job that outlives its claim without renewal is requeued by the sweep and handed \
         to a second worker, which is the same import running twice (claimed {:?})",
        thief.as_ref().map(|job| job.id.clone())
    );
    let still_ours = jobs::find(&db, job)
        .await
        .expect("find")
        .expect("the probe is still a row");
    assert_eq!(
        still_ours.lease_owner.as_deref(),
        Some("jd-worker"),
        "the probe is still leased to the worker running it"
    );

    let report = running.await.expect("the worker task").expect("the pass");
    assert_eq!(report.job.as_ref().map(|(_, state)| *state), Some(JobState::Succeeded));

    // 4. One attempt, and it finished: no second worker ever started the job.
    let attempts = jobs::attempts_for(&db, job).await.expect("attempts");
    assert_eq!(attempts.len(), 1, "one attempt, not one per worker: {attempts:?}");
    assert_eq!(attempts[0].outcome.as_deref(), Some("succeeded"));
    assert_eq!(attempts[0].worker, "jd-worker");

    // 5. The owner's view: the history, without this instance's worker identity.
    let (status, view) = reader.get(format!("/api/v1/jobs/{job}/attempts")).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    let rows = view["attempts"].as_array().expect("attempts");
    assert_eq!(rows.len(), 1, "{view}");
    assert_eq!(rows[0]["attempt"], serde_json::json!(1), "{view}");
    assert_eq!(rows[0]["outcome"], serde_json::json!("succeeded"), "{view}");
    assert!(rows[0]["finished_at"].as_str().is_some(), "{view}");
    assert!(
        rows[0].get("worker").is_none(),
        "the worker id is an operational detail, not part of the caller's job: {view}"
    );

    // 6. The operator's view: the same rows plus which worker ran them.
    let (status, view) = operator.get(format!("/api/v1/jobs/{job}/attempts")).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(
        view["attempts"][0]["worker"],
        serde_json::json!("jd-worker"),
        "{view}"
    );

    // 7. A stranger's probe is `404`: a job id is not a capability, and an empty list would confirm the id exists.
    let (status, body) = stranger.get(format!("/api/v1/jobs/{job}/attempts")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // 8. The summary now counts the finished job in `succeeded`, and `queued` is back to zero: the counts follow the rows.
    let (status, summary) = operator.get("/api/v1/admin/jobs/summary").await;
    assert_eq!(status, StatusCode::OK, "{summary}");
    let count_of = |summary: &serde_json::Value, state: &str| {
        summary["states"]
            .as_array()
            .expect("states")
            .iter()
            .find(|row| row["state"] == serde_json::json!(state))
            .map_or(-1, |row| row["count"].as_i64().expect("a count"))
    };
    assert_eq!(count_of(&summary, "succeeded"), 1, "{summary}");
    // The maintenance pass above also enqueues the syndication pass, and the second worker leased it, so the totals are not
    // ours to pin. What must hold is that the count is the count: the states add up to the total, and every one of them is
    // listed.
    let summed: i64 = summary["states"]
        .as_array()
        .expect("states")
        .iter()
        .map(|row| row["count"].as_i64().expect("a count"))
        .sum();
    assert_eq!(summed, summary["total"].as_i64().expect("a total"), "{summary}");
}
