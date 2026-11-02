# 43. Recommendation-first browsing — one ordering contract for every surface

**Status:** reserved
**Milestone:** —
**Source:** §43 of `docs/spec.md`

> **2026-09-21 addition.** §16 defines the engines, §16.5 promises the reader's dial applies to
> "all surfaces", and §16's acceptance says exact and chronological sorts stay exact — but no
> section ever named the surfaces or defined how a browse page chooses an order. In practice each
> route author chose, and this section says otherwise: a reader who has told the instance what they
> like gets that taken into account *wherever they browse*, and every other ordering remains one tap
> away. This section is the contract; §16 is the mechanism.

## 43.1 Surfaces

Every one of these takes the §43.2 vocabulary, and each has a configured default:

`/discover`, `/fandoms/:id`, `/tags/:tag`, `/moods/:mood`, `/people`, `/authors/:id`,
`/collections/:id`, `/series/:id`, `/reading-paths/:id` (§18.9), the discovery rails inside
`/library`, directory lists (§39), the challenge, request and wishlist boards (§18.2, §18.5), and
the e-mail digest.

A surface added later inherits this contract rather than inventing an order; §26's route ownership
table names the owning module, and a new surface without a `sort` parameter is a defect.

## 43.2 The vocabulary

One enum, everywhere, documented and translated once:

| `sort` | Meaning | Exactness |
|---|---|---|
| `for-you` | the §16 engine blend and the reader's recipe | a permutation (see 43.3) |
| `new` | publication or update event order, from the §4.3 event log rather than a row timestamp | exact |
| `updated` | last publication event | exact |
| `top` | §15.10's quality signals: completion rate, positive-feedback ratio, recency and update velocity, bibliography quality | exact, never taste-steered |
| `trending` | time-windowed with §16.14's decay | exact |
| `best-match` | literal query relevance; search only | **never taste-steered (§15.10)** |
| `az` | alphabetical; people, tags, collections | exact |

An unknown value is a validation error naming the accepted set, never a silent fallback.

## 43.3 `for-you` is a permutation, never a filter

The reachable set for a given filter is **identical under every `sort` value**. `for-you` changes
the order of results and the page they appear on; it never removes one, never truncates the last
page, and never changes a count. Pagination must be able to walk to the final item of the
underlying set under `for-you` exactly as it can under `new`.

This is the section's load-bearing rule. A taste-skewed default that quietly reduces reachability is
a shadowban (§19.7) applied by ranking instead of by moderation — invisible by construction, because
§0.3 forbids explaining the order, and unaccountable for the same reason. A test asserts set
equality: for one fixture, the union of pages under `for-you` equals the union under `new`, in both
directions, including for a work `for-you` ranks last.

## 43.4 Defaults

- **Signed in with a profile:** `for-you`, unless the operator configures otherwise per surface.
- **Signed in with no profile:** `for-you` must be indistinguishable from the baseline order. §16's
  acceptance already requires the fallback; this makes it observable.
- **Anonymous:** the surface's neutral order (`top`, `new` or `trending`). `for-you` is refused for
  anonymous traffic while the instance's declared topics are empty or all `public = false`, because
  the *ordering itself* would publish a private theme on the front page (§0.4.3, §16.16.2). With at
  least one public topic, a taste-skewed default for anonymous readers is permitted and the
  mechanism may be documented.
- **Stickiness:** the choice is per-pseud and remembered across sessions, mirroring §16.8's
  per-pseud dashboard layouts. It is a *sort choice*, not a dial — §16.5's weight remains the only
  influence control, and there is no per-surface strength setting.

## 43.5 Composition rules

A `for-you` page is assembled in the order the implementation already uses for `/discovery`
(`crates/app/src/routes/discovery.rs`): shared candidate fetch → blend → silent reordering
(half-life §41.1, operator affinity §16.3) → diversity reservations → paginate. Consequences:

- **Rank after the shared fetch.** Per-reader ordering is never part of a shared cache key; the
  candidate pool stays cacheable and only the ordering is per-reader (§10.4's cache boundaries).
- **§16.4's diversity budget applies per surface**, not only on `/discover`. A reservation that
  holds on one page and not the next just moves the collapse it prevents.
- **Every candidate still passes §16.1's shared eligibility rules.** Ordering is the last step, not
  a bypass.
- **The `reason` field travels with `for-you` items** (§16.1) and obeys §16.16.2: a reason may name
  the reader's own terms, and instance terms only for a topic declared `public = true`.

## 43.6 Measurement

The operator's discovery page reports, per surface and per period: what `for-you` surfaced, what
each explicit sort surfaced, the overlap between them, and the share of surfaced items that had
never been surfaced before. This is §16.13's "the slot's effect is measured and reported" extended
to every browse page, and it is the only way an operator can see `for-you` collapse into the same
forty works on every page. Measurements are aggregate; no per-reader ordering history is exposed to
anyone, including the operator (§3.7).

## 43.7 Acceptance

- Every §43.1 surface accepts the §43.2 vocabulary and answers an unknown value with a named error.
- Under two different `sort` values, the reachable set for one filter is identical, counts included.
- `for-you` with no profile matches the baseline order exactly.
- An anonymous reader on an instance with no public topic is never served a taste-skewed default,
  and no reason field names a theme term on any anonymous page.
- An exact or literal query is not reordered by any profile, at any weight (§15.10).
- The sort choice survives a new session and does not leak across pseuds.
- Per-surface diversity reservations hold under the §25.4 load profile.
- A `for-you` page lands within the timing budget of the same surface's `new` page; ranking happens
  once per page over a bounded candidate pool, not once per item.

## 43.8 What this section deliberately does not do

- **No taste-steering of exact search.** §15.10's rule is untouched; `for-you` is an ordering among
  eligible results, and a literal query is not a recommendation request.
- **No per-surface influence dial.** One dial (§16.5) governs instance influence; this section
  governs display order only.
- **No new privacy setting.** Defaults follow §0.4.3's existing topic-visibility rule. This section
  introduces no separate "recommendations on browse pages" consent, because the reader's existing
  dial and sort choice already cover it.
- **No reordering of a path, series or curated list.** §18.9 is explicit that a path never reorders
  its stops: `for-you` orders the *list of paths*, never the contents of one.
- **No change to `top`.** The quality ordering stays as §15.10 defines it; `for-you` sits beside it
  rather than replacing it.


---
