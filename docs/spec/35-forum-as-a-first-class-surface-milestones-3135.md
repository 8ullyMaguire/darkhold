# 35. Forum as a First-Class Surface (Milestones 31–35, repo)

**Status:** reserved
**Milestone:** —
**Source:** §35 of `docs/spec.md`

The forum is not a feature bolted onto a fiction site; it is a first-class
surface of the creative ecosystem. Two design decisions drive everything in
this section:

1. **One conversation, one place.** Quick reactions and real discussion are
   different behaviors pretending to be one. Reactions are signal, not
   conversation; text discussion belongs in exactly one place — a linked
   forum thread with full infrastructure (threading, search, moderation,
   typed votes, follow/notify, federation).
2. **Typed votes with meta-moderation** (Slashdot-style) replace the generic
   like/reaction model on community surfaces, and become the positivity
   infrastructure for the work page: `well-written` and `insightful` are
   inherently positive signals; abuse is handled by meta-moderation, not by a
   text classifier on a one-click surface.

Everything here is subject to §0.3. Votes and karma never purchase trust,
moderation authority, or search ranking.

## 35.0 Work discussion modes

Each work carries a discussion mode. The instance sets a default; each author
may override per work; moderators may not override authors.

```rust
enum WorkDiscussionMode {
    ThreadOnly,    // default: typed-vote reaction bar on the work page,
                   // all text discussion in the linked forum thread
    CommentsOnly,  // inline work/chapter comments, no forum link (legacy)
    Both,          // both surfaces, admin-warned: splits conversation
}
```

- **`ThreadOnly`** — the work page shows the typed-vote reaction bar and a
  "Discuss" link to the linked thread (§35.1). No text input on the work
  page. The author follows one thread, gets one notification stream, and
  replies with full formatting and context.
- **`CommentsOnly`** — preserves the §17.1 comment surface and its positivity
  gate unchanged, for admins who prefer the classic model and do not care
  about federation of discussion.
- **`Both`** — offered as the escape hatch; the admin UI states plainly that
  it splits conversation across two surfaces.

Migration path for instances with existing comments: ship threads + reaction
bar first with `CommentsOnly` unchanged for existing works; per-work opt-in
to `ThreadOnly`; flip the instance default when the forum is mature; provide
a batch tool that converts existing comment threads into forum topics
(preserving authorship, body, and timestamps) for authors who switch.
`CommentsOnly` remains supported indefinitely for federation compatibility.

When a work federates, its discussion mode travels with the metadata so a
remote instance knows how to render the discussion surface. A `ThreadOnly`
work arriving at a `CommentsOnly` instance may show a link back to the origin
thread; local mirroring of remote forum posts into a comment thread is
best-effort and never authoritative.

The positivity pipeline (§12) still applies to every surface where readers
write text addressed to the author: work comments in `CommentsOnly`/`Both`
modes, and forum posts under the lighter forum policy (§17.2). In
`ThreadOnly` mode there is no work-page text to classify; the typed-vote
taxonomy is itself the positive-signal infrastructure, and the negative vote
types are costed and meta-moderated (§35.2).

## 35.1 Milestone 31 — Work-linked threads and the reaction bar

**Work-linked threads.** Publishing a new chapter auto-creates (or prompts
the author to create) a discussion topic linked to that work/chapter. The
work page shows a "Discuss" button with the reply count; the topic shows a
backlink card to the work. One linked topic per work by default (chapter
sections inside it), optionally one per chapter for high-traffic works.

Schema: `topic_work_links (topic_id, work_id, chapter_id NULL, unique(topic_id))`.
The link is visible wherever the topic is, including federation surfaces.

**Reaction bar.** On the work page (all modes), a typed-vote bar:

```text
[well-written: 42] [insightful: 17] [funny: 8] [disagree: 3]
                    [Discuss this chapter (47 replies) →]
```

- One vote per pseud per work, changeable, retractable.
- Vote types are the instance's configured set (§35.2), restricted to the
  positive types plus `disagree` on this surface.
- Aggregate counts are public; individual votes follow §35.2 transparency
  tiers.
- The author sees aggregated sentiment at a glance — the point of the bar is
  that the author stops wading through "great chapter!" to find substance.

**Batch migration tool.** Converts a work's comment thread into a forum
topic: comments become posts in order, authorship and timestamps preserved,
original comment soft-deleted with a tombstone pointing at the topic.
Reversible only by re-running the reverse conversion before the tombstones
are pruned.

**API.** `GET/PUT /works/{id}/discussion-mode` (author), `GET /works/{id}/reactions`
(public aggregates), `POST /works/{id}/reactions` (vote, change, retract),
`GET /works/{id}/thread` (linked topic), `POST /works/{id}/migrate-comments`
(author, batch tool). Admin config: `[forum] work_discussion_default`.

**Data model.** `topic_work_links`, `work_reactions (work_id, pseud, vote_type,
created_at, updated_at, unique(work_id, pseud))`, `works.discussion_mode`.

**Acceptance.**
- A `ThreadOnly` work page renders no comment form and shows the bar and
  Discuss link; the link resolves to the linked topic.
- Publishing a chapter creates or prompts the linked topic exactly once
  (idempotent per chapter).
- Votes are one-per-pseud, changeable, and the aggregate counts are correct
  after concurrent votes.
- The migration tool round-trips a comment thread into a topic with
  authorship and timestamps intact.
- A `CommentsOnly` work behaves exactly as §17.1 specifies today.

## 35.2 Milestone 32 — Typed votes, budgets, meta-moderation, karma

**Typed vote taxonomy.** A small fixed set per surface, configurable per
category: default `insightful | funny | interesting | well-written |
disagree`. A Critique category may configure `constructive | harsh-but-fair |
needs-sources` instead. Negative types (default: `disagree`) cost the caster
more budget. The taxonomy is data, not code.

**Vote budget.** N votes per rolling 24h window scaled by trust level
(TL1=10, TL3=30, TL5=60; configuration). Unused votes do not roll over.
Forces signal over noise; a budget-exhausted voter is told plainly.

**Meta-moderation.** TL4+ users spend meta-mod points to flag a vote as
`fair` or `unfair`. A user whose votes are consistently flagged unfair has
their **vote weight** decayed (never their ability to vote). Moderate the
moderators: weight, not voice, is the sanction.

**Transparency tiers.** Aggregate counts public; individual votes anonymous
by default; the post author may opt in to see who voted; moderators always
see; meta-mods see meta-mod actions. Consistent with §29.6: a visible vote
is a vote that can be socially pressured.

**Karma.** Forum karma is derived from received typed votes, weighted by the
caster's vote weight. Karma decays 5% per month of the receiver's inactivity
(configuration). Karma is a display signal only — it never gates trust,
moderation, search ranking, or credits (§0.3). Posting-volume leaderboards
remain not implemented (§17.10 stands); karma leaderboards are equally out.

**This replaces** the generic reaction model on forum surfaces; the existing
`reactions` table is retained for works/library surfaces that already use it.

**API.** `POST /forum/posts/{id}/vote`, `DELETE /forum/posts/{id}/vote`,
`GET /forum/posts/{id}/votes` (per transparency tier), `POST /forum/votes/{id}/meta`
(TL4+), `GET /me/vote-budget`, `GET /forum/karma` (own + public profiles).

**Data model.** `forum_vote_types (id, label, category_scope, weight, cost,
is_negative)`, `forum_votes (post_id, pseud, vote_type, weight_at_cast,
created_at, unique(post_id, pseud))`, `forum_meta_votes (vote_id, pseud,
fair, created_at, unique(vote_id, pseud))`, `forum_karma (pseud, karma,
updated_at)`.

**Acceptance.**
- Budget is enforced per rolling window and per trust level; exhaustion is
  reported, not silently dropped.
- Meta-moderation changes a caster's future vote weight, never their ability
  to cast.
- Individual votes are not exposed where the transparency tier forbids it.
- Karma decays on inactivity and never feeds trust, ranking, or credits.
- Configuring a category's taxonomy changes its surfaces without code changes.

## 35.3 Milestone 33 — Thread modes for creative work

Topic modes are data on the topic; each restructures one surface.

- **AMA / Q&A (`ama`).** Questions float to the top; the topic author's
  replies render as highlighted cards; non-author replies to questions nest
  as discussion.
- **Reading group (`reading_group`).** A schedule of sections ("Week 1:
  chapters 1–5") each unlocking on a date; participants see a progress
  tracker. Builds on §17.11 reading clubs, which keep their group machinery.
- **Critique circle (`critique`).** Private group thread mode: members post
  excerpts on a turn queue; turn order enforced; pile-on limited; quality
  gating ties into trust levels. Critique here is opt-in by joining, so the
  positivity gate's constructive-criticism opt-in (§12) is satisfied by
  membership.
- **Wiki pin (`wiki_pin`).** A collaboratively edited post pinned above the
  OP; edits pass a lightweight approval queue held by the topic author or
  group moderators. Turns long lore threads into living documents.
- **Collaborative fiction (`collab_fic`).** Posts stitch into a single
  narrative; a "compile" action renders the thread as one readable story;
  version history preserved. A thread with traction offers one-click
  **promote to work**: creates a proper work with the thread content as
  chapters, links back, and leaves the thread read-only. This closes the
  loop between community discussion and published content.
- **Prompt (`prompt`).** Posted automatically by the prompt engine
  (daily/weekly writing prompts to a dedicated category); replies are flash
  fiction; community typed-votes decide favorites; winners get badges (§28.11
  governs gamification limits).
- **Character voice (`character_voice`).** Posts render as a character from
  the poster's work (avatar and name), tagged with the character id.
  Moderation applies to the user, never the character.

**Data model.** `forum_topics.mode`, `topic_schedules (topic_id, position,
title, unlocks_at, chapter_range)`, `topic_wiki_pins (topic_id, post_id,
revision, approved_by)`, `critique_queue (topic_id, pseud, position,
posted_at)`, `prompt_posts (topic_id, prompt_date, winner_pseud)`.

**Acceptance.**
- Each mode changes exactly its own surface; a plain topic is unaffected.
- Reading-group sections are invisible and unfetchable before their unlock
  time.
- Critique turn order is enforced by the server, not the client.
- Wiki-pin edits are invisible until approved.
- Promote-to-work creates a real work owned by the thread participants under
  the author's stated permission statement (§33.1), and the thread becomes
  read-only with a backlink.
- Character-voice posts resolve to the user for moderation and blocks even
  though they render as the character.

## 35.4 Milestone 34 — Spoilers, warnings, readability

- **Spoiler-aware zones.** Per-topic spoiler scope ("spoilers through chapter
  12"); collapsible spoiler blocks (blur + click-to-reveal) in posts; a
  "catch me up" badge for readers behind the scope. Reader-side spoiler
  hiding honours the reader's own progress (§9.3), not just the topic scope.
- **Structured content warnings.** `content_warnings (post_id, warning_type,
  severity, custom_text)` with types `violence | sexual_content | self_harm |
  spoilers | custom`. Warned posts blur by default with one-click reveal;
  user prefs control auto-hide vs show per type. Shares the instance
  vocabulary machinery with §15.16.
- **Reading time estimates** from word count on topic cards and post headers.
- **Draft autosave.** Post drafts persist every 30s while composing
  (`post_drafts`, §4.6) and offer resume-on-return. Survives browser crashes.
- **Post scheduling.** `post.scheduled_at`; a background job publishes due
  posts. Useful for reading groups and serialized announcements.
- **Collapsible long posts.** Posts over ~800 words (configuration) fold
  behind "Read more"; user-configurable threshold and off switch.

**Acceptance.**
- Spoiler blocks do not leak content to screen readers when collapsed
  (`aria-expanded`, content hidden from the a11y tree).
- Content-warning blur state follows the viewer's prefs, not the poster's.
- Drafts survive a hard reload mid-composition and resume exactly once.
- Scheduled posts publish at (not after) their time, once, idempotently.
- The fold never hides the first screenful of a post.

## 35.5 Milestone 35 — Discovery, navigation, community health, federation

**Discovery.**
- **Thread summaries.** For topics over N replies, a collapsible TL;DR at
  the top, regenerated every M replies. Local LLM or opt-in external API
  under §23.7's consent and cost rules; deterministic abstention when no
  provider. Stored as `topic.summary_text` + `summary_revision`; never
  presented as author text.
- **Semantic search.** Embeddings of topic title + first post in a
  `pgvector` column (PostgreSQL deployments; SQLite deployments keep FTS
  only and say so on `/api/v1/meta`). Powers "similar threads" and
  "search by vibe". Best-effort; failure never loses the FTS path.
- **Thread forking.** A moderator (or TL4+) selects a contiguous post range
  and forks it into a new topic; originals get "moved to [link]" tombstones;
  `post.original_topic_id` preserved for audit.
- **Cross-reference cards.** Internal links to topics, works, and profiles
  auto-expand into preview cards (title, snippet, reply count, last
  activity), parsed on save and cached. Builds on §31's unfurling.
- **Best-of digests.** TL5+ or elected curators flag posts `featured`; a
  `/forum/digest` view shows them with optional editorial commentary;
  optional email newsletter under §17.5's digest rules.
- **Topic clusters.** Related topics grouped by tag overlap + semantic
  similarity, shown as "Related discussions".

**Community health.**
- **Graduated response ladder.** `verbal_warning → post_throttle (1/hour) →
  read_only (24h/72h/1w) → forum_ban → site_ban`. Each step logs to the
  modlog (§19.12) and notifies the user with the specific reason and appeal
  path. Extends §17.10 scoped sanctions with a named ladder.
- **Appeals.** Structured appeal form; appeals route to a different
  moderator than the actor (enforced by the system); outcomes tracked for
  mod accountability (§19.10).
- **Community-elected moderators.** Category-scoped elections
  (`mod_election`), winners get mod privileges scoped to that category,
  appointed through the same quorum and accountability as §17.12 fandom
  moderators.
- **Diversity of voices.** Gini coefficient of post distribution per
  category; a "dominated discussion" flag surfaces to moderators; suggested
  interventions (new-voices-only threads, slow mode).
- **Newcomer onboarding.** TL0/TL1 users get an invitation to a Welcome
  category with guided prompts.
- **Slow mode.** Per-topic or per-category rate limit: one post per user per
  N minutes (`topic.slow_mode_seconds`).

**UX.**
- **Keyboard shortcuts.** `j/k` next/prev post, `r` reply, `e` edit, `q`
  quote, `/` search; documented in a `?` modal.
- **Reading progress bar** for long topics; scroll position persists across
  loads (extends §17.3 read state).
- **Multi-language posts.** `post.language` (auto-detected or user-set);
  filter topics by language; opt-in auto-translation under §23.7.
- **Offline reading.** Service-worker cache of visited topics with a cached
  indicator; read-state syncs on reconnect (extends §13's offline rules).

**Analytics.**
- **Community health dashboard** (admin): daily active posters, new-user
  retention 7d/30d, average thread depth, time-to-first-reply, report
  volume, mod response time. Privacy-preserving per §24.3.
- **Zero-result search tracking**: log queries that return nothing; surface
  the top ones to admins weekly.
- **Activity sparklines** on topic cards (`topic.activity_history`, daily
  reply counts).
- **First-votes notification**: after a post's first N votes, one gentle
  notification to the author — once per post, never per vote.

**Federation.**
- **Per-topic federation scope**: `public | local | unlisted`
  (`topic.federation_scope`); authors choose at creation, moderators may
  override; `local` topics never leave the instance.
- **Remote profile cards** with home instance, local bio, follow button;
  cached with TTL; `Person` and `Service` actor types.
- **Federated moderation**: local ban of a remote user sends `Reject` to the
  home instance; remote reports aggregate in the mod queue with
  instance-of-origin metadata.
- **Instance reputation**: per-instance spam rate, report rate, takedown
  response time; auto-defederation below a threshold; instance health on the
  admin dashboard. Extends §23.10's instance trust relationships.
- **Federated polls**: ActivityPub `Question` objects; remote votes tallied
  locally, best-effort one-vote-per-actor across instances — the limitation
  is documented, not hidden.

**Acceptance.**
- A `local`-scoped topic is absent from every federation outbound payload.
- Forking preserves original authorship, timestamps, and an audit trail.
- The health dashboard aggregates only what §24.3 permits.
- Sparklines and reading-time come from stored data, not client estimation.
- Federated poll tallies never count one actor twice locally.
- Keyboard shortcuts are documented, discoverable, and never trap focus.

## 35.6 What this section deliberately does not do

- No general-purpose karma economy: karma decays, displays, and does nothing
  else (§0.3).
- No AI-required feature: summaries, semantic search, and translation are
  optional providers with deterministic abstention (§23.7).
- No second forum implementation: fandom spaces (§17.12), reading clubs
  (§17.11), and every mode above build on the same topics/posts machinery.
- No vote on trust: nothing in §35.2 may promote, demote, or gate a trust
  level (§19.1 owns that ladder).

---

The resulting project should be judged by these working behaviors—not by the number of screens, lines of code, imported feature names, or claims in a README.


---
