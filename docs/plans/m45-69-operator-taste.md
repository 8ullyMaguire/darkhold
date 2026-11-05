# M45-69 — operator taste: the instance theme seeded from the operator's shelf

Row: `M45-69` · area `operator` · status `planned`
Concord card: `[M] Operator taste`

## Problem

`instance_theme` is computed from private bookmarks (`compute_theme_from_bookmarks`) and M45-50 seasons
override it for a window. Neither lets an operator **state what the instance is for** by pointing at a
shelf. Today an operator who wants a dark-progression instance can only hope their users' bookmarks
drift that way, or set a season that expires.

## Goals

1. An operator names a shelf — a list of seeds, resolved through the same resolver as `?seed=` and
   `POST /catalogue/seeds` (catalogue rows *and* stored external evidence).
2. The instance's theme is derived from those seeds' **tags**, stored as an **operator-sourced**
   vector with provenance: seed count, how many resolved, the top tags, the unused seeds and why.
3. It is **visible, not blended**: published as its own object, and the effective theme says which
   source is in force.
4. A season still overrides for its window; the operator's intent is the standing base.

## Non-goals

- Not a per-user preference: this is one vector per instance, operator-only.
- Not a blend with bookmark data. Blending would make "we are a dark-progression instance" a claim
  nobody can check — the point of the row is that the operator's intent is inspectable on its own.
- No auto-refresh: the shelf is re-derived only when the operator re-posts it. A silent recompute
  would change what the instance claims without anyone saying so.

## Order of precedence (documented in one place)

    season (M45-50, temporary)  >  operator shelf (M45-69, standing)  >  bookmarks (observed)

A season is an event and expires; the shelf is what the instance is for; bookmarks are what its
readers happen to like. The effective-theme response names which one answered.

## Design

### Storage — `migrations/{sqlite,postgres}/0152_operator_theme.sql`

    CREATE TABLE operator_themes (
        instance_id     TEXT NOT NULL PRIMARY KEY,
        theme_vector    TEXT NOT NULL,   -- {"dark-protagonist": 0.9, ...}
        top_tags        TEXT NOT NULL,   -- [{"tag": "...", "weight": 0.9}, ...] max 10
        seeds           TEXT NOT NULL,   -- [{"spec","status","id","title"}] exactly as posted
        seed_count      INTEGER NOT NULL,
        resolved_count  INTEGER NOT NULL,
        sourced_at      TEXT NOT NULL,
        updated_at      TEXT NOT NULL
    );

`seeds` keeps every line with its outcome, so the row can answer "why is `kink` in my theme" and
"why is my fifth seed not in it". Unused seeds are not dropped from the record.

### Domain — `crates/domain/src/operator_theme.rs`

- `pub fn from_tags(per_seed: &[(String, Vec<String>)]) -> (Map<String,f64>, Vec<(String,f64)>)`
  Pure: counts each tag once per seed (a work tagged `dark` twice is one work), weight = count /
  seeds-that-contributed, rounded to 4dp; `top_tags` = the ten heaviest, ties broken by name.
- `pub fn precedence(has_season: bool, has_operator: bool, has_computed: bool) -> Source`
- Unit tests: one work with two tags; a tag on two seeds outweighs one on one; a seed with no tags
  contributes nothing but does not zero the others; ties are name-ordered (deterministic output).

### Store — `crates/db/src/operator_theme.rs`

`upsert(db, instance_id, vector, top_tags, seeds, counts, now)`, `get(db, instance_id)`,
`delete(db, instance_id)`. Per-engine SQL (two statements, no shared `?`).

### Routes — `crates/app/src/routes/federation.rs`

- `POST /api/v1/federation/theme/operator` (operator): body `{"seeds": ["…", …], "replace": true}`
  - classify each line with `darkhold_domain::external_seed::classify`
  - resolve with `metrics::resolve_seed`, then external evidence by id (same order as `?seed=`)
  - tags per resolved row: `SELECT tn.canonical FROM work_tags wt JOIN canonical_entities tn …`
  - refuse with no usable seed: `"theme: none of the N seeds carries a tag — a shelf with no tags
    states nothing this instance can act on"` (naming is the point)
  - store, return `{"theme":…, "top_tags":…, "seeds":[…], "counts":…}`
- `GET /api/v1/federation/theme/operator` (operator): the stored object or `null`
- `DELETE` same path: forget it, returning to bookmarks

Effective theme: `GET /api/v1/instance/theme` gains `"source": "season"|"operator"|"bookmarks"|"none"`.

### Inventory

Three entries, `Audience::Operator`.

## Verification

    cargo test -p darkhold-domain --lib operator_theme          # 4 tests, pure
    cargo test -p darkhold-db --test operator_theme             # upsert/get/delete round trip
    cargo test -p darkhold-app --test m45_69_operator_theme     # POST → GET → DELETE, precedence
    cargo test -p darkhold-app --test route_inventory           # 3 new entries tabled
    bash scripts/gate.sh all && bash scripts/gate.sh ledger && bash scripts/gate.sh repo

Each with a mutation that must go red:
1. `from_tags` counting duplicates → the two-tags-one-seed test fails.
2. precedence returning `bookmarks` when an operator shelf exists → precedence test fails.
3. route storing without `top_tags` → the GET-shape test fails.

Live probe (:8099, session + operator): post three seeds, read the object back, confirm
`source: "operator"`, and confirm a season still wins while its window is open.

## Order of work

1. migration → 2. domain + tests → 3. db store + test → 4. routes + inventory → 5. app test →
6. live probe → 7. ledger (notes quote the probe) → 8. commit, gate, record the sha
