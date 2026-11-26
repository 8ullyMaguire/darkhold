# docs/plans/syndication-sync.md — plan card

Source: `docs/plans/syndication-sync.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

| the catalogue | `crates/scrapers/sources.json` + `src/sources.rs` | 33 platforms with host, software family, adapter key, a **dated robots verdict**, a bodies policy, and a discovery shape. Validated on load: no duplicate host, no `blocked` entry naming an adapter, no entry without a note. |
| tag-page discovery | `crates/scrapers/src/sites/ao3.rs` (`work_ids_from_tag_page`) | the only listing an AO3-software archive exposes to a plain crawler (`/works?…` is disallowed to `*` by all three members). Extracts bare `/works/{digits}` links, drops chapters, dedupes. |
| the import pipeline | `crates/app/src/imports.rs` | already enforces the source's robots policy per fetch (`policy_for`), stores work + units, dedupes by `source_work_key`, and records health. **The job must not reimplement any of this.** |
| the job queue | `crates/domain/src/jobs.rs`, `crates/app/src/worker.rs` | durable, leased, retried jobs with per-kind handlers. |
// crates/domain/src/jobs.rs, inside the `job_kinds!` block
file still parses (`crates/app/src/config.rs`, beside `imports`).
`crates/app/src/source_sync.rs`:
**Chosen: the second.** `crates/db/src/syndication.rs` finds-or-creates one instance account (by

## Copy-pasteable code / commands

// crates/domain/src/jobs.rs, inside the `job_kinds!` block
#[doc = "Walk the source catalogue and enqueue imports for works not yet seen (M67-03)."]
SourceSync => "source_sync",

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

pub async fn run(state: &AppState) -> Result<Summary, HandlerError>

## Verification command and expected output

A `SourceSync` job is a **discovery pass**, not an import. It walks the catalogue, finds work URLs it
6. **Record the pass**: `tracing::info!` with per-source counts (discovered, queued, skipped-known,
Failure policy: a source that fails is **transient** (a flaky host must not fail the whole pass); a
| step | command | expected |
| catalogue | `cargo test -p darkhold-scrapers --lib sources` | 9 passed |
| discovery | `cargo test -p darkhold-scrapers --lib ao3::tests` | 9 passed |
| job kind | `cargo test -p darkhold-domain --lib jobs` | every kind round-trips |
| handler | `cargo test -p darkhold-app --test syndication_sync` | a fixture listing enqueues N imports; a second run enqueues none |
| scheduling | `cargo test -p darkhold-app --test syndication_sync enqueue_if_due` | one job per interval hour, not per tick |
  but do publish "what is new". Scribble Hub's `/series-ranking/` is the first: 25 stories per pass,
  is walked once per pass, not once per configured tag.

## RED/GREEN evidence

| catalogue | `cargo test -p darkhold-scrapers --lib sources` | 9 passed |
| discovery | `cargo test -p darkhold-scrapers --lib ao3::tests` | 9 passed |
