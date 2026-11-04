# M45-48 — hardware-aware presets: a tier derived from measured facts, and what it withholds

Status: implementing. Row `M45-48` (admin). "Gaps review F4. SQLite weekly batches/no embeddings →
Postgres+Redis incremental."

## What exists, measured

- `Preset` (spec §0.6, `crates/domain/src/analytics.rs:903`) is an **instance purpose** —
  `open_library`, `curated_boutique`, `admin_garden`, `genre_haven`, `experimental_lab`, `custom` — and
  it withholds *analytics capabilities* by policy. It is not about hardware, and reusing it would
  conflate "this instance deliberately does not do analytics" with "this machine cannot afford them".
- No detection layer: nothing in the tree reads core count or memory, and `[instance] preset` is a
  string defaulting to `curated_boutique`.
- The two facts the row names are already surfaces in this codebase: embeddings (`analytics` scopes) and
  incremental vs batch analytics (`analytics_k_anonymity`, weekly jobs).

## The shape

Two names, kept apart on purpose:

- **`Capacity`** — what the machine *is*: `cores`, `memory_mb`, `engine` (sqlite/postgres),
  `has_redis`. Facts, injected, never measured inside the rule.
- **`Tier`** — what that buys: `Minimal`, `Small`, `Standard`, `Large`. Derived by a pure function over
  `Capacity`, so a test can sit on either side of every threshold without owning a small computer.

```rust
Tier::of(capacity) -> Tier            // pure, thresholds in one place
tier.report() -> TierReport           // {embeddings, incremental_analytics, batch_size, jobs}
```

Thresholds, in one table with the reasoning attached:

| tier | needs | what it buys |
|---|---|---|
| `Minimal` | anything (`< 2` cores or `< 1024` MB) | no embeddings, weekly batch analytics, batch 50, 1 job |
| `Small` | 2 cores, 1024 MB | no embeddings, weekly batch, batch 200, 2 jobs |
| `Standard` | 4 cores, 4096 MB, postgres | embeddings, incremental analytics, batch 500, 4 jobs |
| `Large` | 8 cores, 8192 MB, postgres, redis | embeddings, incremental, batch 2000, 8 jobs |

The rule is **the lowest tier whose requirements are all met** — a machine that clears eight cores but
runs SQLite is `Small`, because incremental analytics needs the engine's write concurrency, not the
cores. That is why `engine` is a fact in `Capacity` rather than an assumption about "big machines".

## Graceful degradation, stated as a ceiling

A tier never *raises* anything an operator switched off: `TierReport` says what the machine can afford,
and the config says what the operator wants. The effective answer is the intersection, and the route
publishes both so "why are embeddings off" has one of two answers instead of a shrug:

```
GET /api/v1/instance/capacity
{ "tier": "small", "measured": { "cores": 2, "memory_mb": 15872, "engine": "sqlite", "has_redis": false },
  "report": { "embeddings": false, "incremental_analytics": false, "batch_size": 200, "jobs": 2 },
  "override": null }
```

`[capacity] tier = "auto" | "minimal" | "small" | "standard" | "large"`. `auto` derives; a named tier is
the operator's word, and the response names which of the two answered (`override`: the tier, or `null`).
An operator override may sit **above** the measurement — they may know the box will be upgraded — but
the response still publishes the measurement, so the disagreement is visible rather than implied.

## Cost of detection, stated

Detection runs **once**, in `AppState`, into the same `OnceCell` pattern `scale()` uses: cores from
`std::thread::available_parallelism`, memory from `/proc/meminfo` (`MemTotal`), engine from the
database, redis from the configured URL. A machine's RAM does not change under a process, and a
detection that fails to read a file reports the conservative tier rather than guessing — `Minimal` is
the safe wrong answer for a busy server, `Large` is the expensive one.

## Steps

1. `crates/domain/src/capacity.rs` — `Capacity`, `Engine`, `Tier`, `Tier::of`, `TierReport`,
   `Tier::report`, `parse_tier`. Tests: every threshold from both sides, engine beats cores, unknown
   string refused, `TierReport` monotone across tiers.
2. `crates/app/src/state.rs` — `capacity()` cached, `/proc/meminfo` read with a fallback of `0` (which
   lands on `Minimal`), cores/engine/redis gathered.
3. `crates/app/src/config.rs` — `[capacity] tier` (default `auto`) + `CAPACITY_TIER` flag.
4. `crates/app/src/routes/meta.rs` — `GET /api/v1/instance/capacity`, pseudonymous, publishing measured
   facts, derived tier, effective report, and the override when one was given.
5. Tests: `crates/app/tests/m45_48_capacity.rs` — the route publishes a tier consistent with the facts
   it names (re-derived in the test from the same pure function), and an override is reported while the
   measurement stays visible.

## Verification

```bash
cargo test -p darkhold-domain --lib capacity
cargo test -p darkhold-app --test m45_48_capacity
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
bash scripts/gate.sh all
```

Mutation proofs: `Tier::of` ignoring `engine` → the SQLite-with-8-cores test fails; `report()` giving
embeddings on `Small` → the embeddings test fails; the override being published as the measurement →
the override test fails.

## Not done, deliberately

- No automatic *downgrade at runtime* (e.g. turning embeddings off when a query is slow). That is a
  measurement loop with hysteresis, and a tier that changes under load is a tier whose behaviour no
  test can pin.
- Redis presence is read from configuration, not probed. A configured-but-unreachable cache is a
  health question (`/health`), not a capacity one.
