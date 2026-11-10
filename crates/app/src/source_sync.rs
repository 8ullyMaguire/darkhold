//! `M67-03` — the periodic walk over external sources.
//!
//! # What this is, and pointedly is not
//!
//! A **discovery pass**, not an import. It reads a listing page, finds work URLs the queue has not seen,
//! and enqueues one `Import` job per work. The import pipeline already knows about robots, credentials,
//! body ceilings, dedupe by `source_work_key` and source health; a sync that fetched and stored works
//! itself would duplicate all of it and drift from it within a milestone.
//!
//! # The four rules it keeps
//!
//! 1. **The site's `robots.txt`, read live, decides.** The catalogue's verdict is the *reviewed* state
//!    from 2026-11-02; the runtime authority is the file. A source that has changed its mind is skipped
//!    and the disagreement is logged — news, not an error.
//! 2. **One listing page per (source, tag, run),** capped at `max_works_per_listing`. A tag with forty
//!    thousand works must not become forty thousand jobs on the first tick.
//! 3. **Nothing is queued twice.** The idempotency key is `syndication:{source}:{work_id}`, so a second
//!    run over the same listing is a no-op — and the summary says "already known" rather than counting
//!    the same works again.
//! 4. **A broken source does not fail the pass.** Each entry is walked independently; one unreachable
//!    archive must not cost the other seven their turn.
//!
//! # Whose account owns a syndicated work
//!
//! The instance account, with one pseud per source (`crates/db/src/syndication.rs`). A reader sees
//! "SquidgeWorld" as the author of a work that came from SquidgeWorld, and the operator's own identity
//! is not borrowed to stand in for an archive.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use darkhold_db::{imports, jobs, syndication};
use darkhold_domain::jobs::{JobKind, RetryPolicy};
use darkhold_scrapers::sites::ao3::work_urls_from_tag_page;
use darkhold_scrapers::{safety::SafeFetcher, Bodies, Catalogue, Discovery, FetchClass, SourceKey};

use crate::state::AppState;

/// The idempotency key for one work at one source.
///
/// Derived, never stored: two runs must produce the same key from the same facts, or dedupe is a
/// coincidence.
#[must_use]
pub fn work_key(source_id: &str, work_id: &str) -> String {
    format!("syndication:{source_id}:{work_id}")
}

/// What one pass did, per source.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SourceReport {
    /// The catalogue entry.
    pub source: String,
    /// Works seen on the listing pages.
    pub discovered: usize,
    /// Imports queued this pass.
    pub queued: usize,
    /// Works already in the queue or already imported.
    pub known: usize,
    /// Why the source was skipped, when it was.
    pub skipped: Option<String>,
}

/// What the whole pass did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Summary {
    /// One report per source the catalogue says may be walked.
    pub sources: Vec<SourceReport>,
}

impl Summary {
    /// The one-line log line. A pass that reports nothing cannot be told from one that did nothing.
    #[must_use]
    pub fn describe(&self) -> String {
        let queued: usize = self.sources.iter().map(|r| r.queued).sum();
        let known: usize = self.sources.iter().map(|r| r.known).sum();
        let seen: usize = self.sources.iter().map(|r| r.discovered).sum();
        let skipped: Vec<&str> = self
            .sources
            .iter()
            .filter(|r| r.skipped.is_some())
            .map(|r| r.source.as_str())
            .collect();
        format!(
            "syndication: {seen} works seen, {queued} queued, {known} already known, {} sources skipped{}",
            skipped.len(),
            if skipped.is_empty() {
                String::new()
            } else {
                format!(" ({})", skipped.join(", "))
            }
        )
    }

    /// Whether anything was queued.
    #[must_use]
    pub fn queued(&self) -> usize {
        self.sources.iter().map(|r| r.queued).sum()
    }
}

/// Run one pass.
pub async fn run(state: &AppState) -> anyhow::Result<Summary> {
    let config = &state.config().syndication;
    let catalogue = Catalogue::bundled()
        .map_err(|error| anyhow::anyhow!("the bundled source catalogue is invalid: {error}"))?;

    let mut summary = Summary::default();
    if !config.enabled {
        // Not an error: a queued job after an operator disabled the feature must finish quietly rather
        // than fail and retry for ever.
        return Ok(summary);
    }
    if config.tags.is_empty() {
        tracing::info!(
            target: "darkhold::syndication",
            "syndication is on but names no tags; nothing to walk"
        );
        return Ok(summary);
    }

    let account = syndication::ensure_account(state.db(), &config.account_email).await?;
    let registry = state.registry();

    for entry in catalogue.syncable() {
        let mut report = SourceReport {
            source: entry.id.clone(),
            ..SourceReport::default()
        };

        let Some(adapter_key) = &entry.adapter else {
            report.skipped = Some("the catalogue lists no adapter".to_owned());
            summary.sources.push(report);
            continue;
        };
        let Ok(adapter) = registry.by_key(&SourceKey::new(adapter_key.clone())) else {
            report.skipped = Some(format!("no adapter `{adapter_key}` in this build"));
            summary.sources.push(report);
            continue;
        };

        // The instance account needs a pseud per source, created on first use and found afterwards.
        let pseud = syndication::ensure_pseud(state.db(), account, &entry.id, &entry.name).await?;

        // Discovery shapes are per family, and the catalogue says which one each entry has rather than
        // the job guessing: a tag listing (`/tags/{tag}/works`, one per configured tag) or a tagless
        // listing (`/series-ranking/`, walked once per pass).
        let listings: Vec<(String, String)> = match &entry.discovery {
            Discovery::TagPage { .. } => config
                .tags
                .iter()
                .filter_map(|tag| entry.tag_page_url(tag).map(|url| (tag.clone(), url)))
                .collect(),
            Discovery::Listing { .. } => entry
                .listing_url()
                .map(|url| (String::new(), url))
                .into_iter()
                .collect(),
            Discovery::None => Vec::new(),
        };
        if listings.is_empty() {
            report.skipped = Some("no discovery shape this build can walk".to_owned());
            summary.sources.push(report);
            continue;
        }

        let policy = crate::imports::policy_for(adapter, state.config());
        let fetcher = SafeFetcher::new(vec![entry.host.clone()], policy);

        for (tag, url) in listings {
            let fetched = match fetcher.get_declared(&url, FetchClass::Metadata).await {
                Ok(fetched) => fetched,
                Err(error) => {
                    // One listing that will not load costs this tag, not the pass.
                    tracing::warn!(
                        target: "darkhold::syndication",
                        source = %entry.id, tag = %tag, %error,
                        "the listing page could not be read"
                    );
                    report.skipped = Some(format!("{tag}: {error}"));
                    continue;
                }
            };

            let urls = match &entry.discovery {
                Discovery::Listing { .. } => {
                    darkhold_scrapers::sites::scribblehub::ScribbleHub::stories_from_listing(
                        &entry.host,
                        &fetched.body,
                    )
                }
                // The tag-page shape is the AO3-software family's, and `{tag}` in the template is the
                // reason: a different family with a tag browse needs its own extractor, not this one.
                _ => work_urls_from_tag_page(&entry.host, &fetched.body),
            };
            report.discovered += urls.len();

            for url in urls.into_iter().take(config.max_works_per_listing) {
                let work_id = url.rsplit('/').next().unwrap_or_default().to_owned();
                if work_id.is_empty() {
                    continue;
                }
                let key = work_key(&entry.id, &work_id);
                if jobs::exists_by_idempotency_key(state.db(), &key).await? {
                    report.known += 1;
                    continue;
                }

                // Order matters, and for the reason the route spells out: the import's id exists before
                // the queue row that names it, so no worker can claim a job pointing at nothing.
                let import_id = darkhold_domain::ImportJobId::new().to_string();
                let job_id = jobs::enqueue(
                    state.db(),
                    JobKind::Import,
                    &serde_json::json!({ "import_job_id": import_id }).to_string(),
                    Some(&key),
                    Some(account),
                    0,
                    &RetryPolicy::default(),
                )
                .await?;
                imports::create_import_job(
                    state.db(),
                    &import_id,
                    &job_id.to_string(),
                    &account.to_string(),
                    &pseud.to_string(),
                    &entry.id,
                    &url,
                    "library",
                    false,
                )
                .await?;
                report.queued += 1;
            }
        }

        summary.sources.push(report);
    }

    Ok(summary)
}

/// Enqueue a pass when one is due.
///
/// Called from the worker's maintenance sweep, which runs far more often than the interval. The
/// idempotency key is the **scheduled hour**, not the tick: two workers sweeping at the same second both
/// try to enqueue and exactly one row lands, which is the whole reason the key is not `now`.
pub async fn enqueue_if_due(state: &AppState) -> anyhow::Result<bool> {
    let config = &state.config().syndication;
    if !config.enabled || config.tags.is_empty() {
        return Ok(false);
    }

    // Bucket the clock into interval-sized windows since the epoch, so the key is stable for the whole
    // window and changes when the next one starts.
    let interval = config.interval().as_secs().max(3600);
    let now = Utc::now().timestamp().max(0) as u64;
    let bucket = now / interval;
    let key = format!("source_sync:{bucket}");

    if jobs::exists_by_idempotency_key(state.db(), &key).await? {
        return Ok(false);
    }

    jobs::enqueue(
        state.db(),
        JobKind::SourceSync,
        "{}",
        Some(&key),
        None,
        0,
        &RetryPolicy::default(),
    )
    .await?;
    Ok(true)
}

/// The next time a pass becomes due, for an operator's status line.
#[must_use]
pub fn next_due(
    config: &crate::config::SyndicationConfig,
    last: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    if !config.enabled {
        return None;
    }
    let interval = chrono::Duration::from_std(config.interval()).ok()?;
    Some(last.map_or_else(Utc::now, |last| last + interval))
}

/// The catalogue, for a status screen. Cheap enough to rebuild per request: it is 13 KB of JSON.
#[must_use]
pub fn catalogue() -> Option<Catalogue> {
    Catalogue::bundled().ok()
}

/// A source's bodies policy, by host — the question a reader asks before pasting a URL.
#[must_use]
pub fn bodies_for_host(host: &str) -> Option<Bodies> {
    catalogue().and_then(|catalogue| catalogue.by_host(host).map(|entry| entry.bodies))
}

/// Per-source counts, keyed by source id, for a status screen.
#[must_use]
pub fn by_source(summary: &Summary) -> HashMap<String, SourceReport> {
    summary
        .sources
        .iter()
        .map(|report| (report.source.clone(), report.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_derived_from_the_source_and_the_work_and_nothing_else() {
        // Two runs over the same listing must produce the same key, or dedupe is a coincidence.
        assert_eq!(
            work_key("squidgeworld", "104447"),
            work_key("squidgeworld", "104447")
        );
        assert_ne!(
            work_key("squidgeworld", "104447"),
            work_key("adastra", "104447")
        );
        assert_ne!(
            work_key("squidgeworld", "104447"),
            work_key("squidgeworld", "104448")
        );
    }

    #[test]
    fn a_pass_with_nothing_to_say_still_says_it() {
        let summary = Summary {
            sources: vec![SourceReport {
                source: "squidgeworld".to_owned(),
                discovered: 25,
                queued: 3,
                known: 22,
                skipped: None,
            }],
        };
        let line = summary.describe();
        assert!(line.contains("25 works seen"), "{line}");
        assert!(line.contains("3 queued"), "{line}");
        assert!(line.contains("22 already known"), "{line}");
        assert_eq!(summary.queued(), 3);
    }

    #[test]
    fn a_skipped_source_is_named_in_the_log_line() {
        let summary = Summary {
            sources: vec![SourceReport {
                source: "fanfictionlol".to_owned(),
                skipped: Some("robots.txt now disallows /tags/".to_owned()),
                ..SourceReport::default()
            }],
        };
        assert!(
            summary.describe().contains("fanfictionlol"),
            "{}",
            summary.describe()
        );
    }

    #[test]
    fn the_catalogue_answers_what_may_be_read_from_a_host() {
        assert_eq!(bodies_for_host("squidgeworld.org"), Some(Bodies::Allowed));
        assert_eq!(
            bodies_for_host("www.squidgeworld.org"),
            Some(Bodies::Allowed)
        );
        assert_eq!(bodies_for_host("fanfiktion.de"), Some(Bodies::Blocked));
        assert_eq!(bodies_for_host("example.invalid"), None);
    }
}

/// Record every adapter this build carries in the `sources` catalogue.
///
/// `imports::upsert_source`'s doc comment has always said it is "called at startup for every registered adapter, so the
/// catalogue a reader sees cannot drift from the adapters the binary actually has". Nothing called it. The table was
/// whatever a migration had seeded, which means a deploy that added an adapter showed a source nobody could choose and a
/// deploy that removed one left a row pointing at an adapter that no longer exists.
///
/// Two things this deliberately does not do. It does not touch `enabled`: a paused source stays paused across a deploy,
/// which is the half of `upsert_source`'s contract that keeps an operator's decision from being undone by a restart. And
/// it does not fail the boot: a source that cannot be written is logged and skipped, because taking the server down over
/// one catalogue row would be a worse trade than a stale row.
pub async fn sync_source_catalogue(state: &AppState) -> usize {
    let build = crate::version::build_id();
    let mut written = 0;
    for entry in state.registry().catalogue() {
        let capabilities = match serde_json::to_string(&entry.capabilities) {
            Ok(json) => json,
            Err(error) => {
                tracing::warn!(%error, key = %entry.key, "could not serialise a source's capabilities");
                continue;
            }
        };
        // The adapter version is the build that carries the adapter. A source's capabilities are the parser's, and the
        // parser ships with the binary, so "which build wrote this row" is the only version string that means anything.
        match imports::upsert_source(
            state.db(),
            entry.key.as_str(),
            &entry.display_name,
            &build,
            &capabilities,
        )
        .await
        {
            Ok(()) => written += 1,
            Err(error) => {
                tracing::warn!(%error, key = %entry.key, "could not record a source in the catalogue");
            }
        }
    }
    tracing::info!(
        sources = written,
        "source catalogue recorded from the adapter registry"
    );
    written
}
