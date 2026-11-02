# 49. Taste signal — making the profile worth ranking on

**Status:** reserved
**Milestone:** —
**Source:** §49 of `docs/spec.md`

> **2026-10-01 addition.** §47 gave the ranking a substrate: explicit scoring,
> recorded reasons, logged propensities. It has no way to *improve the reader's
> profile*. This section is the input side of that machine, and it adopts the
> gaps-review items M45-16, -19, -20 and the two dimension producers M45-14 and
> -35, which together are what makes a `TagWeights` row mean something.
>
> Source proposals: `docs/spec-gaps-design-review-2026-09-22.md` A5, A7, B1,
> B2, B6, C7, C11. Build plan: `docs/plans/m45-gaps-adoption.md` Phase 1–2.

## 49.1 The problem

§47.2's contract is honest about what a score is: the reader's tag weights, times
the work's tags. That makes the whole quality of the feed a function of one
question — *are the weights right?* — and as built they are whatever a reader's
incidental behaviour happened to produce.

Three specific ways they are wrong:

- **They are unaudited.** A tag the reader never meant to endorse counts as much
  as one they did, because nothing distinguishes an author-applied tag from a
  tag-stuffed one. A7: tag gravity makes tag-stuffing profitable, and the
  cheapest exploit is to apply a hundred tags to one work.
- **They are sparse.** A new reader has no weights at all, and
  `TagWeights::for_reader` returns an empty set, which §47.2 handles
  deterministically but which means the feed is engine order rather than taste.
  Two hundred labelled examples would fix that; a reader has neither the time
  nor the vocabulary to produce them unaided.
- **They are one-dimensional.** `dimensions_for` reads tags. Prose style, dialogue
  ratio and vocabulary richness are not tags, and they are what a reader
  reacting "loved the prose" is actually telling you.

## 49.2 Only confirmed tags count toward gravity

M45-16, from A7.

- A tag contributes to gravity **only if it is reader- or wrangler-confirmed.**
  Author-applied and machine-inferred tags are stored and displayed, and are not
  counted. The distinction is recorded per work-tag pair, not inferred at query
  time, because inference is how the distinction quietly stops existing.
- **A per-work contribution cap.** At most `N` tags per work contribute to
  gravity, and `N` is instance config (§38.6), default **5**. A work with fifty
  tags cannot outweigh a work with one that the reader genuinely reads. The cap
  is on *contribution*, not on display: all fifty tags are still shown and
  searchable.
- **Readers can flag an inaccurate tag.** The flag goes to Milestone 29's
  wrangling queue, and a confirmed-inaccurate tag is excluded from gravity
  immediately rather than at queue resolution — the point of the flag is that the
  reader who noticed should not keep paying for it meanwhile.
- This does **not** change what a work is or how it is filtered. It changes only
  what the ranker reads.

## 49.3 Work coordinates

M45-14, from A5. Four deterministic measures over a work's text:

| measure | what it separates |
|---|---|
| sentence-length variance | flat vs varied prose |
| dialogue ratio | prose-heavy vs scene-driven |
| vocabulary richness (type-token) | plain vs dense |
| chapter length distribution | single-sitting vs serialised |

- **Deterministic and reproducible.** Same text, same coordinates, on both
  engines. No model, no embedding, no randomness — which is what makes them
  comparable across a reader's whole history and therefore usable as a
  dimension.
- **Optional local embeddings only with M27's ML permission**, and off by
  default. Four deterministic measures are enough for the taste profile; an
  embedding is a fifth that costs the instance CPU it may not have (§53.2).
- Coordinates are computed **once at ingest** and stored. Recomputing them per
  request would put a full pass over every work in the path of every feed.
- A work too short to measure has **no coordinates**, and an absent coordinate is
  not a zero. A zero would mean "uniformly flat prose" and would rank against
  short-but-sharp works.

## 49.4 Rec blurbs as second summaries

M45-35, from C11. Where a reader's summary says "I suck at summaries", the best
recommendation note on the work is the second summary they actually wrote.

- The top-rated rec note is surfaced **beside** the author's summary, never
  instead of it, and always attributed and quotable.
- Only notes the author has **allowed to be quoted** are eligible. A rec note is
  a reader's words about someone else's work; surfacing it unquoted is a consent
  question, not a display one.
- The excerpt shown is bounded (a short pull, not the whole note), so the
  author's work is not displaced by commentary about it.
- Rec blurbs are **not** a ranking input. A reader who writes good blurbs does
  not get ranked higher. §47.7's separation of credit from ranking applies here
  for the same reason.

## 49.5 The tasting menu

M45-19, from B1. A calibration queue: summary plus a random 300-word passage,
rated in seconds, with a reason tag.

- **Items are chosen by uncertainty** (active learning), not randomly and not by
  popularity. The reader's time is the scarcest input in the system (§47.2's
  taste is calibrated against the operator's attention), so each sample is
  chosen to be the one most likely to change the model.
- **The 300-word passage is a sample, not a summary.** Prose style is visible in
  an excerpt; §49.3's coordinates need not be read off a full text to be
  labelled.
- **Reason tags are required**, not optional: prose, characters, pacing, trope
  execution, and a free-text "not for me: ___". The reason is the labelled
  signal; a bare rating teaches almost nothing, because "didn't like it" does not
  say whether prose or characterisation was the problem.
- A declined sample is **recorded as a negative with its reason**, not discarded.
  An active-learning queue that throws away the items it guessed wrong learns
  only from what it already knew.
- Sampling is bounded per session. A calibration queue that can consume the whole
  of a reading session is a chore, and a chore gets abandoned.

## 49.6 History import as cold start

M45-20, from B2. AO3, FFN and Goodreads bookmarks, kudos and read history,
imported through existing source credentials (M53).

- **Every reader is offered this.** It is not an operator-only tool; the gaps
  review's point is that two hundred labelled examples beat a quiz for any
  reader, and a quiz is the only alternative they have.
- **Imported signals are tagged as imported** and are distinguishable from
  organic ones everywhere they are read. They are not silently equivalent: an
  imported bookmark from 2019 is weaker evidence than a rating given today.
- **Import pulls in the work where it can**, subject to §51.4's visibility
  default: metadata and a link become visible, the cached body stays in the
  importer's private library until the author claims the work or grants
  permission. M45-53 owns that default; this section only refuses to violate it.
- **Import is idempotent and re-runnable.** A second import adds what is new and
  does not double the first.

## 49.7 Invariants

- **A tag a reader never confirmed never moves their ranking.** §49.2 is the
  filter, and it is applied at the point the weights are read rather than by
  cleaning the weights, so a newly-tagged work cannot retroactively change a
  stored profile.
- **The per-work contribution cap binds before the reader's weights are
  consulted.** A work cannot buy attention with tag volume (§33.2: no influence
  purchased).
- **Coordinates are reproducible.** Same text in, same coordinates out, on both
  engines, with no model and no randomness. A dimension that differs between
  runs is not a dimension.
- **An absent coordinate is not a zero.** Short works are unmeasured, and
  unmeasured never ranks against measured.
- **No sample is discarded for being a surprise.** Every queue item is either a
  positive or a negative with a reason.
- **Imported and organic signals stay distinguishable.** Everywhere, including in
  exports and in an access request.
- **Nothing here changes candidate selection.** §47.10's split holds: §49 decides
  what the ranker *knows*, never who is *eligible*.

## 49.8 Acceptance

- A work with fifty tags, all author-applied and unconfirmed, moves a reader's
  ranking not at all; the same work with three confirmed tags does. Both engines.
- With the contribution cap at 5, a work's sixth-through-fiftieth confirmed tags
  contribute nothing, and all fifty remain visible and searchable.
- A reader flags a tag as inaccurate: gravity drops for it immediately, before
  the wrangling queue resolves, and the flag is queued.
- Four works of identical tags and different prose styles produce different
  coordinates, and re-running the computation reproduces them byte for byte.
- A work under the minimum measurable length has no coordinates and does not rank
  against measured works.
- The tasting menu's next item differs from a random pick when uncertainty is
  concentrated, and the queue is bounded per session.
- A declined tasting item appears in the reader's profile as a negative carrying
  its reason.
- Importing the same history twice leaves the profile identical to importing it
  once.
- An imported bookmark makes the work's metadata visible and leaves its cached
  body private.

## 49.9 What this section deliberately does not do

- **No learning-to-rank, no embeddings, no ML.** §47.10 already refuses it, and
  §49.3's four measures are chosen to be computable without it. A model added
  here would have no evaluation data until §47.3's propensity logging has run.
- **No cross-reader taste pooling.** A reader's weights are theirs. Shared or
  inferred weights are a different mechanism (§50's blendable taste object,
  which is operator-facing, not reader-facing).
- **No tag deletion.** §49.2 changes what counts toward gravity; an inaccurate
  tag stays on the work, because it may be accurate for someone else.
- **No author-side styling.** Rec blurbs and coordinates are reader-facing
  signals. An author is not scored on them.
