# 41. Longevity and ambient social signals

**Status:** reserved
**Milestone:** —
**Source:** §41 of `docs/spec.md`

> **2026-09-21 addition.** Two Gravity-derived signals, adopted because they
> serve the priority stack's first item — maximise high-quality fiction —
> without optimising for engagement (§0.3's vital-sign rule).

## 41.1 Content half-life

A work's **half-life** is how long it keeps being read after publication.
Freshness (§16.14) biases recency; completion rates (§9.8) measure the first
read. Neither answers the question a reader actually asks: *is this still
worth reading now?* A work still being finished and discussed two years on
is evergreen quality — the thing the priority stack names first.

Computation, nightly, per work published more than `[discovery].half_life_min_age_days`
(default 30) days ago: from `reading_events`, the ratio of readers who
started the work in the last 30 days to those who started it in its first 30,
expressed in basis points (`half_life_bp`, `INTEGER`, house binding rules).
The score is written onto the work row by a scheduled job and is **never
shown to readers as a number** — it is a ranking input, consumed by the
discovery engines as a multiplier on `Candidate.score` (§16.3's blend), the
same silent-reordering contract operator affinity follows.

Configuration: `[discovery].enable_half_life` (default `false` until the job
proves itself), `half_life_min_age_days`, `half_life_window_days` (default 30).

## 41.2 Interaction tiers — ambient warmth

Most readers never comment (§17's positivity model shapes comments; it has
nothing for the silent majority). Interaction tiers give a shy reader a
graded way to matter: every interaction with an author's works — reading a
chapter, finishing a work, reacting, commenting — accumulates a private
**warmth** value between the reader's account and the author's. Warmth
crosses thresholds (`lurk < react < comment < create`, the thresholds in
basis points under `[community].warmth_thresholds`) and the resulting tier
is shown to the author as an aggregate only: "this month, 34 lurkers, 12
reactors, 4 commenters, 2 creators engaged with your works." Never a list of
names, never a per-reader value, never shown to the reader themselves — the
reader sees nothing, because a warmth meter on your own reading is a
streak-shaped dark pattern (§0.2 attention rules).

Recording hooks: `record_reading_progress`, quick reactions, comments. Each
hook writes one row (upsert by pair, accumulate the delta, recompute tier)
inside the same transaction as the interaction it accompanies.

## 41.3 Acceptance

- With `enable_half_life = true`, the nightly job writes `half_life_bp` on
  eligible works and a changed score reorders discovery output with no field
  changes (the §16.3 silent rule).
- A work with no recent readers has a lower half-life score than one still
  being finished; the job is idempotent — running twice changes nothing.
- An author's aggregate panel shows tier counts; a test asserts no endpoint
  exposes a per-reader warmth value or a reader list.
- Warmth accumulates across interactions within one transaction; a failed
  interaction writes no warmth.

## 41.4 What this section deliberately does not do

- **No public half-life badge.** The score is a ranking input, not a label;
  publishing it would make recency-of-attention a target.
- **No reader-facing warmth.** The reader never sees their own tier or
  progress toward one — the anti-streak rule is absolute.
- **No warmth in ranking.** Warmth describes a reader-author relationship;
  it never feeds discovery, search or any public ordering.
