# docs/plans/m45-48-hardware-presets.md — plan card

Source: `docs/plans/m45-48-hardware-presets.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

- `Preset` (spec §0.6, `crates/domain/src/analytics.rs:903`) is an **instance purpose** —
1. `crates/domain/src/capacity.rs` — `Capacity`, `Engine`, `Tier`, `Tier::of`, `TierReport`,
2. `crates/app/src/state.rs` — `capacity()` cached, `/proc/meminfo` read with a fallback of `0` (which
3. `crates/app/src/config.rs` — `[capacity] tier` (default `auto`) + `CAPACITY_TIER` flag.
4. `crates/app/src/routes/meta.rs` — `GET /api/v1/instance/capacity`, pseudonymous, publishing measured
5. Tests: `crates/app/tests/m45_48_capacity.rs` — the route publishes a tier consistent with the facts
bash scripts/gate.sh all

## Copy-pasteable code / commands

Tier::of(capacity) -> Tier            // pure, thresholds in one place
tier.report() -> TierReport           // {embeddings, incremental_analytics, batch_size, jobs}

GET /api/v1/instance/capacity
{ "tier": "small", "measured": { "cores": 2, "memory_mb": 15872, "engine": "sqlite", "has_redis": false },
  "report": { "embeddings": false, "incremental_analytics": false, "batch_size": 200, "jobs": 2 },
  "override": null }

cargo test -p darkhold-domain --lib capacity
cargo test -p darkhold-app --test m45_48_capacity
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
bash scripts/gate.sh all

## Verification command and expected output

  `Capacity`, so a test can sit on either side of every threshold without owning a small computer.
   it names (re-derived in the test from the same pure function), and an override is reported while the
cargo test -p darkhold-domain --lib capacity
cargo test -p darkhold-app --test m45_48_capacity
bash scripts/gate.sh all
Mutation proofs: `Tier::of` ignoring `engine` → the SQLite-with-8-cores test fails; `report()` giving
embeddings on `Small` → the embeddings test fails; the override being published as the measurement →
the override test fails.
  test can pin.

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
