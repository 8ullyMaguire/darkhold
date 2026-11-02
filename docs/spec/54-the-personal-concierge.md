# 54. The personal concierge

**Status:** reserved
**Milestone:** —
**Source:** §54 of `docs/spec.md`

Gaps review B4, tracked as M45-22.

> **Implementation note (2026-10-03).** Shipped at `baf297d`, tag
> `v0.56-concierge`; the page wired into the router at `08f7ac5`. Plan:
> `docs/plans/m45-22-concierge.md`. Where each piece lives:
>
> | Concern | Where |
> |---|---|
> | Migration | `migrations/*/0114_concierge.sql` — `concierge_sessions`, `wip_watches` |
> | Selector, budget cut, mood vocabulary | `crates/domain/src/concierge.rs` |
> | Persistence | `crates/db/src/concierge_store.rs` |
> | HTTP surface (5 routes, all behind `RequireSession`) | `crates/app/src/routes/concierge.rs` |
> | Candidate generation, shared with discovery | `crates/app/src/routes/discovery.rs::build_candidate_engines` |
> | The page | `frontend/src/routes/Concierge.svelte`, routed at `/concierge` |
>
> Two implementation facts a reader of this section would otherwise have to
> rediscover. **§54.7's parity** holds because both routes call
> `build_candidate_engines` — they did not at first, and the concierge served the
> discovery feed exactly reversed, because `render_queue` used the pluggable
> registry path while the default `rec_mode` is `legacy` and takes `blend()`.
> **§54.6's explained-empty** is the server's own wording, returned verbatim; the
> client has no paraphrase of it.
>
> Not implemented, deliberately: operator affinity and theme gravity, which
> discovery applies and §54 gives the concierge no `sort` parameter to expose them
> through. On an instance with affinities set, the two feeds may legitimately differ.

## 54.1 What this is, and what it is not

A private queue per reader, ranked by predicted enjoyment, with a **session
selector**: the reader states what kind of sitting they want before the queue is
built — "comfort read", "wreck me", "I have 20 minutes". Plus a watch list:
"tell me when this WIP completes" for fics being saved.

It is **not** a new recommender. §16.1a's registry and RRF blend already rank
works; §36.11 already specifies mood-based recommendation and a private mood
journal; §15.8 already fixes the mood vocabulary. This section is a **session
intent** input to that substrate, and it inherits rather than duplicates. Three
things follow from that, and each is a decision rather than an omission:

- **The queue is the blended output under a declared intent, not a separate
  ranking.** A second ranker would be a second thing to keep correct and a second
  place for the §43 ordering contract to be forgotten.
- **Mood is the only intent vocabulary in v1** (§54.2), because §15.8's list is
  curated and §36.11's journal already populates it. A reader who wants
  "comfort" has a vocabulary for it; a reader who wants "something that will make
  me cry" has §36.11's custom labels.
- **Time is a constraint on the queue's tail, never on its ranking.** Ranking by
  predicted enjoyment and *then* cutting to a budget is the only ordering that
  keeps "I have 20 minutes" from silently meaning "the algorithm now thinks short
  things are better" (§54.4).

## 54.2 The three selectors, and what each one actually constrains

| Selector | Reader says | It constrains | It does **not** constrain |
|---|---|---|---|
| Mood | a mood from §15.8, or a §36.11 custom label | the candidate set — works must carry that mood | the order within the set |
| Time | minutes they have | the tail — the queue is cut at the budget (§54.4) | which works are eligible |
| Both | a mood *and* a budget | candidate set, then tail | — |

A selector that filtered on order as well as set membership would be a second
opinion wearing a selector's clothes: "wreck me" and "comfort read" would return
disjoint rankings rather than one ranking under two stated intents, and the
reader could not tell which of the two they were looking at.

**Unrecognised mood names are refused, not guessed.** A session naming a mood no
node carries is a typo or a stale client, and quietly falling back to the
unfiltered queue would answer a different question than the one asked. The
response names the moods the taxonomy actually has.

## 54.3 The session is recorded, because "what was I in the mood for" is the whole point

Each queue render records the selector that produced it: the mood, the budget,
and the works returned. This is **private to the reader**, like §36.11's journal
and for the same reason — it is a record of the reader's own state.

It exists because §50's reason transparency says a reader is entitled to truth
about their own feed, and "these three works because you said comfort, on a
Tuesday, with 20 minutes" is the useful half of that. It is **not** a ranking
signal (§0.3): a session must never make one work outrank another in §16's blend.

Retention follows §11.15's `cache | aggregate` split: session rows are `cache`
and are dropped on the instance's own schedule. A session log is a record of
intention, not of reading, and nothing derives from it after the window closes.

## 54.4 Time budget: cut the tail, never re-rank

Given a budget of *T* minutes and a reader whose reading speed is known from
§36.11's progress sync, each candidate carries an estimated duration. The queue
is the blended ranking, accumulated in order, cut where the cumulative estimate
crosses *T*.

Three failure modes are named because each is easy to ship by accident:

- **A work with no duration estimate is not excluded.** It is placed at the
  natural midpoint of the queue's estimate range and marked
  `duration_unknown`. Dropping it would make the budget silently depend on how
  complete the archive's metadata is, and an archive with thin metadata would
  serve a shorter queue than a rich one for the same reader.
- **The cut is a prefix, so the queue is deterministic.** Two renders of the same
  session must produce the same list, or "the 20-minute queue" is not a thing the
  reader can have an opinion about.
- **The reader is told what was cut and why.** `truncated_at` names the index the
  budget bound at and `estimated_minutes` the total it was bound to. A queue that
  silently stops is indistinguishable from a queue that ran out of good matches.

If no reading-speed observation exists, the budget applies against the work's own
word count — which lives on `chapter_revisions`, aggregated per work the same way
the arena pool query does — at the default rate, and the response says which basis
it used. Guessing a personal rate is not available when there is no observation,
and inventing one would make the same request mean different things on a reader's
first day and their fortieth.

## 54.5 WIP completion watch

A reader may watch a WIP. When the work's status changes to complete, the reader
is notified once, through §14's existing notification door.

- **One notification per watch, ever.** Not per completion event: a work can be
  un-finished and re-finished by its author, and a reader who asked to be told
  does not want to be told twice.
- **The watch is cancelled by the notification**, not left to re-fire. An
  unrecognised completion must not leave a watch that will fire on the next
  unrelated edit.
- **A watch on a work that is already complete resolves immediately**, at watch
  time, with the same body as a later notification. Otherwise "watch this finished
  fic" appears to work and tells the reader nothing.
- **Withdrawing a watch is silent.** No notification says a watch was removed.

## 54.6 Invariants

- **The concierge never reads another reader's data.** Every candidate is scored
  from the requesting reader's own signals plus work-level metadata. A session is
  private in both directions: it exposes nothing about the reader, and it is
  built from nothing but their own history.
- **An empty intent is not an error.** With no selector the queue is the plain
  §16 blend, which is what `GET /discovery` already returns. The session layer
  must not be able to make the default path worse.
- **A selector that matches nothing yields an explained empty queue, not a
  fallback.** "No works carry `catharsis` and you have not read the others" is a
  true answer; the unfiltered queue is a different one.
- **The queue is a read.** Rendering one creates a session row (§54.3), which is
  a write to the reader's own private store and nothing else. It never writes a
  work, a rating, or an interaction that any other reader or the operator can see.
- **§16.1a's ordering contract applies after the blend, identically.** §16.4's
  diversity budget and §43's ordering run on the cut list exactly as they would
  on the uncut one — cutting first would let the budget select what the diversity
  rules then never see.

## 54.7 Acceptance

- A reader with no selector receives the same works as `GET /discovery`, in the
  same order.
- A mood session returns only works carrying that mood, and every returned work
  names the mood that selected it (§50's `reason` field).
- A mood the taxonomy does not carry is refused by name, and the response lists
  the moods it does carry.
- A 20-minute session returns a prefix of the blended ranking, and
  `truncated_at` names the cut index while `estimated_minutes` names the budget
  used.
- Two renders of one session with no intervening writes return identical lists.
- A work with no duration estimate appears in the queue, marked
  `duration_unknown`, rather than being dropped.
- A session with no reading-speed observation states that the default rate was
  used.
- A reader watching a WIP receives exactly one notification when it completes,
  and the watch is no longer active afterwards.
- Watching an already-complete work notifies immediately.
- A withdrawn watch produces no notification.
- An unauthenticated request is refused with 401 on every route in this section.
- No row written by this section is visible to another reader, to the operator's
  economy dashboard (§53), or in any §20.3 author-multiplier input.

## 54.8 What this section deliberately does not do

- **No new ranking algorithm.** The blend is §16.1a's. A concierge-specific
  ranker would be a second source of ordering truth (§54.1).
- **No collaborative filtering on sessions.** "Readers who asked for comfort also
  read" is instance-level aggregation of private intent, and §0.3 plus §52's
  leakage rules are the reason not to. The operator gets no count of it either.
- **No mood inference.** Only moods the reader chose (§36.11) or an author
  assigned (§15.8). Inferring "you probably want catharsis" from reading history
  is a prediction about a person's interior life derived from private data, and
  this section declines to do it.
- **No "surprise me".** §16.10 already owns surprise, and folding it in here
  would give one selector two incompatible meanings.
- **No session sharing.** Two readers cannot share a concierge queue in v1; the
  session is an instruction to the ranker, not an artefact.

---
