# M45-47 — scale-aware activation: a mechanism declares what it needs

Status: implementing. Row `M45-47` (admin). "Gaps review F3. Small instances get simpler behavior,
features unlock as they grow."

## What exists, measured

| thing | state |
|---|---|
| `Module` registry | 104 modules, fields `id/name/summary/default_on/stability/toggle/requires/milestone/spec/nav/relevance` — **no scale field** |
| `ModuleState` | resolved once from config: `enabled`, `allow_user_overrides` |
| `GET /api/v1/modules` | publishes `enabled`, `enabled_for_caller`, `stability`, `toggle`, `nav` |
| mount gate (`server.rs::module_mount`) | snapshots `on` at mount; a disabled module is `404 MODULE_UNAVAILABLE` |
| instance counts | no `COUNT(*)` helper in the app; `Database` exposes `sqlite_pool()` / `postgres_pool()` and a `backend()` |
| `accounts` columns | `status` ('active'), `deleted_at` — **no `suspended_at`** (this bit the first draft) |

## The mechanism

`Scale { active_readers, active_works }` — two facts, not a score. `Module::min_scale()` answers for
every row; the thresholds live in **one table** (`min_scale(id)`), not in 104 registry entries, so a
reader asking "what does this instance not run yet, and why" gets one answer, and a test refuses a
threshold on an id the registry does not know.

Declared today: `curator` 25 readers / 500 works (a curated shelf of three works is a bookmark
folder), `roadmap` 25 / 0 (a proposal board where one voice is a diary), `collaborators` 50 / 100
(matching needs both sides in numbers). Everything else declares zero, which is an answer and the
right one for a surface that works for one reader with one work.

`ModuleState::dormant(id, scale)` — enabled **and** below its threshold. A core id is never dormant; a
switched-off id is never dormant either, because "off" is a decision and "dormant" is a measurement
and reporting one as the other is a lie about which.

## Where a caller meets it — reported always, refused on request

- **Reported, always.** `GET /api/v1/modules` gains top-level `observed_scale` and `scale_aware`, and
  each row gains `min_scale` and `dormant`. "Why is the curation page empty" now has numbers in its
  answer.
- **Refused, when the operator says so.** `[modules] scale_aware = true` makes a dormant mechanism
  answer `404 MODULE_UNAVAILABLE`, the same refusal a switched-off module gives — to a caller "not on
  this instance" and "not on an instance this small" are one question.

Default **off** on purpose: refusing a surface that works is a product decision about a specific
instance, not something software should decide for a fresh one. And enforcement had to be opt-in for a
second, blunter reason: `curator`, `roadmap` and `collaborators` are `default_on: true` and `Stable`,
so enforcing by default would 404 three working surfaces on every two-account test instance in the
tree. That is a fact from the registry, not a preference.

## Cost, stated

`AppState::scale()` counts once per process into a `tokio::sync::OnceCell` — `COUNT(*)` on `accounts`
(`deleted_at IS NULL AND status = 'active'`) and on `works`, on whichever engine the database is. A
mechanism crosses a threshold on the scale of days, so a count per request would be a tax on the page
least able to afford it. **A restart is what re-measures an instance**, the same way a restart is what
re-reads the config. A count that fails is logged (`tracing::warn`) and treated as zero: a mechanism
that cannot measure its instance stays dormant, because turning on a crowd feature on the strength of
a failed query is how "nothing here" pages get shipped.

## Steps taken

1. `crates/domain/src/modules/mod.rs`: `Scale` (+`satisfies`), `Module::min_scale`, `min_scale(id)`,
   `scaled_ids()`, `ModuleState::dormant`, `ModuleState::dormant_at`.
2. Tests (6 new in the domain): below/at threshold; both facts have to clear; the default is zero;
   every declared threshold names a registered id and asks for something; core and switched-off are
   never dormant; the dormant list is the enabled rows that fall short.
3. `crates/app/src/state.rs`: `scale()` + a `count` helper over both engines, cached, logged on failure.
4. `crates/app/src/routes/modules.rs`: `observed_scale`, `scale_aware`, per-row `min_scale`/`dormant`.
5. `crates/app/src/config.rs`: `[modules] scale_aware` (default `false`), parsed and defaulted.
6. `crates/app/src/server.rs`: the mount gate refuses a dormant module when `scale_aware`, with the
   same `404 MODULE_UNAVAILABLE`.
7. `crates/app/tests/m45_47_scale.rs`: the catalogue publishes the numbers; reporting does not remove
   the surface; `scale_aware` refuses with the same 404 and the catalogue agrees with the door.

## Verification

```bash
cargo test -p darkhold-domain --lib modules        # 22 passed (6 new)
cargo test -p darkhold-app --test m45_47_scale     # 3 passed
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
bash scripts/gate.sh all
```

Mutation, red then restored: `dormant()` returning `false` unconditionally → 2 domain tests and 2
route tests FAILED (20 passed / 2 failed; 1 passed / 2 failed).

## What the first draft got wrong

The `active_readers` query was `WHERE suspended_at IS NULL` — a column `accounts` does not have. The
count failed, the helper returned `-1`, `max(0)` turned it into `0`, and the catalogue published a
plausible number that was simply false. The route test caught it by asserting `active_readers == 1` on
a one-account instance. Two consequences shipped: the query uses the schema's own spelling
(`deleted_at IS NULL AND status = 'active'`), and a failed count is now logged rather than silently
zeroed — the silence was the actual defect, not the column name.

## Not done

- `dh module list` does not draw a dormant marker in its table. The payload carries the fields, so
  `dh module list --json | jq '.modules[] | select(.dormant)'` answers today; a column is a rendering
  choice for the row that asks for it.
- The thresholds are the three above. They are judgement calls with the reasoning attached, not
  measurements of this instance's history — a row that wants calibrated numbers should measure a real
  instance's first hundred works and set them.
