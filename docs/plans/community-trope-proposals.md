# Implementation plan — community trope proposals with quorum (M45-77)

This plan is written for an implementer with the spec amendment open beside it.
It assumes the workstation only edits; builds and tests run on `thinkcentre`.
Do not start a step until the previous step's verification prints what it says.

Every store statement is written twice with `db.sql(sqlite, postgres)`, or in an
explicit `match db.backend()`. SQL in both arms uses `?`; `Database::sql`
rewrites the placeholders for PostgreSQL. Never hand-write `$1`. A new test is
not done until it has been run on SQLite and on PostgreSQL.

## Step 0 — anchors and gates

Files:
- `docs/spec-amendments/community-trope-proposals.md`
- `crates/domain/src/tropes.rs`
- `crates/domain/src/query.rs`
- `crates/db/src/category_governance.rs` (pattern)
- `crates/app/src/config.rs`
- `crates/app/tests/config_sections.rs`
- `crates/app/src/routes/inventory.rs`
- `crates/app/src/server.rs`
- `docs/requirements.csv`

Verify:

```bash
cd ~/code-local/rust/darkhold
git status -sb
test -f docs/spec-amendments/community-trope-proposals.md && echo spec-ok
grep -n 'MAX_MEMBERS' crates/domain/src/tropes.rs | head -3
python3 scripts/check-requirement-ids.py | tail -3
```

Expected: clean `## master...origin/master`, `spec-ok`, `MAX_MEMBERS = 10`
(or equivalent), `989 unique ids / 0 duplicates`.

## Step 1 — ledger rows

Append with Python, preserving CRLF:

```python
import csv
path = 'docs/requirements.csv'
with open(path, newline='') as f:
    rows = list(csv.DictReader(f))
new = [
 ('M45-77','discovery','Curators and TL>=N accounts can propose a trope group of 1..10 real tags; two independent approvals make it active','M45-77','planned','docs/spec-amendments/community-trope-proposals.md','owner decisions 2026-11-24','',''),
 ('M45-77.1','governance','One account gets one vote; the proposer may not vote on their own proposal','M45-77','planned','trope_votes UNIQUE (proposal_id, account_id) + route guard','','' ,''),
 ('M45-77.2','search','Active community groups expand through the same parser and term budget as the five seed groups','M45-77','planned','cargo test -p darkhold-domain parse_query_with_tropes','','',''),
 ('M45-77.3','config','[tropes] min trust, quorum and voting days are read from darkhold.toml by the real loader','M45-77','planned','cargo test --test config_sections tropes','','',''),
 ('M45-77.4','store','Proposal, vote and active state are applied in one transaction on SQLite and PostgreSQL','M45-77','planned','DARKHOLD_TEST_PG_URL cargo test -p darkhold-db --test trope_governance','','',''),
 ('M45-77.5','routes','Proposal/vote/settle routes are tabled with the right audience; public reads never expose an account id','M45-77','planned','cargo test --test route_inventory','','',''),
]
fieldnames = list(rows[0].keys())
assert len(fieldnames) == 9, fieldnames
with open(path, 'a', newline='') as f:
    w = csv.DictWriter(f, fieldnames=fieldnames)
    for values in new:
        w.writerow(dict(zip(fieldnames, values)))
```

Do not assume a 6-column header: the real one is
`id,area,requirement,milestone,status,evidence,notes,verified_at,gates`. Check
`csv.reader` before appending; the original six-value sketch below was wrong
and would have silently dropped evidence.

Verify:

```bash
python3 scripts/check-requirement-ids.py | tail -3
python3 scripts/ledger.py show M45-77 | head -20
git diff --check
```

Expected: `995 unique ids / 0 duplicates`, all six rows printed.

## Step 2 — migration 0219, both engines

New file `migrations/sqlite/0219_community_tropes.sql`.

```sql
CREATE TABLE trope_proposals (
    id              TEXT PRIMARY KEY,
    slug            TEXT NOT NULL,
    action          TEXT NOT NULL CHECK (action IN ('create','update','retire')),
    payload         TEXT NOT NULL DEFAULT '{}',
    status          TEXT NOT NULL DEFAULT 'proposed'
                    CHECK (status IN ('proposed','passed','active','failed','expired','retired')),
    yes_votes       INTEGER NOT NULL DEFAULT 0,
    no_votes        INTEGER NOT NULL DEFAULT 0,
    quorum_needed   INTEGER NOT NULL,
    closes_at       TEXT NOT NULL,
    proposed_by     TEXT NOT NULL REFERENCES accounts(id) ON DELETE SET NULL,
    decided_by      TEXT,
    decision_reason TEXT,
    created_at      TEXT NOT NULL,
    decided_at      TEXT
);
CREATE UNIQUE INDEX trope_proposals_live_slug
    ON trope_proposals (slug) WHERE status IN ('proposed','active');
CREATE INDEX trope_proposals_status_idx ON trope_proposals (status, closes_at);

CREATE TABLE trope_votes (
    id          TEXT PRIMARY KEY,
    proposal_id TEXT NOT NULL REFERENCES trope_proposals(id) ON DELETE CASCADE,
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    value       TEXT NOT NULL CHECK (value IN ('yes','no')),
    created_at  TEXT NOT NULL,
    UNIQUE (proposal_id, account_id)
);
CREATE INDEX trope_votes_proposal_idx ON trope_votes (proposal_id);
```

PostgreSQL arm: same file translated in the repo's PostgreSQL migration
directory (`migrations/postgres/0219_community_tropes.sql`), with `UUID` for
`id`, `proposed_by`, `decided_by`, `proposal_id`, `account_id`, `TEXT` for
`payload`, `TIMESTAMPTZ` for `closes_at`, `created_at`, `decided_at`, and the
same partial indexes. Follow `0217_user_lists.sql` for the cast and index
style. Do not invent a `quorum_records` table.

Verify:

```bash
cd ~/code-local/rust/darkhold
grep -c 'CREATE TABLE' migrations/sqlite/0219_community_tropes.sql
# expected: 2

ssh thinkcentre 'ls /personal/documents/code/rust/darkhold/migrations/postgres/ | grep 0219'
# expected: 0219_community_tropes.sql
```

## Step 3 — domain: resolver-aware parser

File `crates/domain/src/tropes.rs`.

Add besides the static table:

```rust
/// Extra groups supplied by the store. Seed groups keep their slugs.
pub type TropeResolver<'a> = dyn Fn(&str) -> Option<Vec<String>> + 'a;

/// Resolve a slug first from the shipped groups, then from the store.
pub fn resolve<'a>(
    slug: &str,
    extra: Option<&TropeResolver<'a>>,
) -> Option<Vec<String>> {
    if let Some(members) = expand(slug) {
        return Some(members.iter().map(|s| (*s).to_owned()).collect());
    }
    extra.and_then(|r| r(slug)).filter(|m| {
        !m.is_empty() && m.len() <= MAX_MEMBERS
    })
}

/// Every active slug, seed first, for the unknown-name error.
pub fn active_slugs(extra_slugs: &[String]) -> Vec<String> {
    let mut all = slugs().into_iter().map(str::to_owned).collect::<Vec<_>>();
    all.extend(extra_slugs.iter().cloned());
    all.sort();
    all.dedup();
    all
}
```

File `crates/domain/src/query.rs`. Keep `parse_query` and
`parse_query_with` unchanged as seed-only wrappers. Add:

```rust
/// Parse with store-backed trope groups. Every fielded term, budget and
/// error still comes from this one parser (M45-77).
pub fn parse_query_with_tropes<'a>(
    input: &str,
    budget: QueryBudget,
    resolver: &'a TropeResolver<'a>,
    extra_slugs: &[String],
) -> std::result::Result<QueryAst, QueryError> {
    let mut parser = Parser::new(input);
    parser.budget = budget;
    parser.trope_resolver = Some(resolver);
    parser.extra_slugs = extra_slugs.to_vec();
    let ast = parser.parse()?;
    budget.check(&ast, 0)?;
    Ok(ast)
}
```

Add to `Parser<'a>`:

```rust
    trope_resolver: Option<&'a TropeResolver<'a>>,
    extra_slugs: Vec<String>,
```

initialise both in `Parser::new` (`None`, `Vec::new()`), and change the free
`trope_node` into a method:

```rust
    fn trope_node(&self, value: &str, offset: usize) -> Result<QueryAst, QueryError> {
        let members = crate::tropes::resolve(value, self.trope_resolver).ok_or_else(|| {
            QueryError::new(
                format!(
                    "trope:{value}: not a trope group — expected one of {}",
                    crate::tropes::active_slugs(&self.extra_slugs).join(", ")
                ),
                offset,
            )
        })?;
        Ok(QueryAst::Or(
            members.into_iter()
                .map(|member| QueryAst::Fielded(QueryField::Tag, member))
                .collect(),
        ))
    }
```

Change the two `QueryField::Trope` call sites (around lines 1249 and 1300) to
`self.trope_node(..)`. Import `TropeResolver` from `crate::tropes`.

Tests in `crates/domain/src/query.rs`:

```rust
#[test]
fn community_trope_expands_through_the_same_parser() {
    let extra: Vec<String> = vec!["gamer".into(), "litrpg".into()];
    let resolver = |slug: &str| {
        if slug == "system-progression" { Some(extra.clone()) } else { None }
    };
    let ast = parse_query_with_tropes(
        "trope:system-progression", QueryBudget::default(), &resolver,
        &["system-progression".into()],
    ).unwrap();
    match ast {
        QueryAst::Or(parts) => assert_eq!(parts.len(), 2),
        other => panic!("expected OR, got {other:?}"),
    }
}

#[test]
fn unknown_trope_lists_seed_and_community_slugs() {
    let resolver = |_slug: &str| -> Option<Vec<String>> { None };
    let err = parse_query_with_tropes(
        "trope:nope", QueryBudget::default(), &resolver,
        &["system-progression".into()],
    ).unwrap_err().to_string();
    assert!(err.contains("system-grind"), "{err}");
    assert!(err.contains("system-progression"), "{err}");
}
```

Verify:

```bash
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && cargo test -p darkhold-domain tropes -- --nocapture'
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && cargo test -p darkhold-domain unknown_trope_lists_seed_and_community_slugs -- --nocapture'
```

Expected: both tests pass; removing the resolver call reddens the first test;
removing `extra_slugs` from the error reddens the second.

## Step 4 — config with a real file reader

File `crates/app/src/config.rs`.

```rust
/// Trope proposal governance (M45-77, spec amendment).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TropesConfig {
    /// Curator OR trust level at or above this may propose and vote.
    pub proposal_min_trust: i64,
    /// Independent approvals required. Below 2 is refused at load.
    pub quorum: u32,
    /// Days a proposal stays open.
    pub voting_days: i64,
}

impl Default for TropesConfig {
    fn default() -> Self {
        Self { proposal_min_trust: 3, quorum: 2, voting_days: 7 }
    }
}
```

Add `pub tropes: TropesConfig` to `Config`, initialise it in
`development_defaults()` with `TropesConfig::default()`, add
`tropes: Option<TropesSection>` to `FileConfig`, and declare:

```rust
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TropesSection {
    proposal_min_trust: Option<i64>,
    quorum: Option<u32>,
    voting_days: Option<i64>,
}
```

In the loader beside `// --- adoption`:

```rust
        // --- tropes (M45-77) ---------------------------------------------
        let tropes = TropesConfig {
            proposal_min_trust: file.tropes.as_ref()
                .and_then(|t| t.proposal_min_trust)
                .unwrap_or_else(|| TropesConfig::default().proposal_min_trust),
            quorum: file.tropes.as_ref()
                .and_then(|t| t.quorum)
                .unwrap_or_else(|| TropesConfig::default().quorum),
            voting_days: file.tropes.as_ref()
                .and_then(|t| t.voting_days)
                .unwrap_or_else(|| TropesConfig::default().voting_days),
        };
        anyhow::ensure!(
            tropes.quorum >= 2,
            "[tropes].quorum below 2 is not a quorum (§14.7)"
        );
```

Add `tropes,` to the `Config { ... }` constructor. Do **not** put these keys
into `TaxonomyConfig`: `taxonomy.trope_coverage` is the recorded example of a
field declared on `Config` with no `FileConfig` reader.

File `crates/app/tests/config_sections.rs`: add to `FULL`:

```toml
[tropes]
proposal_min_trust = 4
quorum = 3
voting_days = 14
```

and assert those three values. Add a refusal case for `quorum = 1` and a
typo case for `quo=3`; both must fail to load.

Verify:

```bash
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && cargo test --test config_sections tropes -- --nocapture'
```

Expected: three tests pass. The `quorum = 1` case must fail with the ensure
message, not silently load.

## Step 5 — domain governance rule

New file `crates/domain/src/trope_governance.rs`.

```rust
//! Trope proposal rules (M45-77). No I/O, no trust lookups, no timestamps.

/// The only actions a proposal may carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TropeAction { Create, Update, Retire }

impl TropeAction {
    pub fn as_str(self) -> &'static str {
        match self { Self::Create => "create", Self::Update => "update", Self::Retire => "retire" }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s { "create" => Some(Self::Create), "update" => Some(Self::Update),
                  "retire" => Some(Self::Retire), _ => None }
    }
}

/// The proposer is taken from the session; a body cannot claim authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposerRole { Curator, Trusted, Operator }

/// Reuse the tested category-governance tally rather than a second rule.
pub fn proposal_decided(yes: u32, no: u32, quorum: u32) -> Option<bool> {
    crate::category_governance::proposal_decided(yes, no, quorum)
}

/// Effective status before a sweep writes `expired`.
pub fn effective_status(stored: &str, closes_at: &str, now: &str) -> String {
    if stored == "proposed" && now >= closes_at { "expired".to_owned() }
    else { stored.to_owned() }
}

/// Slug grammar. Deliberately closed: a slug becomes a query token.
pub fn valid_slug(s: &str) -> bool {
    let b = s.as_bytes();
    (3..=40).contains(&b.len())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && !s.starts_with('-') && !s.ends_with('-') && !s.contains("--")
}
```

Tests: valid slugs, `2 yes -> Some(true)`, `2 no -> Some(false)`, `1 yes/1 no -> None`, expiry boundary, `Retire.parse`. Add `pub mod trope_governance;` to `crates/domain/src/lib.rs`.

Verify:

```bash
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && cargo test -p darkhold-domain trope_governance -- --nocapture'
```

Expected: all pass. A mutation changing `>=` to `>` in `proposal_decided` is
not here (borrowed); mutating `valid_slug` to accept `--` must redden its test.

## Step 6 — store repository

New file `crates/db/src/trope_governance.rs`.

```rust
//! Community trope proposals (M45-77). Both dialects.
//! SQL is written with `?` and rewritten by `Database::sql`.

use anyhow::Result;
use serde::Serialize;
use sqlx::{FromRow, Row};
use crate::{Backend, Database};
use darkhold_domain::trope_governance::{proposal_decided, TropeAction};

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TropeProposal {
    pub id: String,
    pub slug: String,
    pub action: String,
    pub payload: String,
    pub status: String,
    pub yes_votes: i64,
    pub no_votes: i64,
    pub quorum_needed: i64,
    pub closes_at: String,
    pub proposed_by: String,
    pub decided_by: Option<String>,
    pub decision_reason: Option<String>,
    pub created_at: String,
    pub decided_at: Option<String>,
}
```

Functions to implement, all through `db.sql` and `match db.backend()`:

- `insert_proposal(db, slug, action, payload, quorum, closes_at, proposed_by, now) -> Result<String>`
  — `INSERT ...`; a live-slug conflict returns a typed error carrying
  `"a proposal or active group already uses this slug"`.
- `get_proposal(db, id) -> Result<Option<TropeProposal>>`
- `list_proposals(db, status: Option<&str>, limit: i64) -> Result<Vec<TropeProposal>>`
- `insert_vote(db, proposal_id, account_id, value, now) -> Result<bool>`
  — `ON CONFLICT(proposal_id, account_id) DO NOTHING`; binds with
  `.bind(&id)`, `.bind(proposal_id)`, `.bind(account_id)`, `.bind(value)`,
  `.bind(now)`; returns `rows_affected() == 1`.
- `recount_and_decide(db, proposal_id, now, decider) -> Result<Option<bool>>`
  — copy the structure at `crates/db/src/category_governance.rs:550-640`:
  update yes/no from `trope_votes`, read the row, call
  `proposal_decided(yes as u32, no as u32, quorum as u32)`, and when decided,
  `UPDATE trope_proposals SET status=?, decided_by=?, decided_at=? WHERE id=?`.
  On `Some(true)`, call `apply_proposal` **inside the same transaction**.
- `apply_proposal(tx_or_db, proposal) -> Result<()>` — parse the JSON payload
  into `{ label, summary, members, target_version }`; for `create` insert an
  active row into `trope_proposals` with a new id and `status='active'`, for
  `update` verify `target_version` matches the current active row before
  updating, for `retire` set the active row to `status='retired'`. The
  uniqueness filter allows a new `create` only when no open/active row exists.
  Use `?1::uuid` casts in PostgreSQL branches where a UUID parameter must be
  typed, following the no-op-conflict fix in the M45-76 plan.
- `active_community_groups(db) -> Result<Vec<(String, Vec<String>)>>` — select
  slug and payload where `status='active'`, parse members; used by the
  resolver.
- `settle_expired(db, limit, now) -> Result<SettleReport>` — mark
  `status='proposed' AND closes_at < now` as `expired`; re-apply any `passed`
  row whose active group is missing. Reports
  `{ scanned, expired, applied, remaining }`.

Every public query has a `Backend::Sqlite` arm and a `Backend::Postgres` arm;
never `?` a PostgreSQL hand-written `Row` conversion. Add
`pub mod trope_governance;` to `crates/db/src/lib.rs`.

Repository tests in `crates/db/tests/trope_governance.rs`, run with
`DARKHOLD_TEST_PG_URL` set and unset:

```rust
#[tokio::test]
async fn two_distinct_yes_votes_pass_and_apply() { /* seed two accounts, propose, vote yes twice */ }
#[tokio::test]
async fn a_second_vote_from_the_same_account_does_not_count() { /* insert_vote twice -> false */ }
#[tokio::test]
async fn two_no_votes_fail_without_applying() { /* status='failed', active_groups empty */ }
#[tokio::test]
async fn stale_target_version_refuses_an_update() { /* update proposal against newer version */ }
#[tokio::test]
async fn expiry_is_derived_before_the_sweep_and_written_after_it() { /* effective_status + settle */ }
```

Verify:

```bash
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && cargo test -p darkhold-db --test trope_governance -- --nocapture'
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && DARKHOLD_TEST_PG_URL=postgres://darkhold:darkhold@127.0.0.1:55433/darkhold cargo test -p darkhold-db --test trope_governance -- --nocapture'
```

Expected: five tests pass on each engine. If the PostgreSQL URL is refused,
start the container with `sudo docker start lh-pg-test` on `thinkcentre` and
rerun; do not skip the engine.

## Step 7 — authority predicate

New file `crates/app/src/trope_authority.rs`.

```rust
use darkhold_app::config::TropesConfig;
use darkhold_db::{media_resilience, Database};
use sqlx::{FromRow, Row};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum TropeRole { Curator, Trusted, Operator }

pub struct TropeAuthority {
    pub allowed: bool,
    pub role: Option<TropeRole>,
    pub level: i64,
}

pub async fn for_account(
    db: &Database, cfg: &TropesConfig, account_id: &str,
) -> Result<TropeAuthority, sqlx::Error> {
    let level = darkhold_db::governance::trust_for(db, account_id).await?;
    let sql = db.sql(
        "SELECT coalesce(is_curator, false) AS is_curator FROM accounts WHERE id = ?",
        "SELECT coalesce(is_curator, false) AS is_curator FROM accounts WHERE id = ?",
    );
    let is_curator = match db.backend() {
        Backend::Sqlite => sqlx::query_scalar(&sql).bind(account_id)
            .fetch_one(db.sqlite_pool().expect("sqlite")).await.unwrap_or(false),
        Backend::Postgres => sqlx::query_scalar(&sql).bind(account_id)
            .fetch_one(db.postgres_pool().expect("postgres")).await.unwrap_or(false),
    };
    let active = media_resilience::is_active_curator(db, account_id).await?;
    let role = if is_curator || active {
        Some(TropeRole::Curator)
    } else if level >= 5 {
        Some(TropeRole::Operator)
    } else if level >= cfg.proposal_min_trust {
        Some(TropeRole::Trusted)
    } else {
        None
    };
    Ok(TropeAuthority { allowed: role.is_some(), role, level })
}
```

`is_curator` is checked before the trust branch so a TL2 curator is still a
curator. An operator is labelled `Operator` in public answers but has the same
vote power as any other eligible account and no bypass. Add
`pub mod trope_authority;` to `crates/app/src/lib.rs`.

Verify: unit-test it with file fixtures for `(TL2, non-curator) -> denied`,
`(TL2, is_curator) -> Curator`, `(TL4, non-curator) -> Trusted`,
`(TL6, non-curator) -> Operator`, `(TL4, active curator role) -> Curator`.

```bash
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && cargo test -p darkhold-app trope_authority -- --nocapture'
```

## Step 8 — wire the resolver into every production parse site

Search first; do not guess the list:

```bash
cd ~/code-local/rust/darkhold
grep -rn 'parse_query(' crates --include='*.rs' | grep -v '#\[cfg(test)\]' | grep -v tests
```

Expected production sites: `crates/db/src/search/ast_search.rs`,
`crates/db/src/search/forum_ast_search.rs`, `crates/db/src/search/user_ast_search.rs`,
`crates/app/src/routes/media.rs` (two), `crates/app/src/routes/exports.rs`,
`crates/app/src/routes/bulk_export.rs`. For each site that has a `&Database`:

```rust
let extra = darkhold_db::trope_governance::active_community_groups(db).await?;
let slugs = extra.iter().map(|(s, _)| s.clone()).collect::<Vec<_>>();
let resolver = |slug: &str| -> Option<Vec<String>> {
    extra.iter().find(|(s, _)| s == slug).map(|(_, m)| m.clone())
};
let ast = darkhold_domain::query::parse_query_with_tropes(
    query, budget, &resolver, &slugs,
)?;
```

Keep the existing `parse_query_with` call for budget-only tests. A site with
no database must keep seed-only parsing and name that limitation in its
`VALIDATION_QUERY` error; never let two surfaces silently disagree.

Verify:

```bash
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && cargo check --workspace --all-targets'
```

Expected: exit 0, no `unused variable` warnings from this change.

## Step 9 — routes

New file `crates/app/src/routes/trope_proposals.rs`.

```rust
use axum::{extract::{Path, State}, Json, Router};
use darkhold_app::auth::{MaybeSession, RequireSession};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tropes/proposals", axum::routing::post(create_proposal))
        .route("/tropes/proposals", axum::routing::get(list_proposals))
        .route("/tropes/proposals/:id", axum::routing::get(get_proposal))
        .route("/tropes/proposals/:id/vote", axum::routing::post(vote))
        .route("/operator/tropes/settle", axum::routing::post(settle))
}
```

Handler rules:

- `create_proposal(State, RequireSession, Json<CreateTropeProposal>)` parses the
  action with `TropeAction::parse`, checks `valid_slug`, resolves every member
  with the same tag lookup `query_sql.rs::taxonomy_predicate` uses (a helper
  in `crates/db/src/taxonomy.rs`, reused, not re-implemented), refuses seed
  slugs from `darkhold_domain::tropes::slugs()`, and checks
  `trope_authority::for_account`. Errors use `AppError::Validation` with
  `field_errors` for slug/members and `AppError::AccessDenied` for the authority
  case. `ApiError` comes from `crate::http::ApiError`; `?` is enough where
  `From<AppError> for ApiError` exists — don't add a second `.map_err`.
- `list_proposals(State, MaybeSession)` and `get_proposal(State, MaybeSession)`
  map rows to a public struct with only coarse role, tallies and `expands_to`.
  **Never serialise `proposed_by`, `decided_by` or individual votes.**
- `vote(State, RequireSession, Path(id), Json<VoteBody>)` re-checks authority,
  refuses `account_id == proposal.proposed_by` with 403 naming the
  independence rule, then calls `insert_vote` and `recount_and_decide`.
- `settle(State, RequireSession, Json<SettleBody>)` calls
  `crate::routes::discovery::require_operator(&state, &user)?` (the existing
  operator helper), then `settle_expired`.
- `GET /tropes` in `crates/app/src/routes/taxonomy.rs` gains `source`,
  `proposal_id` and `version` on each group, and merges active community
  groups from `active_community_groups`.

Register `pub mod trope_proposals;` in `crates/app/src/routes/mod.rs` and nest
`routes::trope_proposals::router()` beside `routes::taxonomy::router()` at
`crates/app/src/server.rs:1104`. Add five `RouteEntry` rows to
`crates/app/src/routes/inventory.rs`: three public (`MaybeSession`), two
authenticated (`RequireSession`), and the operator one marked as requiring
operator.

Verify:

```bash
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && cargo test --test route_inventory -- --nocapture'
```

Expected: inventory 5/5 with the new lines present; no `unmounted routers`
failure.

## Step 10 — acceptance tests

New file `crates/app/tests/trope_proposals.rs`, with a file-backed SQLite app
and the PostgreSQL app helper the other route tests use.

Cases:

1. `TL2` account POST -> 403 naming `proposal_min_trust` and the account's level.
2. `TL4` account POST -> 201, `status="proposed"`, `quorum_needed=2`.
3. Curator by `accounts.is_curator` POST -> 201; curator by active role POST -> 201.
4. Duplicate seed slug -> 422 naming `system-grind`.
5. Unknown member -> 422 naming the unknown member and all missing names.
6. Eleven members -> 422 naming the parse budget.
7. Proposer vote -> 403 naming independence.
8. Two distinct yes votes -> 200 with `status="passed"`; `GET /tropes` lists the
   group with `source="community"`; search for `trope:slug` returns the same
   hits as the OR of member `tag:` queries.
9. Same account votes twice -> `changed=false`, tallies unchanged.
10. Two no votes -> `status="failed"`, no active group.
11. Update with stale `target_version` -> 422; with current version -> active
    payload changes; `GET /tropes` shows the new `version`.
12. Retire -> group absent from `GET /tropes`; query 422 lists active slugs.
13. Public GET -> no substring of any account UUID from `proposed_by` or votes.
14. Expired proposal derived before sweep, written after `settle`.
15. `[tropes] quorum=1` config -> load refused.

Verify on both engines:

```bash
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && cargo test --test trope_proposals -- --nocapture'
ssh thinkcentre 'cd /personal/documents/code/rust/darkhold && DARKHOLD_TEST_PG_URL=postgres://darkhold:darkhold@127.0.0.1:55433/darkhold cargo test --test trope_proposals -- --nocapture'
```

Expected: 15 tests pass on each. Do not run `gate.sh all`; targeted only.

## Step 11 — mutation checks

For each mutation, revert it immediately after seeing the test fail:

- Change `account_id == proposal.proposed_by` to `false` -> case 7 fails.
- Remove the member-existence lookup -> case 5 fails.
- Change `quorum >= 2` to `quorum >= 1` in config -> Step 4 refusal fails.
- Delete the resolver in one parse site -> that surface's case 8 fails.
- Replace `proposal_decided` with `yes >= quorum` only -> case 10 fails.

Record each red test name in the commit body. A mutation that does not redden
a test means the test is not evidence; fix the test before shipping.

## Step 12 — live probe and handoff

With the binary deployed on `thinkcentre`, unset any test token variables first:

```bash
unset DARKHOLD_BASE_URL DARKHOLD_TOKEN DARKHOLD_COOKIE
curl -s http://127.0.0.1:8081/api/v1/tropes | python3 -c 'import json,sys; d=json.load(sys.stdin); print(sorted((g["slug"],g["source"]) for g in d["tropes"]))'
```

Expected: five `seed` groups, no community groups (nothing proposed yet).

Then, using a real TL3+ test account through the operator's own test flow,
propose, vote from two distinct eligible accounts, and probe:

```bash
curl -s "http://127.0.0.1:8081/api/v1/search?q=trope:<slug>&limit=20" | python3 -c 'import json,sys; print(json.load(sys.stdin)["total"])'
```

Expected: non-zero and equal to the same query with the member tags written
out. Record the live counts of eligible proposers before the probe — they were
two TL3+ accounts and zero curators on 2026-11-24, so two distinct non-proposer
approvals may be impossible until more curators are granted. That is an
operator staffing action, not a reason to lower the quorum.

## Commit sequence

1. `docs(darkhold): trope proposal governance spec and ledger rows (M45-77)`
2. `feat(db): migration and repository for community trope proposals`
3. `feat(domain): resolver-aware trope expansion`
4. `feat(config): [tropes] min trust, quorum and voting days`
5. `feat(app): trope proposal, vote and settle routes`
6. `test(darkhold): two-engine trope governance suite and mutation record`

Every commit builds on `thinkcentre`; do not push until step 11's mutation list
is in the last commit body. Land by fast-forwarding `master` in both checkouts.
