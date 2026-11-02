# 52. Taste leakage: what a public artifact gives away

**Status:** reserved
**Milestone:** —
**Source:** §52 of `docs/spec.md`

M45-17, from A9. §0.3 forbids the administrator's taste profile being visible,
inferable, or hinted at through any user-facing label. §16.5 then concedes that
"public behavior may permit broad inference — do not promise mathematical
secrecy."

Those two are not in tension, but the reconciliation is not obvious, and A9 is the
place where the difference becomes concrete: **an instance is allowed to be
readable and is not allowed to be readable by a machine probing for a lens.**
The line is not what is inferable in principle. It is whether the artifact was
built to be read by a curious person or to be measured by someone scoring the
operator.

So this section is mostly about *how* the answer is surfaced, not about what it
contains. That is deliberate: a view that only listed what is inferable would be
a disclosure checklist nobody reads, and a view that only refused would be the
thing §16.5 explicitly does not promise.

## 52.1 The leakage view answers "what could someone infer", not "what do they know"

- **The view is an operator instrument, on `/admin/discovery`, and is not
  public.** It is for the operator deciding whether an artifact is too revealing,
  not for a reader and not for a plugin. A public "we hide N things" count would
  itself become a probe: the difference between the count now and after a
  configuration change is a measurement of the operator's taste.
- **Each row is an artifact and a coarse inference, never a dimension.** The unit
  is a thing a person could plausibly already know and put together — "standing
  bounties in this fandom pay promptly," "your work matched a standing bounty
  after a curation round" — rather than "your affinity for hurt/comfort is 0.7."
  A9's own framing is "what an observant user could infer," and an observer is a
  person, so a row that a person could state is in scope and a row that only a
  fitted model could state is not.
- **Absence of a row is not a claim of un-inferability.** The view enumerates what
  has been reviewed; it does not certify that nothing else leaks. Presenting the
  list as exhaustive would make it the most leak-prone artifact on the instance.
- **A row carries a disposition, and the default is to keep.** Each reviewed
  artifact records whether it stays as is, is coarsened, or is removed. Removing
  an artifact the operator wants is a policy change and belongs in the same
  proposal machinery as any other (§43), not as a side effect of reading a page.

## 52.2 Payouts are attributed to the instance, and are batched

- **A curation or bounty payout says "the instance paid for this", never "you were
  rated."** The two are the same transaction with different information content:
  the second tells a prober that one rating action occurred at one moment, which
  is a measurement of the lens. This is A9's second proposal and it is the one
  with the widest blast radius, since every standing-bounty and curation payout
  reads from the same path.
- **Payment is batched on a cadence the operator sets, never on the rating
  event.** Batching is not only about payout rows: a payment timestamp within
  seconds of a rating timestamp is itself a correlation an observer can use even
  without payout detail, so the batch interval bounds what an observer can infer
  about *when* as well as *whether*.
- **The batch is attributed to a window, and the window is stated in the
  attribution.** "Paid by the instance in the weekly round" is honest and carries
  no per-event signal; "paid because your work was rated highly" is not.
- **An author can be paid without any payout artifact existing at their
  pseud.** §0.3's boundary holds here too: the payout answers a supply question
  ("this kind of work is wanted") and is not a scoreboard.

## 52.3 The resonance label is owner-visible, coarse, and weekly

- **The owner-visible resonance label updates from a weekly batch, never on
  read or on the underlying event.** A9's third proposal, and the reason it is
  spelled as a batch rather than a delay is that near-real-time feedback turns
  the label into an oracle: an author who watches it tick after each event can
  infer the lens from the tick pattern, and a shared instance makes "watch it
  tick" something any reader can do.
- **The label is a coarse bucket, never a number, and never a score against
  others.** Something in the shape of "quiet," "steady," "noticed" — enough to
  tell an author their work is landing, not enough to rank. A continuous value
  with visible precision is a probe however coarse the units.
- **The label is owner-visible only.** It is never on a public work row, never in
  an API response a reader can see, and never in a plugin's view of a work. §0.3
  forbids the lens being inferable through any user-facing label, and this label
  is derived from the lens.
- **A weekly batch that has not run reports the previous value, and says it is
  stale.** A label that silently updates is worse than one that lags visibly,
  because an author reading it as current will over-read a movement that is three
  days old.

## 52.4 The prohibitions this section does not lift

- **Nothing here makes the taste vector readable.** §16.5's dial stays a dial,
  private taste profiles stay private, and this view is a summary of *artifact*
  risk rather than a dump of the profile that generates it.
- **Coarse is not the same as safe by default.** Bucketing and batching raise the
  cost of a single probe; they do not remove the signal, and the spec does not
  claim they do. §16.5's "do not promise mathematical secrecy" governs this
  section as much as it governs §16.
- **The view is not a compliance audit and does not block publishing.** It exists
  so the operator can decide with the information in front of them; a rule that
  refused to publish an artifact it could not clear would push the operator
  toward hiding the artifact's existence rather than changing it.
