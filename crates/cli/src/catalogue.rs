//! M63-32 — operator verb for the provenance tag backfill.
//!
//! It reads the instance database directly because this is a one-off data
//! repair, not a reader-facing API. Like every other mutating `dh` verb, it
//! previews by default and applies only with `--yes`.

use clap::Args;
use darkhold_db::{tag_backfill::backfill_from_provenance, Database, DatabaseConfig};

use crate::rec::{environment, Failure};

#[derive(Debug, Clone, Args)]
pub struct CatalogueArgs {
    /// Instance database URL.
    #[arg(long)]
    pub db_url: String,
    /// Limit the pass to one `library_items.source_key`.
    #[arg(long)]
    pub source: Option<String>,
    /// Maximum number of library items to examine.
    #[arg(long, default_value_t = 1000)]
    pub limit: usize,
}

pub async fn run(cli: &crate::Cli, args: &CatalogueArgs) -> Result<(), Failure> {
    let source = args.source.as_deref().unwrap_or("all");
    let preview = format!(
        "backfill source tags from provenance: source={source} limit={}",
        args.limit
    );
    let apply = crate::safety::mutating(cli, &preview)?;
    let db = Database::connect(&DatabaseConfig::new(&args.db_url))
        .await
        .map_err(|error| environment(error.to_string()))?;
    let report = backfill_from_provenance(&db, args.source.as_deref(), args.limit, !apply)
        .await
        .map_err(|error| environment(error.to_string()))?;
    println!(
        "source={source} scanned={} works_tagged={} assertions={} nodes_created={} applied={apply}",
        report.scanned, report.works_tagged, report.assertions, report.nodes_created
    );
    Ok(())
}
