# docs/plans/milestone-05-jobs.md — plan card

Source: `docs/plans/milestone-05-jobs.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

* `migrations/{sqlite,postgres}/0003_works.sql` — the `outbox_events` table M3
  `docs/verification.md` listed that as an open risk.
* `crates/db/src/migrate.rs` — the embedded migration catalogue and its
* `crates/app/src/cli.rs` — `serve` already had `--no-migrate`, and the
* `crates/app/src/auth.rs` — `RequireSession` and `RequirePseud`, which the job
* `crates/app/src/state.rs` — `AppState` carries the config and the database to
`crates/domain/src/jobs.rs`: `JobState`, `JobKind`, `RetryPolicy`,
### Task 3 — Repository: `crates/db/src/jobs.rs`
### Task 4 — Storage: `crates/db/src/storage.rs`
### Task 5 — Secrets: `crates/app/src/secrets.rs`
### Task 6 — The worker: `crates/app/src/worker.rs`
setting with a trust level, and the comment in `config.rs` says so.

## Copy-pasteable code / commands

GET    /jobs?cursor=…                the caller's own jobs, envelope
POST   /jobs/:id/cancel              the caller's own job
POST   /jobs                         development only, the diagnostic probe
GET    /admin/jobs?state=…&cursor=…  operators only
POST   /admin/jobs/:id/retry         operators only

## Verification command and expected output

  dialect-drift test. Any new migration must appear in both dialects with the
scheduled time is reproducible in a test and a job does not re-roll its own
| Requirement | Test |

## RED/GREEN evidence

`next_attempt_at` (pure, `now` passed in), `can_cancel`, `may_retry`.
