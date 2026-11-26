# docs/plans/taste-gravitational-system-v2.md — plan card

Source: `docs/plans/taste-gravitational-system-v2.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Source:** docs/spec-amendments/taste-gravitational-system.md + user streak refinement
**Supersedes:** docs/plans/taste-gravitational-system.md
- `crates/domain/src/taste_vector.rs` — already exists, extend with:
- `crates/app/src/config.rs` — add `TasteConfig` struct with dimensions, exemplars, anti_examples
- `crates/domain/src/taste_vector.rs` — add:
- `crates/db/src/taste_vectors.rs` — already exists, extend with:
- `crates/domain/src/discovery.rs` — extend `Candidate` with `taste_signal: f64` (already done in earlier work)
- `crates/domain/src/discovery.rs` — add:
- `crates/app/src/config.rs` — add `SignalWeightMode` enum, `signal_weight_mode: String`
- `crates/app/src/routes/discovery.rs` — wire taste gravity into discovery pipeline
- `crates/domain/src/discovery.rs` — `inject_diversity_by_taste` already implements this
- `crates/app/src/routes/quiz.rs` — new route:

## Copy-pasteable code / commands

[instance]
preset = "curated_boutique"

[site.taste_profile]
enabled = true
dimensions = [
  { key = "angst", label = "Angst", admin_target = 0.8, weight = 0.3 },
  { key = "pacing", label = "Pacing", admin_target = 0.6, weight = 0.2 },
  { key = "prose_density", label = "Prose Density", admin_target = 0.7, weight = 0.2 },
  { key = "canon_compliance", label = "Canon Compliance", admin_target = 0.5, weight = 0.15 },
  { key = "trope_diversity", label = "Trope Diversity", admin_target = 0.6, weight = 0.15 },
]
exemplars = ["work_id_1", "work_id_2"]
anti_examples = ["work_id_3"]

[site.taste_profile.quiz]
enabled = true
min_works_shown = 10
min_works_picked = 3
skippable = true

[signals]
mode = "taste_weighted"
diversity_injection_percent = 10

[notifications]
taste_threshold = 0.5
taste_batch_window = "3h"
taste_max_per_day = 3
popularity_bypass = 0.9

[lifecycle]
completion_multiplier = 2.0
resurrection_multiplier = 3.0
completion_boost = 1.2

[taste_probes]
frequency = "weekly"
distance = 0.2
max_probes_per_session = 3
auto_expand = true

[vanguard]
method = "resonance_threshold"
threshold_percent = 10
can_pin = true
can_nominate = true
picks_shelf = true
can_create_clubs = true
public_badge = true
bounty_discount = 0.5

[trust]
taste_coupling = [
  { level = 1, min_resonance = 0.0 },
  { level = 2, min_resonance = 0.0 },
  { level = 3, min_resonance = 0.0 },
  { level = 4, min_resonance = 0.3 },
  { level = 5, min_resonance = 0.5 },
  { level = 6, min_resonance = 0.0 },

## Verification command and expected output

**Goal:** Optional taste gate per trust level.
- All domain unit tests pass

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
