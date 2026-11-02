# 39. Resource Directory — a community-curated map of the fandom ecosystem

**Status:** reserved
**Milestone:** —
**Source:** §39 of `docs/spec.md`

> **2026-09-21 addition.** Darkhold is one node in a fandom ecosystem that
> spans archives, Discord servers, author platforms, writing tools and
> communities. Readers arriving at a fresh instance have no map of that
> ecosystem; this section gives them one, ranked by the people who use it.
> The design borrows the shape of a well-known "list-of-sites" directory:
> many categories, one ranked list per category, ranked by users — but every
> ranking input is a named community vote, never traffic, never money, and
> never the administrator's private taste (§0.3).

## 39.1 Scope — lists and their entries

The surface is a set of **curated lists**. A list is a ranked collection of
**entries**, and an entry is one of two kinds:

- **External resources** — things that do not live on this instance: fanfiction
  archives, Discord servers, author platforms and homepages, writing tools,
  communities (subreddits, Tumblr tags, forums), podcasts and newsletters, and
  other Darkhold instances (which also appear in the §36.15 instance
  directory; a list entry links to that record rather than duplicating its
  stats).
- **Internal references** — works, authors (pseuds), tags and fandoms **on this
  instance**. An internal entry stores the referenced id, not a copy: a work
  entry renders from the live work row, so a list never goes stale and never
  widens what the reference alone may show (an ineligible work is refused at
  submission, and an entry whose work later becomes private is hidden from
  everyone but its curator until it is public again).

Every list has a **kind** — `external`, `works`, `authors`, `mixed` — and the
kind gates what an entry may reference: a `works` list accepts only work ids,
an `authors` list only pseud ids, `mixed` and `external` accept any external
resource (internal references only in `mixed`). Categories from §39.2 apply
to external entries inside any list.

**Instance lists.** Any list may be marked by the operator as an **instance
list** — the curated face of this instance, shown on the landing page and in
the site header ("Start here", "This month's picks", "The archives we read").
Instance lists are the operator's editorial surface: the operator picks which
lists represent the instance and in what order; the ranking inside each list
is still the community's votes (§39.4). An instance list is read-only to
everyone but its curator and the operator.

## 39.2 The surface

`/directory` with:

- **Category tabs**, one ranked list per category, ordered by score
  descending, ties broken by submission date (older first).
- **Entry cards**: title (linked), one-line description, tags, score, vote
  controls, submitter handle, submission date. An entry the viewer has already
  voted on shows its state.
- **Search** across title, description and tags, per category or global.
- **Tag filter**: clicking a tag narrows the list to entries carrying it.
- **Pagination** with a page size set by `[directory].page_size` (default 50).

Categories are seeded (`fanfiction_archive`, `discord_server`,
`author_platform`, `writing_tool`, `community`, `podcast_newsletter`,
`darkhold_instance`, `other`) and the operator may add more through
`[directory].extra_categories` — the self-hosted contract (§38) applies: the
category list is instance configuration, not code.

## 39.3 Submission and review

Any signed-in account may submit an entry: category, title, URL, description
(≤ 500 characters), optional tags. The URL must be absolute http(s) and must
not resolve to a private or loopback address (the same SSRF rules the
§14.9 webhook verifier follows).

Review is operator-configurable:

- `[directory].require_approval = true` (default): a submission is **pending**
  and invisible to everyone except its submitter and the operator until the
  operator approves it. Approval is one click from the operator's queue and
  is recorded with approver and timestamp.
- `require_approval = false`: submissions are visible immediately. The
  operator may still remove an entry, and removal is recorded.

An entry may be edited by its submitter while pending; after approval, only
the operator may edit or remove. Removal never cascades to votes — a removed
entry's rows stay for audit and a resubmission of the same URL starts at
zero.

## 39.4 Voting and ranking

One vote per account per entry, `+1` or `-1`, toggleable (voting the same
value again removes the vote; voting the other value flips it). The score is
the sum of live weighted votes, denormalised onto the entry row and
recomputed inside the same transaction as the vote so the list never shows a
stale score.

**Vote weight.** A vote carries a weight — the directory is the instance's
curated front door, and whose recommendation counts more is operator policy:

```text
vote_weight = trust_multiplier(voter_trust_level)
            × taste_multiplier(voter_admin_taste_affinity)
```

- **Trust multiplier** scales with the §19.1 ladder. Defaults:
  TL0 0.5, TL1 0.75, TL2 1.0, TL3 1.25, TL4 1.5, TL5 1.75, TL6 2.0 —
  a reviewed trusted regular's recommendation counts for more than a
  day-old account's, and the operator can retune every rung through
  `[directory].trust_vote_weights` (seven comma-separated multipliers).
- **Taste multiplier** scales with the voter's affinity to the
  administrator's taste profile — the same `aggregate_affinities` signal the
  §9.7.4 demand multiplier uses, applied to a voter instead of a work.
  Affinity 0 maps to `[directory].taste_vote_floor` (default 0.75) and
  affinity 1 maps to `[directory].taste_vote_ceiling` (default 1.25).

Weighting is operator-configurable: `[directory].vote_weighting` is one of
`flat` (every vote weighs 1 — the weightless directory), `trust` (trust
multiplier only), `trust_and_taste` (both; the default). `flat` is always
available as the off switch.

**Silence is the contract (§0.3).** Weights are applied, never shown: no
voter list, no per-voter attribution, no weight breakdown, no user-facing
label that taste influenced anything. A reader sees a score; nothing about
the score reveals the administrator's taste profile or any individual's
affinity to it. The same rule ratings follow (§33.2) applies to the
individual vote; the same rule the demand multiplier follows (§9.7.4)
applies to the taste signal.

A vote on a directory entry is public in the aggregate (the score) and
private in the individual. Weight recomputation is triggered by trust-level
changes and nightly taste-refresh jobs, folded into the denormalised score
in the same transaction as the change.

## 39.5 Acceptance

- An anonymous reader sees only approved entries, ranked by score, and can
  search and filter by category and tag.
- A signed-in reader can submit an entry, and with `require_approval = true`
  sees their own pending entry with a visible "pending review" state.
- Voting twice with the same value removes the vote; voting the other value
  flips it; the score the list shows reflects the change immediately.
- With `vote_weighting = "trust_and_taste"`, a TL4 vote moves the score more
  than a TL0 vote on the same entry; with `vote_weighting = "flat"` they move
  it equally.
- No surface, API response or error message reveals a voter's weight, the
  taste component of any score, or the administrator's affinity to any
  voter.
- A URL that is not absolute http(s), or that points at a loopback or private
  address, is refused with a named reason.
- The operator's queue lists pending entries with submitter and date;
  approval makes the entry visible to everyone in the same request.
- A category added through `[directory].extra_categories` appears on the
  surface after a restart with no code change.

## 39.6 What this section deliberately does not do

- **No paid placement, no affiliate links, no sponsored entries.** Money
  never touches the directory (§0.3).
- **No weight disclosure.** Vote weights — trust or taste — are never
  surfaced, exported or explained per voter (§0.3).
- **No traffic or uptime probing.** The instance does not fetch listed URLs
  on a schedule; a dead link is reported by users, not detected by bots.
- **No federation of the directory itself.** Entries are local to this
  instance. Sharing curated lists between instances is a future concern and
  would ride §36.15's discovery records, not this table.
- **No per-entry comment threads.** The forum (§17) is where discussion
  belongs; an entry links out, it does not host.

---
