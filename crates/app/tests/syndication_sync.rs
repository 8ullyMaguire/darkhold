//! `M67-03` — the scheduled pass is queued once per interval, and a disabled instance queues nothing.
//!
//! The interesting failure here is not "does it run" but **does it run once**: the sweep that calls
//! `enqueue_if_due` runs every few seconds, so an implementation that keyed on `now()` would queue a job
//! on every tick and turn a courtly background sync into a crawler. The bucket is the interval, and these
//! tests hold it to that.

use axum::body::Body;
use axum::http::Request;
use darkhold_app::config::{Config, SyndicationConfig};
use darkhold_app::server;
use darkhold_app::state::AppState;
use darkhold_db::DatabaseConfig;
use std::path::PathBuf;
use tower::ServiceExt;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("darkhold-syndication-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

struct Harness {
    _dir: PathBuf,
    tdb: test_support::TestDb,
    config: Config,
}

impl Harness {
    async fn new(tag: &str, syndication: SyndicationConfig) -> Self {
        let dir = scratch_dir(tag);
        let tdb = test_support::TestDb::connect_with_dir(tag, &dir).await;
        let mut config = Config::development_defaults();
        config.storage.root = dir.clone();
        config.database = DatabaseConfig::new(format!(
            "sqlite://{}/darkhold.sqlite?mode=rwc",
            dir.display()
        ));
        config.syndication = syndication;
        Self {
            _dir: dir,
            tdb,
            config,
        }
    }

    fn state(&self) -> AppState {
        AppState::new(self.config.clone(), self.tdb.db().clone())
    }
}

fn enabled() -> SyndicationConfig {
    SyndicationConfig {
        enabled: true,
        tags: vec!["Naruto".to_owned()],
        ..SyndicationConfig::default()
    }
}

async fn count_jobs(state: &AppState, kind: &str) -> i64 {
    let sql = state.db().sql(
        "SELECT COUNT(*) FROM jobs WHERE kind = ?",
        "SELECT COUNT(*)::bigint FROM jobs WHERE kind = $1",
    );
    // Each arm extracts the number: the dialects' row types differ, so a row cannot cross the match.
    match state.db().backend() {
        darkhold_db::Backend::Sqlite => {
            let row = sqlx::query(&sql)
                .bind(kind)
                .fetch_one(state.db().sqlite_pool().expect("sqlite"))
                .await
                .expect("count");
            use sqlx::Row as _;
            row.try_get::<i64, _>(0).expect("count column")
        }
        darkhold_db::Backend::Postgres => {
            let row = sqlx::query(&sql)
                .bind(kind)
                .fetch_one(state.db().postgres_pool().expect("postgres"))
                .await
                .expect("count");
            use sqlx::Row as _;
            row.try_get::<i64, _>(0).expect("count column")
        }
    }
}

#[tokio::test]
async fn a_disabled_instance_queues_nothing() {
    let harness = Harness::new(
        "disabled",
        SyndicationConfig {
            enabled: false,
            tags: vec!["Naruto".to_owned()],
            ..SyndicationConfig::default()
        },
    )
    .await;
    let state = harness.state();

    assert!(
        !darkhold_app::source_sync::enqueue_if_due(&state)
            .await
            .expect("enqueue"),
        "the feature is off"
    );
    assert_eq!(count_jobs(&state, "source_sync").await, 0);
    harness.tdb.db().close().await;
}

#[tokio::test]
async fn enabled_with_no_tags_queues_nothing_because_there_is_nothing_to_walk() {
    let harness = Harness::new(
        "untagged",
        SyndicationConfig {
            enabled: true,
            tags: Vec::new(),
            ..SyndicationConfig::default()
        },
    )
    .await;
    let state = harness.state();

    assert!(!darkhold_app::source_sync::enqueue_if_due(&state)
        .await
        .expect("enqueue"));
    assert_eq!(count_jobs(&state, "source_sync").await, 0);
    harness.tdb.db().close().await;
}

#[tokio::test]
async fn one_interval_queues_one_pass_no_matter_how_often_the_sweep_runs() {
    let harness = Harness::new("interval", enabled()).await;
    let state = harness.state();

    assert!(
        darkhold_app::source_sync::enqueue_if_due(&state)
            .await
            .expect("first sweep queues"),
        "the first sweep queues a pass"
    );
    // Ten sweeps in the same window: the sweep runs every few seconds, and a job per tick would make the
    // sync a crawler.
    for _ in 0..10 {
        assert!(
            !darkhold_app::source_sync::enqueue_if_due(&state)
                .await
                .expect("later sweeps"),
            "the same interval bucket is already queued"
        );
    }
    assert_eq!(count_jobs(&state, "source_sync").await, 1);
    harness.tdb.db().close().await;
}

/// The config surface an operator actually writes.
#[tokio::test]
async fn the_syndication_section_parses_and_defaults_to_off() {
    // Default: off, twelve hours, no tags. A fresh instance must not start fetching archives on boot.
    let default = SyndicationConfig::default();
    assert!(!default.enabled);
    assert_eq!(default.interval_hours, 12);
    assert!(default.tags.is_empty());
    assert_eq!(default.max_works_per_listing, 25);
    assert_eq!(default.interval().as_secs(), 12 * 60 * 60);

    // A zero interval cannot spin the queue: the floor is an hour.
    let zero = SyndicationConfig {
        interval_hours: 0,
        ..SyndicationConfig::default()
    };
    assert_eq!(zero.interval().as_secs(), 3600);
}

/// The catalogue is reachable from the app crate, which is what a status screen needs.
#[tokio::test]
async fn the_status_question_has_an_answer() {
    use darkhold_scrapers::Bodies;
    assert_eq!(
        darkhold_app::source_sync::bodies_for_host("squidgeworld.org"),
        Some(Bodies::Allowed)
    );
    assert_eq!(
        darkhold_app::source_sync::bodies_for_host("honeyfeed.fm"),
        Some(Bodies::Blocked),
        "a site that disallows everything is recorded as blocked, not merely unread"
    );
    let catalogue = darkhold_app::source_sync::catalogue().expect("the bundled catalogue parses");
    assert!(
        catalogue.syncable().count() >= 3,
        "the AO3-software archives are walkable"
    );
}

#[tokio::test]
async fn the_router_still_answers_while_a_sync_is_configured() {
    // A cheap guard against the wiring breaking the service: the app builds with the new module and
    // answers an unauthenticated door.
    let harness = Harness::new("router", enabled()).await;
    let state = harness.state();
    let response = server::build_router(state)
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert!(
        response.status().is_success() || response.status() == axum::http::StatusCode::NOT_FOUND,
        "the app answers: {}",
        response.status()
    );
    harness.tdb.db().close().await;
}
