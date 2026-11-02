# 48. The Firefox companion — a browser extension over `/api/v1`

**Status:** reserved
**Milestone:** —
**Source:** §48 of `docs/spec.md`

> **2026-09-30 addition.** §7.11 promised a browser extension. This section is
> that promise, made concrete: what the extension may do, what it must not do,
> and where the line between the client and the server sits.
>
> **Forked from `github.com/8ullyMaguire/ao3-reading-companion`** (MIT, 4 commits,
> ~17.5k lines, 41 passing tests), a Firefox MV3 extension that scores AO3 fics
> on-device. The architecture is reused; the recommender is not. The reasoning is
> §48.2 and it is not a stylistic preference.
>
> Separate repo: `~/code-local/rust/darkhold-companion`. This section specifies
> the **contract between the two repos** — the API surface the extension depends
> on and the guarantees it relies on. The extension's own spec lives in its repo.
> Its implementation plan is `darkhold-companion/docs/PLAN.md`, forked from
> upstream `8753a64`.

## 48.1 What is being forked, and what is not

The upstream repo is a self-contained AO3 recommender. Adopting it wholesale would
give Darkhold two rankers that disagree, so the split is explicit:

| Upstream component | Fate | Reason |
|---|---|---|
| `manifest.json` MV3 + gecko shape | **keep, retarget** | Firefox-only was already the case |
| `feed.html/js/css` (2882 lines) | **keep, rewrite data layer** | the feed UI is the point of the fork |
| `myreading.*` (874 lines) | **keep, retarget** | reading-list UI maps onto §9 |
| `preferences.*`, `onboarding.*`, `popup.*` | **keep, retarget** | settings UI maps onto §46 |
| `parsers.js` (447) | **replace** | parses AO3 DOM; Darkhold has JSON |
| `scoring.js` (407) | **delete** | §48.2 |
| `db.js` taste/affinity stores (552) | **delete** | §48.2 |
| `dislike.js` (1021) | **delete** | content filters are server-side (§30.7) |

Row counts verified against the fork at upstream `main`: `feed.js` 2882,
`myreading.js` 874, `preferences.js` 1825 + `onboarding.js` 744, `scoring.js` 407,
`db.js` 552, `dislike.js` 1021, `parsers.js` 447 — 17,575 lines across 25 files,
41 tests passing under `node --test`.
| `test/` (41 tests) | **port the shape, rewrite the cases** | see below |
| `docs/SPEC-integrate-kindred.md` | **read, do not carry** | describes AO3 integration |

Deleted code is deleted on purpose and this section says why, so a later session
does not "restore" it as a missing feature.

## 48.2 The server is the only ranker

**The extension never scores, ranks, or holds a taste profile.**

The upstream design ranks on-device: an IndexedDB store of preferred tags with
user-set weights, author affinity accumulated from completed reads, and **seven**
hand-tuned feed rows — `similar_to_tastes`, `discover_new`, `popular`,
`hidden_gems`, `your_authors`, `completed_long_reads`, `fresh_chapters`
(`scoring.js:19`) — each with its own jitter constant. That is a good design *for a
site with no ranking of its own*. Darkhold has one — §47, built 2026-09-30 — and
having a second one is not a feature:

- **The feeds would disagree.** §47.9 requires `rank_works` to return a
  byte-identical ordering for the same database state. An on-device scorer makes
  that untestable: the extension's order is a function of a local IndexedDB the
  server cannot see.
- **Propensities would be lost.** §47.3's propensity is the probability the reader
  *would* have been shown the work, and inverse-propensity scoring (M45-13) divides
  by it. An impression the extension invents has no defensible propensity, so it
  cannot enter the offline evaluation at all — it would have to be discarded, and
  a discarded impression is worse than none because it looks like data.
- **Incentives would be undiscriminable.** §47.4 requires an interaction to be
  `earned` or `incentivized` *when it is recorded*. A read the extension reports
  later, out of band, cannot carry that classification truthfully.

So the extension is a **surface**, not a brain. It renders what `/api/v1` returns
and reports what the reader did. This is the same reasoning as §33.3's decision to
record a slot's reasons at serve time rather than recompute them: "a recomputed
explanation would be a plausible-looking lie."

**What this costs, stated plainly.** The upstream extension's main appeal is that
it needs no server and shares nothing. A Darkhold reader using this extension must
be signed in to a Darkhold instance. That is a real loss of the upstream
property, and §48.9 does not pretend otherwise.

## 48.3 The API surface it depends on

The extension calls `/api/v1` only, with the reader's session cookie
(`credentials: "include"`). Routes below were read out of the tree on 2026-09-30,
not assumed — and one of them is not what the feature needs, which is a finding
rather than a typo:

| Capability | Route as it exists today | State |
|---|---|---|
| Recommended feed | `GET /api/v1/discovery` | exists |
| Feed with reasons | `GET /api/v1/discovery` has no `explain` param | **does not exist** — see below |
| Reading history | `GET /api/v1/library/history`, `DELETE /api/v1/library/history/{id}` | exists |
| Reading notes | `GET/PUT /api/v1/notes`, `DELETE /api/v1/notes/{id}` | exists |
| Library / works | `GET /api/v1/works`, `GET /api/v1/works/{id}` | exists |
| Kudos | `POST /api/v1/works/{id}/kudos` | exists |
| Reader's taste vector | `GET /api/v1/discovery/taste-profile/me` | exists |
| Content filter state | `GET /api/v1/me/dnf`, `GET /api/v1/works/{id}/dnf/reasons` | display only (§48.4) |
| Streak | `GET /api/v1/me/streak` | exists |

**`GET /api/v1/discovery` does not take an `explain` parameter and does not return
§33.3's `SlotExplanation`.** §48.7 claims the upstream "why this?" breakdown fits
that shape; the *shape* does, but the endpoint does not yet produce it. There is
`/api/v1/operator/rec/shadow` and the `recommendation_slots` table behind it, so
the data exists — but what an ordinary reader may see about their own feed is
`/discovery/taste-profile/me`, which is a taste vector, not a per-item reason.

So the feed's "why this?" is **blocked on a server route that does not exist**, and
that is stated here rather than discovered during the port. Two ways out, and this
section does not choose:

- extend `GET /api/v1/discovery` to return per-item reasons from
  `recommendation_slots` for the requesting reader — the honest fix, and it makes
  §33.3's transparency apply to the extension too;
- ship the feed without per-item reasons and say so in the UI, which is a weaker
  product and a smaller change.

**Every route must be reachable with the reader's cookie and nothing else.** No
extension-specific API key, no separate token store. A browser extension that
carries its own long-lived credential is a credential-exfiltration surface, and
Darkhold's session model already has the right answer.

## 48.4 What the extension must not do

- **It must not filter.** Content filtering is §30.7 and lives on the server, with
  the filter applying at every stage including exposure-floor impressions (§47.5).
  An extension-side filter is a second, weaker implementation of a safety
  property, and it silently breaks the moment the extension is not installed.
  `dislike.js` is deleted for this reason.
- **It must not hold a taste profile.** §0.3: taste is inferred, never declared.
  A local weight store is a declaration, and §0.3 is a ban, not a preference.
- **It must not send page contents anywhere.** No scraping works back to Darkhold
  as bodies — §47.5's floor exists precisely so the *server* decides what a reader
  is shown. An extension that reads a body the server did not offer re-opens the
  import-visibility question (M45-53).
- **It must not reimplement ranking.** §48.2.

## 48.5 Offline and multi-instance

Upstream is fully offline. This one is not, and the difference is stated in the UI
rather than discovered:

- **Offline shows the last cached feed, labelled as cached, with the fetch time.**
  A stale feed presented as current is a lie about the ranking, and §33.3 already
  established that a plausible-looking recomputation is worse than an admitted
  gap. Cache is a browser `storage` value, TTL one hour, never written to
  IndexedDB as if it were state.
- **Multiple instances are first-class**, because an instance is a person's
  community (§38.6: instance config is global, and settings are account data).
  The extension is configured with one base URL per browser profile and *refuses*
  to send credentials anywhere else. No "current tab's host" inference — a reader
  with a Darkhold tab open should not have their session cookie offered to it.
  The base URL is shown in the popup at all times, because an extension that talks
  to a server is one the reader should be able to see the name of.

## 48.6 Attribution and licence

Upstream is MIT, `Copyright (c) 2026 DisastrousClass`, published at
`github.com/8ullyMaguire/ao3-reading-companion`. The fork:

- keeps the upstream `LICENSE` **verbatim**, including the copyright line;
- adds `NOTICE` crediting both the copyright holder and the repository owner,
  because they are not the same name and the difference is unresolved;
- records the upstream commit in the fork's README, so the provenance is a
  fact rather than a claim.

The discrepancy between the copyright holder and the repo owner is **not** ours to
resolve and is flagged rather than quietly normalised. Confirmation is a
precondition of *release*, not of development: the code is MIT-licensed either
way, and stopping work over an attribution question would be the wrong trade.

## 48.7 What the fork gives that a from-scratch extension would not

- A working MV3 Firefox manifest with `data_collection_permissions` already set to
  `["none"]` — a claim Darkhold can keep honest because the extension now talks
  only to the reader's own instance.
- A feed UI with a per-item "why this?" breakdown already rendered
  (`buildMatchStrengthLabel`, feed.js:618), which §33.3's `SlotExplanation` shape
  fits directly.
- A settings and onboarding flow, 2569 lines between them, with the shape of a
  first-run taste calibration already in place — which maps onto Darkhold's own
  arena (§45-56) rather than inventing a second calibration.
- 41 tests and a `tools/check-read-filter.mjs` harness, i.e. a test *approach*
  already validated on this codebase's problem.

## 48.8 Invariants

- **The extension cannot produce a ranking the server did not produce.** No local
  ordering path exists in the forked code; `scoring.js` and the taste stores are
  absent, and a test asserts they stay absent.
- **Every reader action the extension reports is attributable to a §47 impression.**
  Actions without one are dropped, not guessed at.
- **The instance base URL is displayed and is the only host credentials go to.**
- **Cached content is labelled cached, with its age.** Never presented as live.
- **No long-lived credential is stored by the extension.** Session cookie only.

## 48.9 Acceptance

- The extension installs on Firefox 115+ as a temporary add-on and its feed
  renders from a live Darkhold instance.
- With the server unreachable, the feed shows the last cached copy, labelled, with
  its age — and does not present it as live.
- A work the reader's content filter excludes does not appear in the extension's
  feed, including in the cached copy.
- The extension issues no request to any host other than the configured instance.
- `grep` for `scoring.js`'s exported names and the IndexedDB taste store names
  returns nothing in the forked tree: the deletion in §48.1 is real, not a
  commented-out block.
- The upstream test approach is preserved: the fork's own tests run with
  `node --test`, with no build step and no runtime dependencies, as upstream has.

## 48.10 What this section deliberately does not do

- **No Chromium/Chrome build.** Upstream is `browser_specific_settings.gecko`
  only, and §48.1 keeps it that way. Adding a second browser doubles the testing
  matrix for a feature nobody has asked for. If it is wanted, it is a section
  amendment.
- **No cross-origin federation of extension state.** §48.5's multi-instance
  support means *one profile, one instance, chosen*. Exchanging recommendation
  state between instances would be §11.17 territory and needs its own section.
- **No AO3 support retained.** The fork exists to serve Darkhold. Keeping
  `parsers.js` and the AO3 host permission would mean shipping two products in one
  add-on and two privacy stories in one popup.
