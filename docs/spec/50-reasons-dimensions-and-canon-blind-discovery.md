# 50. Reasons, dimensions, and canon-blind discovery

**Status:** reserved
**Milestone:** —
**Source:** §50 of `docs/spec.md`

M45-24, M45-31, from B6 and C7. §49 established what a reader's profile is made
of. This section makes three of those things reach the served row: the *reason* a
reader gave, the *dimensions* a work measures, and works whose fandom the reader
has never signalled.

§49.3 (coordinates) and §49.4 (rec blurbs) are the existing spec for M45-14 and
M45-35 and are not restated here.

## 50.1 Reason-tagged kudos and line-level highlights

M45-24, from B6. Kudos already exist as a mechanism; this row is the **reason
column**, and a highlight that becomes a reason.

- **A kudos carries an optional reason from a fixed set** — prose, characters,
  pacing, trope execution, worldbuilding, and a free-text note. The set is fixed
  and enumerated, as §49.5's tasting reasons are, and for the same reason: a free
  text field is not reviewable and cannot be aggregated.
- **The reason is the signal, not the count.** Ten kudos with reasons train the
  profile; ten bare kudos do not. This is §49.5's rule restated for kudos, and the
  reason it is stated twice is that it is the clause most likely to be quietly
  dropped, since the count is the part that is obviously already implemented.
- **A bare kudos remains valid.** The reason is optional at the API. Making it
  required would push readers toward a rate-limit rather than toward a reason.
- **A highlight is a reason about a span, not about a work.** A highlight on one
  sentence says something narrower than a kudos on the whole work, and it is
  stored against the span with its own reason. Highlighting a sentence is not a
  way to raise a work's gravity: it counts once toward the work, as one signal.
- A deleted or edited work's highlights **go with it.** A highlight is a reference
  to a span of text that no longer exists, and keeping it would let the corpus of
  "what readers respond to" outlive the works it describes.

## 50.2 Fandom-blind discovery

M45-31, from C7. Some works belong to fandoms the operator's instance has never
heard of, and the tag-based ranker cannot surface them: a work tagged `stargate`
goes to readers who have `stargate` in their weights, and a reader who has never
seen that tag has no weight for it.

- **A work whose fandom is unknown to the reader is eligible, not just unknown to
  the instance.** The distinction is the whole mechanism: "unknown fandom" is not
  a synonym for "no fandom", and a tag a reader has never weighed is exactly the
  case §49.5's tasting menu samples for.
- **Canon-agnostic works are a class, not a score.** A work is eligible when its
  own text is readable without having read a parent canon. This is a property of
  the work, computed the same deterministic way §49.3 computes coordinates, so it
  is reproducible and not a moderator's judgement.
- **Surface as a labelled, bounded set.** Canon-agnostic works appear in a distinct
  place, not mixed into the main feed. A reader who does not know a fandom should
  meet it as "you may not know this fandom" rather than as an unexplained result
  outranking something they recognised.
- **They never outrank a recognised match.** §47.2's quality argument applies: the
  ranker is better than a coin flip when it knows the taste. Canon-blind works are
  offered where a wrong guess costs little and labelled as the guess they are.
- **No cross-reader pooling of what "canon-agnostic" means.** §49.9 already
  refuses cross-reader taste pooling, and a derived per-work flag computed from
  one instance's corpus is that reader's corpus, not the platform's.

## 50.3 Invariants

- **A reason is never invented.** No default, no inferred reason, no empty string
  stored as one. Absent means absent, and the profile treats a bare kudos as the
  weak signal it is.
- **A highlight counts once.** Many highlights on one work are many signals about
  spans and one signal about the work. A work cannot buy gravity with quote volume.
- **Coordinates and canon-agnostic flags are reproducible on both engines.** Same
  text in, same values out. §49.7's rule, and it holds for anything derived from
  text rather than from taste.
- **Canon-blind discovery changes eligibility, never ranking.** §49.7's last
  invariant holds: §50 decides who may be *considered*, §47 decides who is
  *shown first*.
- **Nothing here is a ranking input that a reader controls for the wrong reason.**
  §47.7 again — rec blurbs, kudos counts and highlight volume are not taste.

## 50.4 Acceptance

- A kudos with a reason trains the profile on that reason's dimension; the same
  kudos with the reason omitted trains nothing, and both are accepted by the API.
- A reason outside the fixed set is refused, and a free-text note alongside a
  valid reason is stored but never aggregated.
- Fifty highlights on one work move its gravity by the weight of one signal.
- Deleting a work deletes its highlights.
- A work in a fandom the reader has never weighed is reachable, and appears
  labelled as unfamiliar rather than mixed into the recognised results.
- A canon-agnostic work never outranks a recognised match for the same reader.
- Two works of identical tags and different prose have different coordinates, and
  recomputing reproduces them byte for byte on both engines.
- The top rec note appears *beside* the author's summary, never instead of it, and
  only when the author allowed quoting.

## 50.5 What this section deliberately does not do

- **No sentiment analysis on rec notes.** §49.9 refuses ML without evaluation data
  and §47.3 has not run long enough to have any. A rec blurb is displayed because
  a reader wrote it, not because a model scored it.
- **No highlight-to-kudos escalation or popularity mechanic.** A highlight is a
  reader's own note about a span. Turning it into a counter would be the same
  influence-purchase §33.2 forbids.
- **No automatic canon detection.** §50.2's class is computed from the text by the
  deterministic measures §49.3 already defines, not from a franchise database
  that would go stale and would need to be corrected by hand.
