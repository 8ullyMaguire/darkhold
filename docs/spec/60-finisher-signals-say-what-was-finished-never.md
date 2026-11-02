# 60. Finisher signals: say what was finished, never what was not

**Status:** reserved
**Milestone:** —
**Source:** §60 of `docs/spec.md`

Gaps review C5. The archive knows `works.completion` on every work and it knows when every revision
landed; what it has never done is tell an author that they finish things. The review asks for two
signals and one rule, and the rule is the part that decides the shape of the payload:

> Show update cadence ("updates about every two weeks") and completion history ("finished 9 of 10
> works"), with a Finisher badge. **Positive framing only; never label non-finishers.** Authors can
> optionally enable completion pledges through existing reserve/capture escrow. **Off by default** —
> "update pls" pressure is considered rude.

## 60.1 The payload carries counts, not verdicts

- Completion history is `finished` (`completion = 'complete'`) and `concluded` (`complete` +
  `abandoned`). **`in_progress` and `hiatus` are neither**: a work being written is not a verdict,
  and counting it would make every productive author look like they finish a small fraction of their
  output.
- The sentence is `"finished 9 of 10 concluded works"` — the review's own example. It is built in
  `domain::finishers::summary`, and the words `abandoned`, `unfinished`, `incomplete`, `failed`,
  `stalled` and `gave up` are a named list (`NEGATIVE_WORDS`) that the sentence must not contain.
- **No field is named for what was not finished.** `History` has `finished` and `concluded` and no
  third field; a top-level `abandoned` count is a label, and a label is the thing the rule forbids.
  The route test asserts the absence of those keys against the JSON that goes out, not against the
  function that built it.
- An author with nothing concluded gets a sentence — `"no concluded works yet"` — rather than a ratio
  of `0 of 0`.

## 60.2 Cadence can be unmeasurable, and says so

- Cadence is the **median gap in whole days** between the days on which a work gained a revision.
  Days, not seconds: "about every two weeks" is a claim about days, and a second-granular claim is
  precision nobody asked for.
- A work whose revisions all landed on one day has **no cadence**: that is the shape of an import,
  and a median over a single day would report the import's rhythm as the author's. The answer is
  `{"measurable": false, "reason": …}` with a `200`, not an error — the same shape §58.2 uses for a
  drought that cannot be measured.
- **An author's cadence is the median of their works' cadences**, never the median over pooled days:
  pooling measures the gap *between* two works as if it were an update interval, so a work begun a
  year later drags the number. The `samples` field says how many works the answer rests on.
- The sample is capped (`CADENCE_SAMPLE = 20` works); a median of twenty is not a different answer
  from a median of seventy, and the cap keeps the door off a full scan.
- When `revisions` is absent from an instance's schema the answer is unmeasured with a reason
  (M63-33), never a `500`.

## 60.3 The surface ships off

Per the review, `finishers` has `default_on: false` and `relevance: 3`, and it is `Toggle::Operator`.
A default configuration answers `404 MODULE_UNAVAILABLE`; an operator writes
`[modules] enabled = ["finishers"]`. The route test asserts both halves, because "off by default" is
one line in a registry table and the line is the easiest thing in this section to lose.

## 60.4 What this deliberately does not do

- **No stored badge.** A badge is a threshold, a threshold is a policy, and the review gives no
  number. The surface states what was finished; a badge is a `WHERE finished >= N` over the same
  counts when someone decides N.
- **No completion pledges in this build.** The review points at "existing reserve/capture escrow",
  and `credit_holds` is the nearest thing — but it is keyed by `job_id` and `capture_hold` post no
  ledger entry, so a pledge built on it would *look* like it held credits and would not. A pledge
  needs a balance check, a ledger leg each way and a defined sink for a burned promise, which is the
  economy's work. Recorded here rather than faked there.
- **No per-work "unfinished" state anywhere public.** Not for works, not for authors, not in a
  tooltip.

## Acceptance

- Finished counts `complete` only, and concluded counts `complete` + `abandoned`; a test includes
  `in_progress` and `hiatus` rows and asserts both numbers ignore them.
- The signal payload contains none of `NEGATIVE_WORDS` and no top-level key named for an unfinished
  work — asserted on the serialised JSON.
- A work whose revisions landed on one day answers `measurable: false` with a reason, and a work
  spanning two days answers the gap in days.
- An author's cadence is the median of their works' medians, with `samples` naming how many works it
  rests on.
- An instance without `revisions` answers unmeasured, never `500`.
- With a default configuration both doors answer `404 MODULE_UNAVAILABLE`, and enabling the module in
  `[modules] enabled` answers `200`.
