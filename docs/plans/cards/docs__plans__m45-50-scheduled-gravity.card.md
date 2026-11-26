# docs/plans/m45-50-scheduled-gravity.md — plan card

Source: `docs/plans/m45-50-scheduled-gravity.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`crates/db/src/instance_theme.rs` — a **taste** vector, not colours: `InstanceTheme { instance_id,
1. Migration `0150_theme_schedules.sql` (sqlite + postgres): the table and one index on
2. `crates/domain/src/theme_window.rs`: `Window::{starts_at, ends_at}`, `parse_window(&str, &str) ->
3. `crates/db/src/theme_schedule.rs`: `create`, `list_for_instance`, `active_at`, `revoke`. Both
4. `crates/app/src/routes/federation.rs`: `GET/POST/DELETE /api/v1/admin/theme/schedule`, admin pseud
bash scripts/gate.sh all
AUTH_REQUIRED`. The session middleware in `server.rs` is layered on the `/api/v1` router, and the

## Copy-pasteable code / commands

theme_schedules(id, instance_id, label, theme_vector, starts_at, ends_at, created_at, revoked_at)

cargo test -p darkhold-domain --lib theme_window
cargo test -p darkhold-db --test theme_schedule
cargo test -p darkhold-app --test m45_50_theme_schedule
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
bash scripts/gate.sh all

## Verification command and expected output

cargo test -p darkhold-domain --lib theme_window
cargo test -p darkhold-db --test theme_schedule
cargo test -p darkhold-app --test m45_50_theme_schedule
bash scripts/gate.sh all
Mutation proofs: make `covers` inclusive at `ends_at` → the adjacent-window test fails; make
`active_at` prefer the *earliest* start → the overlap test fails; drop the `revoked_at IS NULL`
predicate → the revoke test fails.
The first version of the route test asserted `403` for a registered reader and got `401
request alone when there is not. Red before, green after, on the same test.
## And the gate found the second one
The first full gate failed on `dialects`: I had written each statement once and used it from both

## RED/GREEN evidence

request alone when there is not. Red before, green after, on the same test.
The first full gate failed on `dialects`: I had written each statement once and used it from both
engines, and PostgreSQL spells placeholders `$1..$n`. The SQLite-only run passed, so nothing else in
