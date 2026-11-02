# 56. WIP adoption, closure notes, and lineage

**Status:** reserved
**Milestone:** —
**Source:** §56 of `docs/spec.md`

Gaps review C4. A work whose author stopped is the archive's most common failure, and
today §8.8 *reports* the fact (`active | slow | dormant | concluded`) while leaving the
reader and the adopting author with nothing to act on. This section supplies the action.

Three clauses, in the order they matter. The first is a promise; the second is a mechanism;
the third is a prohibition.

## 56.1 An adoption offer is an ask, never a transfer

A work marked `abandoned` (§8.2) may be offered for adoption. An adopting author takes on
*authorship of the next chapter*; they never take ownership of the original author's work.

- **Ownership does not move.** `works.owner_pseud_id` is unchanged by adoption, permanently
  and irreversibly. The adopting pseud is recorded as a *contributor* on subsequent
  revisions, never as an owner.
- The original author may withdraw an adoption offer at any time, with no notice period.
- Adoption requires the adopting pseud to hold the trust level that publishing requires
  (§19.4). Adoption is a publishing act and is gated identically. **This tree has no publish
  route and no named level to borrow** — `trust_levels` is `(account, level, computed_at,
  basis)` with no seed, and `Tier` is a *reading* tier — so the bar is configured as
  `[adoption] min_trust_level` (default `2`, one above the level a freshly registered account
  starts at). The comparison lives in `domain::wip_adoption::may_adopt`, the refusal reports
  **both** the bar and the caller's level, and an operator who moves the bar sees the change in
  the answer rather than in a silence.
- Only `abandoned` is offerable. `in_progress` is a work still being written, `hiatus` is an
  author who means to come back, and `complete` has nothing to continue; all three answer
  `NOT_ABANDONED` with the field named. (Measured: the first test cut covered only `complete`,
  and the mutation that let `in_progress` through survived it.)
- One **open** offer per work, and a second offer may follow a withdrawn one. The mechanism is
  the partial unique index `(work_id) WHERE status = 'open'`, not a check in the handler: a
  plain unique index would forbid the author changing their mind twice.
- An offer belongs to the work's owner. A reader who is not the owner gets the same `404` an
  absent work gets, because confirming the existence of somebody else's unlisted WIP is the
  thing a refusal must not do.

## 56.2 Closure notes are first-class and credited

When an author concludes a work — whether completing it or marking it `abandoned` — they
may publish a **closure note**: a short statement of what happened to the story and why.

- The note is **versioned with the work**, not with the author, and is revision-restorable
  like any other revision (§8.5).
- The note is credited: its author pseudonym is shown, and a closure note earns the same
  credit as a chapter. It is not "metadata". **Deferred in this build, by name:** nothing in
  this tree awards per-chapter credit either, so "the same credit as a chapter" would be a
  number with nothing to be equal to. The note is stored with its author (`M45-28`), the
  economy is the missing half, and the acceptance bullet below is unmet until it lands.
- **A closure note is optional and never a precondition.** A work may conclude with no
  note. This is stated because the obvious failure is a publish flow that refuses to
  conclude a work until the author has written a note, which turns a courtesy into a tax.
- A closure note may be edited by its author afterwards; each edit is a new revision.

## 56.3 Lineage is a fact about revisions, not a story about people

When a work changes hands, who wrote what is a question the archive must answer exactly.

- Every revision records `created_by_pseud_id`, and lineage is *derived* from it — never
  hand-written, and there is no "lineage" field an author can fill in. The tables are `revisions`
  joined to `units` (0122 renamed `chapter_revisions` and `chapters`; §5's vocabulary is the
  current one, and this section's wording about "chapters" is about the act, not the table).
- When those tables are absent from an instance's schema, the door answers
  `{contributors: [], available: false, reason}` rather than a `500` about a missing relation.
  A schema that cannot be read is a fact about the instance, and a reader can render it.
- A work's reader-facing contributor list is ordered by **first contribution**, so an
  adopting author does not leapfrog the person who wrote chapter one.
- **Lineage never affects ranking, credits, awards or trust.** §8.8 says derived activity
  status is information and never a penalty; the same rule governs who contributed.
  A work adopted by a stranger is the same work, and the adopting author's contributions
  earn exactly what the original author's did.

## 56.4 What this deliberately does not do

- **No automatic abandonment.** Nothing marks a work `abandoned` without the author or an
  operator doing it. §8.8's derived `dormant` is a *reading*, not a state change.
- **No transfer of ownership, ever.** See 56.1. There is no route that can move
  `owner_pseud_id` because of adoption.
- **No paywall on the archive.** An adopting author is not compensated by a share of
  anything; §0.3 and the standing rule against bought trust.
- **No lineage ranking.** See 56.3.

## Acceptance

- Adopting a work does not change `works.owner_pseud_id`, and the test asserts the column
  value before and after.
- An author can withdraw an open adoption offer, and the offer is then not listable.
- Adoption is refused below the publishing trust level, and the refusal names the bar.
- A work with `completion = 'complete'` cannot be adopted.
- A work may conclude with **no** closure note; the conclude path succeeds without one.
- A closure note's author is shown, and it earns credit equal to a chapter. **(Unmet in this
  build: the note is credited to its author, the economy that would pay a chapter does not exist
  yet — see 56.2. Recorded as unmet rather than ticked.)**
- An offer on a work that is `in_progress` or `hiatus` is refused, naming `NOT_ABANDONED`.
- A second offer after a withdrawal succeeds, and two open offers on one work cannot coexist.
- The contributor door answers `available: false` with a reason on a schema without `revisions`,
  and ordered-by-first-contribution on one with them.
- Editing a closure note creates a new revision; restoring it creates another.
- The contributor list is ordered by first contribution, so an adopter is not first for a
  work they joined after chapter one.
- Two works adopted by the same pseud rank identically in search and recommendations,
  which is what 56.3's "lineage never affects ranking" means in a test.

## Prior art already in the schema

`works.completion` already carries `abandoned` and `hiatus` (§8.2), and
`works.owner_pseud_id` already belongs to the *pseud* (ADR 0003, §8's "pseud switching does
not change ownership"). 56.1 follows from the schema rather than adding to it.

---
