# SPEC TA3-2 — per-source import worker concurrency

Problem: the body archive has ~21k queued imports; a single worker drains too slowly, but naive parallelism would hammer one archive with several simultaneous fetches.

Goals:
- Let several worker processes run concurrently.
- Guarantee at most one leased `import` job per `import_jobs.source_key` across all workers.
- Keep other job kinds on the existing fairness and bulk caps.

Non-goals:
- No per-host or per-archive rate-limit tuning other than the one-job-per-source guard.
- No change to retry/backoff, fetch pacing, private preference files, or reader-facing schema.
- No global concurrency cap beyond the number of worker processes the operator starts.

Constraints:
- SQLite and PostgreSQL must agree.
- Guard belongs in `jobs::claim_next` so every caller obeys it, not only `dh worker`.
- Default cap is 1; owners may run multiple workers with distinct ids.

Success criteria:
1. Targeted two-engine test: with two queued imports for the same source, worker A claims one and worker B claims none.
2. A queued import for a different source remains claimable by worker B in the same situation.
3. Removing the per-source guard makes the same test fail.
4. Live: multiple `dh worker` processes run; at most one leased import per source_key; queued import count trends down.

Evidence commands:
- `cargo test -p darkhold-app --test per_source_worker`
- `sudo -u postgres psql -d darkhold -Atc "select source_key,count(*) from jobs j join import_jobs ij on ij.job_id=j.id where j.state='leased' group by 1 having count(*)>1;"` → no rows.
