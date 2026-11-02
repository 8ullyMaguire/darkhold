# 47. Ranking — the substrate the discovery mechanisms plug into

**Status:** reserved
**Milestone:** —
**Source:** §47 of `docs/spec.md`

> **2026-09-30 addition.** §30.7 and §43.3 define what `for-you` must *be*
> (a permutation, never a filter; the aligned set; the trust bar). They do not
> define how a candidate becomes ordered, and no scoring function exists in the
> tree to modify: `personalized_recommendations` orders by `w.updated_at DESC`
> and `public_recommendations` orders by the same. The M45 "gaps review" block —
> 45 rows — assumes a ranking substrate with Scout credit, an exposure floor,
> exploration slots, propensities, and MMR re-ranking. This section builds that
> substrate and pins the contract the mechanisms plug into.
>
> **The order in this section is forced, and two rows are marked retrofit-critical
> in `requirements.csv` for a reason that cannot be argued with: once ranking has
> shipped, an interaction that was not logged with its selection propensity
> cannot be recovered afterwards.** There is no backfill. §47.3 (propensity
> logging) and §47.4 (interaction tagging) are therefore written *before* the
> first mechanism that depends on them, and M45-13/-11 must land before any
> scoring work, not after it.

## 47.1 The problem this section solves

Four things are currently true and one of them is a bug waiting to happen:

1. `personalized_recommendations` (`crates/db/src/discovery.rs:209`) selects by
   tag overlap and orders by `w.updated_at DESC`. There is no score.
2. `public_recommendations` orders by `w.updated_at DESC` as well.
3. Nothing records *why* a reader saw a work, or *how likely they were* to be
   shown it. So no mechanism can be evaluated honestly, and no offline
   evaluation is possible.
4. Nothing distinguishes an earned interaction from a bought one, so the first
   reading-club or bounty bonus silently becomes a ranking signal.

The last one is the reason this section exists rather than a pile of tuning
knobs. §0.3 is the project's privacy ban and it is drawn here too: taste is
inferred, never declared, and a ranking substrate that cannot tell an earned
engagement from an incentivized one will learn the cheaper one.

## 47.2 The ranking contract

`rank_works` is the one entry point. Every mechanism is a stage; the order is
fixed by this section and not by the caller.

```
candidates → [ineligible dropped] → [scored] → [interleaved: exploration slots]
           → [re-ranked: MMR + satiation] → [capped] → ordered result
```

**A stage may not reorder the pipeline.** A mechanism that wants a different
pipeline writes its own function and is called from a route, not from
`rank_works`. The reason is that the propensity log in §47.3 must describe the
distribution that actually produced the output; a mechanism that could reorder
the pipeline could emit results that the log does not describe, and the log is
the only thing making offline evaluation possible.

- **The output is a permutation of the candidate set**, minus dropped rows. It
  is never a filter that silently omits a work the reader was eligible for
  without an exclusion that was logged (§43.3).
- **Every ordered row carries its reason**: the stage that placed it, its
  score, and its propensity. A row with no reason is a bug, not a default.
- **Determinism.** Given the same database state and the same reader, the
  ordered result is byte-identical except for the exploration slots (§47.3),
  which are random by design and whose randomness is logged rather than seeded
  from the clock.

## 47.3 Selection propensity — logged from the first day

M45-13 (uniform-random exploration slots) is marked retrofit-critical, and this
clause is why.

- **Every impression records the probability that this reader would have been
  shown this work.** The log row carries `work_id`, `reader`, `slot_kind`
  (`ranked` | `exploration` | `exposure_floor`), `propensity` (a real in
  (0,1]), and `logged_at`.
- **`propensity` is the probability the item was *selected* into the position
  the reader saw it in, not the probability of appearing in the candidate set
  at all.** The distinction matters: inverse-propensity weighting corrects for
  selection bias using this number, so a propensity that is merely "was it a
  candidate" makes the correction wrong in a way no later analysis can detect.
- **No backfill exists and none is planned.** An interaction with no propensity
  is not repairable: the counterfactual that would have been logged is gone.
  Rows are therefore written in the same transaction that records the
  impression, and a row that cannot be written fails the impression rather than
  dropping the log.
- **Exploration slots are uniform-random over the eligible-but-unshown set**,
  not over the top-N. The slot's probability is `1/|eligible-unshown|`; a slot
  that has already shown a work to this reader has its probability recomputed
  against the reduced set, so consecutive slots are independent draws rather
  than repeats.

## 47.4 Earned and incentivized interactions are different rows

M45-11 (tagging incentives at write time) shares the retrofit-critical marking
for a different reason: an incentive introduced *after* an interaction has been
recorded cannot be subtracted from it.

- **An interaction carries a `kind`: `earned` or `incentivized`.** The kind is
  written when the interaction is recorded, not inferred later from which
  feature produced it.
- **Ranking counts only `earned` interactions.** An incentivized interaction
  still counts toward the author's own credit ledger (§17) and is still visible
  to the author; it is excluded from ranking signals specifically because
  crediting it would make the cheapest engagement the most valuable one.
- **Reading-club reads, topic-subscription reads, bounty reads, and
  taste-notification impressions are `incentivized` by definition.** Not "usually
  incentivized" — by definition, because the reader was routed there by the
  incentive rather than by the ranking.
- **§0.3 consequence: a detector's output is never sufficient to reclassify.**
  Generated-content posture (M45-12) may *require* disclosure, but it never
  silently converts an interaction's kind. Reclassification is an operator
  action, recorded.

## 47.5 The exposure floor

M45-15: every new work is guaranteed a minimum number of impressions.

- The floor is **per work, counted over a window**, not per reader: a work with
  zero impressions is surfaced to readers who have not seen it, independent of
  whether the ranking would have chosen them.
- **The floor cannot promote work above the trust bar** (§19.14, §30.7). A floor
  impression is still an impression with a propensity, and it still respects
  content filters — the floor is a guarantee of *opportunity*, not a guarantee
  of delivery to a reader who filtered it out.
- Because the floor draws from readers who did not ask for the work, these
  impressions carry `slot_kind = 'exposure_floor'` and a propensity reflecting
  the pool they were drawn from. Otherwise the exploration data the floor
  generates would be indistinguishable from ranked impressions, and M45-13's
  inverse-propensity scoring would be silently biased by them.

## 47.6 Variety: MMR and satiation

M45-49: "three angst fics in a row nudges the next pick toward fluff."

- **MMR (maximal marginal relevance) re-ranks the top of the list**, with
  `λ` trading relevance against diversity. It re-ranks; it does not filter.
  Every work that entered the aligned set leaves it still present (§43.3).
- **Satiation is a per-reader running state over recent picks**, decaying with
  time so a reader who returns after a week is not still saturated from last
  week. Satiation applies to the *tag distribution* of recent picks, not to
  authors: the §47.6 problem being solved is "three angst fics in a row", which
  is a tag property.
- **Satiation is a nudge, not a ban.** A reader who has asked for nothing but
  angst keeps getting angst; the term raises the score of alternatives, it does
  not exclude the category. A hard exclusion here would be a filter, and §43.3
  forbids filters.

## 47.7 Scout value

M45-10 replaces overlap-based resonance in curation credit.

- **Scout value credits engagement with works that later earn a high operator
  rating or vanguard consensus, weighted by how obscure the work was at the
  time of engagement.** The obscurity weight is what makes this "scouting"
  rather than "being early to a popular thing" — without it, the mechanism pays
  the largest audience, which is the opposite of its purpose.
- **Obscurity is measured at engagement time and stored**, not recomputed from
  the present. A work that was obscure when read and popular now must still
  score as an obscure read, or the mechanism pays whoever happened to arrive
  early, which is measurable and would be gamed immediately.
- **This is a credit mechanism, not a ranking one.** Scout value is computed for
  curation credit (§17) and does not directly influence `rank_works`. A reader
  does not get better recommendations because they scouted well; an author does
  not get ranked higher for being scouted. Keeping them separate is what stops
  the two feedback loops from amplifying each other.

## 47.8 Invariants

- **A reader never sees their own work in `for-you`.** Already true
  (`personalized_recommendations` excludes the account's own pseud ids) and
  unchanged here.
- **Content filters apply at every stage**, including exploration slots and
  exposure-floor impressions. The aligned set is computed from the filtered
  candidate set, so a floor impression cannot route around a filter.
- **Every ordered row has a logged impression with a non-null propensity.**
  Asserted, not assumed: the test in `crates/app/tests/` breaks the
  impression write and requires the ranking call to fail.
- **The output is a permutation of the filtered candidate set.** A row dropped
  after the aligned-set computation carries an exclusion reason.

## 47.9 Acceptance

- `rank_works` orders by score with exploration slots interleaved, and every
  returned row has an `Impression` row with `propensity > 0` — both engines.
- With exploration slots disabled, two calls with the same database state return
  byte-identical ordering.
- An `incentivized` interaction raises the author's credit ledger total and
  leaves every ranking signal unchanged.
- Three consecutive picks sharing a tag raise the fourth pick's diversity term;
  with `λ = 1.0` the ordering is identical to no MMR at all.
- Scout value for an engagement on a work that was obscure then and popular now
  is unchanged from the value computed at engagement time.
- The exposure floor gives a zero-impression work impressions without promoting
  it past the trust bar or past a reader's content filter.

## 47.10 What this section deliberately does not do

- **No learning-to-rank and no embeddings.** The substrate is explicit scoring
  with recorded reasons. A model can be added behind `rank_works` later without
  changing the contract; adding one now would mean the offline evaluation in
  §47.3 has nothing to evaluate yet.
- **No operator-tunable weights in this section.** §38.6 governs instance config,
  and M45-48 (hardware-aware presets) and M45-45 (versioned blendable taste) own
  weight configuration. Two places to set a weight is one too many.
- **No rewriting of `personalized_recommendations`'s tag-overlap candidate
  selection.** It decides *who is eligible*; §47.2 decides *in what order*. The
  split is deliberate — eligibility is a privacy and trust question (§30.7),
  ordering is a taste question, and merging them would put ranking weight behind
  the trust bar.
