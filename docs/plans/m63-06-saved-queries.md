# M63-06 — saved queries, and a diff that means "new since last time"

> **Superseded in part by `M63-13`**: the "cookie-only" finding below was true when it was
> measured, and `dh cron install` needed a credential that survives a month. The three verbs a
> scheduled run uses (list, get, record-a-run) now also accept a token carrying `library.read`;
> create/rename/delete are still session-only. See `docs/plans/m63-13-cron-install.md`.


Status: implementing. Row: `M63-06` (power-user). Idea source:
`docs/ideas/2026-11-01-idea-sweep-power-users.md` (`--save`, `--digest-diff`).

## The incumbent, measured before designing

Storage already exists and is complete: `saved_views` (migration `0009_library.sql`, both dialects) with
`id, account_id, name, query_json, query_version, sort, scope, pinned, is_public, created_at,
updated_at, version`; full CRUD in `darkhold_db::library` (`create_saved_view`, `saved_views_for`,
`find_saved_view`, `update_saved_view`, `delete_saved_view`); and five routes
(`GET/POST /saved-views`, `GET/PATCH/DELETE /saved-views/{id}`) with `view_json`. Two facts decide the
whole shape of this row:

1. **The stored document is library-shaped.** `StoredQuery { version, query: LibraryQuery }` at version
   `1`, decoded by `decode_saved_view` under a strict equality on the version, validated by
   `darkhold_domain::library::validate_query`. A `dh rec` query is a *catalogue* query
   (`CatalogueQuery`: fandom, the three windows, q, sort, source, min_likes, author, seed), not a
   library one — so today `--save` has nothing it is allowed to store.
2. **There is no run state anywhere.** `grep -rn "digest" crates/` is empty; nothing records what a
   saved query answered last time, so `--digest-diff` has nothing to diff against.

Two sibling rows take the rest of the idea: `M63-12` (`dh watch`), `M63-13` (`dh cron install`) and
`M63-14` (per-query feeds) are separate rows, so "schedulable" here means **the query lives on the
server, addressable by name** — not that a scheduler exists.

## Decisions, with the rejected alternative named

1. **A catalogue view stores the canonical query string, not a typed `CatalogueQuery`.**
   The document becomes version 2 with a `kind`, and a catalogue view holds `params` — the exact string
   `dh rec` already builds (`fandom=naruto&updated=7d&sort=kudos&limit=20`). Rejected: a typed document.
   The route converts params to a query through `build_query`, which needs `AppState` (fandom
   resolution) and `now()` (window anchors); a second, document-shaped conversion would be a second
   implementation of the same rules, and the two would drift exactly where windows are involved. The
   string is what the client sends, so replaying it is byte-identical to the original request. When a
   server-side runner exists (M63-14), `CatalogueParams` deserializes from that same string.
2. **Run state is its own table, `saved_query_runs`, not a column on `saved_views`.**
   `saved_views.version` is optimistic concurrency for *edits* (`update_saved_view` refuses a stale
   version). A run that wrote the seen-set onto that row would bump the version, and every run would
   then invalidate an open editor. A separate row also cascades on delete and keeps the view's
   definition free of run history. Rejected: a `last_run_at` timestamp column alone — see (3).
3. **"New" means "was not in the last answer", not "created since".** A catalogue row can *start*
   matching a saved query without being new: `source_updated_at` moves when the author posts and a
   recheck (M63-32) rewrites it, the tag backfill adds fandoms, and a window anchored on `-7d` slides.
   Only the previous answer's own ids can say what is new, so the server diffs id sets.
   Rejected: `WHERE created_at > last_run_at` — it misses every row that entered the window from the
   other side, which is the common case for `--updated 7d`.
4. **The first run reports everything as new, and says so.** `previous: 0` in the response and a line in
   the CLI ("first run — nothing to diff against"). Rejected: suppressing the first run's rows, which
   would make `dh rec --saved weekly --digest-diff` look broken the first time it is used.
5. **Every saved-query verb needs a *session*, and a session means the cookie.** Written first the
   other way, then measured: `/saved-views` is a `RequireSession` surface, `RequireSession` reads the
   cookie middleware's `SessionUser` and never a bearer token, so a valid `DARKHOLD_TOKEN` is a 401
   there. `--save`, `--saved`, `--queries` and `--digest-diff` therefore take `DARKHOLD_COOKIE`, and
   the refusal names that variable plus why the token cannot work. The plain `dh rec` query still needs
   nothing, because it reads the public catalogue.

## Steps

### Step 1 — migration `0149_saved_query_runs.sql`, both dialects

`migrations/sqlite/0149_saved_query_runs.sql`:

```sql
-- M63-06: what a saved query answered last time, so a rerun can say what is new.
--
-- One row per view: the id set is the *answer*, not a history, and keeping it out
-- of `saved_views` keeps a run from bumping the row's optimistic version.
CREATE TABLE saved_query_runs (
    saved_view_id TEXT    PRIMARY KEY REFERENCES saved_views (id) ON DELETE CASCADE,
    ran_at        TEXT    NOT NULL,
    seen_ids_json TEXT    NOT NULL,
    seen_count    INTEGER NOT NULL
);
```

`migrations/postgres/0149_saved_query_runs.sql`: identical, with
`saved_view_id UUID PRIMARY KEY REFERENCES saved_views (id) ON DELETE CASCADE`.

### Step 2 — domain: the diff is a pure function, with tests

`crates/domain/src/library.rs`, beside `validate_query`:

```rust
/// Which of `current` were not in `previous`, in the order they arrived.
///
/// The diff is over ids, not timestamps: a catalogue row can enter a saved
/// query's answer because its `source_updated_at` moved, because a tag backfill
/// gave it a fandom, or because a `-7d` window slid — none of which is a
/// "created since" fact. Only the previous answer can say what is new.
///
/// Duplicates in `current` are the caller's mistake and are preserved, not
/// collapsed: this function answers a question about a page, and a page that
/// repeats an id is a bug the caller should see.
#[must_use]
pub fn new_ids_since(previous: &[String], current: &[String]) -> Vec<String> {
    current
        .iter()
        .filter(|id| !previous.contains(id))
        .cloned()
        .collect()
}
```

Tests (RED first: `error[E0425]: cannot find function 'new_ids_since'`):

```rust
    #[test]
    fn a_first_run_is_all_new_and_a_repeat_is_none() {
        let page = vec!["a".into(), "b".into()];
        assert_eq!(library::new_ids_since(&[], &page), page);
        assert_eq!(library::new_ids_since(&page, &page), Vec::<String>::new());
    }

    #[test]
    fn an_id_that_left_the_page_and_came_back_is_new_again() {
        // A window slides: `--updated 7d` run today drops last week's answer and
        // a row can walk back in. Comparing sets, not history, is the point.
        let previous = vec!["a".into(), "b".into()];
        let current = vec!["b".into(), "a".into(), "c".into()];
        assert_eq!(library::new_ids_since(&previous, &current), vec!["c".to_owned()]);
    }
```

### Step 3 — store: document version 2, and the run

`crates/db/src/library.rs`:

```rust
pub const QUERY_DOCUMENT_VERSION: i64 = 2;

/// Which query language a saved view's document holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedViewKind {
    /// A library query (`GET /library/items`'s parameters).
    #[default]
    Library,
    /// A catalogue query, stored as the canonical query string.
    Catalogue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredQuery {
    version: i64,
    #[serde(default)]
    kind: SavedViewKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    query: Option<LibraryQuery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    params: Option<String>,
}
```

`decode_saved_view` (the version-1 path is a real migration, not a rewrite):

```rust
    let stored: Option<StoredQuery> = serde_json::from_str(&row.query_json).ok();
    let mut kind = SavedViewKind::Library;
    let mut query = None;
    let mut params = None;
    let mut needs_repair = true;
    if let Some(stored) = stored {
        // Version 1 documents carry a library query and no `kind`; they are read,
        // not rewritten, so an old view keeps working and its bytes stay as its
        // author's build wrote them.
        let readable = stored.version == 1 || stored.version == QUERY_DOCUMENT_VERSION;
        if readable {
            kind = if stored.version == 1 { SavedViewKind::Library } else { stored.kind };
            match kind {
                SavedViewKind::Library => {
                    query = stored.query;
                    needs_repair = query.is_none();
                }
                SavedViewKind::Catalogue => {
                    params = stored.params.filter(|p| !p.trim().is_empty());
                    needs_repair = params.is_none();
                }
            }
        }
    }
```

`SavedView` gains `pub kind: SavedViewKind` and `pub params: Option<String>`.

New writer, beside `create_saved_view` (which keeps its signature and writes `kind: Library`):

```rust
/// Store a catalogue query as a named view.
///
/// `params` is the canonical query string the caller already sends to
/// `/catalogue` — stored verbatim, because a second conversion of the same
/// parameters is a second set of rules for windows and fandoms.
///
/// A catalogue view with `exclude_read` in its params is refused unless it is
/// scoped to the library: that parameter reads the caller's own history, and a
/// public view claiming to be shareable while depending on one account's
/// reading would answer differently for every reader.
pub async fn create_catalogue_view(
    db: &Database,
    account_id: &str,
    name: &str,
    params: &str,
    scope: ViewScope,
    pinned: bool,
) -> Result<SavedView>
```

with the run recorder:

```rust
/// One recorded run: what the query answered, and what was new in it.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryRun {
    /// Ids in this run that were not in the previous one.
    pub new_ids: Vec<String>,
    /// How many ids the previous run recorded; 0 on the first run.
    pub previous: i64,
    /// When this run was recorded, RFC 3339.
    pub ran_at: String,
}

/// Record a run and answer what is new since the last one.
pub async fn record_run(
    db: &Database,
    account_id: &str,
    view_id: &str,
    ids: &[String],
) -> Result<QueryRun>
```

`record_run` reads the previous set (scoped to the account — a view id from another account is
`NotFound`, not an empty diff), computes `darkhold_domain::library::new_ids_since`, upserts this run's
ids (capped at `SEEN_CAP = 5_000`, in arrival order), and returns the diff.

### Step 4 — routes

`crates/app/src/routes/library.rs`:

- `ViewBody` gains `kind: Option<String>` and `params: Option<String>`; `create_view` branches on
  `kind` (`catalogue` → `create_catalogue_view`, else the existing library path). Presenting both
  `query` and `params` is a `422` naming the choice.
- `view_json` gains `"kind"` and `"params"`.
- New handler and route:

```rust
        .route("/saved-views/{id}/runs", post(record_view_run))
```

```rust
#[derive(Debug, Deserialize)]
struct RunBody {
    ids: Vec<String>,
}

/// Record that the caller ran a saved query, and answer what is new.
async fn record_view_run(
    State(state): State<AppState>,
    RequireSession(user): RequireSession,
    Path(id): Path<String>,
    Json(body): Json<RunBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let run = library::record_run(state.db(), &user.account_id.to_string(), &id, &body.ids).await?;
    Ok(Json(serde_json::json!({
        "new_ids": run.new_ids,
        "previous": run.previous,
        "ran_at": run.ran_at,
    })))
}
```

- `crates/app/src/routes/inventory.rs`: one `ROUTE_TABLE` entry for `POST /saved-views/{id}/runs`
  (`Session`, the same class as its siblings). `route_inventory` fails otherwise, by design.

### Step 5 — CLI (`crates/cli/src/rec.rs`)

Four flags, all on the surface that already exists:

```
--save <NAME>       store this query as a saved view (needs DARKHOLD_COOKIE)
--saved <NAME>      run a stored query (the params are replayed verbatim)
--digest-diff       with --saved: print only rows new since that query last ran
--queries           list saved queries: name, kind, updated
```

**Deviation, measured live:** these use the session cookie (`DARKHOLD_COOKIE`), not
`DARKHOLD_TOKEN`. Decision 5 above said a token; the probe disproved it. `/saved-views` is a
`RequireSession` surface, `RequireSession` reads `SessionUser` from the cookie middleware only, and a
*valid* bearer token gets `401` there — verified against a running server with a token row whose hash
matched and whose account was active. The CLI already had the answer for exactly this case (`dh module`
sends the cookie and refuses with `auth_required`); this follows it, including the CSRF header parsed
from the `darkhold_csrf` cookie. Consequence, recorded rather than papered over: a saved query cannot
yet be run unattended, which is what `M63-13` (`dh cron install`) needs — widening a session surface to
tokens is an auth decision, and it belongs to that row, not to this one.

`--save` posts to `/api/v1/saved-views` with `{"name", "kind":"catalogue", "params": <the canonical
string this call would have sent>, "scope":"public"}` and prints the id plus the shareable URL
`<base>/api/v1/catalogue?<params>`. `--saved` resolves the name through `/api/v1/saved-views`, refuses
a `needs_repair` view by name, and replays its `params` against `/api/v1/catalogue`. `--digest-diff`
then POSTs the page's ids to `/saved-views/{id}/runs` and prints only the new rows, plus the count line
`first run — nothing to diff against` or `N new of M on the page (was N_prev)`.

Refusals: `--digest-diff` without `--saved`; `--save` with `--saved`; `--saved` on a library view
(refused: "a library view is not a catalogue query"); `--queries` with `--save`/`--saved`.

### Step 6 — verification

```bash
cargo test -p darkhold-domain --lib library       # the diff, pure
cargo test -p darkhold-db --test saved_queries    # both dialects: v1 decode, catalogue view, runs
cargo test -p darkhold-app --test m63_06_saved_queries   # the four routes through the router
cargo test -p darkhold-cli --lib rec              # flags, refusals, rendering
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
bash scripts/gate.sh ledger && bash scripts/gate.sh repo
```

Mutation proof: `new_ids_since` returning `current` unchanged must fail the repeat-run test; summing
`previous` from the current page instead of the stored row must fail the db test.

Live probe (token needed): save `fandom=<x>&limit=2`, run it, record the run, change nothing, record
again (0 new), then seed a row that enters the query and record a third time (1 new).

## Step 7 — the record

Ledger row through `scripts/ledger.py`; `docs/backlog/` + `docs/ideas-catalogue.json` regenerated;
README's `dh rec` paragraph gains the four flags; STANDING-PROMPT state block re-derived.
