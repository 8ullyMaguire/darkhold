//! Run the moderation ladder sweep and print what it found, as JSON.
//!
//! This is the entry point a scheduled job calls. It prints and does not grant: `polaris-id` owns entitlements, and a job that both
//! decided and granted could not be tested in halves. Output shape:
//!
//! ```json
//! {"changes":N,"initialised":N,"crossings":["<account>", ...]}
//!
//! Run: DARKHOLD_LADDER_DB=postgresql:///darkhold?host=/run/postgresql cargo run -p darkhold-db --example sweep_crossings
use std::time::Duration;

use darkhold_db::Database;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DARKHOLD_LADDER_DB")
        .unwrap_or_else(|_| "postgresql:///darkhold?host=/run/postgresql".to_owned());
    let accepted_since = std::env::var("DARKHOLD_LADDER_SINCE").ok();

    let db = Database::connect(&darkhold_db::DatabaseConfig {
        url,
        max_connections: 2,
        acquire_timeout: Duration::from_secs(10),
        slow_query_warn: Duration::ZERO,
    })
    .await?;

    let settings = darkhold_db::governance::TrustSettings {
        ladder: &darkhold_domain::trust::TrustLadder::default_ladder(),
        axes: &darkhold_domain::trust::TrustAxes::default(),
        requirements: &darkhold_domain::trust::TrustRequirements::default(),
        policy: darkhold_domain::trust::ElasticPolicy::default(),
        guest_level: 1,
        accepted_since: accepted_since.as_deref(),
        velocity: false,
    };

    let sweep = darkhold_db::moderation_ladder::sweep(&db, &settings).await?;
    let crossings: Vec<String> = sweep.crossings.iter().map(|c| format!("\"{c}\"")).collect();
    println!(
        "{{\"changes\":{},\"initialised\":{},\"crossings\":[{}]}}",
        sweep.changes.len(),
        sweep.initialised,
        crossings.join(",")
    );
    db.close().await;
    Ok(())
}
