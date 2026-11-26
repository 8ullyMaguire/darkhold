# docs/plans/m59-10-reader-body-request.md — plan card

Source: `docs/plans/m59-10-reader-body-request.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Requirement row:** `M59-10` in `docs/requirements.csv`, currently `planned`.
**Amendment:** `docs/spec-amendments/crawling-retention-and-preservation.md`
grep -rn 'body_snapshot\|reader_snapshot\|per.reader snapshot' crates/ --include=*.rs
`document_json` / `sanitized_html` / `plain_text` (`migrations/sqlite/0003_works.sql:101`).
`works` (`0003_works.sql:37`) is owner/title/summary/language/rating/visibility/
(`0006_imports.sql:108`): `work_id REFERENCES works (id)` beside
- `migrations/sqlite/0095_reader_body_copies.sql`
- `migrations/postgres/0095_reader_body_copies.sql`
Identical ids and column order in both (§2.1 of `docs/plans/README.md`).
ls migrations/sqlite/0095_reader_body_copies.sql migrations/postgres/0095_reader_body_copies.sql
In `crates/app/src/config.rs`:
3. `docs/config-reference.md` gains the row, and `darkhold.toml.example` gains

## Copy-pasteable code / commands

grep -rn 'body_snapshot\|reader_snapshot\|per.reader snapshot' crates/ --include=*.rs
# no hits

CREATE TABLE IF NOT EXISTS reader_body_copies (
    id             TEXT PRIMARY KEY,
    work_id        TEXT NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    account_id     TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    -- NOT a foreign key and NOT a copy of library_items.source_key: it is
    -- recorded so the fetch can be bounded by the source's robots posture, and
    -- so a work re-imported from a different source later does not silently
    -- re-point an in-flight request at another site's terms.
    source_key     TEXT NOT NULL,
    chapter_key    TEXT NOT NULL,          -- the work's chapter list at request time
    state          TEXT NOT NULL,          -- pending | ready | refused | failed
    reason_code    TEXT,                   -- a RetentionReason code, when refused
    plain_text     TEXT,                   -- the bytes, once fetched
    sanitized_html TEXT,
    requested_at   TEXT NOT NULL,
    settled_at     TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    version        INTEGER NOT NULL DEFAULT 1
);

-- The audit surface §6.2 asks for. NOT retention_policy_changes: that table
-- records mode changes and its actor is an operator, so a reader's own request
-- written there would be rendered as a policy change they made. This table is
-- the record of who asked for what, and when.
CREATE TABLE IF NOT EXISTS retention_body_requests (
    id             TEXT PRIMARY KEY,
    work_id        TEXT NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    account_id     TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    source_key     TEXT NOT NULL,
    trust_at_request INTEGER NOT NULL,     -- the reader's level, recorded, not re-derived
    requested_at   TEXT NOT NULL,
    outcome        TEXT NOT NULL,          -- pending | ready | refused | failed
    reason_code    TEXT
);

-- One copy per (work, reader): a second request is a re-fetch, not a second row.
-- Without this, a reader who asks twice has two blobs and the read path has to
-- pick, and "picks the older" is a bug nobody writes a test for.
CREATE UNIQUE INDEX IF NOT EXISTS idx_reader_body_copies_work_account
    ON reader_body_copies (work_id, account_id);

ls migrations/sqlite/0095_reader_body_copies.sql migrations/postgres/0095_reader_body_copies.sql
cargo test -p darkhold-db migration     # expect: the dialect-parity test to pass

## Verification command and expected output

   according to the viewer's trust level**, and a test fails the build if one
demands a test that fails the build if any surface varies body visibility by
every reader eligible for the work sees the same thing. The guard test (§4.4
below) is a **source scan**, not a behavioural test, because the property is
-- pick, and "picks the older" is a bug nobody writes a test for.
### Verify
cargo test -p darkhold-db migration     # expect: the dialect-parity test to pass
`retention_governance.*` reading as configurable in every test while being
### Verify
cargo test -p darkhold-app --test config_sections    # expect: one more passing than before
### Verify
run it there. Do not add a lib-only test that never reaches a route.
inventory is enforced by a test and will fail otherwise.
### 4.1 The gate, in the order the spec states
Order matters and is worth a test: an `aggregate` instance refuses on mode
### 4.4 The guard test — §6.4.4, as a source scan
Empty is the pass condition, and it is asserted in
`crates/app/tests/config_sections.rs` by reading the same paths — a test that
**Why a scan and not a behavioural test:** the property is about *future* code
that might branch on the viewer's trust. A behavioural test proves today's

## RED/GREEN evidence

(`pending | ready | refused | failed`) plus the refusal's reason code. It does
    state          TEXT NOT NULL,          -- pending | ready | refused | failed
    outcome        TEXT NOT NULL,          -- pending | ready | refused | failed
pub enum CopyState { Pending, Ready, Refused, Failed }   // parse_stored, as BodyMode does
or `failed` row to `pending` and returns a `ready` row untouched — so a reader
never failed is not evidence, and the Phase E privacy test is the precedent.
green. The guard test is written **last**, because until the route exists it has
