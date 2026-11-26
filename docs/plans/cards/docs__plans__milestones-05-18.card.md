# docs/plans/milestones-05-18.md — plan card

Source: `docs/plans/milestones-05-18.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`docs/verification.md`. The summary below is what the plan said, kept for the
1. `crates/domain/src/jobs.rs` — `JobState {Queued, Leased, Running, Succeeded,
2. Repository `crates/db/src/jobs.rs`: `enqueue`, `claim_next(worker, lease)`,
3. Worker loop in `crates/app/src/worker.rs`, started from a `--worker` flag or
4. `crates/db/src/storage.rs` — content-addressed blobs: `put(bytes) ->
5. Secret encryption `crates/app/src/secrets.rs` with `xchacha20poly1305`, a key
1. `crates/imports/` — a new crate. `trait Adapter { fn matches(&self, url) ->
2. `crates/domain/src/imports.rs` — `ImportDestination {PrivateLibrary,
   every generated file in a test with a strict parser. `docs/spec.md` §12.3
`crates/db/src/library.rs`; routes `/library`, `/library/shelves`,
2. `crates/domain/src/query.rs` — the typed AST and its **parser**, returning
// crates/domain/src/policy.rs

## Copy-pasteable code / commands

// crates/domain/src/policy.rs
pub enum TrustLevel { Member, Contributor, Moderator, Steward, Operator }
pub fn may_review_reports(level: TrustLevel) -> bool;
pub fn may_take_emergency_action(level: TrustLevel) -> bool;
pub fn may_read_private_pseud_linkage(level: TrustLevel) -> bool; // spec §4.2:
// "only specifically authorized staff may retrieve private pseud ownership"

## Verification command and expected output

   `config.administration.operator_account_id: Option<AccountId>` and gate the
   every generated file in a test with a strict parser. `docs/spec.md` §12.3
   `signing_out_evicts_offline_content` as a test.
8. The acceptance fixture in spec §14 `Acceptance fixture` becomes a test
   module: build the described works, run the queries, assert the sets.
*unavailable*, *misconfigured* and *failed* states, and each must be a test.

## RED/GREEN evidence

   Failed, Cancelled}`, `JobKind`, `RetryPolicy { max_attempts, backoff,
*unavailable*, *misconfigured* and *failed* states, and each must be a test.
