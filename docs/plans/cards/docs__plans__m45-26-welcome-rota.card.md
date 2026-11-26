# docs/plans/m45-26-welcome-rota.md — plan card

Source: `docs/plans/m45-26-welcome-rota.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Queue head by `docs/requirements.csv` (milestone column, then row id): M45-26
It is also not the thing worth fixing first. Reading `crates/db/src/supply.rs:891-935` this session:
## Schema — `migrations/{sqlite,postgres}/0135_welcome_rota.sql`
- `docs/snapshot-column-policy.json` gains one entry per column. The gate names what it wants:
  run `python3 scripts/check-snapshot-pii.py` after adding the file and add exactly the columns it
## Store — `crates/db/src/welcome.rs`
## Routes — `crates/app/src/routes/welcome.rs` (module id `welcome`)
`crates/app/src/server.rs`; the registry-row test fails if a module is mounted without a row.
## Registry row — `crates/domain/src/modules/registry.rs`
relevance: 8, source: "crates/app/src/routes/welcome.rs",
## Spec — `docs/spec.md` §58.6
`crates/db/tests/welcome_rota.rs` (both engines, `--test-threads=1`):

## Copy-pasteable code / commands

CREATE TABLE welcome_rota (
    id           TEXT PRIMARY KEY,
    work_id      TEXT NOT NULL,
    pseud_id     TEXT NOT NULL,
    author_key   TEXT NOT NULL DEFAULT '',
    assigned_at  TEXT NOT NULL,
    due_at       TEXT NOT NULL,
    completed_at TEXT,
    comment_id   TEXT
);
CREATE INDEX welcome_rota_open ON welcome_rota (pseud_id, completed_at);
CREATE INDEX welcome_rota_work ON welcome_rota (work_id);

pub const DUE_HOURS: i64 = 72;

pub struct Assignment { id, work_id, title, source_key, author_key, assigned_at, due_at, overdue }
pub enum Claim { Assigned(Assignment), AlreadyOpen(Assignment), NothingToWelcome { eligible: i64 } }
pub enum Complete { Done(Assignment), NoCommentYet { work_id: String }, NotYours, NoSuchAssignment }
pub struct RotaReport { measured: bool, reason: Option<String>, eligible, assigned, completed, overdue }

pub async fn claim(db: &Database, pseud_id: &str) -> Result<Claim>;
pub async fn mine(db: &Database, pseud_id: &str) -> Result<Vec<Assignment>>;
pub async fn complete(db: &Database, pseud_id: &str, id: &str) -> Result<Complete>;
pub async fn report(db: &Database) -> Result<RotaReport>;

SELECT li.id, CAST(li.work_id AS TEXT) AS work_id, li.title, li.source_key,
       lower(trim(li.author_text)) AS author_key
  FROM library_items li
 WHERE li.work_id IS NOT NULL
   AND (li.likes > 0 OR li.status = 'complete')
   AND NOT EXISTS (SELECT 1 FROM comments c
                    WHERE c.subject_type = 'work' AND c.subject_id = CAST(li.work_id AS TEXT))
   AND NOT EXISTS (SELECT 1 FROM welcome_rota r
                    WHERE r.work_id = CAST(li.work_id AS TEXT) AND r.completed_at IS NULL)
   AND lower(trim(li.author_text)) NOT IN (SELECT author_key FROM welcome_rota)
 ORDER BY (li.likes > 0) DESC, CAST(li.likes AS BIGINT) DESC, li.title
 LIMIT 1

## Verification command and expected output

payment path deserves its own row, its own test and its own mutation — and the welcome rota is what
- `docs/snapshot-column-policy.json` gains one entry per column. The gate names what it wants:
`crates/app/src/server.rs`; the registry-row test fails if a module is mounted without a row.
`crates/db/tests/welcome_rota.rs` (both engines, `--test-threads=1`):
| test | mutation that must break it |
cargo test -p darkhold-domain -p darkhold-db -p darkhold-app --lib
sudo docker start lh-pg-test
cargo test -p darkhold-db --test welcome_rota            # sqlite
LOREHAVEN_TEST_PG_URL=… cargo test -p darkhold-db --test welcome_rota
cargo test -p darkhold-app --test m45_26_welcome_rota
`requirements.csv`: M45-26 → `implemented-fully-tested` with the test paths as evidence; new row

## RED/GREEN evidence

  A button that marks the rota done without the comment is the feature failing while showing green.
python3 scripts/ideas_catalogue.py                        # the idea catalogue must stay green
