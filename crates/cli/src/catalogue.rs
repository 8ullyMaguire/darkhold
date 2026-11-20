//! M63-32 — operator verb for catalogue tag backfill.
//!
//! It reads the instance database directly because this is a data repair, not
//! a reader-facing API. Like every other mutating `dh` verb, it previews by
//! default and applies only with `--yes`.

use clap::Args;
use darkhold_db::{
    tag_backfill::{assert_claims, backfill_from_provenance, Report},
    Backend, Database, DatabaseConfig,
};

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
    /// Re-fetch source metadata for works whose stored provenance has no tags.
    #[arg(long)]
    pub refetch: bool,
}

pub async fn run(cli: &crate::Cli, args: &CatalogueArgs) -> Result<(), Failure> {
    let source = args.source.as_deref().unwrap_or("all");
    let action = if args.refetch {
        "refetch source tags"
    } else {
        "backfill source tags from provenance"
    };
    let preview = format!("{action}: source={source} limit={}", args.limit);
    let apply = crate::safety::mutating(cli, &preview)?;
    let db = Database::connect(&DatabaseConfig::new(&args.db_url))
        .await
        .map_err(|error| environment(error.to_string()))?;
    let report = if args.refetch {
        refetch_source_tags(&db, source, args.limit, !apply)
            .await
            .map_err(|error| environment(error.to_string()))?
    } else {
        backfill_from_provenance(&db, args.source.as_deref(), args.limit, !apply)
            .await
            .map_err(|error| environment(error.to_string()))?
    };
    println!(
        "source={source} scanned={} works_tagged={} assertions={} nodes_created={} applied={apply}",
        report.scanned, report.works_tagged, report.assertions, report.nodes_created
    );
    Ok(())
}

/// Re-fetch metadata for works whose source key is known but whose stored
/// provenance carries no tags. Uses the site adapter's own URL routing and the
/// shared fetcher, so robots and per-host pacing are not reimplemented here.
async fn refetch_source_tags(
    db: &Database,
    source: &str,
    limit: usize,
    dry_run: bool,
) -> anyhow::Result<Report> {
    let mut report = Report::default();
    let registry = darkhold_scrapers::sites::default_registry();
    let Some(adapter) = registry
        .adapters()
        .iter()
        .find(|adapter| adapter.key().as_str() == source)
    else {
        return Err(anyhow::anyhow!("no adapter for source {source}"));
    };
    let policy = darkhold_scrapers::FetchPolicy {
        min_interval_per_host: std::time::Duration::from_millis(1200),
        default_interval_per_host: std::time::Duration::from_secs(2),
        ..darkhold_scrapers::FetchPolicy::default()
    };
    let fetcher = darkhold_scrapers::SafeFetcher::new(adapter.hosts(), policy);

    let rows = match db.backend() {
        Backend::Sqlite => {
            let query = sqlx::query_as::<_, (String, String)>(
                "SELECT CAST(work_id AS TEXT) AS work_id, source_url FROM library_items
                  WHERE source_key = ? AND work_id IS NOT NULL AND source_url IS NOT NULL
                  ORDER BY id LIMIT ?",
            )
            .bind(source)
            .bind(limit as i64);
            query.fetch_all(db.sqlite_pool().expect("sqlite")).await?
        }
        Backend::Postgres => {
            let query = sqlx::query_as::<_, (String, String)>(
                "SELECT CAST(work_id AS TEXT) AS work_id, source_url FROM library_items
                  WHERE source_key = $1 AND work_id IS NOT NULL AND source_url IS NOT NULL
                  ORDER BY id LIMIT $2",
            )
            .bind(source)
            .bind(limit as i64);
            query.fetch_all(db.postgres_pool().expect("postgres")).await?
        }
    };

    for (work_id, raw_url) in rows {
        report.scanned += 1;
        let Ok(url) = url::Url::parse(&raw_url) else {
            continue;
        };
        let Ok(work) = adapter.preview(&fetcher, &url, None).await else {
            continue;
        };
        let claims: Vec<(String, String)> = work
            .tags
            .iter()
            .filter_map(|tag| {
                let canonical = tag.trim();
                (!canonical.is_empty()).then(|| ("tag".to_owned(), canonical.to_owned()))
            })
            .collect();
        if claims.is_empty() {
            continue;
        }
        let (written, created) = assert_claims(db, &work_id, &claims, dry_run).await?;
        report.assertions += written;
        report.nodes_created += created;
        if written > 0 {
            report.works_tagged += 1;
        }
    }
    Ok(report)
}
