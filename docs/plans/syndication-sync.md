# Syndication: a periodic sync over external sources

**Requirement:** `M67-01` (catalogue, landed), `M67-02` (discovery, landed), `M67-03` (the job — this plan)
**Status:** `M67-01` and `M67-02` are implemented and tested. `M67-03` is specified here and not yet built.

## The ask, in one sentence

Register the ~33 archives a fandom scatters to when AO3 wobbles, and run a background job that keeps
their catalogue entries fresh — **all metadata, prose only where `robots.txt` allows it**.

## What is already in the tree

| piece | where | what it does |
|---|---|---|
| the catalogue | `crates/scrapers/sources.json` + `src/sources.rs` | 33 platforms with host, software family, adapter key, a **dated robots verdict**, a bodies policy, and a discovery shape. Validated on load: no duplicate host, no `blocked` entry naming an adapter, no entry without a note. |
| tag-page discovery | `crates/scrapers/src/sites/ao3.rs` (`work_ids_from_tag_page`) | the only listing an AO3-software archive exposes to a plain crawler (`/works?…` is disallowed to `*` by all three members). Extracts bare `/works/{digits}` links, drops chapters, dedupes. |
| the import pipeline | `crates/app/src/imports.rs` | already enforces the source's robots policy per fetch (`policy_for`), stores work + units, dedupes by `source_work_key`, and records health. **The job must not reimplement any of this.** |
| the job queue | `crates/domain/src/jobs.rs`, `crates/app/src/worker.rs` | durable, leased, retried jobs with per-kind handlers. |

## The design

A `SourceSync` job is a **discovery pass**, not an import. It walks the catalogue, finds work URLs it
has not seen, and **enqueues one `Import` job per work** — reusing the pipeline that already knows about
robots, credentials, bodies, dedupe and health. A sync that imported works itself would duplicate that
machinery and drift from it.

### 1. The job kind

```rust
// crates/domain/src/jobs.rs, inside the `job_kinds!` block
#[doc = "Walk the source catalogue and enqueue imports for works not yet seen (M67-03)."]
SourceSync => "source_sync",
```

No payload: the catalogue is the input, and a payload that named entries would let a caller sync a
subset the operator never approved.

### 2. Config

```toml
[syndication]
# Off unless an operator turns it on: a fresh instance that starts fetching eight archives on boot is
# a surprise, and "relevant features enabled by default" does not extend to outbound traffic.
enabled = false
interval_hours = 12
# Tags to walk, one listing page each, per source. These are the instance's own interests.
tags = ["Naruto", "Mass Effect"]
# A ceiling per (source, tag, run). A tag with 40,000 works must not become 40,000 jobs on the first
# tick; the cursor below carries the rest to the next one.
max_works_per_listing = 25
```

`Config` gains `pub syndication: SyndicationConfig` with `#[serde(default)]` so every existing config
file still parses (`crates/app/src/config.rs`, beside `imports`).

### 3. The handler

`crates/app/src/source_sync.rs`:

```rust
pub async fn run(state: &AppState) -> Result<Summary, HandlerError>
```

Steps, per catalogue entry that `is_syncable()`:

1. **Re-read `robots.txt`.** The catalogue's verdict is the *reviewed* state from 2026-11-02; the
   runtime authority is the live file. Fetch it through the existing `SafeFetcher` under
   `FetchClass::Metadata`, parse with `robots::Robots::parse`, and skip the entry when the tag path is
   disallowed — logging the disagreement (a site that changed its mind is news, not an error).
2. **Pace.** Honour `Crawl-delay` between requests to one host, floored at one request per second
   (`robots::resolve_posture` already computes this; reuse it, do not re-derive).
3. **Walk one listing page per tag**: `entry.tag_page_url(tag)`, then `work_urls_from_tag_page`.
4. **Filter** against `imports::find_by_source_work_key` (already exists for dedupe) and against works
   already queued in this run. Cap at `max_works_per_listing`, **newest first** (the listing's own
   order), and record the last id seen so the next run continues rather than re-reading the head.
5. **Enqueue**: `jobs::enqueue(JobKind::Import, {"import_job_id": …}, idempotency_key =
   "syndication:{entry}:{work_id}", …)` after `imports::create_import_job(…)` with
   `destination_type = "library"` and `dry_run = false`.
6. **Record the pass**: `tracing::info!` with per-source counts (discovered, queued, skipped-known,
   skipped-robots). A job that reports nothing cannot be distinguished from one that did nothing.

Failure policy: a source that fails is **transient** (a flaky host must not fail the whole pass); a
catalogue that will not parse is **fatal** (that is a build defect, not a network one).

### 4. Scheduling

`Worker::maintenance_pass` (already runs on the worker loop) gains:

```rust
crate::source_sync::enqueue_if_due(state).await?;
```

which enqueues `SourceSync` when `syndication.enabled` and either no `SourceSync` row exists or the last
terminal one is older than `interval_hours`. The idempotency key is `"source_sync:{unix_hour}"`, so two
workers cannot both enqueue the same hour.

### 5. The open question this design must settle before step 5 ships

**Whose account owns an imported work?** `import_jobs` requires an `account_id` and a `pseud_id`, and a
syndicated work has no local author. Three options:

| option | consequence |
|---|---|
| the operator's account and its first pseud | simplest, but the operator's profile becomes the author of every imported work |
| a dedicated instance account (`syndication@…`) with one pseud per source | honest attribution ("imported from SquidgeWorld"), needs a migration and a config field |
| a pseud named after the source's author, created on demand | best attribution, but it claims an identity on someone else's behalf — this needs a spec decision, not a coding one |

The plan recommends the second, and **step 5 is blocked on the user's choice**. Steps 1–4 are not.

## Verification, per step

| step | command | expected |
|---|---|---|
| catalogue | `cargo test -p darkhold-scrapers --lib sources` | 9 passed |
| discovery | `cargo test -p darkhold-scrapers --lib ao3::tests` | 9 passed |
| job kind | `cargo test -p darkhold-domain --lib jobs` | every kind round-trips |
| handler | `cargo test -p darkhold-app --test syndication_sync` | a fixture listing enqueues N imports; a second run enqueues none |
| scheduling | `cargo test -p darkhold-app --test syndication_sync enqueue_if_due` | one job per interval hour, not per tick |
| live smoke | `dh supply …` → no; run the instance and read `source_sync` in the job table | one row, counts by source |

## What is deliberately not in this plan

* **Bodies from sites that disallow them.** The catalogue's `metadata_only` entries are read for
  metadata only; an operator cannot flip that per source, because the site's `robots.txt` is not the
  operator's to override. (Darkhold's `imports.honour_robots` exists for a self-hosted instance's own
  content, not for other people's servers.)
* **Sites with no adapter.** 24 of the 33 entries have `adapter: null`. They are listed, with the reason,
  so the gap is visible. Adding one is a fixture + an adapter, in that order.
* **Comment/thread scraping from forums.** A XenForo thread is a work-in-progress with an index post;
  reading it as a work is a different parser and a different plan.
