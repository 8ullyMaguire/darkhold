//! TA1 — operator verbs for private cross-archive taste ranking.
//!
//! `rank` prints a private ranking; `bodies` enqueues body-store imports for
//! the top candidates. Both read the catalogue directly because this is data
//! curation, not a reader-facing API. Every mutation previews by default.

use clap::{Args, Subcommand};
use darkhold_db::{imports, jobs, Database, DatabaseConfig};
use darkhold_domain::jobs::{JobKind, RetryPolicy};
use darkhold_domain::{AccountId, ImportJobId};
use darkhold_taste::curation::{score, Candidate};
use serde::Deserialize;
use std::path::PathBuf;

use crate::archive_preferences::load;
use crate::rec::{environment, Failure};

#[derive(Debug, Clone, Args)]
pub struct ArchiveArgs {
    /// Instance database URL.
    #[arg(long)]
    pub db_url: String,
    /// Account that owns the syndicated catalogue works.
    #[arg(long)]
    pub account: String,
    /// Pseud for body imports. Required for `bodies`.
    #[arg(long)]
    pub pseud: Option<String>,
    /// Private 0600 preferences file.
    #[arg(long, default_value = "config/darkhold-taste.local.json")]
    pub prefs: PathBuf,
    #[command(subcommand)]
    pub command: ArchiveCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ArchiveCommand {
    /// Print ranked candidates without writing anything.
    Rank {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Enqueue body-store imports for the top candidates.
    Bodies {
        #[arg(long, default_value_t = 20000)]
        limit: usize,
    },
}

#[derive(Debug, Deserialize)]
struct ProvenanceTags {
    #[serde(default)]
    tags: Vec<String>,
}

struct Ranked {
    item_id: String,
    source_key: String,
    source_url: String,
    score: f64,
    reason: String,
}

fn tags_from(provenance: &str) -> Vec<String> {
    serde_json::from_str::<ProvenanceTags>(provenance)
        .map(|value| value.tags)
        .unwrap_or_default()
}

pub async fn run(cli: &crate::Cli, args: &ArchiveArgs) -> Result<(), Failure> {
    let prefs = load(&args.prefs).map_err(|error| environment(error.to_string()))?;
    let account = args
        .account
        .parse::<AccountId>()
        .map_err(|error| environment(format!("invalid --account: {error}")))?;

    let (requested_limit, apply) = match &args.command {
        ArchiveCommand::Rank { .. } => (None, false),
        ArchiveCommand::Bodies { limit } => {
            let cap = (*limit).min(prefs.max_bodies);
            let apply = crate::safety::mutating(
                cli,
                &format!("enqueue at most {cap} private body imports"),
            )?;
            (Some(cap), apply)
        }
    };

    let pseud = match &args.command {
        ArchiveCommand::Bodies { .. } if apply => Some(
            args.pseud
                .as_deref()
                .ok_or_else(|| environment("--pseud is required for archive bodies"))?,
        ),
        _ => None,
    };

    let db = Database::connect(&DatabaseConfig::new(&args.db_url))
        .await
        .map_err(|error| environment(error.to_string()))?;
    let rows = darkhold_db::taste_curation::candidates(&db, &account.to_string(), 100_000)
        .await
        .map_err(|error| environment(error.to_string()))?;
    let mut excluded = 0usize;
    let mut ranked: Vec<Ranked> = Vec::new();
    for row in rows {
        let scored = score(
            &prefs.preference,
            &Candidate {
                title: row.title,
                summary: row.summary,
                tags: tags_from(&row.provenance_json),
            },
        );
        if scored.excluded {
            excluded += 1;
            continue;
        }
        if scored.value <= 0.0 {
            continue;
        }
        ranked.push(Ranked {
            item_id: row.library_item_id,
            source_key: row.source_key,
            source_url: row.source_url,
            score: scored.value,
            reason: scored.reason,
        });
    }
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.item_id.cmp(&b.item_id))
    });

    match &args.command {
        ArchiveCommand::Rank { limit } => {
            println!("preference-source-terms={} excluded={}", prefs.source_terms, excluded);
            for row in ranked.into_iter().take(*limit) {
                println!(
                    "rank item={} score={:.2} reason={} url={}",
                    row.item_id, row.score, row.reason, row.source_url
                );
            }
        }
        ArchiveCommand::Bodies { .. } => {
            let cap = requested_limit.expect("body cap set above");
            let selected: Vec<Ranked> = ranked.into_iter().take(cap).collect();
            let mut queued = 0usize;
            let mut skipped_existing = 0usize;
            for row in selected {
                let key = format!("body:{}", row.item_id);
                if jobs::exists_by_idempotency_key(&db, &key)
                    .await
                    .map_err(|error| environment(error.to_string()))?
                {
                    skipped_existing += 1;
                    continue;
                }
                if !apply {
                    println!(
                        "would queue body item={} score={:.2} url={}",
                        row.item_id, row.score, row.source_url
                    );
                    continue;
                }
                let import_id = ImportJobId::new().to_string();
                let job_id = jobs::enqueue(
                    &db,
                    JobKind::Import,
                    &serde_json::json!({
                        "import_job_id": import_id,
                        "body_policy": "store"
                    })
                    .to_string(),
                    Some(&key),
                    Some(account),
                    0,
                    &RetryPolicy::default(),
                )
                .await
                .map_err(|error| environment(error.to_string()))?;
                imports::create_import_job(
                    &db,
                    &import_id,
                    &job_id.to_string(),
                    &account.to_string(),
                    pseud.expect("checked above"),
                    &row.source_key,
                    &row.source_url,
                    "library",
                    false,
                )
                .await
                .map_err(|error| environment(error.to_string()))?;
                queued += 1;
            }
            println!(
                "body-select queued={queued} skipped_existing={skipped_existing} cap={cap} applied={apply}"
            );
        }
    }
    Ok(())
}
