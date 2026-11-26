# docs/plans/milestone-08-library.md — plan card

Source: `docs/plans/milestone-08-library.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

| Migration 0009 — `shelves`, `shelf_items`, `bookmarks`, `private_tags`, `reading_status`, `saved_views`, `update_checks` | `migrations/{sqlite,postgres}/0009_library.sql` |
| The vocabulary and the rules | `crates/domain/src/library.rs` (18 tests) |
| Storage: reads, writes, the query builder, storage accounting | `crates/db/src/library.rs` |
| HTTP: shelves, shelf items, bookmarks, private tags, statuses, saved views, the listing, storage, the check | `crates/app/src/routes/library.rs` |
| The update check as a job | `crates/app/src/library_updates.rs`, `JobKind::UpdateCheck` |
| Acceptance tests | `crates/app/tests/milestone_8.rs` (10 tests) |
| The library page | `frontend/src/routes/Library.svelte` (+ 8 tests) |
| Whole-work mode | `frontend/src/routes/Reader.svelte` (+ 3 tests) |
| The card's three densities | `frontend/src/lib/components/WorkCard.svelte` |
`crates/db/src/migrate.rs` compared the two dialects' migration **ids** and

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

## The migration parity test was strengthened, and falsified before being trusted

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
