# 57. Continuing, and the honest shape of "coming back"

**Status:** reserved
**Milestone:** —
**Source:** §57 of `docs/spec.md`

§9.3 already stores progress: `work_or_library_item_id`, `chapter_id`,
`content_revision`, `paragraph_anchor`, `position_fraction`, `device_id`. Nothing
*reads* it. A reader who closed a chapter and came back is asked to find it again,
which is the cheapest possible way to lose them.

This section adds the surfaces that make a returning reader oriented, and it is
deliberately narrow: **seven features, all of which read data that already exists
or that the reader already created.**

## 57.1 Continue Reading is the first thing on the home page

- One banner, above the feed, showing the most recent work, the chapter, and a
  progress fraction. It appears only when there is something to continue.
- **Progress is a fraction of the work, not of the chapter.** "Chapter 4 of 19"
  is the honest form when a work has nineteen chapters; a percentage of the last
  chapter is a number that resets to zero every time a reader finishes one, which
  is the opposite of what "continue" means.
- When several devices disagree (§9.3), the banner shows the most recent and
  offers the choice rather than picking silently.
- **The banner is dismissible and stays dismissed.** A reader who knows what they
  are doing does not need a card about it on every page.
- A signed-out visitor never sees it. There is no reader to continue.

## 57.2 Length is on the card, because it decides the click

A reader chooses a work in under two seconds, and length is a primary input to
that choice.

- Every work card shows `word_count` and an estimated reading time.
- **The estimate is `word_count / 250` and is labelled an estimate.** The divisor
  is a constant in `Config` (§38.1), not a number in a template, and the label
  says "about N min" rather than "N min" — a reading time presented as exact is a
  measurement nobody took.
- **Per-chapter length in the table of contents**, from `chapter_revisions.word_count`,
  so a reader can tell a 500-word interlude from a 15k monster before committing.
  This is the same number rendered one level deeper, not a new feature.
- **A rating ceiling can suppress the estimate**, exactly as it suppresses the body:
  a length is a teaser. An estimate computed over text the reader may not see is a
  number derived from content the server just refused to serve.

## 57.3 Completion status is on the card

Readers avoid abandoned works, and being surprised by one is worse than not
recommending it.

- A badge for each of `works.completion`'s values (§8.2): `complete`, `in_progress`,
  `hiatus`, `abandoned`.
- **The badge is always the work's real state, never an inference.** §8.8's derived
  activity status (`dormant`) is *not* rendered as an abandonment badge. An author
  on hiatus has not abandoned anything, and a derived status shown as a decision
  reads as a judgement on the author's reliability that nobody made.
- An `abandoned` badge never carries a reason. The author may not have given one,
  and inventing one would be worse than silence.

## 57.4 "Did not finish" is a first-class status

- A reader marks a work DNF, optionally with a **private** reason. The reason is
  never public, never aggregated, and never shown to the author.
- **DNF suppresses future recommendations.** §16.10's surprise-me mode and
  meaningful opt-out treat DNF as the strongest negative signal a reader can give,
  and it survives a taste-profile recompute (§16.2) rather than being rebuilt.
- DNF is **private by construction and reversible.** Deleting it restores
  eligibility. There is no public count of DNFs on a work, because a public DNF count
  is a quality judgement on the author published by readers, and §12's positivity
  rules and feedback-drought rules (§19.15) are both about not doing that.
- The author's statistics (§57.9) may show a *rate* to the author alone, labelled as
  reader-side, and never as a verdict.

## 57.5 Recommendations carry their reason, and it is the real one

§16.1 already defines `reason` fields. Nothing renders them.

- Every recommendation states why it is here, in the reader's terms: "because you
  bookmarked *Stars Fall Softly*", "more in Harry Potter", "complete, under 20k".
- **The reason is computed from the signal that produced the recommendation.** It is
  not a template chosen after ranking. A "because you read X" line attached to a
  recommendation that had nothing to do with X is worse than no line, and it is the
  failure this clause exists to prevent.
- **A recommendation with no derivable reason is not shown.** Surfacing an unexplained
  item is how a recommendation feed starts to feel like noise.

## 57.6 "Surprise me" is §16.10's surprise-me mode, and it says so

- One control on Discover that requests recommendations outside the reader's profile,
  which is §16.10's existing clause rather than a new mechanism.
- **The diversity budget (§16.4) still applies inside it.** Surprise-me relaxes the
  weighting; it does not suspend the rules that keep an instance from becoming
  monothematic. A "surprise" that is six works of the same ship is not a surprise.
- A floor below which nothing is returned, so it cannot serve a work the reader
  cannot access or a work below the instance's quality floor.

## 57.7 "New in your fandoms" is a time window on an existing join

- Recent publications filtered to the fandoms the reader has bookmarked.
- **The window is in `Config` and defaults to 30 days.** A hard-coded window is a
  number that goes stale (§38.1).
- **It is empty rather than absent when the reader has no fandoms.** An empty section
  teaches a reader something; a missing one is ambiguous.
- "New" means *first published or newly updated within the window*, and the label
  says which — a work updated three years ago and surfaced here is not "new".

## 57.8 What this section deliberately does not do

- **No streak counter.** §9.7 has the infrastructure and a streak is a
  well-understood retention mechanic, but it rewards daily return over a good
  session, and it manufactures guilt on the days a reader does not read. A reader
  who opens the site to check one thing should not be told they have a broken
  streak. Not built; not planned.
- **No reading-time self-calibration.** A timed 200-word passage is a good idea and
  it is also a modal that interrupts the first session to improve a number nobody
  asked for. The 250 WPM constant with an honest "about" label does the job.
- **No inline content-warning filtering.** §46's server-enforced content filters
  stand; a per-reader hide-list is a separate mechanism with its own failure modes
  (a hidden warning is a hidden warning) and deserves its own section.
- **No email digest.** A scheduled job, an SMTP dependency and a subscription
  surface for a retention gain that the in-app surfaces (§54's concierge) should be
  measured against first.

## 57.9 Acceptance

- The banner shows work, chapter and progress, and appears only with something to
  continue; a signed-out visitor sees none.
- Progress is a fraction of the **work**, and a test asserts that finishing a chapter
  does not reset it to zero.
- Dismissing the banner keeps it dismissed on the next visit.
- Every work card shows a word count and an estimate labelled "about"; the divisor
  is read from `Config` and changing it changes the rendered estimate.
- The TOC carries per-chapter length.
- A rating ceiling suppresses the estimate, and the test asserts the estimate is
  **absent**, not merely hidden by CSS.
- Each of the four completion values renders its own badge, and `hiatus` is visually
  distinct from `abandoned`.
- **A `dormant` derived status never renders as an `abandoned` badge.** The test
  drives activity through the derivation and asserts the badge does not appear.
- DNF suppresses recommendations, survives a taste recompute, and deleting it
  restores eligibility.
- A DNF reason is never present in any API response for another reader.
- Every rendered recommendation has a reason, and the reason test asserts the reason
  is derived from the signal rather than matching a template list.
- Surprise-me returns nothing below the quality floor, and still respects §16.4.
- "New in your fandoms" is empty rather than absent with no fandoms, and states
  whether an item is new or updated.

## Prior art in the schema

`reading_progress` (§9.3) is the whole of 57.1. `chapter_revisions.word_count`
(§4.3) is the whole of 57.2. `works.completion` (§8.2) is the whole of 57.3.
§16.1's `reason` is the whole of 57.5 and §16.10's surprise-me is the whole of
57.6. **The only new state this section introduces is DNF**, which is a per-reader
status in the same shape as the rating it sits beside.
