# The preference ladder: platform default, operator override, reader personalisation

One page, because the bounds are asked for in three different places and answered in none: M69-26 (this ladder),
M64-01 (per-surface algorithm choice) and §16.1a (`rec_mode`). Stated once here, so a later feature can cite it instead
of re-deciding it.

## The three levels, and what each may decide

| Level | Who sets it | What it may decide | Where it is written |
|---|---|---|---|
| **Platform default** | the build | The closed vocabulary: which surfaces exist, which algorithms exist, what each algorithm needs, and the default algorithm set per surface. It cannot be raised or lowered by configuration. | `darkhold_domain::recommendation_surfaces::SURFACES`, `engine_reason` |
| **Operator override** | the instance's operator | Which of that vocabulary this instance runs: a per-surface algorithm set (`[discovery.surfaces]`), the instance-wide strategy set (`rec_enabled_strategies`), the recommender mode (`rec_mode`), and the experiment arm count. | `config.toml`, validated at load |
| **Reader personalisation** | one reader | Which of the operator's *enabled* algorithms rank **their** feed, and their sort preference per surface. Never an algorithm the operator turned off. | per-account setting, resolved by `rec_preference::resolve_strategies`; `reader_sort_preferences` |

## The four bounds

1. **The registry is closed at the top, and that is a bound on everyone.** No level can invent a surface id or an
   algorithm name. A typo in the operator's file is a load error, not a silently ignored line — `validate_configured`
   refuses an unknown surface, an unknown algorithm, a duplicate, and a count outside one to four.
2. **The operator narrows; the reader narrows further, never wider.** `resolve_strategies` intersects the reader's choice
   with the operator's enabled set, so a reader whose stored preference names a strategy the operator has since disabled
   falls back rather than resurrecting it. The resolution says which happened: `instance_default` (no reader choice),
   `honored` (the reader's choice was in the enabled set), `unavailable` (it was not, and the instance default applies).
   Those three words are the whole contract, and they are why the reader's settings screen can be honest about a
   preference that no longer does anything.
3. **A disabled algorithm costs nothing.** `enabled_engines_for_bucket` filters the engine list before the engines run, so
   an operator who turns the ladder off does not pay for a taste-service call whose result is discarded. A bound that is
   only applied to the output is a suggestion.
4. **An experiment arm narrows the operator's set, never the platform's, and never moves.** With
   `[discovery] experiment_buckets > 1`, a signed-in reader's arm is stored on first use and read thereafter
   (`experiment_assignments`); reducing the arm count later leaves existing readers in their stored arm rather than
   reassigning them, and an arm that is now out of range gets the **whole** operator set rather than a narrowed feed.

## What the ladder is not

- **Not a trust ladder.** The three levels describe configuration, not permission. A reader's ability to change their own
  ordering is a settings permission (it is their own feed), while a mood, a tag merge or an instance rule is a trust gate
  under §18 — those are decided by `actor::TierConfig`, not here.
- **Not a place to put content policy.** A reader cannot personalise their way past a content filter the operator set, and
  an operator cannot personalise their way past a legal hold. Filtering runs before ranking; the ladder governs ranking.

## What decides a fight between two levels

The narrower level wins, in this order: reader choice, then operator override, then platform default — and the winning
level is the one that is **reachable**. A reader preference the operator's set no longer contains does not win by being
narrower; it loses because it is unreachable, and the resolution says `unavailable` rather than pretending the reader's
choice applied.
