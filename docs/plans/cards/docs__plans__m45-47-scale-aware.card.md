# docs/plans/m45-47-scale-aware.md — plan card

Source: `docs/plans/m45-47-scale-aware.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

| mount gate (`server.rs::module_mount`) | snapshots `on` at mount; a disabled module is `404 MODULE_UNAVAILABLE` |
1. `crates/domain/src/modules/mod.rs`: `Scale` (+`satisfies`), `Module::min_scale`, `min_scale(id)`,
3. `crates/app/src/state.rs`: `scale()` + a `count` helper over both engines, cached, logged on failure.
4. `crates/app/src/routes/modules.rs`: `observed_scale`, `scale_aware`, per-row `min_scale`/`dormant`.
5. `crates/app/src/config.rs`: `[modules] scale_aware` (default `false`), parsed and defaulted.
6. `crates/app/src/server.rs`: the mount gate refuses a dormant module when `scale_aware`, with the
7. `crates/app/tests/m45_47_scale.rs`: the catalogue publishes the numbers; reporting does not remove
bash scripts/gate.sh all

## Copy-pasteable code / commands

cargo test -p darkhold-domain --lib modules        # 22 passed (6 new)
cargo test -p darkhold-app --test m45_47_scale     # 3 passed
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
bash scripts/gate.sh all

## Verification command and expected output

| mount gate (`server.rs::module_mount`) | snapshots `on` at mount; a disabled module is `404 MODULE_UNAVAILABLE` |
reader asking "what does this instance not run yet, and why" gets one answer, and a test refuses a
so enforcing by default would 404 three working surfaces on every two-account test instance in the
6. `crates/app/src/server.rs`: the mount gate refuses a dormant module when `scale_aware`, with the
cargo test -p darkhold-domain --lib modules        # 22 passed (6 new)
cargo test -p darkhold-app --test m45_47_scale     # 3 passed
bash scripts/gate.sh all
plausible number that was simply false. The route test caught it by asserting `active_readers == 1` on

## RED/GREEN evidence

a failed query is how "nothing here" pages get shipped.
cargo test -p darkhold-domain --lib modules        # 22 passed (6 new)
cargo test -p darkhold-app --test m45_47_scale     # 3 passed
Mutation, red then restored: `dormant()` returning `false` unconditionally → 2 domain tests and 2
route tests FAILED (20 passed / 2 failed; 1 passed / 2 failed).
count failed, the helper returned `-1`, `max(0)` turned it into `0`, and the catalogue published a
(`deleted_at IS NULL AND status = 'active'`), and a failed count is now logged rather than silently
