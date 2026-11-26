# docs/plans/m45-25-supply-keystones.md — plan card

Source: `docs/plans/m45-25-supply-keystones.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Plan position:** `docs/plans/m45-gaps-adoption.md` Phase 3 (`M45-25, -26, -28, -29`), gate
| `docs/spec.md` | **new §58** — Supply: keystone authors, droughts, and the routing record |
| `migrations/{sqlite,postgres}/0133_supply_keystones.sql` | `supply_keystones`, `supply_routing` |
| `crates/db/src/supply.rs` | new: `recompute`, `keystones`, `drought`, `route`, `complete`, `routes_for` |
| `crates/db/src/lib.rs` | `pub mod supply;` |
| `crates/app/src/routes/supply.rs` | new: 5 routes (2 operator, 3 reader, 1 operator recompute) |
| `crates/app/src/routes/mod.rs`, `crates/app/src/server.rs` | register the router |
| `crates/cli/src/supply.rs`, `crates/cli/src/main.rs` | `dh supply keystones\|droughts\|recompute\|route\|complete` |
| `crates/db/tests/supply_keystones.rs` | 7 tests, SQLite + optional `DARKHOLD_TEST_PG_URL` |
| `crates/app/tests/m45_25_supply.rs` | 6 route tests |
1. **Spec §58.** `grep -n "^## 58" docs/spec.md` → one hit.
Migration **0133** (`supply_keystones`, `supply_routing`), `crates/db/src/supply.rs` (five

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

**Plan position:** `docs/plans/m45-gaps-adoption.md` Phase 3 (`M45-25, -26, -28, -29`), gate
2. **Migration.** `cargo test -p darkhold-db --lib` applies it; then
3. **Store.** `cargo test -p darkhold-db --test supply_keystones` → `7 passed` on SQLite, then
4. **Mutations** (each must be caught by the named test):
5. **Routes.** `cargo test -p darkhold-app --test m45_25_supply` → `6 passed`.
| 1 | taste bonus "clamped to ±0.25 and floored at zero" | the table has `CHECK (weight >= 0.0)`, so a stored negative weight is impossible and the *floor* is the schema's, not the code's. §58.1 was rewritten to say which half is which; the test asserts both (a 0.4 weight lands at 0.25, an author with no weight gets 0). |
| 4 | — | **`scripts/check-snapshot-pii.py` refused the new migration.** §11.16 requires a policy decision for every column that carries an identity, and `supply_routing.account_id` had none; `cargo test -p darkhold-app --lib` failed with "the gate check reported Fatal". The decision is `drop_table`, with the reason that the routing record exists to hold one reader's opt-in and its research value is derivable without it. |
| 5 | complete-once via the idempotency key | **there are two guards**, and either alone suffices: the `completed_at` early return and the `supply-route:<id>` key. Removing one leaves the test green, which is the point of having both — the mutation that fails is the one that removes both. Reported rather than dressed up as a single-guard test. |

## RED/GREEN evidence

3. **Store.** `cargo test -p darkhold-db --test supply_keystones` → `7 passed` on SQLite, then
   the same with `DARKHOLD_TEST_PG_URL` set → `7 passed`.
5. **Routes.** `cargo test -p darkhold-app --test m45_25_supply` → `6 passed`.
| 4 | — | **`scripts/check-snapshot-pii.py` refused the new migration.** §11.16 requires a policy decision for every column that carries an identity, and `supply_routing.account_id` had none; `cargo test -p darkhold-app --lib` failed with "the gate check reported Fatal". The decision is `drop_table`, with the reason that the routing record exists to hold one reader's opt-in and its research value is derivable without it. |
| 5 | complete-once via the idempotency key | **there are two guards**, and either alone suffices: the `completed_at` early return and the `supply-route:<id>` key. Removing one leaves the test green, which is the point of having both — the mutation that fails is the one that removes both. Reported rather than dressed up as a single-guard test. |
