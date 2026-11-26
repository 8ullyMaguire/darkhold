# docs/plans/m32-m35-detailed.md — plan card

Source: `docs/plans/m32-m35-detailed.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Migration `0039_forum_votes.sql`:
Domain module `forum_votes.rs`:
DB layer `forum_votes.rs`:
HTTP routes (add to `community.rs` router):
Acceptance tests (create `milestone_32.rs`):
- Extend `ForumTopic.svelte` with vote bar per post (show counts, allow voting)
- New component `ForumVoteBar.svelte` (similar to ReactionBar but for posts)
- `VoteBudget.svelte` badge showing remaining votes
- `KarmaBadge.svelte` on pseud profiles
Migration `0040_thread_modes.sql`:
Domain: `forum_thread_modes.rs`
DB layer `forum_thread_modes.rs`:

## Copy-pasteable code / commands

-- Taxonomy per category (default for categories is NULL = use instance default).
CREATE TABLE forum_vote_types (
    id TEXT PRIMARY KEY,
    label TEXT NOT NULL UNIQUE,
    category_scope TEXT,            -- NULL = instance-wide default
    weight REAL NOT NULL DEFAULT 1.0,
    cost INTEGER NOT NULL DEFAULT 1, -- budget cost (>=1; negatives >1)
    is_negative INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX forum_vote_types_category ON forum_vote_types (category_scope);

-- One vote per pseud per post. weight_at_cast captures the caster's effective
-- weight at vote time (after meta-mod decay), so re-computation is unnecessary.
CREATE TABLE forum_votes (
    post_id TEXT NOT NULL,
    pseud TEXT NOT NULL,
    vote_type TEXT NOT NULL,
    weight_at_cast REAL NOT NULL DEFAULT 1.0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (post_id, pseud)
);
CREATE INDEX forum_votes_type ON forum_votes (post_id, vote_type);

-- Meta-moderation of votes. TL4+ spend meta-mod points to flag fair/unfair.
CREATE TABLE forum_meta_votes (
    vote_post_id TEXT NOT NULL,
    vote_pseud TEXT NOT NULL,
    pseud TEXT NOT NULL,            -- the meta-mod who flagged
    fair INTEGER NOT NULL,          -- 1 fair, 0 unfair
    created_at TEXT NOT NULL,
    PRIMARY KEY (vote_post_id, vote_pseud, pseud),
    FOREIGN KEY (vote_post_id, vote_pseud) REFERENCES forum_votes(post_id, pseud)
);

-- Karma per pseud: derived from received votes, decayed on inactivity.
-- Display-only — never gates trust, moderation, ranking, or credits.
CREATE TABLE forum_karma (
    pseud TEXT PRIMARY KEY,
    karma REAL NOT NULL DEFAULT 0.0,
    updated_at TEXT NOT NULL
);

-- [forum] config additions: vote_budget_base, vote_budget_tl_multiplier,
-- meta_mod_tl_required (default 4), karma_decay_monthly (default 0.05),
-- karma_inactivity_threshold_days (default 30)

POST   /forum/posts/{id}/vote        -> post_vote (RequirePseud)
DELETE /forum/posts/{id}/vote        -> delete_vote (RequirePseud)
GET    /forum/posts/{id}/votes       -> get_votes (RequireSession; tier-aware)
POST   /forum/votes/{id}/meta        -> post_meta_vote (RequirePseud, TL4+)
GET    /me/vote-budget               -> get_budget (RequirePseud)
GET    /forum/karma                  -> get_own_karma (RequirePseud)
GET    /forum/users/{id}/karma       -> get_user_karma (public)

ALTER TABLE forum_topics ADD COLUMN mode TEXT NOT NULL DEFAULT 'plain';
-- plain | ama | reading_group | critique | wiki_pin | collab_fic | prompt | character_voice

CREATE TABLE topic_schedules (
    id TEXT PRIMARY KEY,
    topic_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    title TEXT NOT NULL,
    unlocks_at TEXT NOT NULL,
    chapter_start INTEGER,
    chapter_end INTEGER
);
CREATE INDEX topic_schedules_topic ON topic_schedules (topic_id);

CREATE TABLE topic_wiki_pins (
    id TEXT PRIMARY KEY,
    topic_id TEXT NOT NULL UNIQUE,
    post_id TEXT,
    revision INTEGER NOT NULL DEFAULT 0,
    approved_by TEXT,
    approved_at TEXT
);

CREATE TABLE critique_queue (
    id TEXT PRIMARY KEY,
    topic_id TEXT NOT NULL,
    pseud TEXT NOT NULL,
    position INTEGER NOT NULL,
    posted_at TEXT,
    UNIQUE (topic_id, pseud)
);
CREATE INDEX critique_queue_topic ON critique_queue (topic_id);

CREATE TABLE prompt_posts (
    id TEXT PRIMARY KEY,
    topic_id TEXT NOT NULL UNIQUE,
    prompt_date TEXT NOT NULL,
    winner_pseud TEXT
);

-- Character voice: posts reference a character from the author's work.
ALTER TABLE forum_posts ADD COLUMN character_id TEXT;

## Verification command and expected output

- `karma_decays_on_inactivity` (simulate 31 days, verify *= 0.95)
- `compile_stitches_posts_in_order` (verify output text)
Spec §35.4. Depends on: M12 (post drafts, scheduled posts tables exist — verify).
-- Post drafts (if not already in M12 — verify)
- `spoiler_block_hidden_from_screen_reader` (verify aria + content visibility)
- `ContentWarningBar.svelte` (per-type prefs gate)
- `local_topic_absent_from_federation_outbound` (verify ActivityPub payload)

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
