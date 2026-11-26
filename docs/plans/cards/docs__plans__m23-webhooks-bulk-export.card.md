# docs/plans/m23-webhooks-bulk-export.md — plan card

Source: `docs/plans/m23-webhooks-bulk-export.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

(`docs/spec.md:4659-4663`), whose acceptance is "Bulk export of a large query
bypasses download grants" (`docs/spec.md:4679-4680`).
  router at `crates/app/src/routes/media.rs:1126-1147`.
  writes — `crates/app/src/routes/media.rs:1150-1161`.
- One filter surface, `MediaQuery` (`crates/app/src/routes/media.rs:1168`), fed
  into `list_media_filtered` (`crates/db/src/media.rs:312`) which builds SQL from
  not see. Cursor is compound (`created_at|id`, `crates/db/src/media.rs:369-379`).
- The query AST is a domain type, `QueryAst` (`crates/domain/src/query.rs:82`), so
- Files are listed per work (`crates/db/src/media.rs:1213`) from `media_files`
  (`migrations/sqlite/0025_media_files.sql:5-18`: id, work_id, edition_kind, url,
- `export_jobs` (`migrations/sqlite/0008_exports.sql:49-93`): `subject_type IN
- Route shape to copy (`crates/app/src/routes/exports.rs:1-10`): `POST

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

  HMAC. Its unit test only signs and verifies with the same function
fact rather than a claim: the test reads the table.
archives by hand and verify by recomputing CRCs — either way the test asserts the
running. (d) is the anti-starvation half and is exactly what a test can prove:
Search, Default (`crates/app/src/limiter.rs:139-146`) with a round-trip test
time, with RFC 4231 test vectors as the known-answer test — the current
sign-and-verify-with-the-same-function test cannot prove anything about the
### Status (fifth review, 2026-09-19 — FK fixed, gate still red)
`f909775` fixed the SQLite foreign-key break (verified with the fourth pass's own
4. **Gate red on two fronts**: `cargo fmt` wants two blocks in `bulk_export.rs`
   `OutboxEvent` literal (`error[E0063]`), so no test runs at all.
5. The fourth pass's shape gaps are untouched: ZIP served as `text/html`/`*.html`,
   recorded as "skipped", no `has_entitlement`, no `ContentRead` scope, and no test
- `RouteClass::Export` + config field + defaults + the class round-trip test.
  as items/expected, bundle build (D4), blob reference via `BlobStore` with a bulk
  Tests: a local test receiver asserts the signature against an RFC 4231 vector,
## 4. Acceptance → test map
| Spec line | Test |
| "is fair-queued" | the claim-query fairness test plus the bulk concurrency guard |
| "never bypasses download grants" | the paywalled-work test (absent from the archive, named in the manifest) and the entitlement test (present for an entitled reader) |

## RED/GREEN evidence

Each phase is committable on its own and leaves the tree green
### Status (fifth review, 2026-09-19 — FK fixed, gate still red)
broken) and `route_inventory` 2/2, with `cargo fmt` still red. What is
4. **Gate red on two fronts**: `cargo fmt` wants two blocks in `bulk_export.rs`
