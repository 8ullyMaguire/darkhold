# 45. Directory Category Governance — community-moderated categories and entry management

**Status:** reserved
**Milestone:** —
**Source:** §45 of `docs/spec.md`

> **2026-09-24 addition.** The directory category system (§39.2) is extended
> with community governance: TL2 members propose category changes (rename,
> merge, deprecate, create), TL1 members vote, and decisions auto-execute
> when quorum is reached. Entry moderation (move/remove between categories)
> also supports quorum review. Taste never touches governance — §0.3 drawn
> plainly — and operator override exists for emergency cases.

## 45.1 Category lifecycle

Categories become DB rows with a state lifecycle:

- **active** — appears in category tabs, accepts new entries, votes count
- **deprecated** — hidden from new submission forms, existing entries remain
  visible with a deprecation notice, votes still count
- **merged** — a redirect to another category; entries are re-homed to the
  target category preserving their scores and votes (§39.3 no-cascade rule)

Category identity is the **slug** (URL-safe, stable). Renaming changes only
the human-facing label; the slug, all entries, scores, votes, and URLs
remain stable. A merge/deprecate preserves entry scores and vote rows — no
entry loses its history because the category changed.

Sources of categories:

- **seed** — built-in categories from §39.2 (`SEED_CATEGORIES`)
- **config** — `extra_categories` from TOML (upserted on startup)
- **community** — created through governance proposals

The config upsert on startup adds new configured categories but **never
resurrects** a category the community removed (hard delete refused).

## 45.2 Proposals and voting

- **TL2** may propose: rename, merge, deprecate, create category
- **TL1** may vote on open proposals
- **Flat weights** — every vote weighs 1. Taste affinity, trust level, and
  private preference are never part of governance (§0.3)

Quorum (per §19.4):

- **routine** (rename, deprecate): 2 votes required
- **high-impact** (merge, create, delete): 3 votes required

Auto-execution: when the deciding vote reaches quorum, the category change
applies in the same transaction as the vote. No operator action needed.

TTL: 14 days. Proposals that never reach quorum are closed as `expired`
and remain in the public changelog for transparency.

## 45.3 Operator override

- Operator may **veto** any open proposal with a reason (recorded in audit)
- Operator may **freeze** all category governance via config switch
  (`directory.governance.frozen`) — no proposals or votes accepted
- Operator may **emergency deprecate** per §19.5 (instant, no quorum)
- **Single-admin degradation**: frozen = "no proposals, no votes"; never
  breakage of the existing category system

## 45.4 Anti-churn guards

All named errors, never silent rejection:

- **72h cooldown** — a category may only be the target of one proposal per 72h
- **5 open proposals** — a category may have at most 5 concurrent open proposals
- **32 categories** — soft ceiling on active categories (operator-raiseable)
- **per-target exclusivity** — only one open proposal per (target, action) pair

## 45.5 Public changelog

- Every category event (proposal, vote, execution, veto, expiry) is recorded
  in `category_changelog` and visible anonymously via `/directory/governance`
- §20.3 quorum-vote credit applies: voters earn curation credit for votes
  that reach quorum

## 45.6 Entry quorum moderation

Directory entries (§39.1 entries) may be moderated across categories:

- **move** — an entry is relocated from one category to another, preserving
  its score and all vote rows
- **remove** — an entry is soft-deleted (per §39.3 removal rules: audit trail,
  no cascade, votes preserved for review)

Both actions require quorum (2 independent TL2+ voters). The moderation
request opens a 14-day voting window. When quorum is reached, the action
applies automatically in the deciding vote's transaction.

## 45.7 Data model

```sql
CREATE TABLE categories (
    id              TEXT PRIMARY KEY,
    slug            TEXT NOT NULL UNIQUE,
    label           TEXT NOT NULL,
    state           TEXT NOT NULL DEFAULT 'active',
    merged_into     TEXT REFERENCES categories(id),
    source          TEXT NOT NULL DEFAULT 'seed',
    created_by      TEXT NOT NULL,
    created_at      TEXT NOT NULL
);

CREATE TABLE category_proposals (
    id              TEXT PRIMARY KEY,
    category_slug   TEXT NOT NULL,
    action          TEXT NOT NULL,
    payload         TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'open',
    yes_votes       INTEGER NOT NULL DEFAULT 0,
    no_votes        INTEGER NOT NULL DEFAULT 0,
    quorum_needed   INTEGER NOT NULL,
    closes_at       TEXT NOT NULL,
    created_by      TEXT NOT NULL,
    decided_by      TEXT,
    decision_reason TEXT,
    created_at      TEXT NOT NULL,
    decided_at      TEXT
);

CREATE TABLE category_votes (
    id              TEXT PRIMARY KEY,
    proposal_id     TEXT NOT NULL REFERENCES category_proposals(id),
    account_id      TEXT NOT NULL,
    value           TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    UNIQUE (proposal_id, account_id)
);

CREATE TABLE category_changelog (
    id              TEXT PRIMARY KEY,
    category_slug   TEXT NOT NULL,
    event           TEXT NOT NULL,
    actor           TEXT NOT NULL,
    document        TEXT NOT NULL DEFAULT '{}',
    created_at      TEXT NOT NULL
);

CREATE TABLE entry_moderation_proposals (
    id              TEXT PRIMARY KEY,
    entry_id        TEXT NOT NULL REFERENCES directory_entries(id),
    action          TEXT NOT NULL,
    target_category TEXT,
    status          TEXT NOT NULL DEFAULT 'open',
    yes_votes       INTEGER NOT NULL DEFAULT 0,
    no_votes        INTEGER NOT NULL DEFAULT 0,
    quorum_needed   INTEGER NOT NULL DEFAULT 2,
    closes_at       TEXT NOT NULL,
    created_by      TEXT NOT NULL,
    decided_by      TEXT,
    created_at      TEXT NOT NULL,
    decided_at      TEXT
);

CREATE TABLE entry_moderation_votes (
    id              TEXT PRIMARY KEY,
    proposal_id     TEXT NOT NULL REFERENCES entry_moderation_proposals(id),
    account_id      TEXT NOT NULL,
    value           TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    UNIQUE (proposal_id, account_id)
);
```

## 45.8 API block

```
POST   /directory/categories/propose        TL2+ — propose rename/merge/deprecate/create
GET    /directory/categories/proposals      public — list open proposals
POST   /directory/categories/proposals/:id/vote  TL1+ — vote yes/no
POST   /directory/categories/proposals/:id/veto  operator only — veto with reason
GET    /directory/governance               public — changelog + category state
POST   /directory/categories               operator only — direct create (bypass governance)
POST   /directory/entries/:id/moderate     TL2+ — propose move/remove entry
GET    /directory/entries/:id/moderation   public — list moderation proposals for entry
POST   /directory/entries/:id/moderate/:pid/vote  TL1+ — vote on entry moderation
```

## 45.9 Acceptance

- A renamed category keeps its slug, label changes only
- A merged category's entries re-home to target, scores and votes preserved
- A deprecated category rejects new submissions but existing entries remain
- A config upsert never resurrects a community-removed category
- A second proposal on the same target+action within 72h is a named error
- More than 5 open proposals on one category is a named error
- 32 active categories is the ceiling (named error at limit)
- Routine quorum = 2, high-impact quorum = 3
- Veto records the operator's reason and appears in changelog
- Frozen governance returns a named error on proposal/vote attempts
- Entry move preserves score and vote rows
- Entry remove is soft-delete with audit trail, no cascade
- 14-day TTL, expired proposals visible in changelog
- Category governance credit per §20.3 quorum-vote rules

## 45.10 Exclusions

- Taste-weighted ranking signals are never part of governance (§0.3)
- Entry voting (§39.4) is separate from entry moderation
- Category governance does not affect §39.1 internal references (works, authors)
- Does not replace operator_create for initial seeding

---
