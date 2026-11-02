# 51. Generated content, and who gets credit for what

**Status:** reserved
**Milestone:** —
**Source:** §51 of `docs/spec.md`

M45-12, from gaps review A3. The problem this section exists for is not that
generated fiction exists; it is that **anything paid for posting words can be
farmed with a language model**, and the incentive shape decides which behaviour the
instance pays for.

Three rules, and they are deliberately separable, because an instance that wants
generated fiction on its terms must not have to accept the incentive problems that
usually come with it.

## 51.1 Posture is instance policy, and it has exactly three values

- **Posture is `forbid`, `disclose`, or `allow`,** set by the operator. There is no
  per-work override and no per-author opt-in, because a posture a work can opt out
  of is not a posture — it is a suggestion, and `forbid` would be unenforceable.
- **`forbid` refuses at write time**, before the work row exists. A generated work
  on a forbidding instance never reaches the corpus, so no later process has to
  decide what to do with one.
- **`disclose` accepts and labels.** The work carries a visible generated-content
  marker, shown wherever the work is shown. Disclosure is per work and is the
  author's own statement: the platform does not assert it, because §34 already keeps
  classifiers away from sanctions and §51.3 keeps them away from assertions.
- **`allow` accepts and does not label.** An instance that wants generated fiction
  in its corpus does not have to run a disclosure UI it does not believe in.
- **The posture is recorded with the work, not looked up at read time.** A work
  published under `allow` does not become `disclose`-labelled because the operator
  later tightened the policy, and a `forbid` does not retroactively invalidate work
  already published. Policy changes prospectively; that is what makes it a policy
  rather than a retroactive judgement.
- **`forbid` is the default for a new instance.** `allow` is opt-in because it is
  the only value that changes what the corpus *is*, and an instance should have to
  say so out loud.

## 51.2 Credits vest on reader completion, never on posting

- **Author credit vests when a work reaches completion as judged by distinct trusted
  readers**, not when words are posted. This is the rule that answers A3's farming
  problem: the paid-for action becomes "was this finished and read", which a
  generator cannot farm, because generating the last chapter is not the expensive
  part — getting readers to finish it is.
- **Distinct trusted readers, not distinct accounts.** §33's trust tiers supply the
  set; the count is of *readers*, so one enthusiastic account with many pseuds does
  not vest credit on its own.
- **A credit already vested is not revoked** by later moderation, because the credit
  was for the completion event and not an ongoing endorsement. Revocation has its own
  path — §34's — and does not launder history through this one.
- **Un-vested credit is visible as un-vested** on the author's public profile. A
  reader deciding whether to follow someone should see what has actually been
  confirmed, not a number that could be either.

## 51.3 Attestation, and the line detectors do not cross

- **Bounty claims require an authorship attestation.** The claimant states the work
  is their own writing. The attestation is a statement, not a verification, and it
  is recorded with the claim so the record shows what was asserted.
- **Never sanction on detector output.** §34 already keeps classifiers away from
  sanctions and this repeats it for the most tempting case: a detector is the obvious
  tool here and it is the one tool that must not decide. False positives land on real
  authors, and the sanction is the part that cannot be walked back.
- **Detector output may be shown to a moderator as one input** to a human decision,
  labelled as unverified, and is never persisted on a work as a fact. A stored
  "this is generated" that a later query treats as ground truth is a sanction with a
  delay on it.
- **No work is withheld for being suspected.** `disclose` and `allow` differ only in
  labelling, and neither withholds.

## 51.4 Acceptance

- A new instance's posture is `forbid`; setting it to `allow` is an explicit operator
  action.
- Under `forbid`, a write declaring itself generated is refused and **no work row is
  created**.
- Under `disclose`, the same write succeeds and the marker is present on every
  surface that shows the work.
- Under `allow`, the same write succeeds and no marker is present.
- The posture stored on the work does not change when the operator later changes the
  instance policy; a later write does carry the new posture.
- A generated work authored by someone else is refused under `forbid` and accepted
  under `disclose` — the policy is about the work, not the author.
- Author credit is 0 immediately after publishing and only non-zero once enough
  distinct trusted readers have completed the work.
- Publishing more words does not increase un-vested credit for an already-vested
  author; a second work starts a new credit.
- A bounty claim records its attestation, and a claim without one is refused.
- No sanction anywhere in this system is derived from detector output.

## 51.5 What this section deliberately does not do

- **No AI detector as a gate.** §34's rule and §51.3's: the false-positive rate is
  the argument, and a gate cannot be walked back. The detectors that exist may inform
  a human and nothing else.
- **No per-author disclosure opt-out under `forbid`.** The posture is instance
  policy; an author's preference does not make a forbidden work acceptable.
- **No retroactive policy application.** Works published under an earlier posture
  keep it. Announcing a change and applying it to the past would relabel work its
  author agreed to under different terms.
- **No credit decay or re-vesting.** The completion event is a fact about a moment;
  scoring an author on whether readers *still* finish their work would turn credit
  into a ranking signal, which is §47.7's forbidden thing.
