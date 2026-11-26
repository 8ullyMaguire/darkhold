# docs/plans/media-generalization.md — plan card

Source: `docs/plans/media-generalization.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Authority chain: `docs/spec.md` §32 > ADR 0019 > this plan > the session
| R2 | M23 | Media query engine + `/api/v1` doors with filters (quality, dates, everything), cursors, ETag/304, Atom/RSS, OPDS, webhooks, grant-gated bulk export, JSON-LD/Dublin Core | **mostly implemented** — query engine, doors, cursors, ETag/304, Atom/RSS, OPDS, JSON-LD/Dublin Core done (M23-01/03/04); webhooks and grant-gated bulk export remain, planned in `docs/plans/m23-webhooks-bulk-export.md` |
. ~/.hermes/plans/lhpg-env.sh && \
  (`crates/domain/src/query.rs`, `query_sql.rs`); reuse `works_index_terms`
  `crates/db/src/media.rs` (registered in `crates/db/src/lib.rs`). Replace
  the 501 stubs in `crates/app/src/routes/media.rs` one door at a time;
  delete the matching 501 assertion in `milestone_22.rs` as each body
  columns, `?` placeholders in `db.sql()` PG strings, honest multi-line

## Copy-pasteable code / commands

# SQLite (harness reads the env var at runtime — unset it)
unset DARKHOLD_TEST_PG_URL && CARGO_TARGET_DIR=~/.cargo-target/darkhold \
  cargo test --workspace --no-fail-fast

# PostgreSQL (scratch container lh-review-pg on 55432)
. ~/.hermes/plans/lhpg-env.sh && \
  cargo test -p darkhold-app --no-fail-fast -- --test-threads=4

cargo fmt --all -- --check
CARGO_TARGET_DIR=~/.cargo-target/darkhold cargo clippy --workspace --all-targets 2>&1 | grep -c warning  # 0

## Verification command and expected output

## Gate discipline (every phase, both dialects)
  cargo test --workspace --no-fail-fast
  cargo test -p darkhold-app --no-fail-fast -- --test-threads=4
accumulate when a test panics past cleanup).
  lands, replacing it with a behavior test.

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
