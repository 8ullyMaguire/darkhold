//! `/metrics`, and the handful of counters darkhold actually decides on.
//!
//! # Why this product publishes metrics at all
//!
//! Measured 2026-11-05: `curl 127.0.0.1:8081/metrics` answered **200 with the SPA shell**, because the frontend
//! fallback serves every unknown path. A monitor checking status codes therefore believed darkhold published
//! metrics. The registry and the content type live in `polaris_core::metrics` — one implementation for the
//! estate, so four services cannot disagree about the format a scraper dispatches on — and this module is the
//! product's half: which numbers, incremented where the code already knows the answer.
//!
//! # What is counted, and what is deliberately not
//!
//! Three things someone can be wrong about while nothing errors:
//!
//! * **comments created** — a comment that silently fails to appear looks exactly like one nobody wrote
//! * **exports queued** — the preservation path; a queue that stops accepting jobs is the archive quietly ending
//! * **feed renders** — every poll of every saved view and of the catalogue, which is what a 304 makes cheap;
//!   without the number, "the feeds are fine" is a guess
//!
//! Not counted: total requests, per-route latency, per-account anything. The first is noise on a single-tenant
//! box, the second belongs to a proxy, and the third would put identity into a document built to be scraped.

use std::sync::LazyLock;

use axum::routing::get;
use axum::Router;
use polaris_core::metrics::{self, Counter};

use crate::state::AppState;

static COMMENTS_CREATED: LazyLock<&'static Counter> = LazyLock::new(|| {
    metrics::counter(
        "darkhold_comments_created_total",
        "Comments written to the database.",
    )
});
static EXPORTS_QUEUED: LazyLock<&'static Counter> = LazyLock::new(|| {
    metrics::counter(
        "darkhold_exports_queued_total",
        "Export jobs queued through the product's own door.",
    )
});
static FEED_RENDERS: LazyLock<&'static Counter> = LazyLock::new(|| {
    metrics::counter(
        "darkhold_feed_renders_total",
        "Feed documents rendered (saved views and the catalogue).",
    )
});

/// Mounted at the server root, beside the SPA fallback rather than under `/api/v1`: a scraper asks `/metrics`,
/// and a metric behind a versioned API prefix is one somebody has to explain to a monitoring vendor.
pub fn router() -> Router<AppState> {
    Router::new().route("/metrics", get(metrics))
}

async fn metrics() -> impl axum::response::IntoResponse {
    metrics::handler().await
}

/// Register the families at startup, without counting anything.
///
/// A counter that appears only after the first event is a counter a dashboard cannot graph from boot — measured
/// on `polaris-inbox`, whose `/metrics` answered `content-length: 0` until traffic arrived.
pub fn register() {
    LazyLock::force(&COMMENTS_CREATED);
    LazyLock::force(&EXPORTS_QUEUED);
    LazyLock::force(&FEED_RENDERS);
}

/// A comment that reached the database.
pub fn comment_created() {
    COMMENTS_CREATED.inc();
}

/// An export job the product accepted.
pub fn export_queued() {
    EXPORTS_QUEUED.inc();
}

/// One feed document rendered, whatever the format.
pub fn feed_rendered() {
    FEED_RENDERS.inc();
}
