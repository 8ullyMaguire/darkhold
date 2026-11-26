# docs/plans/roadmap-card-bodies.md — plan card

Source: `docs/plans/roadmap-card-bodies.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Implements `docs/spec-amendments/roadmap-card-bodies.md`.
Read `docs/plans/README.md` §2 for the house rules this inherits. The ones
- `db.sql(...)` borrows; use `sql_owned` for assembled statements.
- Run the frontend with `bash frontend/scripts/fe.sh <cmd>`.
1  migration (both dialects)      0090_roadmap_card_body.sql
4  frontend                       RoadmapCard.svelte + the board link
### 2.1 `migrations/sqlite/0090_roadmap_card_body.sql`
-- docs/spec-amendments/roadmap-card-bodies.md).
### 2.2 `migrations/postgres/0090_roadmap_card_body.sql`
-- docs/spec-amendments/roadmap-card-bodies.md).
-- The counterpart of migrations/sqlite/0090_roadmap_card_body.sql. Read that
-- crates/db/src/migrate.rs parses CREATE TABLE bodies and inline REFERENCES.

## Copy-pasteable code / commands

1  migration (both dialects)      0090_roadmap_card_body.sql
2  domain + repository            Card.body, six queries, find_card_by_id [NEW]
3  routes                         board body, detail route, suggest accepts body
4  frontend                       RoadmapCard.svelte + the board link
5  the journey, by hand
6  tests that pin what the journey proved
7  requirements.csv               the body column, and 667 bodies
8  the brainstorm seed            162 new cards

-- Roadmap card bodies (spec §44.1, amendment
-- docs/spec-amendments/roadmap-card-bodies.md).
--
-- Dialect: SQLite.
--
-- WHAT THIS IS. One column on `roadmap_cards`. §44.1 defined a card as "one
-- idea, stated in one sentence", and the board rendered the title alone. The
-- preservation brainstorm gave the arena 162 ideas whose entire argument is
-- in a paragraph that had nowhere to go, so a card gained a body: a page of
-- prose saying what the feature is and why it exists. The board still lists
-- the title; the detail view shows both.
--
-- NOT NULL DEFAULT '' is deliberate and is the whole compatibility story:
-- 667 cards exist, none of them has a body, and none of them may break. A
-- NULL body would force every reader to branch on Option<String> and every
-- writer to supply a value, in exchange for a distinction (absent versus
-- empty) that no surface renders differently. Empty is a real state, rendered
-- as a placeholder.
--
-- NOT a separate table. A card's body is edited with the card, travels with
-- it through a stage move, and is read on every detail view. A side table
-- makes every read a join and puts the question of a card with a row in one
-- and not the other permanently on the table.
--
-- RETENTION. No cascade, nothing references this column, nothing is deleted
-- by it. Cards are never deleted (§44.1); a body dies when its card does.
ALTER TABLE roadmap_cards ADD COLUMN body TEXT NOT NULL DEFAULT '';

-- Roadmap card bodies (spec §44.1, amendment
-- docs/spec-amendments/roadmap-card-bodies.md).
--
-- Dialect: PostgreSQL.
--
-- The counterpart of migrations/sqlite/0090_roadmap_card_body.sql. Read that
-- file first: it carries the reasoning.
--
-- PARITY. `the_two_dialects_declare_the_same_columns_and_indexes` in
-- crates/db/src/migrate.rs parses CREATE TABLE bodies and inline REFERENCES.
-- It does NOT parse `ALTER TABLE ... ADD COLUMN` -- there is no "ADD COLUMN"
-- in the runner's parser, and the two dialects declare this column
-- identically, so this migration is invisible to that test in both files.
-- That is a known blind spot in the check, not a licence to skip parity:
-- the column name, order and default are identical above by inspection.
--
-- TEXT NOT NULL DEFAULT '' matches every other prose column in this schema
-- (ADR 0004, and the 0085 files' note that timestamps are TEXT here too).
-- The migration is a single ALTER, so it takes a table lock briefly; at 667
-- rows that is microseconds.
--
-- RETENTION. As the SQLite file: no cascade, nothing deleted by it.
ALTER TABLE roadmap_cards ADD COLUMN body TEXT NOT NULL DEFAULT '';

## Verification command and expected output

expected output is written down. Where a step needs a symbol that does not
-- identically, so this migration is invisible to that test in both files.
### 2.3 Verify
cargo test -p darkhold-db --lib migrate:: 2>&1 | tail -20
Expected: all `migrate::` tests pass, including
Expected: `0` (not yet applied to a fresh dev DB — `--status` lists *pending*;
Expected: both paths printed.
  each other, so two 0086 files pass it happily and the failure arrives later,
is invisible to the compiler and to a test that asserts a card's category.
### 3.5 Verify
cargo test -p darkhold-db --lib roadmap 2>&1 | tail -15
Expected: existing roadmap tests pass. Any failure naming a column is the
index shift; any failure naming `expected String, found` is a mapper.
Expected: no warnings.
### 4.5 Verify
cargo test -p darkhold-app --test milestone_45_roadmap 2>&1 | tail -20
Expected: all 60+ existing roadmap tests pass. The board's added `body` field
### 5.5 Verify
bash frontend/scripts/fe.sh test 2>&1 | tail -20
Expected: `svelte-check` reports 0 errors; the existing `Roadmap.test.ts`

## RED/GREEN evidence

start*, and do not trust this document's 0090 if time has passed.
Record which of these passed, in `docs/verification.md`, with the status
