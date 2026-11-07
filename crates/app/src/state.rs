//! Shared application state.

use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use darkhold_db::Database;
use darkhold_scrapers::registry::Registry;

use crate::config::Config;
use crate::limiter::RateLimiter;
use crate::webhook_sender::WebhookSenderConfig;

/// Cloneable handle to everything a request needs.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    config: Config,
    db: Database,
    rate_limiter: RateLimiter,
    started_at: Instant,
    /// The source adapters.
    ///
    /// One instance for the whole process, shared by the preview a reader waits
    /// on and the import the worker runs later. Two registries would mean a URL
    /// could route to one adapter when previewed and another when imported, and
    /// the reader would have confirmed a plan that nothing then followed.
    registry: Registry,
    /// The extension sandbox's concurrency bound (M16-06).
    ///
    /// Per **state**, not a process static: the count belongs to the instance that runs work, and a static made every test in a binary
    /// share one counter — which is a real bug in tests and a lie about the design, since two `AppState`s are two servers.
    sandbox_slots: crate::routes::sandbox::Slots,
    /// The external converters this instance found at startup.
    ///
    /// Detected once rather than per request: what an instance can produce is a
    /// property of the machine it runs on, and an interface that offered a format
    /// on one request and not the next would be describing nothing.
    converters: crate::exports::Converters,
    /// The configured narration engine, built once at startup.
    ///
    /// Held rather than rebuilt per job for the same reason the converters are:
    /// which voices an instance can speak in is a property of the host, and a
    /// worker that rebuilt the engine per chunk would re-read the same paths on
    /// every one. A `Result` because an instance may name an engine this build
    /// does not have — that is reportable state, not a startup failure, so a
    /// deployment with a typo in `tts.engine` still serves its library.
    tts: Result<Arc<dyn crate::tts::TtsEngine>, String>,
    /// HTTP client for webhook delivery — built once, reused for all attempts.
    reqwest_client: reqwest::Client,
    /// The estate's taste service (`polaris-taste`): the works ladder, as client calls.
    ///
    /// Built once because it holds an HTTP client, and it is a *client* rather than a table because the rating
    /// of a work must mean the same thing on every product (ADR 0001). It degrades: with no secret configured
    /// `can_cast()` is false and the arena keeps its dimension calibration and loses only the shared ladder.
    taste: crate::taste::TasteClient,
    /// Webhook sender configuration.
    webhook_config: WebhookSenderConfig,
    /// The most recent shadow-mode evaluation, for the operator surface.
    ///
    /// Per instance, inside the `Arc`, rather than the process-global static
    /// this replaced. One process serving one instance is the production shape
    /// and the global modelled it correctly — but the test binary builds
    /// *many* instances in one process, so the global was every test's report
    /// to write and every other test's to read. `a_non_shadow_instance_reports_
    /// that_it_has_not_evaluated` failed whenever a shadow run finished first,
    /// and passed whenever it finished second: a test whose result depended on
    /// thread scheduling, and which read as a flake for weeks.
    ///
    /// Resetting a global before each test is not a fix, only a narrower race —
    /// another test can still record after the reset and before the assertion.
    /// Scoping the slot to the instance makes the two tests independent, which
    /// is the property was actually missing.
    shadow_report: crate::rec_shadow::ReportSlot,
    /// §59 — which registered surfaces this instance runs.
    ///
    /// Resolved once from validated configuration, so the per-request question
    /// "is this surface on" is a set lookup and never a config parse. The
    /// registry itself is `&'static`, so there is nothing here to invalidate:
    /// changing what an instance runs is a restart, which is what a config file
    /// edit already means.
    modules: darkhold_domain::modules::ModuleState,
    /// M45-47 — how big this instance is, counted once.
    ///
    /// A `OnceCell` on the instance, not a field resolved at startup, because the
    /// counts need the database and `AppState::new` is synchronous by design (tests
    /// build a router without a runtime). Counting once per process is also the
    /// honest granularity: a mechanism crosses its threshold on the scale of days,
    /// and a `COUNT(*)` per request would be a tax on the page least able to afford
    /// it. The consequence is stated where a reader will meet it: **a restart is
    /// what re-measures an instance**, the same way a restart is what re-reads the
    /// config.
    scale: Arc<tokio::sync::OnceCell<darkhold_domain::modules::Scale>>,
    /// M45-48: what this machine is. Measured once — RAM does not change under a
    /// process, and a tier that moves under load is a tier no test can pin.
    capacity: Arc<tokio::sync::OnceCell<darkhold_domain::capacity::Capacity>>,
}

impl AppState {
    /// Build state from a resolved configuration and an open database.
    #[must_use]
    pub fn new(config: Config, db: Database) -> Self {
        let rate_limiter = RateLimiter::new(config.rate_limits);
        let converters = crate::exports::Converters::detect();
        // `which piper` is the same discovery the converters use, so an operator
        // who installed Piper next to Calibre gets both without configuring
        // either by hand.
        let piper = converters.discover_program("piper");
        let tts = crate::tts::build_engine(&config.tts, piper.as_deref())
            .map(Arc::<dyn crate::tts::TtsEngine>::from)
            .map_err(|error| error.to_string());
        let reqwest_client = reqwest::Client::builder()
            .user_agent(format!("darkhold/{}", crate::version::VERSION))
            .redirect(reqwest::redirect::Policy::none()) // Disable redirects to prevent SSRF via redirect chains
            .build()
            .expect("reqwest client");
        let webhook_config = crate::webhook_sender::WebhookSenderConfig {
            timeout: Duration::from_secs(config.administration.webhook_timeout_secs),
            max_attempts: config.administration.webhook_max_attempts,
            base_delay: Duration::from_millis(config.administration.webhook_base_delay_ms),
            allowed_hosts: config.administration.webhook_allowed_hosts.clone(),
        };
        // Resolved before the literal: `config` is moved into `Inner` first.
        let modules = darkhold_domain::modules::ModuleState::resolve(
            &config.modules.disabled,
            &config.modules.enabled,
            config.modules.allow_user_overrides,
        );
        Self {
            inner: Arc::new(Inner {
                config,
                db,
                rate_limiter,
                started_at: Instant::now(),
                sandbox_slots: crate::routes::sandbox::Slots::new(
                    crate::routes::sandbox::MAX_CONCURRENT_RUNS,
                ),
                registry: darkhold_scrapers::sites::default_registry(),
                converters,
                tts,
                reqwest_client,
                webhook_config,
                taste: crate::taste::TasteClient::from_env(),
                shadow_report: crate::rec_shadow::ReportSlot::default(),
                modules,
                scale: Arc::new(tokio::sync::OnceCell::new()),
                capacity: Arc::new(tokio::sync::OnceCell::new()),
            }),
        }
    }

    /// The extension sandbox's concurrency bound (M16-06).
    #[must_use]
    pub fn sandbox_slots(&self) -> &crate::routes::sandbox::Slots {
        &self.inner.sandbox_slots
    }

    /// The active configuration.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    /// The source adapters.
    #[must_use]
    pub fn registry(&self) -> &Registry {
        &self.inner.registry
    }

    /// This instance's shadow-mode report slot.
    #[must_use]
    pub fn shadow_report(&self) -> &crate::rec_shadow::ReportSlot {
        &self.inner.shadow_report
    }

    /// The external converters this instance found at startup.
    #[must_use]
    pub fn converters(&self) -> &crate::exports::Converters {
        &self.inner.converters
    }

    /// The configured narration engine, or the reason this instance has none.
    ///
    /// Callers get a typed `Result` rather than an `Option` so the reason — a
    /// name this build does not know, a missing voice model — travels with the
    /// failure instead of being flattened into "unavailable".
    pub fn tts_engine(&self) -> Result<&dyn crate::tts::TtsEngine, &str> {
        match &self.inner.tts {
            Ok(engine) => Ok(engine.as_ref()),
            Err(reason) => Err(reason.as_str()),
        }
    }

    /// Whether a narration can be produced on this instance.
    #[must_use]
    pub fn can_narrate(&self) -> bool {
        self.inner
            .tts
            .as_ref()
            .map(|engine| engine.is_available())
            .unwrap_or(false)
    }

    /// HTTP client for webhook delivery.
    #[must_use]
    pub fn reqwest_client(&self) -> &reqwest::Client {
        &self.inner.reqwest_client
    }

    /// The estate's taste service client (ADR 0001).
    #[must_use]
    pub fn taste(&self) -> &crate::taste::TasteClient {
        &self.inner.taste
    }

    /// Webhook sender configuration.
    #[must_use]
    pub fn webhook_config(&self) -> &WebhookSenderConfig {
        &self.inner.webhook_config
    }

    /// Replace the adapters.
    ///
    /// For tests, which need an adapter that answers from a recorded page: a
    /// test that reaches the network fails on a plane, and an import rule that
    /// is only exercised against a live site is a rule nobody has pinned.
    #[must_use]
    pub fn with_registry(mut self, registry: Registry) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("state is configured before it is shared")
            .registry = registry;
        self
    }

    /// The database handle.
    #[must_use]
    pub fn db(&self) -> &Database {
        &self.inner.db
    }

    /// The resolved module state (§59).
    #[must_use]
    pub fn modules(&self) -> &darkhold_domain::modules::ModuleState {
        &self.inner.modules
    }

    /// How big this instance is, counted once (M45-47).
    ///
    /// `active_readers` counts accounts that are neither deleted nor in a non-active
    /// status (`deleted_at IS NULL AND status = 'active'` — the schema's spelling, not
    /// a `suspended_at` column this table does not have); `active_works` counts work
    /// rows. Both are `COUNT(*)` — facts, not scores — and the first
    /// caller pays for them. A failed count is **zero**, deliberately: a mechanism
    /// that cannot measure its instance should stay dormant, because turning on a
    /// crowd feature on the strength of a failed query is how "nothing here" pages
    /// get shipped.
    pub async fn scale(&self) -> darkhold_domain::modules::Scale {
        let scale = self
            .inner
            .scale
            .get_or_init(|| async {
                let readers = count(
                    self.db(),
                    "SELECT COUNT(*) FROM accounts WHERE deleted_at IS NULL AND status = 'active'",
                )
                .await;
                let works = count(self.db(), "SELECT COUNT(*) FROM works").await;
                darkhold_domain::modules::Scale {
                    active_readers: u32::try_from(readers.max(0)).unwrap_or(u32::MAX),
                    active_works: u32::try_from(works.max(0)).unwrap_or(u32::MAX),
                }
            })
            .await;
        *scale
    }

    /// What this machine is, measured once (M45-48).
    ///
    /// Every fact here can fail to be read, and every failure lands on the *conservative*
    /// side: cores default to 1, memory to 0, the cache to absent. `Minimal` is the safe
    /// wrong answer for a busy server; `Large` is the expensive one.
    pub async fn capacity(&self) -> darkhold_domain::capacity::Capacity {
        let capacity = self
            .inner
            .capacity
            .get_or_init(|| async {
                let cores = std::thread::available_parallelism()
                    .map(std::num::NonZeroUsize::get)
                    .unwrap_or(1);
                let memory_mb = read_memory_mb().unwrap_or(0);
                let engine = match self.db().backend() {
                    darkhold_db::Backend::Sqlite => darkhold_domain::capacity::Engine::Sqlite,
                    darkhold_db::Backend::Postgres => darkhold_domain::capacity::Engine::Postgres,
                };
                let has_redis = self.config().capacity.has_cache();
                darkhold_domain::capacity::Capacity {
                    cores,
                    memory_mb,
                    engine,
                    has_redis,
                }
            })
            .await;
        capacity.clone()
    }

    /// The rate limiter, shared across every request in this process.
    #[must_use]
    pub fn rate_limiter(&self) -> &RateLimiter {
        &self.inner.rate_limiter
    }

    /// Milliseconds since the process began serving.
    #[must_use]
    pub fn uptime_ms(&self) -> u64 {
        u64::try_from(self.inner.started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// One `COUNT(*)` on whichever engine this database is, or `-1` when the query
/// failed — so a caller can tell "zero rows" from "could not count".
async fn count(db: &Database, sql: &str) -> i64 {
    let result = match db.backend() {
        darkhold_db::Backend::Sqlite => {
            sqlx::query_scalar(sql)
                .fetch_one(db.sqlite_pool().expect("sqlite handle"))
                .await
        }
        darkhold_db::Backend::Postgres => {
            sqlx::query_scalar(sql)
                .fetch_one(db.postgres_pool().expect("postgres handle"))
                .await
        }
    };
    match result {
        Ok(count) => count,
        Err(error) => {
            // Not silent: a count that failed makes a mechanism look dormant, and
            // "the curation page is empty" with a failed query behind it is a bug
            // nobody would find from the page itself.
            tracing::warn!(%error, "instance scale count failed; treating the count as zero: {sql}");
            -1
        }
    }
}

/// `MemTotal` from `/proc/meminfo`, in megabytes.
///
/// `None` when the file is unreadable or the line is absent — the caller treats that as "unknown",
/// which lands on `Minimal`. A machine that cannot say how much memory it has is not a machine to
/// switch embeddings on for.
fn read_memory_mb() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text
        .lines()
        .find(|line| line.starts_with("MemTotal:"))?
        .to_owned();
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1_024)
}
