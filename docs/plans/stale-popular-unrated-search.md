# M45-75 — implementation plan: stale, popular, unrated search across sources

Status: **ready to implement**, written 2026-11-24 from
`docs/spec-amendments/stale-popular-unrated-search.md` (row `M45-75`).
Spec first, then this plan; do not start with code. Work in
`~/code-local/rust/darkhold` on a worktree, build/test on **thinkcentre**
(there is no local Rust toolchain for this workspace on the workstation), and
land by fast-forwarding `master` in both checkouts. Each step is a small commit.

## Conventions this plan obeys

- Every store statement is written twice (`db.sql(sqlite, postgres)` or a
  `match db.backend()`), never one string with `?` reused for PostgreSQL.
- Every new test runs on SQLite **and** on PostgreSQL
  (`DARKHOLD_TEST_PG_URL`, container `lh-pg-test`, 55433, reached from the
  workstation via `ssh -f -N -L 127.0.0.1:55433:127.0.0.1:55433 thinkcentre`).
- Targeted tests only. No `gate.sh all`, no full-workspace sweep mid-turn.
- Unset `DARKHOLD_BASE_URL DARKHOLD_TOKEN DARKHOLD_COOKIE` before CLI tests.
- `route_inventory` needs a `RouteEntry` only if a new route is registered;
  this plan adds **no route**, so that gate should stay green unchanged.
- Two clocks: `works.updated_at` = ours; `library_items.source_updated_at` =
  author's. Never mix them in one predicate.

## Step 0 — reproduce the gap and freeze the baseline

**0.1** Sync the two checkouts (the deploy tree is the one with the live code):

```bash
git -C ~/code/rust/darkhold status --short          # expect empty
git -C ~/code-local/rust/darkhold fetch cmp
git -C ~/code-local/rust/darkhold merge --ff-only cmp/master
git -C ~/code-local/rust/darkhold worktree add -b m45-75-stale-search \
  ~/code-local/rust/darkhold-m45-75 master
export CARGO_TARGET_DIR=~/.cargo-target-darkhold-dev
```

Expected: `Preparing worktree (new branch 'm45-75-stale-search')`, both
checkouts clean afterwards.

**0.2** Reproduce on the live instance (2026-11-24 baseline; keep this output
in the commit message of step 1):

```bash
ssh thinkcentre python3 - <<'PY'
import json, urllib.parse, urllib.request
base = 'http://127.0.0.1:8081/api/v1/search'
for q in ['source:qq', 'source:qq OR source:sv', 'trope:system-grind',
          'tag:celestial', 'tag:cyoa', 'updated:<=-120d']:
    u = base + '?' + urllib.parse.urlencode({'q': q, 'limit': 5})
    with urllib.request.urlopen(u, timeout=180) as r:
        b = json.loads(r.read().decode())
    print(q, len(b['items']), [i['title'][:28] for i in b['items'][:2]])
PY
```

Expected (measured): `source:qq` → `0`, `source:qq OR source:sv` → `0`,
`trope:system-grind` → 5, `tag:celestial` → 1, `tag:cyoa` → 5,
`updated:<=-120d` → `0`. The first two and the last one are the defects.

**0.3** Confirm the clock split in read-only SQL:

```bash
ssh thinkcentre "sudo -u postgres psql -d darkhold -P pager=off -c \"
select count(*) filter (where updated_at::timestamptz < now() - interval '120 days') as works_stale, count(*) as works_total from works;
select count(*) filter (where source_updated_at < to_char(now() - interval '120 days','YYYY-MM-DD\\\"T\\\"HH24:MI:SS\\\"Z\\\"')) as items_stale, count(*) as items_total from library_items where source_updated_at is not null;\""
```

Expected: `0 | 58042` and `86541 | 90662` (counts may drift with ingestion; the
contrast must not).

## Step 1 — name the subgenre once

File: `crates/domain/src/tropes.rs`. Add a group **before** `MAX_MEMBERS`:

```rust
    TropeGroup {
        slug: "system-progression",
        label: "System progression fantasy",
        summary: "Numbers, levels and a power curve: system, gamer, LitRPG, celestial and CYOA stories.",
        members: &[
            "system",
            "gamer",
            "litrpg",
            "stat-grinding",
            "progression",
            "celestial",
            "cyoa",
        ],
    },
```

Also correct the stale comment above `MAX_MEMBERS`: a term is now a predicate,
so a `tag:` leaf costs **one** term, not two (see `query.rs::QueryBudget::walk`).

Test in the same file:

```rust
    #[test]
    fn system_progression_names_the_five_words_a_reader_types() {
        let members = expand("system-progression").expect("a group");
        for word in ["system", "gamer", "celestial", "cyoa", "progression"] {
            assert!(members.contains(&word), "{word} missing from {members:?}");
        }
        assert!(members.len() <= MAX_MEMBERS);
    }
```

Verify:

```bash
cargo test -p darkhold-domain system_progression_names_the_five_words_a_reader_types
```

Expected: `test result: ok. 1 passed; 0 failed` on the worktree. Commit:
`feat(query): name system-progression fantasy as one trope group (M45-75)`.

## Step 2 — extend the query vocabulary

Files: `crates/domain/src/query.rs`, `crates/domain/src/query_sql.rs`.

**2.1** Fields. Add to `QueryField`:

```rust
    /// An import's source key, e.g. `qq`, `sv`, `sb`, `ao3`.
    Source,
    /// The author's clock, from `library_items.source_updated_at`.
    SourceUpdated,
```

Extend `as_str` (`"source"`, `"source_updated"`), `parse` (both names), and
`entity()` (both → `EntityKind::Works`). Add `SourceUpdated` to
`is_comparable()` and to the `ValueKind::Date` arm of `comparison_value()`;
`Source` stays text and non-comparable.

**2.2** Viewer-relative leaf:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeField { Rated }

impl MeField {
    pub fn as_str(self) -> &'static str { match self { Self::Rated => "rated" } }
    pub fn parse(raw: &str) -> Option<Self> {
        match raw { "rated" => Some(Self::Rated), _ => None }
    }
}
```

Add `Me(MeField)` to `QueryAst`, a leaf arm (`leaf(terms)?`) to
`QueryBudget::walk`, and a parse branch **before** the generic `field:` branch:

```rust
                if let Some((head, value)) = term.split_once(':') {
                    if head == "me" {
                        return MeField::parse(value)
                            .map(QueryAst::Me)
                            .ok_or_else(|| QueryError::new(
                                format!("me:{value} is not a viewer field; me:rated is available"),
                                start,
                            ));
                    }
                }
```

**2.3** SQL. In `query_sql.rs` add the viewer-bind sentinel near the top:

```rust
/// Replaced with the viewer's account id by the search store. A bind that must
/// never be a literal: `me:rated` without a viewer is a 401, not a search for
/// the string "\0viewer".
pub const VIEWER_BIND: &str = "\0viewer";
```

In `render_node`, add:

```rust
        QueryAst::Me(MeField::Rated) => Ok(SqlFragment::new(
            "EXISTS (SELECT 1 FROM rating r WHERE r.work_id = works.id \
             AND r.account_id = ? AND r.deleted_at IS NULL)",
        ).with_bind(VIEWER_BIND)),
```

In `render_fielded`, add:

```rust
        QueryField::Source => Ok(SqlFragment::new(
            "EXISTS (SELECT 1 FROM library_items li_src \
             WHERE li_src.work_id = works.id AND li_src.source_key = ?)",
        ).with_bind(value.to_owned())),
```

In `date_column`, add:

```rust
        QueryField::SourceUpdated => Some(
            "SUBSTR((SELECT MAX(li_src_up.source_updated_at) FROM library_items li_src_up \
             WHERE li_src_up.work_id = works.id), 1, 10)",
        ),
```

Why `MAX`: the same story can appear on several archives; "when did the author
last touch it" is the newest of the source rows. `SUBSTR(...,1,10)` matches the
existing day-granularity date comparison and the canonical timestamp shape
migration 0132 enforces.

Verify:

```bash
cargo test -p darkhold-domain query --lib
cargo test -p darkhold-domain --lib query_sql
```

Expected: both `test result: ok`, no failures. Commit:
`feat(query): add source, source_updated and me:rated to the shared grammar (M45-75)`.

## Step 3 — wire the viewer bind and a real popularity sort in the search store

File: `crates/db/src/search/ast_search.rs`.

**3.1** Resolve the sentinel in `prepared`, after `let (sql, texts, numbers)`:

```rust
    let mut texts = texts;
    for b in &mut texts {
        if b == darkhold_domain::query_sql::VIEWER_BIND {
            match viewer_id {
                Some(vid) => *b = vid.to_owned(),
                None => return Err(SearchError::render(
                    "me:rated needs a signed-in viewer; use DARKHOLD_COOKIE or the web search box"
                ).into()),
            }
        }
    }
```

The bind order stays `viewer (visibility) → user terms → viewer (blocks)`;
replacing in place keeps `me:rated`'s position correct on both engines.

**3.2** Add the endorsement column to the inner select, next to `score`:

```rust
         (SELECT COALESCE(CAST(MAX(li_likes.likes) AS BIGINT), 0) \
          FROM library_items li_likes WHERE li_likes.work_id = works.id) AS likes, \
```

Extend `SearchRow` with one more `i64` (`_likes`) after `score`, and add it to
the destructuring. `CAST(... AS BIGINT)` is needed because PostgreSQL's `SUM`
widenings already bit this file once; do not rely on SQLite's flexible typing.

**3.3** Replace the `order_by` match:

```rust
    let order_by = match scope.sort.as_deref() {
        Some("newest") => "s.updated_at DESC, s.score DESC",
        Some("title") => "s.title ASC, s.score DESC",
        Some("popularity" | "likes" | "kudos" | "aggregate") => {
            "s.likes DESC, s.score DESC, s.updated_at DESC"
        }
        // Empty is "no preference". Any other word is a claim we cannot honour;
        // echoing it in `filters.sort` while ranking by score is the lie this
        // branch used to tell.
        Some(other) => {
            return Err(SearchError::render(format!(
                "unknown sort: {other}; available: popularity, likes, kudos, aggregate, newest, title"
            )).into())
        }
        None | Some("") => "s.score DESC, s.updated_at DESC",
    }
    .to_owned();
```

`scope_sql` returns `Result<BoundScope>` after this change; update its one
caller in `render_statement` with `?`.

**3.4** Contract test `crates/db/tests/stale_popular_unrated_search.rs`
(fresh suite, header says "both engines", config honours
`DARKHOLD_TEST_PG_URL` exactly as `docs/plans/.../gate-run-hygiene` prescribes).
Fixtures: four works with tags `system`/`cyoa`, two source rows each (`qq`,
`sv`), `source_updated_at` 30d and 200d old, one rating from viewer A, one
different work rated by viewer B. Cases:

- `source:qq OR source:sv` returns both; `source:ffnet` returns none.
- `source_updated:<=-4m` returns only the 200d works; `updated:<=-4m` returns
  none (the clock split, asserted in one test).
- `NOT me:rated` with viewer A removes exactly A's rated work; with viewer B a
  different one; anonymous → error containing `me:rated needs a signed-in viewer`.
- `sort=popularity` orders by likes; `sort=newest` orders by our clock, and a
  fixture where the two disagree proves the difference; `sort=no-such-sort`
  is an error naming the word and the vocabulary.

Verify:

```bash
cargo test -p darkhold-db --test stale_popular_unrated_search
sudo docker start lh-pg-test    # on thinkcentre, or the ssh tunnel from the workstation
DARKHOLD_TEST_PG_URL='postgres://darkhold:darkhold@127.0.0.1:55433/postgres' \
  cargo test -p darkhold-db --test stale_popular_unrated_search
```

Expected: `test result: ok. N passed; 0 failed` **twice**, same N both times.
Mutation check: delete the `popularity` ORDER BY arm and rerun the ordering
test — it must go red; restore before committing. Commit:
`feat(search): source clocks and popularity ordering in the AST store (M45-75)`.

## Step 4 — the CLI can type the query

File: `crates/cli/src/q.rs`.

Add to `QArgs`:

```rust
    /// Restrict to these source keys, comma-separated: qq,sv,sb,ffnet,ao3.
    #[arg(long, value_name = "KEYS")]
    pub source: Option<String>,

    /// Order by this metric; `popularity`, `newest`, `title` are accepted.
    #[arg(long, value_name = "SORT")]
    pub sort: Option<String>,

    /// Append `AND NOT me:rated` (needs DARKHOLD_COOKIE).
    #[arg(long)]
    pub exclude_rated: bool,
```

Change `url()` to take `sort: Option<&str>` and append `&sort=<encoded>`; keep
`explain` as the last parameter. Build the expression in `run()`:

```rust
    let mut expression = joined_expression(args)?;
    if let Some(sources) = args.source.as_deref() {
        let keys: Vec<&str> = sources.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        if keys.is_empty() { return Err(bad_request("--source needs at least one key")); }
        let clause = keys.iter().map(|k| format!("source:{k}"))
            .collect::<Vec<_>>().join(" OR ");
        expression = format!("({expression}) AND ({clause})");
    }
    if args.exclude_rated && !expression.contains("me:rated") {
        expression.push_str(" AND NOT me:rated");
    }
```

Resolve the cookie through the same helper `dh module` uses
(`DARKHOLD_COOKIE` + the `darkhold_csrf` header for state-changing verbs; this
one is a GET, so the cookie alone suffices). If `me:rated` is present with no
cookie, refuse locally with both variable names in the message rather than
sending a request that will 401.

Tests in `crates/cli/tests/q_source_sort.rs`: URL contains five `source:` terms
joined by `OR`, contains `sort=popularity`, and `--exclude-rated` appends the
predicate exactly once even with `--from-file`. Unset probe env vars before
running:

```bash
env -u DARKHOLD_BASE_URL -u DARKHOLD_TOKEN -u DARKHOLD_COOKIE \
  cargo test -p darkhold-cli --test q_source_sort
```

Expected: `test result: ok`. Commit:
`feat(cli): dh q --source --sort --exclude-rated (M45-75)`.

## Step 5 — route proof and the live acceptance probe

**5.1** Add `crates/app/tests/m45_75_search_route.rs` (session harness from
`references/live-probe-recipes.md` in the `darkhold-local-dev` skill): the five
clause query returns the expected work for a signed-in reader, anonymous
`me:rated` returns 401, `sort=popularity` and `sort=newest` return different
first ids, and `?sort=nope` is 422 naming `nope`.

```bash
cargo test -p darkhold-app --test m45_75_search_route
DARKHOLD_TEST_PG_URL='postgres://darkhold:darkhold@127.0.0.1:55433/postgres' \
  cargo test -p darkhold-app --test m45_75_search_route
cargo test -p darkhold-app --test route_inventory
```

Expected: all three green; `route_inventory` stays 5/5 (no new route).

**5.2** Build in the required order on thinkcentre, then probe the real binary:

```bash
node frontend/node_modules/vite/bin/vite.js build   # only if UI files changed; grep 'built in'
cargo build -p darkhold-app --bin darkhold
```

Deploy per `darkhold-thinkcentre-deploy`, then:

```bash
curl -sG 'http://127.0.0.1:8081/api/v1/search' \
  --data-urlencode 'q=(trope:system-progression) AND (source:qq OR source:sv OR source:sb OR source:ffnet OR source:ao3) AND source_updated:<=-4m AND NOT me:rated' \
  --data-urlencode 'sort=popularity' \
  --data-urlencode 'limit=20' | jq '.items | length, .[0].title'
curl -sG 'http://127.0.0.1:8081/api/v1/search' \
  --data-urlencode 'q=sort:nope' | jq '.error'
```

Expected: a non-zero count in the first probe (a `me:rated` probe without a
cookie must instead be 401 naming the credential), and a 422 naming
`sort=nope` for the second. If the first is zero, re-run without
`AND NOT me:rated` to separate "no stale rows" from "viewer filter dropped
everything" before calling the row done.

## Step 6 — ledger and handoff

- Set `docs/requirements.csv` rows `M45-75.1` … `M45-75.5` from `planned` to
  `implemented-verified-e2e` only after step 5.2, with the command and its
  output in the evidence column.
- Write `docs/handoffs/<timestamp>-m45-75-stale-popular-unrated-search.md`:
  what shipped (commits), measured counts and both engine runs, what was not
  run (and why), next-highest row.
- Update `docs/known-gaps.md`: replace the "cannot answer" section with a
  pointer to this plan and the live probe output.
- Land: `git push origin master && git push github master` from the deploy
  tree, then `--ff-only` the workstation and the worktree.

## Decisions already made (do not re-litigate)

- A source is a field on the works surface, not a fifth catalogue verb.
- Multi-source is `OR` of fielded terms, not a comma or a repeated flag.
- The stale clock is `library_items.source_updated_at`; `updated:` is unchanged.
- Viewer state is a bind, not a post-filter and not a public parameter.
- Unknown sort words refuse; an empty sort keeps the legacy score order.
- No migration is needed: both columns and the rating table already exist.
