# docs/plans/m65-04-user-overrides.md — plan card

Source: `docs/plans/m65-04-user-overrides.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

| `migrations/{sqlite,postgres}/0134_module_user_state.sql` | new table |
| `docs/snapshot-column-policy.json` | `module_user_state.account_id` → `rekey_account` |
| `crates/domain/src/error.rs` | `ModuleNotUserToggleable { module, toggle }` → 403 |
| `crates/domain/src/modules/mod.rs` | `Module::may_user_toggle()`, `modules::find_any` |
| `crates/db/src/module_state.rs` | `is_off`, `overrides_for`, `set_off`, `clear`, `count` |
| `crates/app/src/routes/modules.rs` | `PATCH /{id}`, caller-resolved `enabled_for_caller` |
| `crates/app/src/server.rs` | the per-request narrowing inside `module_mount` |
| `crates/cli/src/module.rs` | `dh module off <id>`, `dh module on <id>` |
   only the three pre-existing `work_ratings.rs` warnings.
`crates/db/src/module_state.rs` (five functions, no widening path), `PATCH /api/v1/modules/{id}` is

## Copy-pasteable code / commands

curl -s -o /dev/null -w '%{http_code}\n' -X PATCH .../api/v1/modules/quiz   → 401 (anonymous)
curl -s -X PATCH -b <cookie> -d '{"enabled":false}' .../modules/quiz         → 200, enabled false
curl -s -o /dev/null -w '%{http_code}\n' -b <cookie> .../api/v1/quiz/works   → 404 MODULE_UNAVAILABLE
curl -s -o /dev/null -w '%{http_code}\n' -b <other> .../api/v1/quiz/works    → 401
curl -s -X PATCH -b <cookie> -d '{"enabled":true}' .../modules/quiz          → 200, row gone

## Verification command and expected output

> per-request narrowing in the gate.
4. **Narrowing is invisible to everyone else.** The gate reads the account from the session the
The cheap-common-case optimisation — an atomic "does any account hold an override" flag so the gate
1. `cargo test -p darkhold-db --lib` → green, 0 new warnings.
2. `cargo test -p darkhold-db --test module_user_state` (SQLite) → `5 passed`.
3. `sudo docker start lh-pg-test`;
   `DARKHOLD_TEST_PG_URL=postgres://…@127.0.0.1:55433/postgres cargo test -p darkhold-db --test module_user_state -- --test-threads=1`
   → `5 passed`. Unset the variable before step 2's re-run — a shared DSN points the SQLite test at
4. `cargo test -p darkhold-domain --lib modules` → `16 passed`.
5. `cargo test -p darkhold-app --test m65_user_overrides` → `6 passed`.
6. `cargo test -p darkhold-cli` → `16 passed`.
| mutation | test that must fail |
| gate ignores the user row | `a_reader_who_switched_it_off_gets_404` |
| gate applies narrowing to an operator row | `an_operator_surface_is_not_narrowable` |
the only write, and the gate in `module_mount` reads the row for `per_user` surfaces when the request
   policy is `rekey_account`; the erasure pass is **M65-10**.
3. **`enabled_for_caller` is computed with the gate's own rule** (`per_user` and overridden), not
   `catalogue` as gone for a caller who held a stray row on it — while the gate went on serving it.
- The stray-row case above. It needed a test that writes a row the route refuses to write, because
  a route-level test cannot reach the gate's mistake.

## RED/GREEN evidence

1. `cargo test -p darkhold-db --lib` → green, 0 new warnings.
2. `cargo test -p darkhold-db --test module_user_state` (SQLite) → `5 passed`.
   → `5 passed`. Unset the variable before step 2's re-run — a shared DSN points the SQLite test at
4. `cargo test -p darkhold-domain --lib modules` → `16 passed`.
5. `cargo test -p darkhold-app --test m65_user_overrides` → `6 passed`.
6. `cargo test -p darkhold-cli` → `16 passed`.
the mutation rewritten to keep the enum intact — and both then failed the right test.
